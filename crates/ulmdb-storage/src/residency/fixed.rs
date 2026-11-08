//! Shared fixed slots own lock charges until their actual VMA is replaced.
//! Mapping aliases never release another owner's required lock or quota.
use super::*;
impl ResidencyController {
    pub(in crate::residency) fn admit_fixed_with<O: LockOps>(
        &self,
        table: &SegmentTable,
        state: &Arc<State>,
        ops: &O,
    ) -> Result<ResidencyAdmission> {
        let fixed = table
            .fixed_map()
            .ok_or(Error::InvalidInput("missing fixed placement"))?;
        fixed.check()?;
        let _serial = state.serial.lock().map_err(|_| Error::Busy)?;
        let health_epoch = state.ledger.lock().map_err(|_| Error::Busy)?.health_epoch;
        let (mut mapped, mut pages, mut additional) = (0u64, 0u64, 0u64);
        for mapping in table.mappings() {
            let lease = mapping
                .fixed_lease()
                .ok_or(Error::InvalidInput("mixed fixed lock admission"))?;
            lease.check_creator()?;
            let rounded = lease.rounded_len() as u64;
            mapped = mapped
                .checked_add(lease.len() as u64)
                .ok_or(Error::LimitExceeded)?;
            pages = pages.checked_add(rounded).ok_or(Error::LimitExceeded)?;
            let charges = lease.required_charges().lock().map_err(|_| Error::Busy)?;
            match charges.get(&state.id) {
                Some(charge) if !charge.healthy => {
                    return Err(Error::InvalidInput("fixed lock cleanup unresolved"));
                }
                Some(_) => {}
                None => {
                    additional = additional
                        .checked_add(rounded)
                        .ok_or(Error::LimitExceeded)?
                }
            }
        }
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
        let mut acquired = Vec::new();
        for mapping in table.mappings() {
            match lock_fixed(state, mapping, ops) {
                Ok(true) => acquired.push(mapping),
                Ok(false) => {}
                Err(error) => {
                    rollback_fixed(&acquired, state.id, ops);
                    return Err(error);
                }
            }
        }
        Ok(ResidencyAdmission {
            controller: state.id,
            table: table.residency_identity(),
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
fn lock_fixed<O: LockOps>(state: &Arc<State>, mapping: &Mapping, ops: &O) -> Result<bool> {
    let fixed = mapping
        .fixed_lease()
        .ok_or(Error::InvalidInput("missing fixed lease"))?;
    fixed.check_creator()?;
    let mut charges = fixed.required_charges().lock().map_err(|_| Error::Busy)?;
    if let Some(charge) = charges.get(&state.id) {
        if !charge.healthy {
            return Err(Error::InvalidInput("fixed lock cleanup unresolved"));
        }
        return Ok(false);
    }
    let bytes = fixed.rounded_len() as u64;
    {
        let mut ledger = state.ledger.lock().map_err(|_| Error::Busy)?;
        ledger.reserved = ledger
            .reserved
            .checked_add(bytes)
            .filter(|n| *n <= state.budget)
            .ok_or(Error::LimitExceeded)?;
    }
    let mut lease = Lease {
        state: state.clone(),
        bytes,
        kind: LeaseKind::Pending,
    };
    match ops.lock(mapping) {
        Ok(()) => {
            lease.transition(LeaseKind::Locked);
            charges.insert(
                state.id,
                FixedCharge {
                    lease,
                    healthy: true,
                },
            );
            Ok(true)
        }
        Err(error) => {
            revoke_health(state);
            if ops.unlock(mapping).is_err() {
                lease.transition(LeaseKind::Failed);
                charges.insert(
                    state.id,
                    FixedCharge {
                        lease,
                        healthy: false,
                    },
                );
            }
            Err(error.into())
        }
    }
}
fn rollback_fixed<O: LockOps>(mappings: &[&Mapping], owner: NonZeroU64, ops: &O) {
    for mapping in mappings.iter().rev() {
        let Some(fixed) = mapping.fixed_lease() else {
            continue;
        };
        let mut charges = fixed
            .required_charges()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(charge) = charges.get_mut(&owner) {
            if !charge.healthy {
                continue;
            }
            revoke_health(&charge.lease.state);
            if ops.unlock(mapping).is_ok() {
                charges.remove(&owner);
            } else {
                charge.healthy = false;
                charge.lease.transition(LeaseKind::Failed);
            }
        }
    }
}
