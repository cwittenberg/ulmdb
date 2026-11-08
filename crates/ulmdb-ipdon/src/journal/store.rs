use super::{
    ApplyMode, ApplyReport, Cursor, Event, Limits, Page, PeerAck, PruneReport, SnapshotReceipt,
    Versions, wire::*,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Bound,
};
use ulmdb::{DatabaseKind, Error, ReadTxn, Result, WriteTxn};

pub fn initialize(txn: &mut WriteTxn<'_>) -> Result<()> {
    initialize_inner(txn, None)
}

/// Independently initialized replicas of one logical dataset must configure the
/// same nonzero realm. Existing environments cannot silently change namespace.
pub fn initialize_with_realm(txn: &mut WriteTxn<'_>, realm: u128) -> Result<()> {
    if realm == 0 {
        return Err(Error::InvalidInput("zero journal realm"));
    }
    initialize_inner(txn, Some(realm))
}

fn initialize_inner(txn: &mut WriteTxn<'_>, expected_realm: Option<u128>) -> Result<()> {
    let mut save = txn.savepoint()?;
    match save.get(META, FORMAT) {
        Err(Error::MissingDatabase) => {
            for table in [META, EVENTS, CURSORS, HEADS, FLOORS] {
                save.create_database(table, DatabaseKind::Kv)?;
            }
            save.put(META, FORMAT, &[1])?;
            let realm = match expected_realm {
                Some(realm) => realm,
                None => ulmdb_storage::new_identity()?,
            };
            save.put(META, REALM, &realm.to_be_bytes())?;
            save.put(META, VERSIONS, &encode_versions(Versions::default())?)?;
            save.put(META, b"origin_count", &0u64.to_be_bytes())?;
        }
        Ok(Some([1])) => {
            let realm = realm_write(&save)?;
            if expected_realm.is_some_and(|expected| expected != realm) {
                return Err(Error::InvalidInput("journal realm cannot change"));
            }
            decode_versions(
                save.get(META, VERSIONS)?
                    .ok_or(Error::Corrupt("missing journal versions"))?,
            )?;
            for table in [EVENTS, CURSORS, HEADS, FLOORS] {
                save.get(table, b"")?;
            }
        }
        Ok(_) => return Err(Error::Corrupt("unsupported journal format")),
        Err(e) => return Err(e),
    }
    save.release()
}

pub(super) fn realm_write(txn: &WriteTxn<'_>) -> Result<u128> {
    let b = txn
        .get(META, REALM)?
        .ok_or(Error::Corrupt("missing journal realm"))?;
    let realm = u128::from_be_bytes(
        b.try_into()
            .map_err(|_| Error::Corrupt("journal realm length"))?,
    );
    if realm == 0 {
        return Err(Error::Corrupt("zero journal realm"));
    }
    Ok(realm)
}
pub fn realm(read: &ReadTxn) -> Result<u128> {
    if read.get(META, FORMAT)? != Some(&[1][..]) {
        return Err(Error::Corrupt("unsupported journal format"));
    }
    let b = read
        .get(META, REALM)?
        .ok_or(Error::Corrupt("missing journal realm"))?;
    let realm = u128::from_be_bytes(
        b.try_into()
            .map_err(|_| Error::Corrupt("journal realm length"))?,
    );
    if realm == 0 {
        return Err(Error::Corrupt("zero journal realm"));
    }
    Ok(realm)
}

pub fn versions(read: &ReadTxn) -> Result<Versions> {
    decode_versions(
        read.get(META, VERSIONS)?
            .ok_or(Error::Corrupt("missing journal versions"))?,
    )
}

/// Persist application codec/resolution-policy versions in the same transaction
/// as the corresponding projection migration. Existing event bytes stay immutable.
pub fn set_versions(txn: &mut WriteTxn<'_>, value: Versions) -> Result<()> {
    realm_write(txn)?;
    let encoded = encode_versions(value)?;
    txn.put(META, VERSIONS, &encoded)
}

