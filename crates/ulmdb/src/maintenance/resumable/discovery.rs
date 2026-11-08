//! Resumable protected-root discovery. Progress is never deletion authority.
use super::*;
use std::fs::ReadDir;
use ulmdb_storage::coordination::{
    MaintenanceLease, PendingPhysicalPin, PinDiscovery, PinDiscoveryBudget,
};
use ulmdb_storage::{DescriptorCursorBudget, FlatDescriptorCursor, IndexedDescriptorCursor};

enum Cursor {
    Flat(FlatDescriptorCursor),
    Indexed(IndexedDescriptorCursor),
}
struct Input {
    pin: PendingPhysicalPin,
    cursor: Cursor,
}
pub(super) struct Progress {
    scan: PinDiscovery,
    pub roots: Discovery,
    queue: VecDeque<PendingPhysicalPin>,
    input: Option<Input>,
    held: Vec<PendingPhysicalPin>,
    generations: BTreeSet<u128>,
    names: Option<ReadDir>,
    names_done: bool,
    names_count: usize,
    names_identity: Option<ulmdb_platform::FileIdentity>,
    previous: Option<(Checkpoint, Arc<ProtectedRoot>)>,
    dynamic: BTreeSet<Checkpoint>,
    anchor: Checkpoint,
    previous_protected: bool,
}
impl Progress {
    pub fn new(env: &Environment, gate: &mut MaintenanceLease, anchor: Checkpoint) -> Result<Self> {
        let writer = env
            ._file_lock
            .as_ref()
            .ok_or(Error::InvalidInput("discovery requires writer fence"))?;
        let scan = gate.begin_pin_discovery(writer)?;
        let anchor_pin = gate.protect_checkpoint_pending(&anchor, writer)?;
        Ok(Self {
            scan,
            roots: Discovery {
                roots: BTreeMap::new(),
                entries: 0,
                selected: Vec::new(),
                previous: None,
            },
            queue: VecDeque::new(),
            input: None,
            held: vec![anchor_pin],
            generations: BTreeSet::new(),
            names: None,
            names_done: false,
            names_count: 0,
            names_identity: None,
            previous: None,
            dynamic: BTreeSet::new(),
            anchor,
            previous_protected: false,
        })
    }
    pub fn anchor(
        &mut self,
        checkpoint: Checkpoint,
        gate: &MaintenanceLease,
        writer: &ulmdb_storage::WriterFence,
    ) -> Result<()> {
        if self.anchor != checkpoint {
            self.held
                .push(gate.protect_checkpoint_pending(&checkpoint, writer)?);
            self.anchor = checkpoint;
        }
        Ok(())
    }
    pub fn retention_changed(&mut self) {
        self.names = None;
        self.names_done = false;
        self.names_count = 0;
        self.names_identity = None;
    }
    pub fn previous_owned(&mut self, checkpoint: Checkpoint, manifest: &Manifest) {
        self.previous = Some((
            checkpoint,
            Arc::new(ProtectedRoot::Manifest(manifest.clone())),
        ));
    }
    fn add(
        &mut self,
        checkpoint: Checkpoint,
        root: Arc<ProtectedRoot>,
        b: ResumableReclaimBudget,
        uuid: [u8; 16],
        _epoch: u64,
    ) -> Result<()> {
        if root.uuid() != uuid || root.minor() > 4 {
            return Err(Error::Corrupt("foreign discovery descriptor"));
        }
        if self.roots.roots.contains_key(&checkpoint) {
            return Ok(());
        }
        let count = self
            .roots
            .entries
            .checked_add(root.len())
            .ok_or(Error::LimitExceeded)?;
        if self.roots.roots.len() >= b.max_roots || count > b.max_inventory_entries {
            return Err(Error::LimitExceeded);
        }
        self.roots.entries = count;
        self.roots.roots.insert(checkpoint, root);
        Ok(())
    }
    fn open_input(
        env: &Environment,
        pin: PendingPhysicalPin,
        b: ResumableReclaimBudget,
    ) -> Result<Input> {
        pin.check()?;
        let minor = if let Some(c) = pin.checkpoint() {
            if &c[..8] == b"ULMDBT04" {
                4
            } else if &c[..8] == b"ULMDBC03" {
                3
            } else {
                2
            }
        } else {
            env.current.load().manifest.format_minor
        };
        let source = if let Some(c) = pin.checkpoint() {
            ulmdb_storage::open_descriptor_source(&env.directory, c)?
        } else {
            ulmdb_storage::open_descriptor_generation(&env.directory, pin.generation(), minor)?
        };
        let transform = pin.transform_context()?;
        let cursor = if source.format_minor == 4 {
            Cursor::Indexed(if let Some(c) = pin.checkpoint() {
                if source.transformed {
                    IndexedDescriptorCursor::new_with_transform(
                        source.file,
                        source.start,
                        source.length,
                        *c,
                        b.max_inventory_entries,
                        transform.ok_or(Error::CryptoMismatch)?,
                    )?
                } else {
                    IndexedDescriptorCursor::new(
                        source.file,
                        source.start,
                        source.length,
                        *c,
                        b.max_inventory_entries,
                    )?
                }
            } else {
                IndexedDescriptorCursor::observe_identity_with_transform(
                    source.file,
                    source.start,
                    source.length,
                    source.generation,
                    b.max_inventory_entries,
                    if source.transformed { transform } else { None },
                )?
            })
        } else {
            Cursor::Flat(if let Some(c) = pin.checkpoint() {
                if source.transformed {
                    FlatDescriptorCursor::new_with_transform(
                        source.file,
                        source.start,
                        source.length,
                        *c,
                        b.max_inventory_entries,
                        transform.ok_or(Error::CryptoMismatch)?,
                    )?
                } else {
                    FlatDescriptorCursor::new(
                        source.file,
                        source.start,
                        source.length,
                        *c,
                        b.max_inventory_entries,
                    )?
                }
            } else {
                FlatDescriptorCursor::observe_identity_with_transform(
                    source.file,
                    source.start,
                    source.length,
                    source.generation,
                    source.format_minor,
                    b.max_inventory_entries,
                    if source.transformed { transform } else { None },
                )?
            })
        };
        Ok(Input { pin, cursor })
    }
    pub fn advance(
        &mut self,
        env: &Environment,
        gate: &mut MaintenanceLease,
        current: &Arc<Snapshot>,
        budget: ResumableReclaimBudget,
        units: &mut usize,
    ) -> Result<bool> {
        let writer = env
            ._file_lock
            .as_ref()
            .ok_or(Error::InvalidInput("discovery requires writer fence"))?;
        self.roots.selected = checked_selector(&env.directory, "CURRENT", false)?
            .ok_or(Error::Corrupt("missing selected descriptor"))?;
        let selected = *current
            .physical_pin
            .as_ref()
            .ok_or(Error::InvalidInput("unbound discovery head"))?
            .root()
            .checkpoint();
        // Keep the complete selector for pre-unlink verification. Cohort and
        // weak selectors wrap their checked checkpoint in a larger image.
        let observation = ulmdb_storage::observe_selector(&self.roots.selected).map_err(|_| {
            env.poisoned.store(true, Ordering::Release);
            Error::OutcomeUnknown
        })?;
        if observation.checkpoint() != &selected {
            env.poisoned.store(true, Ordering::Release);
            return Err(Error::OutcomeUnknown);
        }
        // The scheduler normally rejects weak cached state before discovery.
        // A fresh selector observation must independently retain that fence.
        if observation.is_weak() || observation.has_data_debt() {
            return Err(Error::Busy);
        }
        self.roots.previous = if current.manifest.format_minor < 3 {
            checked_selector(&env.directory, "PREVIOUS", true)?
        } else {
            None
        };
        // Only two forward-owned snapshots are kept dynamically; history and reader roots retain their own pending leases.
        let prior = self.previous.clone();
        let keep = prior.as_ref().map(|p| p.0);
        let old: Vec<_> = self
            .dynamic
            .iter()
            .copied()
            .filter(|p| *p != selected && Some(*p) != keep && *p != self.anchor)
            .collect();
        for p in old {
            self.dynamic.remove(&p);
            if !self.held.iter().any(|h| h.checkpoint() == Some(&p)) {
                if let Some(root) = self.roots.roots.remove(&p) {
                    self.roots.entries -= root.len();
                }
            }
        }
        self.add(
            selected,
            Arc::new(ProtectedRoot::Manifest(current.manifest.clone())),
            budget,
            current.manifest.uuid,
            current.manifest.epoch,
        )?;
        self.dynamic.insert(selected);
        if let Some((p, m)) = prior.filter(|(_, m)| m.generation() == current.manifest.previous) {
            self.add(p, m, budget, current.manifest.uuid, current.manifest.epoch)?;
            self.dynamic.insert(p);
        }
        if !self.previous_protected {
            self.previous_protected = true;
            if let Some((checkpoint, _)) = self
                .roots
                .roots
                .iter()
                .find(|(_, r)| r.generation() == current.manifest.previous)
            {
                if *checkpoint != selected
                    && !self.held.iter().any(|p| p.checkpoint() == Some(checkpoint))
                {
                    self.held
                        .push(gate.protect_checkpoint_pending(checkpoint, writer)?);
                }
            }
        }
        if current.manifest.previous != 0
            && !self
                .roots
                .roots
                .values()
                .any(|m| m.generation() == current.manifest.previous)
            && self.generations.insert(current.manifest.previous)
        {
            if let Some(pin) =
                gate.protect_previous_generation(current.physical_pin.as_ref().unwrap())?
            {
                self.queue.push_back(pin);
            }
        }
        let allowance = budget.mark_units.saturating_sub(*units);
        if allowance == 0 {
            return Ok(false);
        }
        let records = self.scan.advance(
            gate,
            PinDiscoveryBudget {
                records: allowance.min(32),
            },
        )?;
        // A record is work even when it names an already covered checkpoint.
        *units = units
            .saturating_add(records.work_units)
            .min(budget.mark_units);
        for pin in records.pins {
            if pin
                .checkpoint()
                .is_some_and(|p| self.roots.roots.contains_key(p))
            {
                continue;
            }
            if self.queue.len() + self.held.len() >= budget.max_roots {
                return Err(Error::LimitExceeded);
            }
            self.queue.push_back(pin);
        }
        if let Some(identity) = self.names_identity {
            let actual = ulmdb_platform::path_identity(&env.directory.join("retained"))?;
            if !actual.is_dir() || !identity.same_object(&actual) {
                return Err(Error::Corrupt(
                    "retention directory replaced during discovery",
                ));
            }
        }
        while *units < budget.mark_units && !self.names_done {
            if self.names.is_none() {
                let parent = env.directory.join("retained");
                let identity = match ulmdb_platform::path_identity(&parent) {
                    Ok(i) => i,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        self.names_done = true;
                        break;
                    }
                    Err(e) => return Err(e.into()),
                };
                if !identity.is_dir() {
                    return Err(Error::Corrupt("invalid retention directory"));
                }
                self.names = Some(fs::read_dir(&parent)?);
                if !identity.same_object(&ulmdb_platform::path_identity(&parent)?) {
                    return Err(Error::Corrupt("retention directory changed during open"));
                }
                self.names_identity = Some(identity);
            }
            *units += 1;
            self.names_count += 1;
            if self.names_count > budget.max_roots.saturating_mul(16).saturating_add(64) {
                return Err(Error::LimitExceeded);
            }
            let Some(entry) = self.names.as_mut().unwrap().next() else {
                self.names = None;
                self.names_done = true;
                break;
            };
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(stem) = name.strip_suffix(".pin") else {
                continue;
            };
            if stem.is_empty()
                || stem.len() > 64
                || !stem
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err(Error::Corrupt("invalid retained snapshot name"));
            }
            let mut bytes = [0u8; 64];
            use std::io::Read;
            let mut f = ulmdb_platform::open_readonly(&entry.path())?;
            if f.metadata()?.len() != 64 {
                return Err(Error::Corrupt("invalid retention pointer"));
            }
            f.read_exact(&mut bytes)?;
            if !self.roots.roots.contains_key(&bytes) {
                self.queue
                    .push_back(gate.protect_checkpoint_pending(&bytes, writer)?);
            }
        }
        while *units < budget.mark_units {
            if self.input.is_none() {
                let Some(pin) = self.queue.pop_front() else {
                    break;
                };
                *units += 1;
                pin.check()?;
                if pin
                    .checkpoint()
                    .is_some_and(|p| self.roots.roots.contains_key(p))
                    || (pin.checkpoint().is_none()
                        && self
                            .roots
                            .roots
                            .values()
                            .any(|r| r.generation() == pin.generation()))
                {
                    continue;
                }
                if self.held.len() >= budget.max_roots {
                    return Err(Error::LimitExceeded);
                }
                self.input = Some(Self::open_input(env, pin, budget)?);
            }
            let left = budget.mark_units.saturating_sub(*units);
            if left == 0 {
                break;
            }
            let input = self.input.as_mut().unwrap();
            input.pin.check()?;
            let b = DescriptorCursorBudget {
                bytes: left.checked_mul(8192).ok_or(Error::LimitExceeded)?,
                entries: left,
            };
            let (ready, work, needed) = match &mut input.cursor {
                Cursor::Flat(c) => {
                    let p = c.advance(b)?;
                    (
                        p.ready,
                        p.decoded_entries.max(usize::from(p.read_bytes != 0)),
                        None,
                    )
                }
                Cursor::Indexed(c) => {
                    let p = c.advance(b)?;
                    (
                        p.ready,
                        p.work_units.max(usize::from(p.read_bytes != 0)),
                        p.needed_origin,
                    )
                }
            };
            *units = units.saturating_add(work).min(budget.mark_units);
            input.pin.check()?;
            if let Some(origin) = needed {
                if *units == budget.mark_units {
                    break;
                }
                *units += 1;
                let source = ulmdb_storage::open_descriptor_generation(&env.directory, origin, 4)?;
                if let Cursor::Indexed(c) = &mut input.cursor {
                    c.supply_origin_with_transform(
                        origin,
                        source.file,
                        source.start,
                        source.length,
                        if source.transformed {
                            input.pin.transform_context()?
                        } else {
                            None
                        },
                    )?;
                }
                continue;
            }
            if !ready {
                break;
            }
            let Input { pin, cursor } = self.input.take().unwrap();
            pin.check()?;
            let (checkpoint, root) = match cursor {
                Cursor::Flat(c) => {
                    let f = c.finish()?;
                    (*f.checkpoint(), ProtectedRoot::Flat(f))
                }
                Cursor::Indexed(c) => {
                    let f = c.finish()?;
                    (*f.checkpoint(), ProtectedRoot::Indexed(f))
                }
            };
            if pin.claimed_sequence().is_some_and(|s| s != root.sequence()) {
                return Err(Error::Corrupt("pin sequence changed"));
            }
            if pin.claimed_commit().is_some_and(|c| c != root.commit()) {
                return Err(Error::Corrupt("pin commit changed"));
            }
            self.add(
                checkpoint,
                Arc::new(root),
                budget,
                current.manifest.uuid,
                current.manifest.epoch,
            )?;
            self.held.push(pin);
        }
        let done =
            records.complete && self.names_done && self.queue.is_empty() && self.input.is_none();
        if !done {
            return Ok(false);
        }
        if *units == budget.mark_units {
            return Ok(false);
        }
        // A frontier check can seal new membership even when advance() had no
        // entries left. Charge that control work so the caller can distinguish
        // real progress from a blocked scan and continue within this budget.
        *units += 1;
        // Fresh Capture EX stays with this call; any later births revoke readiness.
        if self.scan.refresh(gate)? {
            return Ok(false);
        }
        for pin in &self.held {
            pin.check()?;
        }
        if let Some(identity) = self.names_identity {
            if !identity.same_object(&ulmdb_platform::path_identity(
                &env.directory.join("retained"),
            )?) {
                return Err(Error::Corrupt("retention directory replaced"));
            }
        }
        verify(env, gate, &self.roots.selected, &self.roots.previous)?;
        Ok(true)
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "discovery_tests.rs"]
mod tests;
