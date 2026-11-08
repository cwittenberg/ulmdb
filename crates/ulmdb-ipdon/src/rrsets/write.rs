use super::{codec::*, *};
use crate::{Error, Result};
use ulmdb::{DatabaseKind, ReadTxn, WriteTxn};

pub struct Rrsets;
impl Rrsets {
    pub fn initialize(txn: &mut WriteTxn<'_>) -> Result<()> {
        let mut save = txn.savepoint()?;
        match save.get(META, b"version") {
            Ok(Some(b"\x01")) => {
                for table in TABLES {
                    save.get(table, b"")?;
                }
            }
            Err(Error::MissingDatabase) => {
                for table in TABLES {
                    save.create_database(table, DatabaseKind::Kv)?;
                }
                save.put(META, b"version", &[1])?;
            }
            _ => return Err(Error::Corrupt("RRset schema version")),
        }
        save.release()
    }

    pub fn view(read: &ReadTxn) -> Result<View<'_>> {
        View::new(read)
    }

    pub fn replace(txn: &mut WriteTxn<'_>, set: &Rrset, freshness: Freshness) -> Result<Delta> {
        let mut save = txn.savepoint()?;
        let report = Self::replace_inner(&mut save, set, freshness)?;
        save.release()?;
        Ok(report)
    }

    /// One savepoint for a bounded batch; an invalid final item restores every
    /// earlier relationship/content/freshness mutation in the batch.
    pub fn replace_batch(
        txn: &mut WriteTxn<'_>,
        sets: &[(Rrset, Freshness)],
    ) -> Result<Vec<Delta>> {
        if sets.len() > MAX_PAGE {
            return Err(Error::LimitExceeded);
        }
        let total = sets.iter().try_fold(0usize, |n, (s, _)| {
            n.checked_add(s.addresses.len()).ok_or(Error::LimitExceeded)
        })?;
        if total > 65_536 {
            return Err(Error::LimitExceeded);
        }
        let mut save = txn.savepoint()?;
        let mut reports = Vec::with_capacity(sets.len());
        let mut work = 0usize;
        for (set, freshness) in sets {
            let report = Self::replace_inner(&mut save, set, *freshness)?;
            work = work
                .checked_add(report.added + report.removed + report.retained)
                .ok_or(Error::LimitExceeded)?;
            if work > 65_536 {
                return Err(Error::LimitExceeded);
            }
            reports.push(report);
        }
        save.release()?;
        Ok(reports)
    }

    pub(crate) fn replace_inner(
        txn: &mut WriteTxn<'_>,
        set: &Rrset,
        freshness: Freshness,
    ) -> Result<Delta> {
        validate_freshness(freshness)?;
        let set = canonical(set)?;
        let id = set.key.id()?;
        let prior = read_current(txn, id)?;
        let old_addresses = match &prior {
            Some((old, state)) => {
                if set.key != old.key {
                    return Err(Error::Corrupt("RRset identity collision"));
                }
                if set.revision < state.revision
                    || freshness.recorded_at < state.freshness.recorded_at
                {
                    return Err(Error::InvalidInput("stale RRset replacement"));
                }
                if set.revision == state.revision {
                    if set != *old || state.status == Status::Withdrawn {
                        return Err(Error::InvalidInput("conflicting/reused RRset revision"));
                    }
                    let mut report = Self::refresh_inner(txn, id, set.revision, freshness)?;
                    if state.status == Status::Active {
                        report.retained = set.addresses.len();
                    }
                    return Ok(report);
                }
                if state.status == Status::Active {
                    old.addresses.as_slice()
                } else {
                    &[]
                }
            }
            None => &[],
        };
        let mut report = Delta::default();
        // Sorted merge changes only added/removed memberships. Retained members
        // carry the SetId, so replacing a revision does not rewrite every edge.
        let (mut a, mut b) = (0, 0);
        while a < old_addresses.len() || b < set.addresses.len() {
            match (old_addresses.get(a), set.addresses.get(b)) {
                (Some(old), Some(new)) if old == new => {
                    report.retained += 1;
                    a += 1;
                    b += 1;
                }
                (Some(old), new) if new.is_none_or(|new| old < new) => {
                    remove_edge(txn, &set, id, *old)?;
                    report.removed += 1;
                    a += 1;
                }
                (_, Some(new)) => {
                    add_edge(txn, &set, id, *new)?;
                    report.added += 1;
                    b += 1;
                }
                _ => unreachable!(),
            }
        }
        if let Some((_, old)) = prior
            && old.status == Status::Active
        {
            txn.delete(DUE, &due(id, old))?;
        }
        let state = State {
            revision: set.revision,
            freshness,
            status: Status::Active,
        };
        txn.put(CONTENT, &id.0, &encode(&set)?)?;
        txn.put(STATE, &id.0, &state_bytes(state))?;
        txn.put(DUE, &due(id, state), &set.revision.to_be_bytes())?;
        report.freshness_changed = true;
        Ok(report)
    }

    pub fn refresh(
        txn: &mut WriteTxn<'_>,
        id: SetId,
        revision: u64,
        freshness: Freshness,
    ) -> Result<bool> {
        let mut save = txn.savepoint()?;
        let report = Self::refresh_inner(&mut save, id, revision, freshness)?;
        save.release()?;
        Ok(report.freshness_changed)
    }

    pub(crate) fn refresh_inner(
        txn: &mut WriteTxn<'_>,
        id: SetId,
        revision: u64,
        freshness: Freshness,
    ) -> Result<Delta> {
        validate_freshness(freshness)?;
        let old = txn
            .get(STATE, &id.0)?
            .ok_or(Error::InvalidInput("refresh references missing RRset"))?;
        let old = state(old)?;
        if revision != old.revision || old.status == Status::Withdrawn {
            return Err(Error::InvalidInput(
                "refresh references replaced/withdrawn RRset",
            ));
        }
        if freshness.observed_at < old.freshness.observed_at
            || freshness.recorded_at < old.freshness.recorded_at
        {
            return Err(Error::InvalidInput("stale RRset refresh"));
        }
        if freshness == old.freshness {
            return Ok(Delta::default());
        }
        if freshness.observed_at == old.freshness.observed_at
            && freshness.recorded_at == old.freshness.recorded_at
        {
            return Err(Error::InvalidInput("conflicting RRset refresh identity"));
        }
        let mut report = Delta {
            freshness_changed: true,
            ..Delta::default()
        };
        if old.status == Status::Expired {
            let set = decode(
                txn.get(CONTENT, &id.0)?
                    .ok_or(Error::Corrupt("RRset content missing"))?,
            )?;
            for address in &set.addresses {
                add_edge(txn, &set, id, *address)?;
            }
            report.added = set.addresses.len();
        } else {
            txn.delete(DUE, &due(id, old))?;
        }
        let current = State {
            freshness,
            status: Status::Active,
            ..old
        };
        txn.put(STATE, &id.0, &state_bytes(current))?;
        txn.put(DUE, &due(id, current), &revision.to_be_bytes())?;
        Ok(report)
    }

    pub(crate) fn owner(txn: &WriteTxn<'_>, id: SetId) -> Result<u64> {
        let bytes = txn
            .get(CONTENT, &id.0)?
            .ok_or(Error::InvalidInput("RRset is missing"))?;
        let (key, _, _, _) = content_header(bytes)?;
        if key.id()? != id {
            return Err(Error::Corrupt("RRset owner identity mismatch"));
        }
        Ok(key.origin)
    }

    pub fn withdraw(
        txn: &mut WriteTxn<'_>,
        id: SetId,
        revision: u64,
        recorded_at: u64,
    ) -> Result<Delta> {
        let mut save = txn.savepoint()?;
        let report = Self::withdraw_inner(&mut save, id, revision, recorded_at)?;
        save.release()?;
        Ok(report)
    }
    pub(crate) fn withdraw_inner(
        txn: &mut WriteTxn<'_>,
        id: SetId,
        revision: u64,
        recorded_at: u64,
    ) -> Result<Delta> {
        let (set, mut current) = read_current(txn, id)?
            .ok_or(Error::InvalidInput("withdraw references missing RRset"))?;
        if current.revision != revision || recorded_at < current.freshness.recorded_at {
            return Err(Error::InvalidInput("stale RRset withdrawal"));
        }
        if current.status == Status::Withdrawn {
            if recorded_at > current.freshness.recorded_at {
                current.freshness.recorded_at = recorded_at;
                txn.put(STATE, &id.0, &state_bytes(current))?;
            }
            return Ok(Delta::default());
        }
        let mut report = Delta::default();
        if current.status == Status::Active {
            for address in &set.addresses {
                remove_edge(txn, &set, id, *address)?;
                report.removed += 1;
            }
            txn.delete(DUE, &due(id, current))?;
        }
        current.status = Status::Withdrawn;
        current.freshness.recorded_at = recorded_at;
        txn.put(STATE, &id.0, &state_bytes(current))?;
        Ok(report)
    }

    /// Apply a bounded read-side plan. Revalidation makes stale plans harmless
    /// after concurrent refresh/replacement; the caller commits this transaction.
    pub fn expire(txn: &mut WriteTxn<'_>, plan: &ExpiryPlan) -> Result<Delta> {
        let mut save = txn.savepoint()?;
        let mut report = Delta::default();
        for (id, expected) in &plan.entries {
            let (set, mut current) =
                read_current(&save, *id)?.ok_or(Error::Corrupt("due RRset missing"))?;
            if current != *expected
                || current.status != Status::Active
                || current.freshness.expires_at > plan.at
            {
                continue;
            }
            for address in &set.addresses {
                remove_edge(&mut save, &set, *id, *address)?;
                report.removed += 1;
            }
            save.delete(DUE, &due(*id, current))?;
            current.status = Status::Expired;
            save.put(STATE, &id.0, &state_bytes(current))?;
        }
        save.release()?;
        Ok(report)
    }
}

