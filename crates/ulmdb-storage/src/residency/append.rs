//! Delta admission requires both a live old table and nonrevoked lock evidence.
use super::*;
impl ResidencyController {
    /// Reuse a healthy required-lock admission only for a private single-insert
    /// lineage. Both tables remain borrowed/pinned for this entire operation.
    /// Every other case uses full admission; Prefault ALWAYS retouches all maps.
    pub fn admit_append(
        &self,
        candidate: &SegmentTable,
        base: &SegmentTable,
        prior: &ResidencyAdmission,
    ) -> Result<ResidencyAdmission> {
        self.admit_append_with(candidate, base, prior, &os::OsLocks)
    }
    pub(in crate::residency) fn admit_append_with<O: LockOps>(
        &self,
        candidate: &SegmentTable,
        base: &SegmentTable,
        prior: &ResidencyAdmission,
        ops: &O,
    ) -> Result<ResidencyAdmission> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if candidate.fixed_map().is_some() {
            return self.admit_with(candidate, ops);
        }
        let Inner::Managed(state) = &self.inner else {
            return self.admit_with(candidate, ops);
        };
        if !matches!(state.policy, ResidencyPolicy::RequiredLocked { .. })
            || !prior.is_for(self)
            || !prior.is_for_table(base)
        {
            return self.admit_with(candidate, ops);
        }
        let Some(mapping) = candidate.appended_mapping(base) else {
            return self.admit_with(candidate, ops);
        };
        let Some(evidence) = prior.report.as_ref() else {
            return self.admit_with(candidate, ops);
        };
        let old = evidence.report;
        let (Some(mapped), Some(pages)) = (old.mapped_bytes, old.page_rounded_bytes) else {
            return self.admit_with(candidate, ops);
        };
        if old.policy != state.policy
            || old.page_size != Some(state.page_size as u64)
            || old.locked_bytes != pages
            || old.prefaulted_bytes != 0
            || mapped > pages
        {
            return self.admit_with(candidate, ops);
        }
        let serial = state.serial.lock().map_err(|_| Error::Busy)?;
        let health_epoch = state.ledger.lock().map_err(|_| Error::Busy)?.health_epoch;
        if health_epoch.is_none() || evidence.health_epoch != health_epoch {
            drop(serial);
            return self.admit_with(candidate, ops);
        }
        let rounded = rounded(mapping, state.page_size)?;
        let mapped = mapped
            .checked_add(mapping.bytes.len() as u64)
            .ok_or(Error::LimitExceeded)?;
        let pages = pages.checked_add(rounded).ok_or(Error::LimitExceeded)?;
        let additional = {
            let guard = mapping.residency.lock().map_err(|_| Error::Busy)?;
            match guard.as_ref() {
                Some(lock) if lock.lease.state.id != state.id => {
                    return Err(Error::InvalidInput(
                        "mapping belongs to another residency lock controller",
                    ));
                }
                Some(lock) if !lock.healthy => {
                    return Err(Error::InvalidInput("mapping lock cleanup unresolved"));
                }
                Some(_) => 0,
                None => rounded,
            }
        };
        if pages > state.budget
            || state
                .ledger
                .lock()
                .map_err(|_| Error::Busy)?
                .reserved
                .checked_add(additional)
                .is_none_or(|n| n > state.budget)
        {
            return Err(Error::LimitExceeded);
        }
        // All aggregate arithmetic/checks precede the only new lock. Successful
        // lock_one never revokes an epoch; failure handles partial cleanup itself.
        lock_one(state, mapping, ops)?;
        Ok(ResidencyAdmission {
            controller: state.id,
            table: candidate.residency_identity(),
            report: Some(Box::new(ManagedAdmission {
                health_epoch,
                report: ResidencyReport {
                    policy: state.policy,
                    mapped_bytes: Some(mapped),
                    page_rounded_bytes: Some(pages),
                    page_size: Some(state.page_size as u64),
                    locked_bytes: pages,
                    prefaulted_bytes: 0,
                },
            })),
        })
    }
}