pub(super) fn write_cursor(txn: &WriteTxn<'_>, origin: u64) -> Result<Cursor> {
    if origin == 0 {
        return Err(Error::InvalidInput("zero journal origin"));
    }
    txn.get(CURSORS, &origin.to_be_bytes())?
        .map(decode_cursor)
        .transpose()
        .map(|v| v.unwrap_or_default())
}
fn hash(bytes: Option<&[u8]>) -> Result<[u8; 32]> {
    bytes
        .ok_or(Error::Corrupt("missing origin chain"))?
        .try_into()
        .map_err(|_| Error::Corrupt("origin chain length"))
}
pub(super) fn chain_at(read: &ReadTxn, origin: u64, sequence: u64, c: Cursor) -> Result<[u8; 32]> {
    if sequence == 0 {
        return Ok([0; 32]);
    }
    if sequence == c.floor {
        return hash(read.get(FLOORS, &origin.to_be_bytes())?);
    }
    if sequence < c.floor || sequence > c.applied {
        return Err(Error::InvalidInput("origin chain outside retained history"));
    }
    let bytes = read
        .get(EVENTS, &key(origin, sequence))?
        .ok_or(Error::Corrupt("missing retained origin chain"))?;
    let (event, digest) = decode_record(bytes)?;
    if event.origin != origin || event.sequence != sequence {
        return Err(Error::Corrupt("origin chain identity"));
    }
    Ok(digest)
}
pub(super) fn chain_at_write(txn: &WriteTxn<'_>, origin: u64, sequence: u64, c: Cursor) -> Result<[u8; 32]> {
    if sequence == 0 {
        return Ok([0; 32]);
    }
    if sequence == c.floor {
        return hash(txn.get(FLOORS, &origin.to_be_bytes())?);
    }
    if sequence < c.floor || sequence > c.applied {
        return Err(Error::InvalidInput("origin chain outside retained history"));
    }
    let bytes = txn
        .get(EVENTS, &key(origin, sequence))?
        .ok_or(Error::Corrupt("missing retained origin chain"))?;
    let (event, digest) = decode_record(bytes)?;
    if event.origin != origin || event.sequence != sequence {
        return Err(Error::Corrupt("origin chain identity"));
    }
    Ok(digest)
}
pub fn cursor(read: &ReadTxn, origin: u64) -> Result<Cursor> {
    if origin == 0 {
        return Err(Error::InvalidInput("zero journal origin"));
    }
    realm(read)?;
    read.get(CURSORS, &origin.to_be_bytes())?
        .map(decode_cursor)
        .transpose()
        .map(|v| v.unwrap_or_default())
}
pub fn cursors(read: &ReadTxn) -> Result<BTreeMap<u64, Cursor>> {
    realm(read)?;
    let mut result = BTreeMap::new();
    for item in read.range(CURSORS, Bound::Unbounded, Bound::Unbounded, false)? {
        let (key, bytes) = item?;
        let origin = number(key)?;
        if origin == 0 || result.len() >= MAX_ORIGINS {
            return Err(Error::Corrupt("journal origin limit/identity"));
        }
        result.insert(origin, decode_cursor(bytes)?);
    }
    Ok(result)
}

