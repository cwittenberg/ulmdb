//! Checkpoint-bound replication pages. Digests establish integrity and common
//! history; applications still authenticate peers and transport metadata.
use super::{
    ApplyMode, ApplyReport, Cursor, Event, Limits, ProtocolError, ProtocolResult, Versions,
    baseline, store, wire,
};
use ulmdb::{Error, ReadTxn, Result, WriteTxn};

/// A cumulative native origin-chain position, including a retained baseline or
/// prune-floor anchor. The all-zero digest denotes an ordinary empty origin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OriginCheckpoint {
    pub origin: u64,
    pub sequence: u64,
    pub digest: [u8; 32],
}

/// Portable page metadata and events from one pinned source snapshot.
///
/// Fields are public so an authenticated application transport can encode them.
/// Receiving code must call [`apply_verified_page`], which checks every field
/// and recomputes native event hashes before invoking application callbacks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedPage {
    pub realm: u128,
    pub versions: Versions,
    /// Imported-baseline identity is a history-namespace fence, including for
    /// origins that had no positive sequence at the baseline.
    pub baseline_id: Option<[u8; 32]>,
    pub base: OriginCheckpoint,
    pub tail: OriginCheckpoint,
    /// Complete source position at capture, which may be ahead of this page.
    pub cursor: Cursor,
    pub events: Vec<Event>,
    /// Persisted native encodings, including identity/digest/chain overhead.
    pub encoded_bytes: usize,
}
impl VerifiedPage {
    pub fn more(&self) -> bool {
        self.tail.sequence < self.cursor.applied
    }
}

/// Obtain a public cumulative checkpoint without exposing native storage tables.
/// Any position below retention, including genesis after an anchored import,
/// explicitly requires a recovery snapshot.
pub fn checkpoint(read: &ReadTxn, origin: u64, sequence: u64) -> ProtocolResult<OriginCheckpoint> {
    let cursor = store::cursor(read, origin)?;
    check_position(origin, sequence, cursor)?;
    Ok(OriginCheckpoint {
        origin,
        sequence,
        digest: store::chain_at(read, origin, sequence, cursor)?,
    })
}

/// Exact charge used by journal apply/page/prune limits. Callers should use this
/// helper rather than depending on a private encoding-overhead constant.
pub fn encoded_len(event: &Event) -> Result<usize> {
    wire::encoded_len(event)
}

/// Read a bounded page plus its common-history base and returned tail anchors.
/// Everything, including realm, versions, baseline and cursor, borrows one read
/// transaction's immutable source state.
pub fn verified_page(
    read: &ReadTxn,
    origin: u64,
    after: u64,
    limits: Limits,
) -> ProtocolResult<VerifiedPage> {
    let base = checkpoint(read, origin, after)?;
    let page = store::page(read, origin, after, limits)?;
    let last = page.events.last().map_or(after, |event| event.sequence);
    Ok(VerifiedPage {
        realm: store::realm(read)?,
        versions: store::versions(read)?,
        baseline_id: baseline::identity(read)?,
        base,
        tail: checkpoint(read, origin, last)?,
        cursor: page.cursor,
        events: page.events,
        encoded_bytes: page.encoded_bytes,
    })
}

fn check_position(origin: u64, sequence: u64, cursor: Cursor) -> ProtocolResult<()> {
    if sequence < cursor.floor {
        return Err(ProtocolError::SnapshotRequired {
            origin,
            requested: sequence,
            floor: cursor.floor,
        });
    }
    if sequence > cursor.applied {
        return Err(ProtocolError::InvalidPage("checkpoint is ahead of origin"));
    }
    Ok(())
}

