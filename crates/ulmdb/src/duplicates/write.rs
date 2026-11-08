use super::{
    bucket::{Bucket, INLINE_BYTES, INLINE_COUNT, InlineBuffer, OwnedBucket, tree_bytes},
    check_key, check_value, validate_stored, width,
};
use crate::{Error, Result, WriteTxn};
use std::ops::Bound;
use ulmdb_core::storage::{ReadStore, Reference, WriteStore};

pub(crate) enum Mutation<'a> {
    Insert(&'a [u8]),
    Delete(&'a [u8]),
    DeleteAll,
}

fn insert<S: WriteStore>(
    store: &mut S,
    parent: Reference,
    key: &[u8],
    value: &[u8],
    width: Option<u32>,
) -> Result<(Reference, u64)> {
    let bucket = Bucket::get(store, parent, key, width)?;
    if let Some(bucket) = bucket
        && bucket.seek(store, value)?.is_some()
    {
        return Ok((parent, 0));
    }
    let count = bucket
        .map_or(0, Bucket::count)
        .checked_add(1)
        .ok_or(Error::LimitExceeded)?;
    match OwnedBucket::copy(bucket) {
        OwnedBucket::Tree { root, .. } => {
            let root = ulmdb_kv::put(store, root, value, b"")?;
            Ok((
                ulmdb_kv::put(store, parent, key, &tree_bytes(root, count))?,
                1,
            ))
        }
        owned => {
            let source = match owned {
                OwnedBucket::Inline(source) => source,
                _ => InlineBuffer::new(),
            };
            let inline = source.decode(width)?;
            if inline.count < INLINE_COUNT && inline.bytes.len() + 4 + value.len() <= INLINE_BYTES {
                let position = inline.search(value, false);
                let mut next = InlineBuffer::new();
                for index in 0..=inline.count {
                    if index == position && !next.push(value) {
                        return Err(Error::Corrupt("inline duplicate builder limit"));
                    }
                    if index < inline.count && !next.push(inline.value(index)) {
                        return Err(Error::Corrupt("inline duplicate builder limit"));
                    }
                }
                Ok((ulmdb_kv::put(store, parent, key, next.as_bytes())?, 1))
            } else {
                let mut child = Reference::NULL;
                for index in 0..inline.count {
                    child = ulmdb_kv::put(store, child, inline.value(index), b"")?;
                }
                child = ulmdb_kv::put(store, child, value, b"")?;
                Ok((
                    ulmdb_kv::put(store, parent, key, &tree_bytes(child, count))?,
                    1,
                ))
            }
        }
    }
}

fn try_inline<S: ReadStore>(
    store: &S,
    root: Reference,
    count: u64,
    width: Option<u32>,
) -> Result<Option<InlineBuffer>> {
    if count > INLINE_COUNT as u64 {
        return Ok(None);
    }
    let mut next = InlineBuffer::new();
    let mut seen = 0;
    for pair in ulmdb_kv::Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, false)? {
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

fn delete<S: WriteStore>(
    store: &mut S,
    parent: Reference,
    key: &[u8],
    value: &[u8],
    width: Option<u32>,
) -> Result<(Reference, u64)> {
    let Some(bucket) = Bucket::get(store, parent, key, width)? else {
        return Ok((parent, 0));
    };
    if bucket.seek(store, value)?.is_none() {
        return Ok((parent, 0));
    }
    if bucket.count() == 1 {
        return Ok((ulmdb_kv::delete(store, parent, key)?, 1));
    }
    match OwnedBucket::copy(Some(bucket)) {
        OwnedBucket::Inline(source) => {
            let inline = source.decode(width)?;
            let position = inline.search(value, false);
            let mut next = InlineBuffer::new();
            for index in 0..inline.count {
                if index != position && !next.push(inline.value(index)) {
                    return Err(Error::Corrupt("inline duplicate builder limit"));
                }
            }
            Ok((ulmdb_kv::put(store, parent, key, next.as_bytes())?, 1))
        }
        OwnedBucket::Tree { root, count } => {
            let root = ulmdb_kv::delete(store, root, value)?;
            let count = count - 1;
            let root = if let Some(inline) = try_inline(store, root, count, width)? {
                ulmdb_kv::put(store, parent, key, inline.as_bytes())?
            } else {
                ulmdb_kv::put(store, parent, key, &tree_bytes(root, count))?
            };
            Ok((root, 1))
        }
        OwnedBucket::Missing => Err(Error::Corrupt("missing duplicate bucket during deletion")),
    }
}

pub(crate) fn mutate_root<S: WriteStore>(
    store: &mut S,
    old_root: Reference,
    kind: crate::DatabaseKind,
    key: &[u8],
    mutation: Mutation<'_>,
) -> Result<(Reference, u64)> {
    check_key(store, key)?;
    let width = width(kind)?;
    match mutation {
        Mutation::Insert(value) => {
            check_value(value, width)?;
            if value.len() > store.max_duplicate_value_size() {
                return Err(Error::InvalidInput(
                    "duplicate store value size limit exceeded",
                ));
            }
            insert(store, old_root, key, value, width)
        }
        Mutation::Delete(value) => {
            check_value(value, width)?;
            if value.len() > store.max_duplicate_value_size() {
                return Err(Error::InvalidInput(
                    "duplicate store value size limit exceeded",
                ));
            }
            delete(store, old_root, key, value, width)
        }
        Mutation::DeleteAll => match Bucket::get(store, old_root, key, width)? {
            None => Ok((old_root, 0)),
            Some(bucket) => {
                let count = bucket.count();
                Ok((ulmdb_kv::delete(store, old_root, key)?, count))
            }
        },
    }
}

impl WriteTxn<'_> {
    /// Copy pairs in key/value order, for foreign-language cursor ownership.
    pub fn duplicate_pairs_owned(&self, name: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        let width = width(db.kind)?;
        let keys = crate::ordering::BoundComparator::bind(db.ordering.keys, None)?;
        let values = crate::ordering::BoundComparator::bind(db.ordering.duplicates, None)?;
        let mut pairs = Vec::new();
        for entry in ulmdb_kv::Cursor::with_order(
            &self.arena,
            db.root,
            Bound::Unbounded,
            Bound::Unbounded,
            false,
            keys.clone(),
        )? {
            let (key, bytes) = entry?;
            match Bucket::decode_with(bytes, width, Some(&values))? {
                Bucket::Inline(inline) => {
                    for index in 0..inline.count {
                        pairs.push((key.to_vec(), inline.value(index).to_vec()));
                    }
                }
                Bucket::Tree { root, .. } => {
                    for entry in ulmdb_kv::Cursor::with_order(
                        &self.arena,
                        root,
                        Bound::Unbounded,
                        Bound::Unbounded,
                        false,
                        values.clone(),
                    )? {
                        let (value, empty) = entry?;
                        validate_stored(value, width)?;
                        if !empty.is_empty() {
                            return Err(Error::Corrupt("duplicate child value must be empty"));
                        }
                        pairs.push((key.to_vec(), value.to_vec()));
                    }
                }
            }
        }
        Ok(pairs)
    }

    pub fn duplicate_first(&self, name: &[u8], key: &[u8]) -> Result<Option<&[u8]>> {
        self.live()?;
        check_key(&self.arena, key)?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        let width = width(db.kind)?;
        let keys = crate::ordering::BoundComparator::bind(db.ordering.keys, None)?;
        let values = crate::ordering::BoundComparator::bind(db.ordering.duplicates, None)?;
        match Bucket::get_with(&self.arena, db.root, key, width, &keys, &values)? {
            None => Ok(None),
            Some(Bucket::Inline(inline)) => Ok(Some(inline.value(0))),
            Some(Bucket::Tree { root, .. }) => ulmdb_kv::Cursor::with_order(
                &self.arena,
                root,
                Bound::Unbounded,
                Bound::Unbounded,
                false,
                values,
            )?
            .next()
            .transpose()
            .map(|entry| entry.map(|(value, _)| value)),
        }
    }

    pub fn duplicate_count(&self, name: &[u8], key: &[u8]) -> Result<u64> {
        self.live()?;
        check_key(&self.arena, key)?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        let keys = crate::ordering::BoundComparator::bind(db.ordering.keys, None)?;
        let values = crate::ordering::BoundComparator::bind(db.ordering.duplicates, None)?;
        Ok(
            Bucket::get_with(&self.arena, db.root, key, width(db.kind)?, &keys, &values)?
                .map_or(0, Bucket::count),
        )
    }
    pub fn contains_duplicate(&self, name: &[u8], key: &[u8], value: &[u8]) -> Result<bool> {
        self.live()?;
        check_key(&self.arena, key)?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        let width = width(db.kind)?;
        check_value(value, width)?;
        let keys = crate::ordering::BoundComparator::bind(db.ordering.keys, None)?;
        let values = crate::ordering::BoundComparator::bind(db.ordering.duplicates, None)?;
        let Some(bucket) = Bucket::get_with(&self.arena, db.root, key, width, &keys, &values)?
        else {
            return Ok(false);
        };
        Ok(bucket.seek_with(&self.arena, value, &values)?.is_some())
    }
    /// Insert one unique pair; false means the pair already exists. An error
    /// requires aborting the writer or rolling back its enclosing savepoint.
    pub fn insert_duplicate(&mut self, name: &[u8], key: &[u8], value: &[u8]) -> Result<bool> {
        self.mutate_duplicate(name, key, Mutation::Insert(value))
            .map(|count| count != 0)
    }
    pub fn delete_duplicate(&mut self, name: &[u8], key: &[u8], value: &[u8]) -> Result<bool> {
        self.mutate_duplicate(name, key, Mutation::Delete(value))
            .map(|count| count != 0)
    }
    pub fn delete_duplicates(&mut self, name: &[u8], key: &[u8]) -> Result<u64> {
        self.mutate_duplicate(name, key, Mutation::DeleteAll)
    }
    fn mutate_duplicate(&mut self, name: &[u8], key: &[u8], mutation: Mutation<'_>) -> Result<u64> {
        self.live()?;
        let Some(db) = self.catalog.get(name) else {
            self.failed = true;
            return Err(Error::MissingDatabase);
        };
        if db.ordering != Default::default() {
            return self.mutate_duplicates_with_ordering(name, key, mutation, &Default::default());
        }
        self.failed = true;
        let result = self.mutate_duplicate_inner(name, key, mutation);
        if result.is_ok() {
            self.failed = false;
        }
        result
    }
    fn mutate_duplicate_inner(
        &mut self,
        name: &[u8],
        key: &[u8],
        mutation: Mutation<'_>,
    ) -> Result<u64> {
        check_key(&self.arena, key)?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        let old_root = db.root;
        let revision = self.arena.revision();
        let (root, affected) = mutate_root(&mut self.arena, old_root, db.kind, key, mutation)?;
        self.catalog
            .get_mut(name)
            .ok_or(Error::MissingDatabase)?
            .root = root;
        if root != old_root || self.arena.revision() != revision {
            self.mark_changed(name);
        }
        Ok(affected)
    }
}
