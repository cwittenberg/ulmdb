//! Streaming relocation of duplicate buckets to an independent object store.
use super::{
    bucket::{Bucket, Inline, InlineBuffer, tree_bytes},
    validate_stored,
};
use crate::{Error, Result};
use std::ops::Bound;
use ulmdb_core::storage::{ReadStore, Reference, WriteStore};

enum Values<'a, S: ReadStore> {
    Inline { bucket: Inline<'a>, next: usize },
    Tree(ulmdb_kv::Cursor<'a, S>),
}
impl<'a, S: ReadStore> Values<'a, S> {
    fn new(store: &'a S, bucket: Bucket<'a>) -> Result<Self> {
        Ok(match bucket {
            Bucket::Inline(bucket) => Self::Inline { bucket, next: 0 },
            Bucket::Tree { root, .. } => Self::Tree(ulmdb_kv::Cursor::new(
                store,
                root,
                Bound::Unbounded,
                Bound::Unbounded,
                false,
            )?),
        })
    }
}
impl<'a, S: ReadStore> Iterator for Values<'a, S> {
    type Item = Result<&'a [u8]>;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Inline { bucket, next } => {
                if *next == bucket.count {
                    return None;
                }
                let index = *next;
                *next += 1;
                Some(Ok(bucket.value(index)))
            }
            Self::Tree(cursor) => cursor.next().map(|pair| {
                pair.and_then(|(value, empty)| {
                    if !empty.is_empty() {
                        return Err(Error::Corrupt("duplicate child value must be empty"));
                    }
                    Ok(value)
                })
            }),
        }
    }
}

/// Rebuild every reachable bucket without retaining any source object reference.
/// Buffers the bounded inline candidate, cursor paths and direct KV frontiers.
pub(crate) fn rebuild<S: ReadStore, T: WriteStore>(
    source: &S,
    root: Reference,
    target: &mut T,
    width: Option<u32>,
) -> Result<(Reference, u64)> {
    let mut result = ulmdb_kv::InOrderBuilder::new();
    let mut total = 0u64;

    for pair in ulmdb_kv::Cursor::new(source, root, Bound::Unbounded, Bound::Unbounded, false)? {
        let (key, bytes) = pair?;
        if key.len() > source.max_key_size() {
            return Err(Error::Corrupt("invalid duplicate relocation key"));
        }

        let bucket = Bucket::decode_with(bytes, width, None)?;
        let expected = bucket.count();
        let mut inline = InlineBuffer::new();
        let mut child = None;
        let mut count = 0u64;

        for value in Values::new(source, bucket)? {
            let value = value?;
            validate_stored(value, width)?;

            count = count.checked_add(1).ok_or(Error::LimitExceeded)?;
            if child.is_none() && inline.push(value) {
                continue;
            }
            if child.is_none() {
                let prior = inline.decode_structure(width)?;
                let mut builder = ulmdb_kv::InOrderBuilder::duplicate_values();
                for index in 0..prior.count {
                    builder.push(target, prior.value(index), b"")?;
                }
                child = Some(builder);
            }
            child.as_mut().unwrap().push(target, value, b"")?;
        }
        if count != expected {
            return Err(Error::Corrupt("duplicate relocation count mismatch"));
        }
        total = total.checked_add(count).ok_or(Error::LimitExceeded)?;
        if let Some(child) = child {
            let built = child.finish(target)?;
            if built.records != count {
                return Err(Error::Corrupt("duplicate construction count mismatch"));
            }
            result.push(target, key, &tree_bytes(built.root, count))?;
        } else {
            result.push(target, key, inline.as_bytes())?;
        }
    }
    Ok((result.finish(target)?.root, total))
}

/// Compare logical pairs, permitting a subtree to compact into an inline bucket.
pub(crate) fn equivalent<S: ReadStore, T: ReadStore>(
    source: &S,
    old: Reference,
    target: &T,
    new: Reference,
    width: Option<u32>,
) -> Result<()> {
    let mut before = ulmdb_kv::Cursor::new(source, old, Bound::Unbounded, Bound::Unbounded, false)?;
    let mut after = ulmdb_kv::Cursor::new(target, new, Bound::Unbounded, Bound::Unbounded, false)?;
    loop {
        match (before.next().transpose()?, after.next().transpose()?) {
            (None, None) => return Ok(()),
            (Some((old_key, old_bytes)), Some((new_key, new_bytes))) if old_key == new_key => {
                let old_bucket = Bucket::decode_with(old_bytes, width, None)?;
                let new_bucket = Bucket::decode_with(new_bytes, width, None)?;
                if old_bucket.count() != new_bucket.count() {
                    return Err(Error::Corrupt("compacted duplicate count changed"));
                }
                let mut a = Values::new(source, old_bucket)?;
                let mut b = Values::new(target, new_bucket)?;
                loop {
                    match (a.next().transpose()?, b.next().transpose()?) {
                        (None, None) => break,
                        (Some(a), Some(b)) if a == b => {}
                        _ => return Err(Error::Corrupt("compacted duplicate value changed")),
                    }
                }
            }
            _ => return Err(Error::Corrupt("compacted duplicate key changed")),
        }
    }
}

