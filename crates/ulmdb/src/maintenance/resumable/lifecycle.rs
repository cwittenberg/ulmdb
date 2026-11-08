//! Optional exact history floor + bounded orphan discovery. All deletions use
//! the parent's fresh writer/Capture root coverage; metadata is progress only.
use super::*;
use ulmdb_storage::coordination::{HistoryFloorBuilder, MaintenanceLease, PhysicalPin};
use ulmdb_storage::{HistoryAvailability, HistoryFileKind, HistoryLifecycleBudget, HistoryPolicy};
#[derive(Default)]
struct Names {
    directory: usize,
    entries: Option<fs::ReadDir>,
    identity: Option<ulmdb_platform::FileIdentity>,
}
impl Names {
    fn next(
        &mut self,
        root: &std::path::Path,
    ) -> Result<Option<(std::path::PathBuf, String, usize)>> {
        let dirs = [
            "commits",
            "cohorts",
            "segments",
            "transactions",
            "",
            "retained",
        ];
        let kind = self.directory;
        let path = root.join(dirs[kind]);
        let actual = match ulmdb_platform::path_identity(&path) {
            Ok(actual) => actual,
            Err(e) if kind == 5 && e.kind() == std::io::ErrorKind::NotFound => {
                self.entries = None;
                self.identity = None;
                self.directory = 0;
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        if !actual.is_dir() {
            return Err(Error::Corrupt("history discovery directory"));
        }
        if let Some(old) = self.identity {
            if !old.same_object(&actual) {
                return Err(Error::Corrupt("history discovery directory replaced"));
            }
        }
        if self.entries.is_none() {
            self.entries = Some(fs::read_dir(&path)?);
            let after = ulmdb_platform::path_identity(&path)?;
            if !actual.same_object(&after) {
                return Err(Error::Corrupt("history directory changed during open"));
            }
            self.identity = Some(actual);
        }
        let Some(entry) = self.entries.as_mut().unwrap().next() else {
            self.entries = None;
            self.identity = None;
            self.directory = (kind + 1) % dirs.len();
            return Ok(None);
        };
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Ok(Some((entry.path(), String::new(), kind)));
        };
        Ok(Some((entry.path(), name.to_owned(), kind)))
    }
    fn check_parent(&self, path: &std::path::Path) -> Result<()> {
        let actual = ulmdb_platform::path_identity(
            path.parent()
                .ok_or(Error::Corrupt("history candidate parent"))?,
        )?;
        if !actual.is_dir() || !self.identity.is_some_and(|i| i.same_object(&actual)) {
            return Err(Error::Corrupt("history candidate directory replaced"));
        }
        Ok(())
    }
    fn candidate_metadata(&self, path: &std::path::Path) -> Result<Option<fs::Metadata>> {
        self.check_parent(path)?;
        match fs::symlink_metadata(path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // ReadDir buffers names across maintenance calls. Another
                // authorized retirement may already have removed this name;
                // absence is benign only within the same verified directory.
                self.check_parent(path)?;
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }
}
struct Plan {
    builder: HistoryFloorBuilder,
    revision: u128,
    origins: Option<ulmdb_storage::MetadataOriginCursor>,
    marked: bool,
    keep: BTreeSet<u128>,
    names: Names,
    published: bool,
}
pub(super) struct Lifecycle {
    info: Option<HistoryAvailability>,
    observation: Option<ulmdb_storage::HistoryAvailabilityObservation>,
    plan: Option<Plan>,
    refresh: bool,
    checked: bool,
    orphan: Names,
    orphan_turn: bool,
    orphan_requested: bool,
    orphan_again: bool,
}
impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            info: None,
            observation: None,
            plan: None,
            refresh: false,
            checked: false,
            orphan: Names::default(),
            orphan_turn: false,
            // A prior owner can leave sealed, unpublished preparation assets
            // without ever retiring an inventory. Request one bounded scan;
            // the ordinary writer/Capture gates and exact live-root marking
            // still supply all deletion authority. Completion clears this bit.
            orphan_requested: true,
            orphan_again: false,
        }
    }
}
impl Lifecycle {
    pub(super) fn reset(&mut self) {
        self.plan = None;
        self.orphan_requested = true;
        self.orphan = Default::default();
        self.orphan_again = false;
        self.refresh = true;
    }
    pub(super) fn pending(&self) -> bool {
        !self.checked
            || self.orphan_requested
            || self.orphan.entries.is_some()
            || self.orphan.directory != 0
            || self.refresh
            || self.plan.is_some()
            || self
                .info
                .as_ref()
                .is_some_and(|i| matches!(i.policy, HistoryPolicy::Retain { .. }))
    }
    pub(super) fn request_orphans(&mut self) {
        // A new preparation can appear behind an active directory cursor.
        // Finish this pass, then coalesce all such requests into one more;
        // resetting on every request could starve the later directories.
        if self.orphan.entries.is_some() || self.orphan.directory != 0 {
            self.orphan_again = true;
        }
        self.orphan_requested = true;
    }
    pub(super) fn dirty(&mut self) {
        self.refresh = true;
    }
    pub(super) fn observe(&mut self, before: &Snapshot, after: &Snapshot, forward: bool) {
        if !forward {
            self.reset();
            return;
        }
        if let Some(plan) = &mut self.plan {
            let result = match (&before.physical_pin, &after.physical_pin) {
                (Some(b), Some(a)) => plan.builder.observe_forward(b, a),
                _ => Err(Error::StaleHandle),
            };
            if result.is_err() {
                self.reset();
            }
        }
    }
    pub(super) fn refresh(&mut self, env: &Environment) -> Result<bool> {
        // Another owner can change policy without selecting a business root.
        // Transferred progress must observe that sidecar even at the same D.
        let current = env.current.load();
        let m = &current.manifest;
        let observation = ulmdb_storage::HistoryAvailabilityObservation::read(
            &env.directory,
            m.uuid,
            m.epoch,
            current.segments.transform().cloned(),
            self.observation.as_ref(),
        )?;
        let info = observation.availability().clone();
        if self.info.as_ref().is_some_and(|old| old != &info) {
            self.plan = None;
        }
        self.checked = true;
        self.refresh = false;
        self.info = Some(info);
        self.observation = Some(observation);
        Ok(self.pending())
    }
    pub(super) fn step(
        &mut self,
        env: &Environment,
        gate: &mut MaintenanceLease,
        roots: &Discovery,
        w: &Window,
        budget: ResumableReclaimBudget,
        mut archive_bytes: u64,
        mut archive_inventory: usize,
        max_unlinks: usize,
        stats: &mut MaintenanceStatus,
    ) -> Result<bool> {
        let info = self.info.clone().unwrap_or_default();
        let finite = match info.policy {
            HistoryPolicy::Retain {
                keep_last_commits,
                budget,
            } => Some((keep_last_commits, budget)),
            HistoryPolicy::KeepAll => None,
        };
        let mut limits = finite.map_or_else(HistoryLifecycleBudget::default, |(_, b)| b);
        limits.unlink_files = limits.unlink_files.min(max_unlinks);
        let current = env.current.load_full();
        let min_live = roots
            .roots
            .values()
            .map(|m| m.sequence())
            .min()
            .unwrap_or(current.manifest.sequence);
        if let Some((keep, _)) = finite {
            let target = current
                .manifest
                .sequence
                .saturating_sub(keep - 1)
                .min(min_live);
            if target != 0 && self.plan.is_none() {
                let cp = current
                    .physical_pin
                    .as_ref()
                    .ok_or(Error::InvalidInput("history lifecycle requires pin"))?
                    .root()
                    .checkpoint();
                self.plan = Some(Plan {
                    builder: gate.begin_history_floor(cp, target.max(info.floor_sequence))?,
                    revision: info.revision,
                    origins: None,
                    marked: false,
                    keep: BTreeSet::new(),
                    names: Default::default(),
                    published: false,
                });
            }
            if let Some(plan) = &mut self.plan {
                if plan.revision != info.revision {
                    self.plan = None;
                    return Ok(true);
                }
                plan.builder.lower_target(target.max(info.floor_sequence))?;
                let mut generations = 0;
                let mut units = 0;
                let mut decoded_entries = 0usize;
                while units < limits.mark_units && !plan.published {
                    if plan.keep.len() >= MAX_MARKS {
                        return Err(Error::LimitExceeded);
                    }
                    if plan.origins.is_none() && !plan.marked {
                        let m = plan.builder.current()?;
                        if m.segments.len() > budget.max_inventory_entries {
                            return Err(Error::LimitExceeded);
                        }
                        plan.keep.insert(m.generation);
                        plan.origins = Some(m.metadata_origin_cursor()?);
                        units += 1;
                        continue;
                    }
                    if !plan.marked {
                        if let Some(origin) = plan.origins.as_mut().unwrap().next_origin()? {
                            plan.keep.insert(origin);
                            units += 1;
                            continue;
                        }
                        plan.origins = None;
                        plan.marked = true;
                    }
                    if plan.builder.ready()? {
                        // Publisher independently rechecks exact selected forward
                        // lineage, physical pins and named pins before fencing.
                        let published = gate
                            .publish_history_floor(&plan.builder, budget.max_inventory_entries)?;
                        self.info = Some(published);
                        plan.published = true;
                        break;
                    }
                    if generations >= limits.generations {
                        break;
                    }
                    plan.builder.advance(
                        gate,
                        budget.max_inventory_entries.saturating_sub(decoded_entries),
                    )?;
                    decoded_entries = decoded_entries
                        .checked_add(plan.builder.current()?.segments.len())
                        .ok_or(Error::LimitExceeded)?;
                    plan.marked = false;
                    generations += 1;
                }
                if !plan.published {
                    return Ok(true);
                }
            }
        }
        let mut removed = 0;
        let mut changed = BTreeSet::new();
        let floor = self.info.as_ref().map_or(0, |i| i.floor_sequence);
        let history_names = if self.plan.is_some() {
            if limits.directory_entries == 1 {
                usize::from(!self.orphan_turn)
            } else {
                limits.directory_entries / 2
            }
        } else {
            0
        };
        self.orphan_turn = !self.orphan_turn;
        if let Some(plan) = &mut self.plan {
            let mut wrapped = false;
            for _ in 0..history_names {
                let prior = plan.names.directory;
                let Some((path, name, directory)) = plan.names.next(&env.directory)? else {
                    if prior == 5 {
                        wrapped = true;
                        break;
                    }
                    continue;
                };
                stats.scanned_directory_entries = stats.scanned_directory_entries.saturating_add(1);
                if removed >= limits.unlink_files {
                    break;
                }
                let Some((generation, kind)) = history_name(&name, directory) else {
                    continue;
                };
                if plan.keep.contains(&generation)
                    || w.protected_cohorts.contains(&generation)
                    || roots.roots.values().any(|m| m.generation() == generation)
                {
                    continue;
                }
                let Some(m) = plan.names.candidate_metadata(&path)? else {
                    continue;
                };
                if !m.is_file() || m.file_type().is_symlink() {
                    return Err(Error::Corrupt("nonregular expired history asset"));
                }
                // Exact body SHA/header observation needs no old metadata-node
                // dependency to be present. It does not certify a serving graph.
                let identity = ulmdb_storage::inspect_history_descriptor(
                    &env.directory,
                    generation,
                    kind,
                    current.segments.transform().cloned(),
                )?;
                if identity.uuid != current.manifest.uuid
                    || identity.epoch != current.manifest.epoch
                {
                    return Err(Error::Corrupt("foreign history sweep asset"));
                }
                if identity.sequence >= floor {
                    continue;
                }
                verify(env, gate, &roots.selected, &roots.previous)?;
                let m = fs::symlink_metadata(&path)?;
                if !m.is_file() || m.file_type().is_symlink() {
                    return Err(Error::Corrupt("nonregular expired history asset"));
                }
                plan.names.check_parent(&path)?;
                #[cfg(windows)]
                    gate.remove_file(&path)?;
                    #[cfg(not(windows))]
                    fs::remove_file(&path)?;
                changed.insert(path.parent().unwrap().to_path_buf());
                removed += 1;
                stats.reclaimed_files = stats.reclaimed_files.saturating_add(1);
                stats.reclaimed_bytes = stats.reclaimed_bytes.saturating_add(m.len());
            }
            if wrapped {
                self.plan = None;
            }
        }
        // Orphan names carry no segment-ID cutoff. Query the freshly discovered,
        // authenticated complete inventories instead of a forward anchor's marks:
        // growing heads can advance faster than those incremental marks finish.
        // Discovery has its own root/inventory bounds, separate from mark_units.
        if self.orphan_requested
            || self.orphan.entries.is_some()
            || self.orphan.directory != 0
        {
            for _ in 0..(limits.directory_entries - history_names) {
                if removed >= limits.unlink_files {
                    break;
                }
                let prior = self.orphan.directory;
                let Some((path, name, directory)) = self.orphan.next(&env.directory)? else {
                    if prior == 5 {
                        self.orphan_requested = std::mem::take(&mut self.orphan_again);
                        break;
                    }
                    continue;
                };
                stats.scanned_directory_entries = stats.scanned_directory_entries.saturating_add(1);
                let temporary = name.starts_with(".prepare-") && name.ends_with(".tmp");
                let segment = (directory == 2)
                    .then(|| hex_name::<32>(&name, ".seg"))
                    .flatten();
                let cohort = (directory == 1)
                    .then(|| hex_name::<16>(&name, ".cohort").map(u128::from_be_bytes))
                    .flatten();
                let remove = temporary
                    || segment.is_some_and(|h| {
                        !roots.roots.values().any(|m| m.has_hash(&h))
                    })
                    || cohort.is_some_and(|g| {
                        !roots.roots.values().any(|m| {
                            m.has_cohort(g)
                        })
                    });
                if !remove {
                    continue;
                }
                let Some(m) = self.orphan.candidate_metadata(&path)? else {
                    continue;
                };
                if !m.is_file() || m.file_type().is_symlink() {
                    return Err(Error::Corrupt("nonregular orphan asset"));
                }
                if let Some(generation) = cohort {
                    // A metadata-only origin may still be referenced by a live
                    // Indexed root. Preserve its exact authenticated descriptor
                    // before unlinking payload: NodeRef/checkpoint readers fall
                    // back to this durable archive without changing identity.
                    let report =
                        gate.archive_cohort_history(generation, archive_bytes, archive_inventory)?;
                    archive_inventory = archive_inventory
                        .checked_sub(report.inventory_entries)
                        .ok_or(Error::LimitExceeded)?;
                    if report.created {
                        stats.resumable_archived_cohorts =
                            stats.resumable_archived_cohorts.saturating_add(1);
                        stats.resumable_archived_metadata_bytes = stats
                            .resumable_archived_metadata_bytes
                            .saturating_add(report.metadata_bytes);
                    }
                    archive_bytes = archive_bytes
                        .checked_sub(report.metadata_bytes)
                        .ok_or(Error::LimitExceeded)?;
                }
                verify(env, gate, &roots.selected, &roots.previous)?;
                self.orphan.check_parent(&path)?;
                #[cfg(windows)]
                    gate.remove_file(&path)?;
                    #[cfg(not(windows))]
                    fs::remove_file(&path)?;
                changed.insert(path.parent().unwrap().to_path_buf());
                removed += 1;
                stats.reclaimed_files = stats.reclaimed_files.saturating_add(1);
                stats.reclaimed_bytes = stats.reclaimed_bytes.saturating_add(m.len());
            }
        }
        for d in changed {
            ulmdb_platform::sync_directory(&d)?;
        }
        verify(env, gate, &roots.selected, &roots.previous)?;
        Ok(self.plan.is_some())
    }
}
fn hex_name<const N: usize>(name: &str, suffix: &str) -> Option<[u8; N]> {
    let b = name.strip_suffix(suffix)?.as_bytes();
    if b.len() != N * 2 {
        return None;
    }
    let mut out = [0; N];
    for (o, b) in out.iter_mut().zip(b.chunks_exact(2)) {
        *o = ulmdb_core::hex::lowercase_byte(b[0], b[1])?;
    }
    Some(out)
}
fn history_name(name: &str, directory: usize) -> Option<(u128, HistoryFileKind)> {
    if directory == 0 {
        hex_name::<16>(name, ".root")
            .map(|g| (u128::from_be_bytes(g), HistoryFileKind::LegacyDescriptor))
            .or_else(|| {
                hex_name::<16>(name, ".history")
                    .map(|g| (u128::from_be_bytes(g), HistoryFileKind::Archive))
            })
    } else if directory == 1 {
        hex_name::<16>(name, ".cohort").map(|g| (u128::from_be_bytes(g), HistoryFileKind::Cohort))
    } else {
        None
    }
}
impl crate::maintenance::MaintenanceState {
    pub(crate) fn request_orphan_discovery(&mut self) {
        if let Some(s) = &mut self.resumable {
            s.state.lifecycle.request_orphans();
        }
    }

