//! Mark inventories of durable/live roots before unlinking any obsolete files.
//! Sealed mapped files are never modified or truncated. Reclaim shares the
//! writer fence with publication, retention changes and compaction.
use crate::{Environment, Error, Result, retention};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};
use ulmdb_storage::Manifest;

mod cursor;
mod names;
#[cfg(target_os = "linux")]
mod plan;
pub(crate) use cursor::ReclaimCursor;
use names::{Selection, temporary};
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod selector;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) use selector::selector as checked_selector;
#[cfg(target_os = "linux")]
pub use plan::{ReclaimPlan, ReclaimPlanLimits, ReclaimProgress, ReclaimStepBudget};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReclaimReport {
    /// Names inspected during this invocation, including protected/unknown files.
    pub scanned_directory_entries: usize,
    /// A bounded automatic scan can resume on a subsequent publishing commit.
    pub enumeration_complete: bool,
    pub removed_segments: usize,
    pub removed_descriptors: usize,
    pub removed_cohorts: usize,
    pub removed_temporary_files: usize,
    pub removed_bytes: u64,
    pub retained_roots: usize,
    pub retained_segment_files: usize,
    pub retained_cohort_files: usize,
}
enum Kind {
    Segment,
    Descriptor,
    Cohort,
    Temporary,
}
struct Candidate {
    path: PathBuf,
    bytes: u64,
    kind: Kind,
}
#[derive(Default)]
struct Marks {
    roots: BTreeSet<u128>,
    segments: BTreeSet<[u8; 32]>,
    cohorts: BTreeSet<u128>,
    generations: BTreeSet<u128>,
}
impl Marks {
    fn mark(&mut self, manifest: &Manifest) {
        // Every caller supplies an authenticated immutable generation. Local
        // readers and physical pins often name the same descriptor; avoid
        // traversing its inventory again for each independent owner.
        if !self.generations.insert(manifest.generation) {
            return;
        }
        if manifest.format_minor >= 3 {
            self.cohorts.insert(manifest.generation);
        } else {
            self.roots.insert(manifest.generation);
        }
        for segment in &manifest.segments {
            if segment.cohort == 0 {
                self.segments.insert(segment.hash);
            } else {
                self.cohorts.insert(segment.cohort);
            }
        }
        self.cohorts
            .extend(manifest.metadata_origins_ref().iter().copied());
        // Origins are physical objects, not additional roots to activate.
        // Never trace their historical descriptor inventories.
    }
}

