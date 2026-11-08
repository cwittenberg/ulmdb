//! Residency admission is completed before any candidate pointer is published.
use crate::{Environment, ReadTxn, Result, environment::Snapshot};
use ulmdb_storage::{ResidencyPolicy, ResidencyReport, ResidencyStatus};

impl Environment {
    pub(crate) fn admit_snapshot(&self, snapshot: &mut Snapshot) -> Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))] self.admit_fixed_placement(snapshot)?;
        // A stale token must never survive changing a candidate's mapping set.
        snapshot.admission = None;
        snapshot
            .manifest
            .certify_indexed_table(&snapshot.segments)?;
        snapshot.admission = Some(self.residency.admit(&snapshot.segments)?);
        Ok(())
    }

    pub(crate) fn admit_snapshot_from(
        &self,
        snapshot: &mut Snapshot,
        base: &Snapshot,
    ) -> Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))] self.admit_fixed_placement(snapshot)?;
        snapshot.admission = None;
        snapshot
            .manifest
            .certify_indexed_table(&snapshot.segments)?;
        snapshot.admission = Some(match &base.admission {
            Some(prior) => {
                self.residency
                    .admit_append(&snapshot.segments, &base.segments, prior)?
            }
            None => self.residency.admit(&snapshot.segments)?,
        });
        Ok(())
    }

    pub fn residency_policy(&self) -> ResidencyPolicy {
        self.residency.policy()
    }

    /// Selected mapped-data coverage; this does not describe heap/catalog RAM.
    pub fn residency_report(&self) -> ResidencyReport {
        report(&self.current.load())
    }

    /// Live mapped-region reservations also include old readers and candidates.
    pub fn residency_status(&self) -> ResidencyStatus {
        self.residency.live_status()
    }
}

impl ReadTxn {
    pub fn residency_report(&self) -> ResidencyReport {
        report(&self.snapshot)
    }
}

fn report(snapshot: &Snapshot) -> ResidencyReport {
    let mut report = snapshot
        .admission
        .as_ref()
        .expect("servable snapshots pass residency admission")
        .report();
    if report.mapped_bytes.is_none() {
        // Demand's hot admission path deliberately does not enumerate maps.
        // This explicit diagnostic may enumerate its bounded inventory.
        report.mapped_bytes = Some(snapshot.manifest.segments.iter().map(|s| s.length).sum());
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DatabaseKind, Error, environment::load_snapshot};
    use std::{fs, sync::Arc};

    #[test]
    fn activation_rejects_foreign_or_stale_readiness_without_swapping_readers() {
        let directory = std::env::temp_dir().join(format!(
            "ulmdb-residency-token-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let env = Environment::create(&directory).unwrap();
        let mut write = env.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        write.put(b"kv", b"key", b"stable").unwrap();
        write.commit().unwrap();
        let _writer = env.writer.lock().unwrap();
        let base = env.current.load_full();
        // Reopening creates different virtual mappings. A cloned token for the
        // same file bytes must not certify those mappings as admitted.
        let mut other = load_snapshot(&directory, base.manifest.clone(), base.owner).unwrap();
        other.admission = base.admission.clone();
        assert!(matches!(
            env.activate(Arc::new(other)),
            Err(Error::OutcomeUnknown)
        ));
        let mut same_maps = Snapshot {
            external: base.external.clone(),
            physical_pin: base.physical_pin.clone(),
            range_backed: false,
            #[cfg(feature = "range-lifecycle")]
            range_checkpoint: None,
            owner: base.owner,
            manifest: base.manifest.clone(),
            segments: base.segments.clone(),
            catalog: base.catalog.clone(),
            admission: None,
        };
        let foreign = ulmdb_storage::ResidencyController::new(ResidencyPolicy::Demand).unwrap();
        same_maps.admission = Some(foreign.admit(&same_maps.segments).unwrap());
        assert!(matches!(
            env.activate(Arc::new(same_maps)),
            Err(Error::OutcomeUnknown)
        ));
        assert!(Arc::ptr_eq(&base, &env.current.load_full()));
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"stable".as_slice())
        );
        drop(base);
        drop(_writer);
        drop(env);
        fs::remove_dir_all(directory).unwrap();
    }
}
