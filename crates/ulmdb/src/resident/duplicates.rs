//! Existing sorted bucket/child-tree codecs over the Resident store. Every
//! mutator validates the unsupported size boundary before touching private data.
use super::{
    ReadTxn, WriteTxn,
    catalog_codec::{MAX_DUPLICATE_VALUE_BYTES, MAX_KEY_BYTES},
};
use crate::{DatabaseKind, Error, Result, duplicates::Mutation};
use std::ops::Bound;
use ulmdb_storage::resident::{Arena, View};
pub type DuplicatesRead<'a> = crate::DuplicatesRead<'a, View>;
pub type DuplicateValues<'a> = crate::DuplicateValues<'a, View>;
pub type DuplicatePairs<'a> = crate::DuplicatePairs<'a, View>;
fn key_limit(key: &[u8]) -> Result<()> {
    if key.len() > MAX_KEY_BYTES {
        Err(Error::InvalidInput("resident duplicate key limit4080"))
    } else {
        Ok(())
    }
}
fn value_limit(value: &[u8], kind: DatabaseKind) -> Result<()> {
    let DatabaseKind::Duplicates { width } = kind else {
        return Err(Error::WrongKind);
    };
    if value.len() > MAX_DUPLICATE_VALUE_BYTES || width.is_some_and(|w| value.len() != w as usize) {
        return Err(Error::InvalidInput(
            "resident duplicate value width/limit65536",
        ));
    }
    Ok(())
}
impl ReadTxn {
    pub fn duplicates(&self, name: &[u8]) -> Result<DuplicatesRead<'_>> {
        self.duplicates_with_ordering(name, &Default::default())
    }
    pub fn duplicates_with_ordering(
        &self,
        name: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<DuplicatesRead<'_>> {
        let db = self
            .snapshot
            .catalog
            .get(name)
            .ok_or(Error::MissingDatabase)?;
        crate::DuplicatesRead::chunked_order(
            &self.snapshot.image,
            db.root,
            db.kind,
            crate::ordering::BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?,
            crate::ordering::BoundComparator::bind(
                db.ordering.duplicates,
                callbacks.duplicates.as_ref(),
            )?,
        )
    }
    pub fn duplicate_count(&self, name: &[u8], key: &[u8]) -> Result<u64> {
        self.duplicates(name)?.count(key)
    }
    pub fn duplicate_first(&self, name: &[u8], key: &[u8]) -> Result<Option<&[u8]>> {
        self.duplicates(name)?.first(key)
    }
    pub fn contains_duplicate(&self, name: &[u8], key: &[u8], value: &[u8]) -> Result<bool> {
        self.duplicates(name)?.contains(key, value)
    }
    pub fn duplicate_seek_pair(
        &self,
        name: &[u8],
        key: &[u8],
        value: &[u8],
    ) -> Result<Option<&[u8]>> {
        self.duplicates(name)?.seek_pair(key, value)
    }
    pub fn duplicate_values(
        &self,
        name: &[u8],
        key: &[u8],
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<DuplicateValues<'_>> {
        self.duplicates(name)?.values(key, lower, upper, reverse)
    }
    pub fn duplicate_pairs(
        &self,
        name: &[u8],
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<DuplicatePairs<'_>> {
        self.duplicates(name)?.pairs(lower, upper, reverse)
    }
}
impl WriteTxn<'_> {
    pub fn duplicates(&self, name: &[u8]) -> Result<crate::DuplicatesRead<'_, Arena>> {
        self.duplicates_with_ordering(name, &Default::default())
    }
    pub fn duplicates_with_ordering(
        &self,
        name: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<crate::DuplicatesRead<'_, Arena>> {
        self.live()?;
        let db = self
            .catalog
            .get(&self.base.catalog, name)
            .ok_or(Error::MissingDatabase)?;
        crate::DuplicatesRead::chunked_order(
            self.arena.as_ref().unwrap(),
            db.root,
            db.kind,
            crate::ordering::BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?,
            crate::ordering::BoundComparator::bind(
                db.ordering.duplicates,
                callbacks.duplicates.as_ref(),
            )?,
        )
    }
    pub fn duplicate_count(&self, name: &[u8], key: &[u8]) -> Result<u64> {
        self.duplicates(name)?.count(key)
    }
    pub fn duplicate_first(&self, name: &[u8], key: &[u8]) -> Result<Option<&[u8]>> {
        self.duplicates(name)?.first(key)
    }
    pub fn contains_duplicate(&self, name: &[u8], key: &[u8], value: &[u8]) -> Result<bool> {
        self.duplicates(name)?.contains(key, value)
    }
    pub fn insert_duplicate(&mut self, name: &[u8], key: &[u8], value: &[u8]) -> Result<bool> {
        self.mutate_duplicates(name, key, Mutation::Insert(value))
            .map(|n| n != 0)
    }
    pub fn delete_duplicate(&mut self, name: &[u8], key: &[u8], value: &[u8]) -> Result<bool> {
        self.mutate_duplicates(name, key, Mutation::Delete(value))
            .map(|n| n != 0)
    }
    pub fn delete_duplicates(&mut self, name: &[u8], key: &[u8]) -> Result<u64> {
        self.mutate_duplicates(name, key, Mutation::DeleteAll)
    }
    fn mutate_duplicates(
        &mut self,
        name: &[u8],
        key: &[u8],
        mutation: Mutation<'_>,
    ) -> Result<u64> {
        self.mutate_duplicates_with_ordering(name, key, mutation, &Default::default())
    }
    pub(super) fn mutate_duplicates_with_ordering(
        &mut self,
        name: &[u8],
        key: &[u8],
        mutation: Mutation<'_>,
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<u64> {
        self.live()?;
        key_limit(key)?;
        let db = self
            .catalog
            .get(&self.base.catalog, name)
            .ok_or(Error::MissingDatabase)?;
        if !matches!(db.kind, DatabaseKind::Duplicates { .. }) {
            return Err(Error::WrongKind);
        }
        match mutation {
            Mutation::Insert(v) | Mutation::Delete(v) => value_limit(v, db.kind)?,
            Mutation::DeleteAll => (),
        }
        let keys =
            crate::ordering::BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?;
        let values = crate::ordering::BoundComparator::bind(
            db.ordering.duplicates,
            callbacks.duplicates.as_ref(),
        )?;
        ulmdb_kv::ordering::Comparator::validate(&keys, key)?;
        if let Mutation::Insert(v) | Mutation::Delete(v) = &mutation {
            ulmdb_kv::ordering::Comparator::validate(&values, v)?;
        }
        let wide_insert = matches!(&mutation,Mutation::Insert(v) if v.len()>4080);
        let id = db.id;
        if let Mutation::Insert(value) = &mutation {
            if value.len() > 4080 {
                match self
                    .duplicates_with_ordering(name, callbacks)?
                    .contains(key, value)
                {
                    Ok(true) => return Ok(0),
                    Ok(false) => (),
                    Err(error) => {
                        self.failed = true;
                        return Err(error);
                    }
                }
            }
        }
        let size = match &mutation {
            Mutation::Insert(v) => v.len(),
            _ => 0,
        };
        self.admit_overflow(size)?;
        let (old, kind) = (db.root, db.kind);
        let revision = self.arena.as_ref().unwrap().revision();
        self.failed = true;
        let (root, count) = crate::duplicates::ordered_write::mutate_root_with(
            self.arena.as_mut().unwrap(),
            old,
            kind,
            key,
            mutation,
            &keys,
            &values,
        )?;
        self.failed = false;
        if wide_insert {
            self.overflow_databases.insert(id);
        }
        if (old, revision) != (root, self.arena.as_ref().unwrap().revision()) {
            self.catalog.set_root(&self.base.catalog, name, root);
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        catalog::Database,
        resident::{audit, catalog_codec},
    };
    use ulmdb_storage::{
        Reference,
        resident::{Config, Pool},
    };
    #[test]
    fn complete_audit_rejects_duplicate_count_width_and_nonempty_child_data() {
        for case in 0..4 {
            let pool = Pool::create(Config {
                kv_pages: 16,
                tiny_pages: 8,
                max_reserved_bytes: 1 << 20,
                max_metadata_bytes: 16 << 20,
                max_views: 16,
            })
            .unwrap();
            let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
            let child = ulmdb_kv::put(
                &mut arena,
                Reference::NULL,
                if case == 2 { b"bad" } else { b"four" },
                if case == 1 { b"not-empty" } else { b"" },
            )
            .unwrap();
            let mut bucket = Vec::from(child.encode());
            bucket.extend_from_slice(&(if case == 0 { 2u64 } else { 1 }).to_le_bytes());
            let parent = ulmdb_kv::put(&mut arena, Reference::NULL, b"k", &bucket).unwrap();
            let db = Database {
                id: 1,
                ordering: Default::default(),
                kind: DatabaseKind::Duplicates { width: Some(4) },
                root: parent,
            };
            let root = ulmdb_kv::put(
                &mut arena,
                Reference::NULL,
                b"dups",
                &catalog_codec::encode(&db),
            )
            .unwrap();
            let view = arena.freeze().unwrap().activate();
            let result = audit::audit(&view, root, 2, 1_000_000);
            assert_eq!(result.is_ok(), case == 3);
        }
    }
}
