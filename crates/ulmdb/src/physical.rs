//! Physical file ownership is attached before activation, not on borrowed reads.
use crate::{Error, Result, environment::Snapshot};
use std::{
    path::Path,
    sync::{Arc, atomic::Ordering},
};
use ulmdb_storage::PreparedPublication;

/// Immutable admitted/pinned snapshot tied to its prepared checkpoint.
pub(crate) struct PreparedSnapshot {
    snapshot: Arc<Snapshot>,
    checkpoint: [u8; 64],
}
impl PreparedSnapshot {
    pub(crate) fn read(&self) -> crate::ReadTxn {
        crate::ReadTxn {
            snapshot: self.snapshot.clone(),
        }
    }
    pub(crate) fn sequence(&self) -> u64 {
        self.snapshot.manifest.sequence
    }
    pub(crate) fn into_parts(self) -> (Arc<Snapshot>, [u8; 64]) {
        (self.snapshot, self.checkpoint)
    }
}

impl Snapshot {
    pub(crate) fn prepare_activation(
        self,
        environment: &crate::Environment,
        publication: &PreparedPublication,
    ) -> Result<PreparedSnapshot> {
        if self.range_backed
            || self.segments.read_ahead() != environment.read_ahead()
            || self.owner != environment.current.load().owner
            || !publication.matches_directory(&environment.directory)?
            || !self.admission.as_ref().is_some_and(|ready| {
                ready.is_for(&environment.residency) && ready.is_for_table(&self.segments)
            })
        {
            return Err(Error::InvalidInput("unprepared snapshot activation"));
        }
        // Authenticate the complete immutable manifest before selection. No
        // mutable Snapshot escapes the resulting private authority.
        self.check_physical_pin(&environment.directory)?;
        let checkpoint = *publication.checkpoint();
        if self.external.is_some() && ulmdb_storage::checkpoint_bytes(&self.manifest)? != checkpoint
        {
            return Err(Error::Corrupt("external prepared checkpoint mismatch"));
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if self.external.is_none()
            && self
                .physical_pin
                .as_ref()
                .ok_or(Error::InvalidInput("unprepared activation pin"))?
                .root()
                .checkpoint()
                != &checkpoint
        {
            return Err(Error::Corrupt("prepared activation checkpoint"));
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        if ulmdb_storage::checkpoint_bytes(&self.manifest)? != checkpoint {
            return Err(Error::Corrupt("prepared activation checkpoint"));
        }
        let mut retired = environment.retired.lock().map_err(|_| Error::Busy)?;
        // The writer guard excludes another activation until this candidate is
        // consumed. Do amortized reader-owner cleanup before final selection,
        // alongside the reservation for its single retired snapshot.
        if retired.len() >= 64 && retired.len().is_power_of_two() {
            retired.retain(|weak| weak.strong_count() != 0);
        }
        retired.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
        Ok(PreparedSnapshot {
            snapshot: Arc::new(self),
            checkpoint,
        })
    }
    pub(crate) fn pin_prepared(
        &mut self,
        directory: &Path,
        publication: &PreparedPublication,
    ) -> Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            let coordination = ulmdb_storage::coordination::Coordination::open_with_transform(
                directory,
                self.manifest.uuid,
                Default::default(),
                self.segments.transform().cloned(),
            )?;
            self.pin_with_prepared_capture(&coordination.try_capture()?, publication)?;
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let _ = (directory, publication);
        Ok(())
    }

    pub(crate) fn pin_with_prepared_capture(
        &mut self,
        capture: &ulmdb_storage::coordination::CaptureLease,
        publication: &PreparedPublication,
    ) -> Result<()> {
        if self.range_backed {
            return Err(Error::InvalidInput(
                "file coordination cannot pin a range snapshot",
            ));
        }
        let pin = capture.pin_prepared(publication, &self.manifest)?;
        self.physical_pin = Some(Arc::new(pin));
        Ok(())
    }
    pub(crate) fn pin_with_capture(
        &mut self,
        capture: &ulmdb_storage::coordination::CaptureLease,
    ) -> Result<()> {
        if self.range_backed {
            return Err(Error::InvalidInput(
                "file coordination cannot pin a range snapshot",
            ));
        }
        let checkpoint = ulmdb_storage::checkpoint_bytes(&self.manifest)?;
        let pin = capture.pin_checkpoint(&checkpoint)?;
        if !pin.root().matches_manifest(&self.manifest)? {
            return Err(Error::Corrupt("snapshot physical pin mismatch"));
        }
        self.physical_pin = Some(Arc::new(pin));
        Ok(())
    }

    pub(crate) fn check_physical_pin(&self, directory: &Path) -> Result<()> {
        if let Some(external) = &self.external {
            return external.check(directory, &self.manifest);
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if !self.range_backed {
            let pin = self
                .physical_pin
                .as_ref()
                .ok_or(Error::InvalidInput("snapshot has no physical pin"))?;
            if !pin.matches_environment(directory, &self.manifest)? {
                return Err(Error::Corrupt("snapshot physical pin mismatch"));
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let _ = directory;
        Ok(())
    }
}

impl crate::Environment {
    pub(crate) fn pin_existing_snapshot(&self, snapshot: &mut Snapshot) -> Result<()> {
        if let Some(coordination) = &self.coordination {
            snapshot.pin_with_capture(&coordination.try_capture()?)?;
        }
        Ok(())
    }
    /// Complete all fallible immutable descriptor/pin work before publication.
    /// Preserve the old conservative uncertainty boundary for descriptor errors.
    pub(crate) fn prepare_file_snapshot(
        &self,
        snapshot: &mut Snapshot,
        publication: Option<PreparedPublication>,
    ) -> Result<PreparedPublication> {
        self.prepare_file_snapshot_inner(snapshot, publication, None, true)
    }

    /// A business candidate carries the actual Legacy sealer receipt across
    /// admission. Cohort/Indexed publications already own their receipt.
    pub(crate) fn prepare_business_file_snapshot(
        &self,
        snapshot: &mut Snapshot,
        publication: Option<PreparedPublication>,
        sealed: Option<ulmdb_storage::SealedAssets>,
        explicit_prepare: bool,
    ) -> Result<PreparedPublication> {
        self.prepare_file_snapshot_inner(snapshot, publication, sealed, !explicit_prepare)
    }

    fn prepare_file_snapshot_inner(
        &self,
        snapshot: &mut Snapshot,
        publication: Option<PreparedPublication>,
        sealed: Option<ulmdb_storage::SealedAssets>,
        conservative_descriptor_error: bool,
    ) -> Result<PreparedPublication> {
        if publication.is_some() && sealed.is_some() {
            return Err(Error::InvalidInput("duplicate prepared asset authorities"));
        }
        let publication = match publication {
            Some(publication) => publication,
            None => match match sealed {
                Some(sealed) => ulmdb_storage::prepare_publication_with_sealed_data(
                    &self.directory,
                    &snapshot.manifest,
                    sealed,
                ),
                None => ulmdb_storage::prepare_publication(&self.directory, &snapshot.manifest),
            } {
                Ok(publication) => publication,
                Err(_) if conservative_descriptor_error => {
                    self.poisoned.store(true, Ordering::Release);
                    return Err(Error::OutcomeUnknown);
                }
                Err(error) => return Err(error),
            },
        };
        if let Some(external) = &snapshot.external {
            external.check(&self.directory, &snapshot.manifest)?;
        }
        // A pin failure cannot have selected CURRENT. Dropping the unpublished
        // token leaves an immutable orphan for the ordinary reclamation pass.
        if let Some(coordination) = &self.coordination {
            snapshot.pin_with_prepared_capture(&coordination.try_capture()?, &publication)?;
        }
        Ok(publication)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod activation_tests {
    use super::*;
    use crate::{Environment, PublicationMode};
    use std::fs;

    struct Directory(std::path::PathBuf);
    impl Directory {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "ulmdb-activation-authority-{}",
                ulmdb_storage::new_identity().unwrap()
            )))
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn snapshot(environment: &Environment) -> Snapshot {
        let base = environment.current.load_full();
        let mut snapshot = crate::environment::load_snapshot(
            &environment.directory,
            base.manifest.clone(),
            base.owner,
        )
        .unwrap();
        environment.admit_snapshot(&mut snapshot).unwrap();
        environment.pin_existing_snapshot(&mut snapshot).unwrap();
        snapshot
    }
    fn pins(environment: &Environment) -> usize {
        fs::read_dir(environment.directory.join(".ulmdb-coordination/pins"))
            .unwrap()
            .count()
    }
    #[test]
    fn prepared_activation_rejects_changed_manifest_and_copied_directory_authority() {
        for mode in [
            PublicationMode::Legacy,
            PublicationMode::Cohort,
            PublicationMode::Indexed,
        ] {
            let a = Directory::new();
            let b = Directory::new();
            let env = Environment::create_with_publication(&a.0, mode).unwrap();
            let checkpoint = env.file_checkpoint().unwrap();
            let before = fs::read(a.0.join("CURRENT")).unwrap();
            let baseline_pins = pins(&env);
            ulmdb_storage::create_layout(&b.0).unwrap();
            // A genuine byte-for-byte copy has the same checkpoint/realm but
            // distinct filesystem publication ownership.
            for name in ["commits", "cohorts", "segments"] {
                for entry in fs::read_dir(a.0.join(name)).unwrap() {
                    let entry = entry.unwrap();
                    assert!(entry.file_type().unwrap().is_file());
                    fs::copy(entry.path(), b.0.join(name).join(entry.file_name())).unwrap();
                }
            }
            fs::copy(a.0.join("CURRENT"), b.0.join("CURRENT")).unwrap();
            let publication = ulmdb_storage::prepare_existing(&a.0, &checkpoint).unwrap();
            assert!(publication.matches_directory(&a.0).unwrap());
            assert!(!publication.matches_directory(&b.0).unwrap());
            let prepared = snapshot(&env)
                .prepare_activation(&env, &publication)
                .unwrap();
            assert_eq!(prepared.sequence(), 0);
            drop(prepared);
            assert_eq!(pins(&env), baseline_pins);

            let mut changed = snapshot(&env);
            changed.manifest.sequence += 1;
            assert!(matches!(
                changed.prepare_activation(&env, &publication),
                Err(Error::Corrupt(_))
            ));
            assert_eq!(pins(&env), baseline_pins);

            let foreign = ulmdb_storage::prepare_existing(&b.0, &checkpoint).unwrap();
            assert_eq!(foreign.checkpoint(), publication.checkpoint());
            assert!(matches!(
                snapshot(&env).prepare_activation(&env, &foreign),
                Err(Error::InvalidInput(_))
            ));
            assert_eq!(pins(&env), baseline_pins);

            let mut unadmitted = snapshot(&env);
            unadmitted.admission = None;
            assert!(matches!(
                unadmitted.prepare_activation(&env, &publication),
                Err(Error::InvalidInput(_))
            ));
            assert_eq!(pins(&env), baseline_pins);
            assert_eq!(fs::read(a.0.join("CURRENT")).unwrap(), before);
            assert_eq!(env.file_checkpoint().unwrap(), checkpoint);
            env.begin_write().unwrap().abort();
        }
    }
}