/// Verify history and an entire bounded page before any application callback.
///
/// The destination can be at the base, within the page, or beyond its tail.
/// Shared base and overlap checkpoints must agree, so exact/partial replay is
/// safe and divergent earlier histories cannot hide behind equal numeric cursors.
/// Any error restores the parent's writer, including successful earlier callbacks.
pub fn apply_verified_page<F>(
    txn: &mut WriteTxn<'_>,
    page: &VerifiedPage,
    limits: Limits,
    mode: ApplyMode,
    apply: F,
) -> ProtocolResult<ApplyReport>
where
    F: FnMut(&mut WriteTxn<'_>, &Event) -> Result<()>,
{
    wire::limit(limits)?;
    if page.events.len() > limits.max_events {
        return Err(Error::LimitExceeded.into());
    }
    let encoded_bytes = page.events.iter().try_fold(0usize, |total, event| {
        total
            .checked_add(encoded_len(event)?)
            .ok_or(Error::LimitExceeded)
    })?;
    if encoded_bytes > limits.max_bytes {
        return Err(Error::LimitExceeded.into());
    }
    if encoded_bytes != page.encoded_bytes {
        return Err(ProtocolError::InvalidPage(
            "encoded page byte count differs",
        ));
    }
    if page.base.origin == 0
        || page.base.origin != page.tail.origin
        || page.cursor.floor > page.cursor.serving
        || page.cursor.serving > page.cursor.applied
        || page.base.sequence < page.cursor.floor
        || page.base.sequence > page.tail.sequence
        || page.tail.sequence > page.cursor.applied
    {
        return Err(ProtocolError::InvalidPage(
            "invalid verified-page cursor bounds",
        ));
    }
    if page.events.is_empty() && page.base.sequence != page.cursor.applied {
        return Err(ProtocolError::InvalidPage(
            "empty page omits an available tail",
        ));
    }

    let mut save = txn.savepoint()?;
    let expected_realm = store::realm_write(&save)?;
    if expected_realm != page.realm {
        return Err(ProtocolError::ForeignRealm {
            expected: expected_realm,
            actual: page.realm,
        });
    }
    let expected_versions = wire::decode_versions(
        save.get(wire::META, wire::VERSIONS)?
            .ok_or(Error::Corrupt("missing journal versions"))?,
    )?;
    if expected_versions != page.versions {
        return Err(ProtocolError::VersionMismatch {
            expected: expected_versions,
            actual: page.versions,
        });
    }
    if baseline::write_identity(&save)? != page.baseline_id {
        return Err(ProtocolError::BaselineMismatch(
            "verified page baseline differs",
        ));
    }
    let destination = store::write_cursor(&save, page.base.origin)?;
    check_position(page.base.origin, page.base.sequence, destination)?;
    let common_base =
        store::chain_at_write(&save, page.base.origin, page.base.sequence, destination)?;
    if common_base != page.base.digest {
        return Err(ProtocolError::Fork {
            origin: page.base.origin,
            sequence: page.base.sequence,
        });
    }

    // Recompute the complete returned chain before mutation. Comparing the
    // overlap as well as the base catches divergent events inside a replayed page.
    let overlap_sequence = destination.applied.min(page.tail.sequence);
    let mut overlap_digest = page.base.digest;
    let mut digest = page.base.digest;
    let mut sequence = page.base.sequence;
    for event in &page.events {
        sequence = sequence
            .checked_add(1)
            .ok_or(ProtocolError::InvalidPage("page sequence overflows"))?;
        if event.origin != page.base.origin || event.sequence != sequence {
            return Err(ProtocolError::InvalidPage(
                "events do not form one contiguous origin",
            ));
        }
        let encoded = wire::encode_record(event, &digest)?;
        digest.copy_from_slice(&encoded[encoded.len() - 32..]);
        if sequence == overlap_sequence {
            overlap_digest = digest;
        }
    }
    if sequence != page.tail.sequence || digest != page.tail.digest {
        return Err(ProtocolError::InvalidPage(
            "verified page tail digest differs",
        ));
    }
    let local_overlap =
        store::chain_at_write(&save, page.base.origin, overlap_sequence, destination)?;
    if local_overlap != overlap_digest {
        return Err(ProtocolError::Fork {
            origin: page.base.origin,
            sequence: overlap_sequence,
        });
    }
    let report = store::apply_page(&mut save, &page.events, limits, mode, apply)?;
    // Callbacks own application projections, not this transport namespace.
    // Reject an accidental context migration before releasing the outer scope.
    if store::realm_write(&save)? != expected_realm
        || wire::decode_versions(
            save.get(wire::META, wire::VERSIONS)?
                .ok_or(Error::Corrupt("journal versions disappeared during apply"))?,
        )? != expected_versions
        || baseline::write_identity(&save)? != page.baseline_id
    {
        return Err(ProtocolError::InvalidPage(
            "application callback changed the replication context",
        ));
    }
    save.release()?;
    Ok(report)
}