fn read_current(txn: &WriteTxn<'_>, id: SetId) -> Result<Option<(Rrset, State)>> {
    match (txn.get(CONTENT, &id.0)?, txn.get(STATE, &id.0)?) {
        (None, None) => Ok(None),
        (Some(content), Some(metadata)) => {
            let set = decode(content)?;
            let state = state(metadata)?;
            if set.key.id()? != id || set.revision != state.revision {
                return Err(Error::Corrupt("RRset identity/revision mismatch"));
            }
            Ok(Some((set, state)))
        }
        _ => Err(Error::Corrupt("incomplete RRset state")),
    }
}
fn add_edge(txn: &mut WriteTxn<'_>, set: &Rrset, id: SetId, address: Address) -> Result<()> {
    let (f, i) = edge_keys(set, id, address)?;
    if txn.get(FORWARD, &f)?.is_some() || txn.get(INVERSE, &i)?.is_some() {
        return Err(Error::Corrupt("duplicate RRset membership"));
    }
    txn.put(FORWARD, &f, b"")?;
    txn.put(INVERSE, &i, b"")
}
fn remove_edge(txn: &mut WriteTxn<'_>, set: &Rrset, id: SetId, address: Address) -> Result<()> {
    let (f, i) = edge_keys(set, id, address)?;
    if txn.get(FORWARD, &f)? != Some(&b""[..]) || txn.get(INVERSE, &i)? != Some(&b""[..]) {
        return Err(Error::Corrupt("missing RRset membership"));
    }
    txn.delete(FORWARD, &f)?;
    txn.delete(INVERSE, &i)
}