fn candidates(
    directory: &Path,
    selection: Selection<'_>,
    output: &mut Vec<Candidate>,
    budget: usize,
    scanned: &mut usize,
) -> Result<()> {
    if output.len() >= budget {
        return Ok(());
    }
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_dir() {
        return Err(Error::Corrupt("invalid reclaim directory"));
    }
    for entry in fs::read_dir(directory)? {
        if output.len() >= budget {
            break;
        }
        let entry = entry?;
        *scanned = scanned.checked_add(1).ok_or(Error::LimitExceeded)?;
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            return Err(Error::Corrupt("non-UTF8 engine file"));
        };
        if let Some(kind) = selection.candidate(name) {
            let metadata = fs::symlink_metadata(entry.path())?;
            if !metadata.file_type().is_file() {
                return Err(Error::Corrupt("nonregular reclaim candidate"));
            }
            output.push(Candidate {
                path: entry.path(),
                bytes: metadata.len(),
                kind,
            });
        }
    }
    Ok(())
}
impl Environment {
    pub fn reclaim(&self) -> Result<ReclaimReport> {
        self.require_coordinated()?;
        self.require_file_backend()?;
        let _writer = self.writer.lock().map_err(|_| Error::Busy)?;
        self.reclaim_under_writer(usize::MAX)
    }
    // Caller retains the exact native writer fence throughout mark/unlink.
    pub(crate) fn reclaim_under_writer(&self, budget: usize) -> Result<ReclaimReport> {
        self.reclaim_with_cursor(budget, None)
    }
    pub(crate) fn reclaim_bounded_under_writer(
        &self,
        budget: usize,
        cursor: &mut ReclaimCursor,
        scan_budget: usize,
    ) -> Result<ReclaimReport> {
        self.reclaim_with_cursor(budget, Some((cursor, scan_budget)))
    }
    fn reclaim_with_cursor(
        &self,
        budget: usize,
        cursor: Option<(&mut ReclaimCursor, usize)>,
    ) -> Result<ReclaimReport> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        // A preceding process may have returned unknown after a rename. Persist
        // the currently visible roots and pin removals before trusting absence.
        ulmdb_platform::sync_path(&self.directory)?;
        #[cfg(not(windows))]
        let retained = File::open(self.directory.join("retained"));
        #[cfg(windows)]
        let retained = ulmdb_platform::open_directory(&self.directory.join("retained"));
        match retained {
            Ok(directory) => ulmdb_platform::sync_directory_file(&directory)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let current = self.current.load_full();
        // Head drift must poison before a corrupt predecessor can mask it.
        // The checked boundary preserves that order without decoding CURRENT twice.
        let (durable, previous) = ulmdb_storage::read_roots_checked_with_transform(
            &self.directory,
            |durable| {
                if durable.generation != current.manifest.generation
                    || durable.uuid != current.manifest.uuid
                {
                    self.poisoned.store(true, Ordering::Release);
                    return Err(Error::OutcomeUnknown);
                }
                Ok(())
            },
            current.segments.transform().cloned(),
        )?;
        if previous.as_ref().is_some_and(|m| m.uuid != durable.uuid) {
            return Err(Error::Corrupt("foreign PREVIOUS root"));
        }
        // If crash recovery left a stale ancillary pointer, preserve the actual
        // predecessor until a normal commit repairs rotation. Restored snapshots
        // deliberately omit history, so absence of that descriptor is legal.
        if durable.format_minor < 3
            && durable.previous != 0
            && previous.as_ref().map(|m| m.generation) != Some(durable.previous)
            && (self
                .directory
                .join("commits")
                .join(format!("{:032x}.root", durable.previous))
                .try_exists()?
                || self
                    .directory
                    .join("cohorts")
                    .join(format!("{:032x}.cohort", durable.previous))
                    .try_exists()?)
        {
            return Err(Error::Busy);
        }
        // Native writer ownership keeps the checked head stable. Preserve its
        // drift/poison priority before reading independent coordination records.
        // Deletion additionally excludes all new physical captures from here.
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        let mut physical_maintenance =
            ulmdb_storage::coordination::Coordination::open_with_transform(
                &self.directory,
                current.manifest.uuid,
                Default::default(),
                current.segments.transform().cloned(),
            )?
            .try_maintenance()?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        let physical_pins = physical_maintenance.scan()?;
        let persistent = retention::pins(self)?;
        let mut readers = Vec::new();
        self.retired
            .lock()
            .map_err(|_| Error::Busy)?
            .retain(|weak| {
                if let Some(snapshot) = weak.upgrade() {
                    readers.push(snapshot);
                    true
                } else {
                    false
                }
            });
        let mut marks = Marks::default();
        marks.mark(&durable);
        if let Some(previous) = &previous {
            marks.mark(previous);
        }
        for snapshot in &readers {
            marks.mark(&snapshot.manifest);
        }
        for (_, manifest) in &persistent {
            marks.mark(manifest);
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        for root in physical_pins.live_roots() {
            marks.mark(root.manifest());
        }
        let mut remove = Vec::new();
        let mut scanned = 0;
        let enumeration_complete;
        if let Some((cursor, scan_budget)) = cursor {
            enumeration_complete = cursor.collect(
                &self.directory,
                &marks,
                &mut remove,
                budget,
                scan_budget,
                &mut scanned,
            )?;
        } else {
            candidates(
                &self.directory.join("segments"),
                Selection::Segments(&marks.segments),
                &mut remove,
                budget,
                &mut scanned,
            )?;
            candidates(
                &self.directory.join("commits"),
                Selection::Descriptors(&marks.roots),
                &mut remove,
                budget,
                &mut scanned,
            )?;
            candidates(
                &self.directory.join("cohorts"),
                Selection::Cohorts(&marks.cohorts),
                &mut remove,
                budget,
                &mut scanned,
            )?;
            candidates(
                &self.directory.join("transactions"),
                Selection::Temporary,
                &mut remove,
                budget,
                &mut scanned,
            )?;
            // Only recognized private names are eligible in these mixed directories.
            for directory in [&self.directory, &self.directory.join("retained")] {
                if !directory.try_exists()? {
                    continue;
                }
                if remove.len() >= budget {
                    break;
                }
                for entry in fs::read_dir(directory)? {
                    if remove.len() >= budget {
                        break;
                    }
                    let entry = entry?;
                    scanned = scanned.checked_add(1).ok_or(Error::LimitExceeded)?;
                    if entry.file_name().to_str().is_some_and(temporary) {
                        let metadata = fs::symlink_metadata(entry.path())?;
                        if !metadata.file_type().is_file() {
                            return Err(Error::Corrupt("nonregular temporary file"));
                        }
                        remove.push(Candidate {
                            path: entry.path(),
                            bytes: metadata.len(),
                            kind: Kind::Temporary,
                        });
                    }
                }
            }
            enumeration_complete = remove.len() < budget;
        }
        let mut report = ReclaimReport {
            scanned_directory_entries: scanned,
            enumeration_complete,
            retained_roots: marks.generations.len(),
            retained_cohort_files: marks.cohorts.len(),
            retained_segment_files: marks.segments.len(),
            ..ReclaimReport::default()
        };
        // Complete the mark and candidate validation pass before the first unlink.
        let _total = remove.iter().try_fold(0u64, |sum, candidate| {
            sum.checked_add(candidate.bytes).ok_or(Error::LimitExceeded)
        })?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        physical_pins.cleanup_stale()?;
        let mut changed_directories = BTreeSet::new();
        for candidate in remove {
            #[cfg(windows)]
            physical_maintenance.remove_file(&candidate.path)?;
            #[cfg(not(windows))]
            fs::remove_file(&candidate.path)?;
            changed_directories.insert(
                candidate
                    .path
                    .parent()
                    .ok_or(Error::Corrupt("reclaim parent"))?
                    .to_path_buf(),
            );
            report.removed_bytes += candidate.bytes;
            match candidate.kind {
                Kind::Segment => report.removed_segments += 1,
                Kind::Descriptor => report.removed_descriptors += 1,
                Kind::Cohort => report.removed_cohorts += 1,
                Kind::Temporary => report.removed_temporary_files += 1,
            }
        }
        for directory in changed_directories {
            ulmdb_platform::sync_path(directory)?;
        }
        Ok(report)
    }
}
