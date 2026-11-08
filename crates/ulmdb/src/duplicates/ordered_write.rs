//! Comparator-aware inline/subtree duplicate mutation; no pair materialization.
use super::{
    Mutation,
    bucket::{Bucket, INLINE_BYTES, INLINE_COUNT, InlineBuffer, OwnedBucket, tree_bytes},
    check_key, check_value, validate_stored, width,
};
use crate::{Error, Result};
use std::ops::Bound;
use ulmdb_core::storage::{ReadStore, Reference, WriteStore};
use ulmdb_kv::ordering::Comparator;
fn insert<S: WriteStore, K: Comparator, V: Comparator + Clone>(
    store: &mut S,
    parent: Reference,
    key: &[u8],
    value: &[u8],
    width: Option<u32>,
    keys: &K,
    values: &V,
) -> Result<(Reference, u64)> {
    let bucket = Bucket::get_with(store, parent, key, width, keys, values)?;
    if let Some(bucket) = bucket
        && bucket.seek_with(store, value, values)?.is_some()
    {
        return Ok((parent, 0));
    }
    let count = bucket
        .map_or(0, Bucket::count)
        .checked_add(1)
        .ok_or(Error::LimitExceeded)?;
    match OwnedBucket::copy(bucket) {
        OwnedBucket::Tree { root, .. } => {
            let root = ulmdb_kv::put_with(store, root, value, b"", values)?;
            Ok((
                ulmdb_kv::put_with(store, parent, key, &tree_bytes(root, count), keys)?,
                1,
            ))
        }
        owned => {
            let source = match owned {
                OwnedBucket::Inline(source) => source,
                _ => InlineBuffer::new(),
            };
            let inline = source.decode_with(width, values)?;
            if inline.count < INLINE_COUNT && inline.bytes.len() + 4 + value.len() <= INLINE_BYTES {
                let position = inline.search_with(value, false, values);
                let mut next = InlineBuffer::new();
                for index in 0..=inline.count {
                    if index == position && !next.push(value) {
                        return Err(Error::Corrupt("inline duplicate builder limit"));
                    }
                    if index < inline.count && !next.push(inline.value(index)) {
                        return Err(Error::Corrupt("inline duplicate builder limit"));
                    }
                }
                Ok((
                    ulmdb_kv::put_with(store, parent, key, next.as_bytes(), keys)?,
                    1,
                ))
            } else {
                let mut child = Reference::NULL;
                for index in 0..inline.count {
                    child = ulmdb_kv::put_with(store, child, inline.value(index), b"", values)?;
                }
                child = ulmdb_kv::put_with(store, child, value, b"", values)?;
                Ok((
                    ulmdb_kv::put_with(store, parent, key, &tree_bytes(child, count), keys)?,
                    1,
                ))
            }
        }
    }
}

fn try_inline<S: ReadStore, V: Comparator + Clone>(
    store: &S,
    root: Reference,
    count: u64,
    width: Option<u32>,
    values: &V,
) -> Result<Option<InlineBuffer>> {
    if count > INLINE_COUNT as u64 {
        return Ok(None);
    }
    let mut next = InlineBuffer::new();
    let mut seen = 0;
    for pair in ulmdb_kv::Cursor::with_order(
        store,
        root,
        Bound::Unbounded,
        Bound::Unbounded,
        false,
        values.clone(),
    )? {
        let (value, empty) = pair?;
        if !empty.is_empty() {
            return Err(Error::Corrupt("duplicate child value must be empty"));
        }
        validate_stored(value, width)?;
        if !next.push(value) {
            return Ok(None);
        }
        seen += 1;
    }
    if seen != count {
        return Err(Error::Corrupt("duplicate count mismatch during demotion"));
    }
    Ok(Some(next))
}

fn delete<S: WriteStore, K: Comparator, V: Comparator + Clone>(
    store: &mut S,
    parent: Reference,
    key: &[u8],
    value: &[u8],
    width: Option<u32>,
    keys: &K,
    values: &V,
) -> Result<(Reference, u64)> {
    let Some(bucket) = Bucket::get_with(store, parent, key, width, keys, values)? else {
        return Ok((parent, 0));
    };
    if bucket.seek_with(store, value, values)?.is_none() {
        return Ok((parent, 0));
    }
    if bucket.count() == 1 {
        return Ok((ulmdb_kv::delete_with(store, parent, key, keys)?, 1));
    }
    match OwnedBucket::copy(Some(bucket)) {
        OwnedBucket::Inline(source) => {
            let inline = source.decode_with(width, values)?;
            let position = inline.search_with(value, false, values);
            let mut next = InlineBuffer::new();
            for index in 0..inline.count {
                if index != position && !next.push(inline.value(index)) {
                    return Err(Error::Corrupt("inline duplicate builder limit"));
                }
            }
            Ok((
                ulmdb_kv::put_with(store, parent, key, next.as_bytes(), keys)?,
                1,
            ))
        }
        OwnedBucket::Tree { root, count } => {
            let root = ulmdb_kv::delete_with(store, root, value, values)?;
            let count = count - 1;
            let root = if let Some(inline) = try_inline(store, root, count, width, values)? {
                ulmdb_kv::put_with(store, parent, key, inline.as_bytes(), keys)?
            } else {
                ulmdb_kv::put_with(store, parent, key, &tree_bytes(root, count), keys)?
            };
            Ok((root, 1))
        }
        OwnedBucket::Missing => Err(Error::Corrupt("missing duplicate bucket during deletion")),
    }
}

pub(crate) fn mutate_root_with<S: WriteStore, K: Comparator, V: Comparator + Clone>(
    store: &mut S,
    old_root: Reference,
    kind: crate::DatabaseKind,
    key: &[u8],
    mutation: Mutation<'_>,
    keys: &K,
    values: &V,
) -> Result<(Reference, u64)> {
    if keys.is_bytewise() && values.is_bytewise() {
        return super::write::mutate_root(store, old_root, kind, key, mutation);
    }
    check_key(store, key)?;
    keys.validate(key)?;
    let width = width(kind)?;
    match mutation {
        Mutation::Insert(value) => {
            check_value(value, width)?;
            values.validate(value)?;
            insert(store, old_root, key, value, width, keys, values)
        }
        Mutation::Delete(value) => {
            check_value(value, width)?;
            values.validate(value)?;
            delete(store, old_root, key, value, width, keys, values)
        }
        Mutation::DeleteAll => match Bucket::get_with(store, old_root, key, width, keys, values)? {
            None => Ok((old_root, 0)),
            Some(bucket) => {
                let count = bucket.count();
                Ok((ulmdb_kv::delete_with(store, old_root, key, keys)?, count))
            }
        },
    }
}