/// Apply every new event and its cursor atomically. Exact replays are compared
/// byte-for-byte and do not invoke the callback. An error restores the parent
/// writer to entry state, including changes made by successful earlier callbacks.
pub fn apply_page<F>(
    txn: &mut WriteTxn<'_>,
    events: &[Event],
    limits: Limits,
    mode: ApplyMode,
    mut apply: F,
) -> Result<ApplyReport>
where
    F: FnMut(&mut WriteTxn<'_>, &Event) -> Result<()>,
{
    limit(limits)?;
    if events.len() > limits.max_events {
        return Err(Error::LimitExceeded);
    }
    let bytes = events.iter().try_fold(0usize, |n, e| {
        n.checked_add(encoded_len(e)?).ok_or(Error::LimitExceeded)
    })?;
    if bytes > limits.max_bytes {
        return Err(Error::LimitExceeded);
    }
    let mut save = txn.savepoint()?;
    realm_write(&save)?;
    let mut report = ApplyReport {
        encoded_bytes: bytes,
        ..ApplyReport::default()
    };
    for event in events {
        let mut c = write_cursor(&save, event.origin)?;
        let k = key(event.origin, event.sequence);
        if event.sequence <= c.floor {
            return Err(Error::InvalidInput(
                "journal replay predates retention floor; snapshot required",
            ));
        }
        if event.sequence <= c.applied {
            let previous = chain_at_write(&save, event.origin, event.sequence - 1, c)?;
            let encoded = encode_record(event, &previous)?;
            if save.get(EVENTS, &k)? != Some(encoded.as_slice()) {
                return Err(Error::InvalidInput("conflicting journal replay"));
            }
            report.replayed += 1;
            continue;
        }
        if c.applied.checked_add(1) != Some(event.sequence) {
            return Err(Error::InvalidInput("journal sequence gap"));
        }
        if mode == ApplyMode::Synchronous && c.serving != c.applied {
            return Err(Error::InvalidInput(
                "serving backlog requires explicit materialization",
            ));
        }
        if c.applied == 0 {
            let count = number(
                save.get(META, b"origin_count")?
                    .ok_or(Error::Corrupt("missing journal origin count"))?,
            )?;
            if count >= MAX_ORIGINS as u64 {
                return Err(Error::LimitExceeded);
            }
            save.put(META, b"origin_count", &(count + 1).to_be_bytes())?;
        }
        let previous = if c.applied == 0 {
            [0; 32]
        } else {
            hash(save.get(HEADS, &event.origin.to_be_bytes())?)?
        };
        let encoded = encode_record(event, &previous)?;
        apply(&mut save, event)?;
        save.put(EVENTS, &k, &encoded)?;
        save.put(
            HEADS,
            &event.origin.to_be_bytes(),
            &encoded[encoded.len() - 32..],
        )?;
        c.applied = event.sequence;
        if mode == ApplyMode::Synchronous {
            c.serving = event.sequence;
        }
        save.put(CURSORS, &event.origin.to_be_bytes(), &encode_cursor(c))?;
        report.applied += 1;
    }
    save.release()?;
    Ok(report)
}

/// Caller must complete the corresponding projection changes before advancing
/// this watermark in the same transaction. Regressions or unapplied positions fail.
pub fn mark_serving(txn: &mut WriteTxn<'_>, origin: u64, through: u64) -> Result<()> {
    let mut c = write_cursor(txn, origin)?;
    if through < c.serving || through > c.applied {
        return Err(Error::InvalidInput("invalid serving watermark"));
    }
    if through == c.serving {
        return Ok(());
    }
    c.serving = through;
    txn.put(CURSORS, &origin.to_be_bytes(), &encode_cursor(c))
}

pub fn page(read: &ReadTxn, origin: u64, after: u64, limits: Limits) -> Result<Page> {
    limit(limits)?;
    let c = cursor(read, origin)?;
    if after < c.floor {
        return Err(Error::InvalidInput(
            "journal cursor predates retention floor; snapshot required",
        ));
    }
    if after > c.applied {
        return Err(Error::InvalidInput("journal cursor ahead of origin"));
    }
    let mut result = Page {
        events: Vec::new(),
        cursor: c,
        more: after < c.applied,
        encoded_bytes: 0,
    };
    if after == c.applied {
        return Ok(result);
    }
    let lower = key(origin, after + 1);
    let upper = key(origin, c.applied);
    let mut next = after + 1;
    let mut previous = chain_at(read, origin, after, c)?;
    for item in read.range(
        EVENTS,
        Bound::Included(&lower),
        Bound::Included(&upper),
        false,
    )? {
        let (k, bytes) = item?;
        if result.events.len() == limits.max_events {
            break;
        }
        if k != key(origin, next) {
            return Err(Error::Corrupt("journal gap below applied cursor"));
        }
        let total = result
            .encoded_bytes
            .checked_add(bytes.len())
            .ok_or(Error::LimitExceeded)?;
        if total > limits.max_bytes {
            if result.events.is_empty() {
                return Err(Error::LimitExceeded);
            }
            break;
        }
        let (event, digest) = decode_record(bytes)?;
        if event.origin != origin || event.sequence != next {
            return Err(Error::Corrupt("journal key/event disagreement"));
        }
        if chain(&previous, &bytes[..bytes.len() - 32]) != digest {
            return Err(Error::Corrupt("journal origin chain disagreement"));
        }
        previous = digest;
        result.encoded_bytes = total;
        result.events.push(event);
        if next == c.applied {
            result.more = false;
            break;
        }
        next += 1;
    }
    if result.events.is_empty() {
        return Err(Error::Corrupt("missing retained journal tail"));
    }
    Ok(result)
}

/// Remove at most one bounded page after an independently verified exported
/// recovery snapshot covers `through` and enough distinct peers acknowledge it.
/// Acknowledgements are caller-authenticated durable positions. Only serving
/// events can be pruned. Native compaction/reclamation remains a separate policy.
pub fn prune(
    txn: &mut WriteTxn<'_>,
    recovery: &SnapshotReceipt,
    origin: u64,
    through: u64,
    limits: Limits,
    acknowledgements: &[PeerAck],
    required_peers: usize,
) -> Result<PruneReport> {
    limit(limits)?;
    let mut save = txn.savepoint()?;
    if realm_write(&save)? != recovery.realm {
        return Err(Error::InvalidInput("foreign recovery snapshot lineage"));
    }
    let mut c = write_cursor(&save, origin)?;
    let coverage = recovery.cursors.get(&origin).copied().unwrap_or_default();
    if through > coverage.applied || through > c.serving || coverage.applied > c.applied {
        return Err(Error::InvalidInput(
            "prune exceeds recovery or serving coverage",
        ));
    }
    let current_versions = decode_versions(
        save.get(META, VERSIONS)?
            .ok_or(Error::Corrupt("missing journal versions"))?,
    )?;
    if current_versions != recovery.versions() {
        return Err(Error::InvalidInput(
            "recovery snapshot application versions differ",
        ));
    }
    if through > c.floor
        && recovery.heads.get(&origin).copied()
            != Some(chain_at_write(&save, origin, coverage.applied, c)?)
    {
        return Err(Error::InvalidInput(
            "recovery snapshot origin lineage differs",
        ));
    }
    let peers = acknowledgements
        .iter()
        .filter(|a| a.peer != 0 && a.origin == origin && a.sequence >= through)
        .map(|a| a.peer)
        .collect::<BTreeSet<_>>();
    if peers.len() < required_peers {
        return Err(Error::InvalidInput(
            "insufficient durable peer acknowledgements",
        ));
    }
    let mut report = PruneReport {
        floor: c.floor,
        ..PruneReport::default()
    };
    let mut previous = chain_at_write(&save, origin, c.floor, c)?;
    while c.floor < through && report.removed < limits.max_events {
        let sequence = c.floor + 1;
        let k = key(origin, sequence);
        let bytes = save
            .get(EVENTS, &k)?
            .ok_or(Error::Corrupt("missing event during journal prune"))?;
        let (event, digest) = decode_record(bytes)?;
        if event.origin != origin || event.sequence != sequence {
            return Err(Error::Corrupt("prune event identity"));
        }
        if chain(&previous, &bytes[..bytes.len() - 32]) != digest {
            return Err(Error::Corrupt("prune origin chain disagreement"));
        }
        let total = report
            .encoded_bytes
            .checked_add(bytes.len())
            .ok_or(Error::LimitExceeded)?;
        if total > limits.max_bytes {
            if report.removed == 0 {
                return Err(Error::LimitExceeded);
            }
            break;
        }
        save.delete(EVENTS, &k)?;
        report.encoded_bytes = total;
        report.removed += 1;
        c.floor = sequence;
        previous = digest;
    }
    if c.floor != report.floor {
        save.put(CURSORS, &origin.to_be_bytes(), &encode_cursor(c))?;
        save.put(FLOORS, &origin.to_be_bytes(), &previous)?;
    }
    report.floor = c.floor;
    save.release()?;
    Ok(report)
}
