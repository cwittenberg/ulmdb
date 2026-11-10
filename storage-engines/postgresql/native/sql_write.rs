//! Optional ordinary named-KV NOOVERWRITE path. The common put entry has
//! already copied/checked inputs, finished reserves and refreshed the binding.
use crate::*;

fn eligible(
    txn: &MDB_txn,
    handle: &ResolvedAlias,
    settings: compat_features::Settings,
) -> FfiResult<bool> {
    if handle.name() == MAIN
        || handle.kind() != DatabaseKind::Kv
        || settings.flags != 0
        || !matches!(txn.state.as_ref(), Some(State::Write(_)))
        || txn.shared.transform.is_some()
        || txn.shared.external.is_some()
        || !txn.pending_reserves.is_empty()
    {
        return Ok(false);
    }
    match &handle.binding.view {
        DataView::Direct => {}
        DataView::Writable(view) if view.handle() == handle.actual() => {}
        _ => return Ok(false),
    }
    {
        let options = txn.shared.options.lock().map_err(|_| BAD_TXN)?;
        if options
            .get(&compat_features::callback_key(handle))
            .is_some_and(|options| {
                options.key_cmp.is_some()
                    || options.dup_cmp.is_some()
                    || options.native_key.is_some()
                    || options.native_dup.is_some()
                    || options.relfunc != 0
            })
        {
            return Ok(false);
        }
    }
    {
        let state = txn.dbis.lock().map_err(|_| BAD_TXN)?;
        if state.detached.contains_key(&handle.native_id())
            || state.retained.contains_key(&handle.native_id())
            || state.hidden.contains(&handle.native_id())
        {
            return Ok(false);
        }
    }
    let order = txn.actual_native_order(handle)?;
    Ok(order.keys == ulmdb::ordering::OrderingSpec::Bytewise
        && order.duplicates == ulmdb::ordering::OrderingSpec::Bytewise)
}

/// False means no mutation/output change and the original path must run.
/// An eligible duplicate retains its old value and returns recoverable KEYEXIST.
pub(crate) fn try_nooverwrite(
    txn: &mut MDB_txn,
    handle: &ResolvedAlias,
    key: &[u8],
    data: &[u8],
    out: *mut MDB_val,
    settings: compat_features::Settings,
) -> FfiResult<bool> {
    if !eligible(txn, handle, settings)? {
        return Ok(false);
    }
    #[cfg(test)]
    LOOKUPS.with(|count| count.set(count.get() + 1));
    if let Some(previous) = txn.lookup(handle, key)? {
        let previous = previous.to_vec();
        txn.retain_pair_output(out, previous)?;
        return Err(KEYEXIST);
    }
    txn.writable_database(handle)?;
    txn.quota_adjust(key.len() + data.len() + 16, 0)?;
    // Flags-zero settings have no integer/fixed widths to learn. Retain the
    // actual mutation/capability/value-limit/failure and cursor revision path.
    let ordered = txn.configure_native_order(handle)?;
    txn.put_alias(&ordered, key, data)?;
    txn.mark_database_dirty(handle)?;
    txn.changed();
    Ok(true)
}

#[cfg(test)]
std::thread_local! { static LOOKUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
#[path = "sql_write_tests.rs"]
mod tests;
