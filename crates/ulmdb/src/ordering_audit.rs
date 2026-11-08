//! Full graph safety is independent of application comparison authority.
use crate::{
    DatabaseKind, Error, Result,
    catalog::Database,
    ordering::{BoundComparator, DatabaseComparators, OrderingSpec},
};
use std::ops::Bound;
use ulmdb_core::storage::{ReadStore, Reference};
use ulmdb_kv::ordering::Comparator;
fn bind(
    spec: OrderingSpec,
    callback: Option<&crate::ordering::CustomComparator>,
    structural_custom: bool,
) -> Result<Option<BoundComparator>> {
    if spec == OrderingSpec::Custom && callback.is_none() && structural_custom {
        Ok(None)
    } else {
        BoundComparator::bind(spec, callback).map(Some)
    }
}
fn tree<S: ReadStore>(
    store: &S,
    root: Reference,
    budget: u64,
    order: Option<&BoundComparator>,
) -> Result<u64> {
    Ok(match order {
        Some(order) => ulmdb_kv::audit_tree_with(store, root, budget, order)?,
        None => ulmdb_kv::audit_tree_structure(store, root, budget)?,
    }
    .pages)
}
fn kv<S: ReadStore>(
    store: &S,
    root: Reference,
    budget: u64,
    order: Option<&BoundComparator>,
    dup: bool,
    width: Option<u32>,
) -> Result<u64> {
    let pages = tree(store, root, budget, order)?;
    for pair in ulmdb_kv::Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, false)? {
        let (key, value) = pair?;
        if key.len() > if dup { 65536 } else { store.max_key_size() }
            || value.len() > store.max_value_size()
            || (dup && (!value.is_empty() || width.is_some_and(|w| key.len() != w as usize)))
        {
            return Err(Error::Corrupt("ordered KV size/duplicate width"));
        }
        if let Some(order) = order {
            order
                .validate(key)
                .map_err(|_| Error::Corrupt("stored ordering width"))?;
            if ulmdb_kv::get_with(store, root, key, order)? != Some(value) {
                return Err(Error::Corrupt("ordered KV routing"));
            }
        }
    }
    Ok(pages)
}
pub(crate) fn database<S: ReadStore>(
    store: &S,
    db: &Database,
    budget: u64,
    callbacks: &DatabaseComparators,
    structural_custom: bool,
) -> Result<u64> {
    let keys = bind(db.ordering.keys, callbacks.keys.as_ref(), structural_custom)?;
    let values = bind(
        db.ordering.duplicates,
        callbacks.duplicates.as_ref(),
        structural_custom,
    )?;
    if db.kind == DatabaseKind::Kv {
        return kv(store, db.root, budget, keys.as_ref(), false, None);
    }
    let DatabaseKind::Duplicates { width } = db.kind else {
        return Err(Error::WrongKind);
    };
    let mut pages = tree(store, db.root, budget, keys.as_ref())?;
    for pair in ulmdb_kv::Cursor::new(store, db.root, Bound::Unbounded, Bound::Unbounded, false)? {
        let (key, bytes) = pair?;
        if key.len() > store.max_key_size() {
            return Err(Error::Corrupt("duplicate key too large"));
        }
        if let Some(keys) = &keys {
            keys.validate(key)
                .map_err(|_| Error::Corrupt("stored key width"))?;
            if ulmdb_kv::get_with(store, db.root, key, keys)? != Some(bytes) {
                return Err(Error::Corrupt("duplicate key routing"));
            }
        }
        match crate::duplicates::bucket::Bucket::decode_with(
            bytes,
            width,
            values.as_ref().map(|v| v as &dyn Comparator),
        )? {
            crate::duplicates::bucket::Bucket::Inline(_) => {}
            crate::duplicates::bucket::Bucket::Tree {
                root,
                count: expected,
            } => {
                pages = pages.max(kv(store, root, budget, values.as_ref(), true, width)?);
                let mut count = 0u64;
                for pair in
                    ulmdb_kv::Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, false)?
                {
                    pair?;
                    count = count.checked_add(1).ok_or(Error::LimitExceeded)?;
                }
                if count != expected {
                    return Err(Error::Corrupt("duplicate count mismatch"));
                }
            }
        }
    }
    Ok(pages)
}
