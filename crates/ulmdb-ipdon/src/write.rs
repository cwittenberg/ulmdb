use crate::{Error, Prefix, Result, codec, model::*, schema::*};
use ulmdb::{DatabaseKind, ReadTxn, WriteTxn};

/// Facade over canonical records and their atomic serving projections.
pub struct Projection;
impl Projection {
    /// Initialize all fixed indexes together. Existing schema version 1 is kept.
    pub fn initialize(txn: &mut WriteTxn<'_>) -> Result<()> {
        let mut scope = txn.savepoint()?;
        match scope.get(META, VERSION) {
            Ok(Some(b"\0\0\0\0\0\0\0\x01")) => {
                for name in TABLES {
                    scope.get(name, b"")?;
                }
                if number(
                    scope
                        .get(META, NEXT)?
                        .ok_or(Error::Corrupt("missing hostname sequence"))?,
                )? == 0
                {
                    return Err(Error::Corrupt("zero hostname sequence"));
                }
            }
            Err(Error::MissingDatabase) => {
                for name in TABLES {
                    scope.create_database(name, DatabaseKind::Kv)?;
                }
                scope.put(META, VERSION, &1u64.to_be_bytes())?;
                scope.put(META, NEXT, &1u64.to_be_bytes())?;
            }
            Ok(_) => return Err(Error::Corrupt("unsupported ipdon projection schema")),
            Err(error) => return Err(error),
        }
        scope.release()
    }
    /// Replace one stable source-record ID; identical replays do no writes.
    pub fn upsert(txn: &mut WriteTxn<'_>, observation: &Observation) -> Result<bool> {
        let normalized = normalize_hostname(&observation.hostname)?;
        let encoded = codec::encode(observation)?;
        let mut scope = txn.savepoint()?;
        check_schema(&scope)?;
        let old = scope.get(OBS, &observation.id.0)?.map(<[u8]>::to_vec);
        if old.as_deref() == Some(encoded.as_slice()) {
            scope.release()?;
            return Ok(false);
        }
        if let Some(bytes) = old {
            let old = codec::decode(observation.id, &bytes)?;
            let name = normalize_hostname(old.hostname)?;
            let id = lookup_hostname(&scope, &name)?
                .ok_or(Error::Corrupt("observation hostname missing"))?;
            remove_support(&mut scope, old, id)?;
        }
        let hostname = assign_hostname(&mut scope, &normalized)?;
        add_support(&mut scope, observation, hostname)?;
        scope.put(OBS, &observation.id.0, &encoded)?;
        scope.release()?;
        Ok(true)
    }
    pub fn delete(txn: &mut WriteTxn<'_>, id: ObservationId) -> Result<bool> {
        let mut scope = txn.savepoint()?;
        check_schema(&scope)?;
        let Some(bytes) = scope.get(OBS, &id.0)?.map(<[u8]>::to_vec) else {
            scope.release()?;
            return Ok(false);
        };
        let observation = codec::decode(id, &bytes)?;
        let name = normalize_hostname(observation.hostname)?;
        let hostname = lookup_hostname(&scope, &name)?
            .ok_or(Error::Corrupt("observation hostname missing"))?;
        remove_support(&mut scope, observation, hostname)?;
        scope.delete(OBS, &id.0)?;
        scope.release()?;
        Ok(true)
    }
    pub fn view(read: &ReadTxn) -> Result<crate::View<'_>> {
        crate::View::new(read)
    }
    /// Profiles are opaque caller-defined claims, isolated by explicit scope.
    pub fn put_profile(
        txn: &mut WriteTxn<'_>,
        scope: ScopeId,
        prefix: Prefix,
        value: &[u8],
    ) -> Result<()> {
        let mut save = txn.savepoint()?;
        check_schema(&save)?;
        let (name, kind) = profile_name(scope, prefix.network());
        match save.longest_match(&name, prefix.network()) {
            Err(Error::MissingDatabase) => save.create_database(&name, kind)?,
            Ok(_) => {}
            Err(error) => return Err(error),
        }
        save.insert_prefix(&name, prefix, value)?;
        save.release()
    }
    pub fn delete_profile(txn: &mut WriteTxn<'_>, scope: ScopeId, prefix: Prefix) -> Result<()> {
        let mut save = txn.savepoint()?;
        check_schema(&save)?;
        let (name, _) = profile_name(scope, prefix.network());
        match save.longest_match(&name, prefix.network()) {
            Err(Error::MissingDatabase) => {}
            Ok(_) => save.delete_prefix(&name, prefix)?,
            Err(error) => return Err(error),
        }
        save.release()
    }
}
fn check_schema(txn: &WriteTxn<'_>) -> Result<()> {
    if txn.get(META, VERSION)? != Some(&1u64.to_be_bytes()) {
        return Err(Error::Corrupt("unsupported ipdon projection schema"));
    }
    Ok(())
}
fn lookup_hostname(txn: &WriteTxn<'_>, name: &str) -> Result<Option<HostnameId>> {
    let Some(bytes) = txn.get(HOSTS, name.as_bytes())? else {
        return Ok(None);
    };
    let id = HostnameId(number(bytes)?);
    if id.0 == 0 || txn.get(NAMES, &id.0.to_be_bytes())? != Some(name.as_bytes()) {
        return Err(Error::Corrupt("inconsistent hostname dictionary"));
    }
    Ok(Some(id))
}
fn assign_hostname(txn: &mut WriteTxn<'_>, name: &str) -> Result<HostnameId> {
    if let Some(id) = lookup_hostname(txn, name)? {
        return Ok(id);
    }
    let next = number(
        txn.get(META, NEXT)?
            .ok_or(Error::Corrupt("missing hostname sequence"))?,
    )?;
    let following = next.checked_add(1).ok_or(Error::LimitExceeded)?;
    if next == 0 || txn.get(NAMES, &next.to_be_bytes())?.is_some() {
        return Err(Error::Corrupt("hostname sequence collision"));
    }
    txn.put(HOSTS, name.as_bytes(), &next.to_be_bytes())?;
    txn.put(NAMES, &next.to_be_bytes(), name.as_bytes())?;
    txn.put(META, NEXT, &following.to_be_bytes())?;
    Ok(HostnameId(next))
}
fn count(txn: &WriteTxn<'_>, table: &[u8], key: &[u8]) -> Result<u64> {
    match txn.get(table, key)? {
        None => Ok(0),
        Some(bytes) => {
            let n = number(bytes)?;
            if n == 0 {
                return Err(Error::Corrupt("zero ipdon stored count"));
            }
            Ok(n)
        }
    }
}
fn adjust(txn: &mut WriteTxn<'_>, table: &[u8], key: &[u8], add: bool) -> Result<()> {
    let old = count(txn, table, key)?;
    let next = if add {
        old.checked_add(1).ok_or(Error::LimitExceeded)?
    } else {
        old.checked_sub(1)
            .ok_or(Error::Corrupt("ipdon count underflow"))?
    };
    if next == 0 {
        txn.delete(table, key)
    } else {
        txn.put(table, key, &next.to_be_bytes())
    }
}
fn evidence_key(mut edge: Vec<u8>, id: ObservationId) -> Vec<u8> {
    edge.extend_from_slice(&id.0);
    edge
}
fn add_support(txn: &mut WriteTxn<'_>, observation: &Observation, id: HostnameId) -> Result<()> {
    let f = forward(observation.scope, observation.role, id, observation.address);
    let i = inverse(observation.scope, observation.role, id, observation.address);
    let evidence = evidence_key(f.clone(), observation.id);
    let old = count(txn, FORWARD, &f)?;
    if count(txn, INVERSE, &i)? != old || txn.get(EVIDENCE, &evidence)?.is_some() {
        return Err(Error::Corrupt("inconsistent existing ipdon edge"));
    }
    let next = old.checked_add(1).ok_or(Error::LimitExceeded)?;
    txn.put(EVIDENCE, &evidence, b"")?;
    txn.put(FORWARD, &f, &next.to_be_bytes())?;
    txn.put(INVERSE, &i, &next.to_be_bytes())?;
    if old == 0 {
        adjust(
            txn,
            HOST_COUNTS,
            &host_key(observation.scope, observation.role, id),
            true,
        )?;
        adjust(
            txn,
            IP_COUNTS,
            &ip_key(observation.scope, observation.role, observation.address),
            true,
        )?;
    }
    Ok(())
}
fn remove_support(
    txn: &mut WriteTxn<'_>,
    observation: ObservationRef<'_>,
    id: HostnameId,
) -> Result<()> {
    let f = forward(observation.scope, observation.role, id, observation.address);
    let i = inverse(observation.scope, observation.role, id, observation.address);
    let evidence = evidence_key(f.clone(), observation.id);
    let old = count(txn, FORWARD, &f)?;
    if old == 0
        || count(txn, INVERSE, &i)? != old
        || txn.get(EVIDENCE, &evidence)? != Some(&b""[..])
    {
        return Err(Error::Corrupt("missing/inconsistent ipdon support"));
    }
    txn.delete(EVIDENCE, &evidence)?;
    if old == 1 {
        txn.delete(FORWARD, &f)?;
        txn.delete(INVERSE, &i)?;
        adjust(
            txn,
            HOST_COUNTS,
            &host_key(observation.scope, observation.role, id),
            false,
        )?;
        adjust(
            txn,
            IP_COUNTS,
            &ip_key(observation.scope, observation.role, observation.address),
            false,
        )?;
    } else {
        txn.put(FORWARD, &f, &(old - 1).to_be_bytes())?;
        txn.put(INVERSE, &i, &(old - 1).to_be_bytes())?;
    }
    Ok(())
}
