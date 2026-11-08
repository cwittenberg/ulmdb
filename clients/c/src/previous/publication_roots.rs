//! Physical protection for the C compound record across native automatic cleanup.
//! The native writer/outer writer lease remain held throughout this operation.
use super::*;
use ulmdb_storage::coordination::PhysicalPin;

fn same_record(left: &visibility::Record, right: &visibility::Record) -> bool {
    left.kind == right.kind
        && left.session == right.session
        && left.revision == right.revision
        && left.identity == right.identity
        && left.gate_inode == right.gate_inode
        && left.record_inode == right.record_inode
        && left.uuid == right.uuid
        && left.epoch == right.epoch
        && left.minor == right.minor
        && left.durable == right.durable
        && left.view == right.view
}

/// Only physical descriptor pins escape. Capture and Visibility are released
/// before any native commit; keeping Capture would make maintenance always Busy.
/// No data-graph admission, payload mapping or business-data copy is performed.
pub(crate) fn capture(shared: &Shared, base: &ReadTxn) -> FfiResult<Vec<PhysicalPin>> {
    if shared.external.is_some() {
        return Err(libc::ENOTSUP);
    }
    shared.check_process()?;
    let expected = base.file_checkpoint().map_err(code)?;
    let directory = begin_shared(shared)?;
    // Capture precedes Visibility. Only the fixed record is read under Visibility;
    // all descriptor IO and caller transform callbacks run after releasing it.
    let record = if let Some(visibility) = &shared.visibility {
        let lease = visibility.lease(false)?;
        Some(visibility.read(&lease)?)
    } else {
        None
    };
    let (_, capture, selected) = directory
        .capture_current()
        .map_err(|error| code(error.into()))?;
    if selected.root().checkpoint() != &expected {
        return Err(libc::EBUSY);
    }
    let durable = Meta::new(selected.root().manifest(), *selected.root().checkpoint())?;
    let authoritative = record
        .as_ref()
        .map(|r| r.matches(durable))
        .transpose()?
        .unwrap_or(false);
    // This is ordinary root discovery, not the narrower same-format copy-pair
    // contract. Native LOCK excludes publication, and Capture excludes reclaim.
    let (_, previous) = ulmdb_storage::read_roots_checked_with_transform(
        &shared.directory,
        |manifest| {
            if ulmdb_storage::checkpoint_bytes(manifest)? != expected {
                return Err(ulmdb_storage::Error::Busy);
            }
            Ok(())
        },
        shared.transform.clone(),
    )
    .map_err(|error| code(error.into()))?;
    let mut pins: Vec<PhysicalPin> = Vec::new();
    pins.try_reserve_exact(3).map_err(|_| -30792)?;
    if let Some(previous) = previous {
        let checkpoint =
            ulmdb_storage::checkpoint_bytes(&previous).map_err(|error| code(error.into()))?;
        if checkpoint != expected
            && !pins
                .iter()
                .any(|pin| pin.root().checkpoint() == &checkpoint)
        {
            pins.push(
                capture
                    .pin_checkpoint(&checkpoint)
                    .map_err(|error| code(error.into()))?,
            );
        }
    }
    if authoritative {
        let record = record.as_ref().ok_or(BAD_TXN)?;
        if record.view != expected
            && !pins
                .iter()
                .any(|pin| pin.root().checkpoint() == &record.view)
        {
            let pin = capture
                .pin_checkpoint(&record.view)
                .map_err(|error| code(error.into()))?;
            let view = pin.root().manifest();
            if record.kind != Kind::Offset
                || durable.previous != view.generation
                || view.uuid != durable.uuid
                || view.epoch != durable.epoch
                || view.format_minor != durable.minor
            {
                return Err(CORRUPT);
            }
            pins.push(pin);
        }
    }
    if let (Some(visibility), Some(before)) = (&shared.visibility, record.as_ref()) {
        let lease = visibility.lease(false)?;
        if !same_record(before, &visibility.read(&lease)?) {
            return Err(libc::EBUSY);
        }
    }
    // The already-admitted base owns D. Keeping this temporary duplicate until
    // all P/view pins are admitted proves one additional slot is available for
    // the coming candidate; release it now instead of consuming that headroom.
    drop(selected);
    // No gate survives; the registry's real live leases remain until the C
    // owner drops, after publish_record (or compound failure/abort cleanup).
    drop(capture);
    Ok(pins)
}