    pub(crate) fn note_history_configuration(&mut self) {
        if let Some(s) = &mut self.resumable {
            s.state.lifecycle.dirty();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffered_candidates_removed_by_real_reclaim_are_absent_in_the_same_parent() {
        let directory = std::env::temp_dir().join(format!(
            "resumable-buffered-names-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let env = Environment::create_with_publication(
            &directory,
            crate::PublicationMode::Legacy,
        )
        .unwrap();
        let history_name = format!("{:032x}.root", u128::MAX);
        let orphan_name = format!("{}.seg", "f".repeat(64));
        let history = directory.join("commits").join(&history_name);
        let orphan = directory.join("segments").join(&orphan_name);
        fs::write(&history, b"unreferenced history candidate").unwrap();
        fs::write(&orphan, b"unreferenced segment candidate").unwrap();
        let mut cursors = Vec::new();
        for (kind, expected) in [(0, history_name), (2, orphan_name)] {
            let mut names = Names {
                directory: kind,
                ..Default::default()
            };
            loop {
                let Some((path, name, _)) = names.next(&directory).unwrap() else {
                    panic!("candidate absent before real reclaim");
                };
                if name == expected {
                    assert!(names.entries.is_some());
                    cursors.push((names, path));
                    break;
                }
            }
        }
        let report = {
            let _writer = env.writer.lock().unwrap();
            env.reclaim_under_writer(usize::MAX).unwrap()
        };
        assert_eq!(report.removed_descriptors, 1);
        assert_eq!(report.removed_segments, 1);
        for (names, path) in &cursors {
            assert!(names.candidate_metadata(path).unwrap().is_none());
        }

        // Existing descriptor bytes still go through authentication; only an
        // absent candidate name is skipped, never an invalid dependency/body.
        fs::write(&history, b"invalid existing descriptor").unwrap();
        assert!(cursors[0].0.candidate_metadata(&history).unwrap().is_some());
        assert!(
            ulmdb_storage::inspect_history_descriptor(
                &directory,
                u128::MAX,
                HistoryFileKind::LegacyDescriptor,
                None,
            )
            .is_err()
        );
        fs::remove_file(&history).unwrap();
        let parent = directory.join("commits");
        let saved = directory.join("saved-commits");
        fs::rename(&parent, &saved).unwrap();
        fs::create_dir(&parent).unwrap();
        assert!(matches!(
            cursors[0].0.candidate_metadata(&history),
            Err(Error::Corrupt("history candidate directory replaced"))
        ));
        drop((cursors, env));
        fs::remove_dir_all(directory).unwrap();
    }
}