#[cfg(test)]
mod tests;

enum ScopedValues<'a> {
    Inline(Inline<'a>, usize),
    Tree(crate::chunked::stream::Pairs<'a>),
}
impl<'a> ScopedValues<'a> {
    fn new(table: &'a ulmdb_storage::SegmentTable, bucket: Bucket<'a>) -> Self {
        match bucket {
            Bucket::Inline(bucket) => Self::Inline(bucket, 0),
            Bucket::Tree { root, .. } => {
                Self::Tree(crate::chunked::stream::Pairs::new(table, root))
            }
        }
    }
}
impl Iterator for ScopedValues<'_> {
    type Item = Result<Vec<u8>>;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Inline(bucket, n) => {
                if *n == bucket.count {
                    return None;
                }
                let value = bucket.value(*n).to_vec();
                *n += 1;
                Some(Ok(value))
            }
            Self::Tree(pairs) => pairs.next().map(|pair| {
                let (value, empty) = pair?;
                if !empty.is_empty() {
                    return Err(Error::Corrupt("duplicate child value must be empty"));
                }
                Ok(value)
            }),
        }
    }
}
pub(crate) fn rebuild_scoped<T: WriteStore>(
    source: &ulmdb_storage::SegmentTable,
    root: Reference,
    target: &mut T,
    width: Option<u32>,
) -> Result<(Reference, u64)> {
    let mut result = ulmdb_kv::InOrderBuilder::new();
    let mut total = 0u64;
    for pair in crate::chunked::stream::Pairs::new(source, root) {
        let (key, bytes) = pair?;
        if key.len() > source.max_key_size() {
            return Err(Error::Corrupt("invalid duplicate relocation key"));
        }
        let bucket = Bucket::decode_with(&bytes, width, None)?;
        let expected = bucket.count();
        let mut inline = InlineBuffer::new();
        let mut child: Option<ulmdb_kv::InOrderBuilder> = None;
        let mut count = 0u64;
        for value in ScopedValues::new(source, bucket) {
            let value = value?;
            validate_stored(&value, width)?;
            count = count.checked_add(1).ok_or(Error::LimitExceeded)?;
            if child.is_none() && inline.push(&value) {
                continue;
            }
            if child.is_none() {
                let prior = inline.decode_structure(width)?;
                let mut builder = ulmdb_kv::InOrderBuilder::duplicate_values();
                for index in 0..prior.count {
                    builder.push(target, prior.value(index), b"")?;
                }
                child = Some(builder);
            }
            child.as_mut().unwrap().push(target, &value, b"")?;
        }
        if count != expected {
            return Err(Error::Corrupt("duplicate relocation count mismatch"));
        }
        total = total.checked_add(count).ok_or(Error::LimitExceeded)?;
        if let Some(child) = child {
            let built = child.finish(target)?;
            if built.records != count {
                return Err(Error::Corrupt("duplicate construction count mismatch"));
            }
            result.push(target, &key, &tree_bytes(built.root, count))?;
        } else {
            result.push(target, &key, inline.as_bytes())?;
        }
    }
    Ok((result.finish(target)?.root, total))
}
pub(crate) fn equivalent_scoped(
    source: &ulmdb_storage::SegmentTable,
    old: Reference,
    target: &ulmdb_storage::SegmentTable,
    new: Reference,
    width: Option<u32>,
) -> Result<()> {
    let mut a = crate::chunked::stream::Pairs::new(source, old);
    let mut b = crate::chunked::stream::Pairs::new(target, new);
    loop {
        match (a.next().transpose()?, b.next().transpose()?) {
            (None, None) => return Ok(()),
            (Some((ka, va)), Some((kb, vb))) if ka == kb => {
                let a = Bucket::decode_with(&va, width, None)?;
                let b = Bucket::decode_with(&vb, width, None)?;
                if a.count() != b.count() {
                    return Err(Error::Corrupt("compacted duplicate count changed"));
                }
                let mut a = ScopedValues::new(source, a);
                let mut b = ScopedValues::new(target, b);
                loop {
                    match (a.next().transpose()?, b.next().transpose()?) {
                        (None, None) => break,
                        (Some(a), Some(b)) if a == b => {}
                        _ => return Err(Error::Corrupt("compacted duplicate value changed")),
                    }
                }
            }
            _ => return Err(Error::Corrupt("compacted duplicate key changed")),
        }
    }
}
