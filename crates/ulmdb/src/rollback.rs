//! Last-business-commit rollback by selecting a fresh immutable physical root.
use crate::{Environment, Error};
use std::{fmt, sync::atomic::Ordering};

/// Ineligible history is distinct from parameter, integrity and IO failures.
#[derive(Debug)]
pub enum RollbackError {
    CannotRollback,
    Engine(Error),
}
impl From<Error> for RollbackError {
    fn from(error: Error) -> Self {
        Self::Engine(error)
    }
}
impl fmt::Display for RollbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CannotRollback => write!(f, "last commit cannot be rolled back"),
            Self::Engine(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for RollbackError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CannotRollback => None,
            Self::Engine(error) => Some(error),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RollbackReport {
    pub undone_sequence: u64,
    pub undone_commit: u128,
    pub undone_generation: u128,
    pub sequence: u64,
    pub commit: u128,
    pub generation: u128,
}
fn point(name: &str) -> crate::Result<()> {
    #[cfg(feature = "fault-injection")]
    ulmdb_storage::injected_fault(name)?;
    #[cfg(not(feature = "fault-injection"))]
    let _ = name;
    Ok(())
}
impl Environment {
    /// Undo the selected business commit only when its sequence equals the
    /// supplied ID and its retained physical predecessor is the preceding
    /// nonzero business commit. Physical maintenance, absent history and an
    /// immediate second rollback are ineligible.
    ///
    /// This file-only native API retains the environment's lifetime LOCK and
    /// local writer mutex. Immutable existing readers and exports remain valid.
    /// Like ordinary file open, this authenticates descriptors, inventory/file
    /// bytes and the catalog; user-tree pages are validated when accessed.
    /// Backup verification/export retain their separate full graph audit.
    /// It does not supply the process-wide exclusive-use session required by a
    /// compatibility facade's external LMDB caller contract.
    pub fn rollback_last_commit(
        &self,
        expected_sequence: u64,
    ) -> std::result::Result<RollbackReport, RollbackError> {
        self.rollback_last_commit_inner(expected_sequence, None)
    }
    /// The compatibility facade supplies its current environment policy under
    /// its exclusive-use contract. This policy fences new R metadata only;
    /// rollback does not repeat inherited user-data fences.
    pub fn rollback_last_commit_with_metadata_policy(
        &self,
        expected_sequence: u64,
        policy: crate::Durability,
    ) -> std::result::Result<RollbackReport, RollbackError> {
        self.rollback_last_commit_inner(expected_sequence, Some(policy))
    }
    fn rollback_last_commit_inner(
        &self,
        expected_sequence: u64,
        policy_override: Option<crate::Durability>,
    ) -> std::result::Result<RollbackReport, RollbackError> {
        self.require_file_backend()?;
        self.require_writable()?;
        self.check_external_head()?;
        let _writer = if self.is_externally_synchronized() {
            self.writer.try_lock().map_err(|_| Error::Busy)?
        } else {
            self.writer.lock().map_err(|_| Error::Busy)?
        };
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown.into());
        }
        let base = self.current.load_full();
        let expected = ulmdb_storage::checkpoint_bytes(&base.manifest)?;
        let (selected, previous) = ulmdb_storage::read_roots_checked_with_transform(
            &self.directory,
            |selected| {
                if ulmdb_storage::checkpoint_bytes(selected)? != expected {
                    self.poisoned.store(true, Ordering::Release);
                    return Err(Error::OutcomeUnknown);
                }
                Ok(())
            },
            base.segments.transform().cloned(),
        )?;
        if expected_sequence == 0 || selected.sequence != expected_sequence {
            return Err(RollbackError::CannotRollback);
        }
        let Some(previous) = previous else {
            return Err(RollbackError::CannotRollback);
        };
        if selected.uuid != previous.uuid
            || selected.epoch != previous.epoch
            || selected.page_size != previous.page_size
        {
            return Err(Error::Corrupt("rollback predecessor realm mismatch").into());
        }
        if previous.sequence == 0
            || selected.format_minor != previous.format_minor
            || previous.sequence.checked_add(1) != Some(selected.sequence)
            || selected.previous != previous.generation
            || selected.parent != previous.commit
            || selected.commit == previous.commit
        {
            return Err(RollbackError::CannotRollback);
        }
        // Retain ordinary open's authenticated segment inventory and coherent
        // catalog checks. User-tree pages are checked lazily on access, as in
        // Environment::open; backup/check/export keep their full graph audit.
        // Native LOCK prevents reclamation while the historical descriptor and
        // its authenticated mapped files are loaded and prepared for pinning.
        let mut snapshot = crate::environment::load_snapshot_like(
            &self.directory,
            previous.clone(),
            base.owner,
            &base.segments,
        )?;
        snapshot.external = base.external.clone();
        // Restore A's user records and commit identity without restoring an
        // obsolete allocator. Both committed B IDs and this owner's aborted
        // private IDs must stay burned after reopen or a cooperating owner.
        snapshot.manifest.value_limit = previous.value_limit.max(selected.value_limit);
        snapshot.segments = snapshot
            .segments
            .with_value_limit(snapshot.manifest.value_limit)?;
        snapshot.manifest.next_database = previous
            .next_database
            .max(selected.next_database)
            .max(self.database_id_floor());
        snapshot.manifest.generation = ulmdb_storage::new_identity()?;
        snapshot.manifest.previous = 0;
        if snapshot.manifest.format_minor == 4 {
            snapshot.manifest.prepare_indexed(Some(&previous))?;
        }
        self.admit_snapshot(&mut snapshot)?;
        let descriptor_policy = policy_override.unwrap_or_else(|| self.durability());
        let publication = ulmdb_storage::prepare_rollback_with_durability(
            &self.directory,
            &snapshot.manifest,
            &expected,
            descriptor_policy,
        )?;
        if let Some(coordination) = &self.coordination {
            snapshot
                .pin_with_prepared_capture(&coordination.try_capture()?, publication.prepared())?;
        }
        let state = self.cached_file_durability()?;
        let publication = if let Some(external) = &snapshot.external {
            RollbackPublication::External(
                publication
                    .prepare_ready_externally_synchronized(state.as_ref(), &external.storage)?,
            )
        } else {
            RollbackPublication::Coordinated(publication.prepare_ready(state.as_ref())?)
        };
        let report = RollbackReport {
            undone_sequence: selected.sequence,
            undone_commit: selected.commit,
            undone_generation: selected.generation,
            sequence: snapshot.manifest.sequence,
            commit: snapshot.manifest.commit,
            generation: snapshot.manifest.generation,
        };
        let floor = snapshot.manifest.next_database;
        let snapshot = snapshot.prepare_activation(self, publication.prepared())?;
        point("rollback.before_publish")?;
        let result: crate::Result<()> = (|| {
            let metadata_policy = policy_override.unwrap_or_else(|| self.durability());
            let selected = publication.publish(metadata_policy)?;
            point("rollback.after_publish")?;
            point("rollback.before_activate")?;
            self.activate_prepared(snapshot)?;
            self.remember_file_durability(selected)?;
            point("rollback.after_activate")?;
            Ok(())
        })();
        if result.is_err() {
            self.poisoned.store(true, Ordering::Release);
            return Err(Error::OutcomeUnknown.into());
        }
        self.database_ids.fetch_max(floor, Ordering::AcqRel);
        Ok(report)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::{DatabaseKind, MaintenancePolicy, PublicationMode};
    use ulmdb_storage::coordination::{Coordination, Limits};
    #[test]
    fn bounded_pin_denial_leaves_head_unpoisoned_and_no_pin_leak() {
        for mode in [
            PublicationMode::Legacy,
            PublicationMode::Cohort,
            PublicationMode::Indexed,
        ] {
            let path = std::env::temp_dir().join(format!(
                "ulmdb-rollback-pin-limit-{}",
                ulmdb_storage::new_identity().unwrap()
            ));
            let mut env = Environment::create_with_publication(&path, mode).unwrap();
            let mut w = env.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            w.put(b"kv", b"key", b"old").unwrap();
            w.commit().unwrap();
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"key", b"new").unwrap();
            w.commit().unwrap();
            // Optional discovery owns separate pending source leases. Isolate
            // ordinary reader/candidate admission, then observe actual records
            // through the validated scanner rather than counting generation
            // directories and registry infrastructure as reader pins.
            env.set_maintenance_policy(MaintenancePolicy::Disabled)
                .unwrap();
            let coordinator = env.coordination.as_ref().unwrap().clone();
            {
                let mut maintenance = coordinator.try_maintenance().unwrap();
                maintenance.scan().unwrap().cleanup_stale().unwrap();
            }
            let pins = |checkpoint: Option<[u8; 64]>| {
                let mut maintenance = coordinator.try_maintenance().unwrap();
                let scan = maintenance.scan().unwrap();
                assert_eq!(scan.stale_records(), 0, "{mode:?}");
                match checkpoint {
                    Some(checkpoint) => {
                        assert_eq!(scan.live_roots().len(), 1, "{mode:?}");
                        assert_eq!(scan.live_roots()[0].checkpoint(), &checkpoint, "{mode:?}");
                    }
                    None => assert!(scan.live_roots().is_empty(), "{mode:?}"),
                }
            };
            let checkpoint = env.file_checkpoint().unwrap();
            pins(Some(checkpoint));
            let original = env.coordination.take();
            env.coordination = Some(
                Coordination::open(
                    &path,
                    env.current.load().manifest.uuid,
                    Limits {
                        max_pin_records: 1,
                        ..Default::default()
                    },
                )
                .unwrap(),
            );
            let head = std::fs::read(path.join("CURRENT")).unwrap();
            assert!(matches!(
                env.rollback_last_commit(2),
                Err(RollbackError::Engine(Error::LimitExceeded))
            ));
            assert_eq!(std::fs::read(path.join("CURRENT")).unwrap(), head);
            assert_eq!(env.begin_read().sequence(), 2);
            assert_eq!(
                env.begin_read().get(b"kv", b"key").unwrap(),
                Some(&b"new"[..])
            );
            assert!(!env.poisoned.load(Ordering::Acquire));
            pins(Some(checkpoint));
            env.begin_write().unwrap().abort();
            pins(Some(checkpoint));
            env.coordination = original;
            assert_eq!(env.rollback_last_commit(2).unwrap().sequence, 1);
            assert_eq!(
                env.begin_read().get(b"kv", b"key").unwrap(),
                Some(&b"old"[..])
            );
            env.reclaim().unwrap();
            pins(Some(env.file_checkpoint().unwrap()));
            drop(env);
            pins(None);
            drop(coordinator);
            std::fs::remove_dir_all(path).unwrap();
        }
    }
}

enum RollbackPublication {
    Coordinated(ulmdb_storage::ReadyRollback),
    External(ulmdb_storage::ExternalReadyPublication),
}
impl RollbackPublication {
    fn prepared(&self) -> &ulmdb_storage::PreparedPublication {
        match self {
            Self::Coordinated(p) => p.prepared(),
            Self::External(p) => p.prepared(),
        }
    }
    fn publish(
        self,
        policy: crate::Durability,
    ) -> crate::Result<ulmdb_storage::FileDurabilityState> {
        match self {
            Self::Coordinated(p) => p.publish(policy),
            Self::External(p) => p.publish(policy),
        }
    }
}
