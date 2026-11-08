//! Persisted order tags with explicit caller-owned comparison bindings.
use super::{ReadTxn, WriteTxn};
use crate::{
    Error, Result,
    ordering::{DatabaseComparators, DatabaseOrdering},
};
impl ReadTxn {
    pub fn database_ordering(&self, name: &[u8]) -> Result<DatabaseOrdering> {
        Ok(self.database(name)?.ordering)
    }
    pub fn get_with_ordering(
        &self,
        name: &[u8],
        key: &[u8],
        callbacks: &DatabaseComparators,
    ) -> Result<Option<&[u8]>> {
        self.kv_with_ordering(name, callbacks)?.get(key)
    }
}
impl WriteTxn<'_> {
    pub fn database_ordering(&self, name: &[u8]) -> Result<DatabaseOrdering> {
        self.live()?;
        Ok(self
            .catalog
            .get(&self.base.catalog, name)
            .ok_or(Error::MissingDatabase)?
            .ordering)
    }
    pub fn create_database_with_ordering(
        &mut self,
        name: &[u8],
        kind: crate::DatabaseKind,
        ordering: DatabaseOrdering,
    ) -> Result<()> {
        ordering.validate(kind)?;
        self.create_database(name, kind)?;
        self.set_database_ordering(name, ordering)
    }
    /// Reordering existing records requires an explicit rebuild.
    pub fn set_database_ordering(&mut self, name: &[u8], ordering: DatabaseOrdering) -> Result<()> {
        self.live()?;
        let mut db = self
            .catalog
            .get(&self.base.catalog, name)
            .ok_or(Error::MissingDatabase)?
            .clone();
        ordering.validate(db.kind)?;
        if db.ordering == ordering {
            return Ok(());
        }
        if !db.root.is_null() {
            return Err(Error::InvalidInput(
                "ordering change requires empty database",
            ));
        }
        db.ordering = ordering;
        self.catalog.insert(&self.base.catalog, name.to_vec(), db);
        Ok(())
    }
    pub fn insert_duplicate_with_ordering(
        &mut self,
        name: &[u8],
        key: &[u8],
        value: &[u8],
        callbacks: &DatabaseComparators,
    ) -> Result<bool> {
        self.mutate_duplicates_with_ordering(
            name,
            key,
            crate::duplicates::Mutation::Insert(value),
            callbacks,
        )
        .map(|n| n != 0)
    }
    pub fn delete_duplicate_with_ordering(
        &mut self,
        name: &[u8],
        key: &[u8],
        value: &[u8],
        callbacks: &DatabaseComparators,
    ) -> Result<bool> {
        self.mutate_duplicates_with_ordering(
            name,
            key,
            crate::duplicates::Mutation::Delete(value),
            callbacks,
        )
        .map(|n| n != 0)
    }
    pub fn delete_duplicates_with_ordering(
        &mut self,
        name: &[u8],
        key: &[u8],
        callbacks: &DatabaseComparators,
    ) -> Result<u64> {
        self.mutate_duplicates_with_ordering(
            name,
            key,
            crate::duplicates::Mutation::DeleteAll,
            callbacks,
        )
    }
    pub fn kv_cursor_with_ordering(
        &self,
        name: &[u8],
        lower: std::ops::Bound<&[u8]>,
        upper: std::ops::Bound<&[u8]>,
        reverse: bool,
        callbacks: &DatabaseComparators,
    ) -> Result<
        ulmdb_kv::Cursor<'_, ulmdb_storage::resident::Arena, crate::ordering::BoundComparator>,
    > {
        self.live()?;
        let db = self
            .catalog
            .get(&self.base.catalog, name)
            .ok_or(Error::MissingDatabase)?;
        if db.kind != crate::DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        let order =
            crate::ordering::BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?;
        ulmdb_kv::Cursor::with_order(
            self.arena.as_ref().unwrap(),
            db.root,
            lower,
            upper,
            reverse,
            order,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DatabaseKind,
        catalog::Database,
        ordering::{Endian, OrderingSpec, Width},
    };
    use ulmdb_storage::{
        Reference,
        resident::{Config, Pool},
    };
    #[test]
    fn ordered_descriptors_are_versioned_and_default_bytes_remain_48() {
        let mut db = Database {
            id: 7,
            kind: DatabaseKind::Kv,
            root: Reference::NULL,
            ordering: Default::default(),
        };
        let default = super::super::catalog_codec::encode(&db);
        assert_eq!(default.len(), 48);
        assert_eq!(&default[10..12], &1u16.to_le_bytes());
        assert!(default[40..].iter().all(|b| *b == 0));
        db.ordering.keys = OrderingSpec::ReverseBytes;
        let ordered = super::super::catalog_codec::encode(&db);
        assert_eq!(ordered.len(), 64);
        assert_eq!(&ordered[10..12], &2u16.to_le_bytes());
        assert_eq!(ordered[48], 1);
        assert_eq!(
            super::super::catalog_codec::decode(&ordered)
                .unwrap()
                .ordering,
            db.ordering
        );
        for at in [10, 48, 49, 50, 63] {
            let mut bad = ordered.clone();
            bad[at] = 255;
            assert!(super::super::catalog_codec::decode(&bad).is_err());
        }
        let mut bad = ordered.clone();
        bad[48] = 0;
        assert!(super::super::catalog_codec::decode(&bad).is_err());
        assert_eq!(
            super::super::catalog_codec::encode(&Database {
                ordering: Default::default(),
                ..db
            }),
            default
        );
    }
    #[test]
    fn cold_builtin_rejects_incorrect_order_while_custom_requires_only_structure() {
        let pool = Pool::create(Config {
            kv_pages: 64,
            tiny_pages: 16,
            max_reserved_bytes: 4 << 20,
            max_metadata_bytes: 16 << 20,
            max_views: 16,
        })
        .unwrap();
        let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
        let mut tree = Reference::NULL;
        for n in 0u32..700 {
            tree = ulmdb_kv::put(&mut arena, tree, &n.to_le_bytes(), b"v").unwrap();
        }
        let mut db = Database {
            id: 1,
            kind: DatabaseKind::Kv,
            root: tree,
            ordering: DatabaseOrdering {
                keys: OrderingSpec::UnsignedInteger {
                    width: Width::U32,
                    endian: Endian::Little,
                },
                duplicates: OrderingSpec::Bytewise,
            },
        };
        let bad = ulmdb_kv::put(
            &mut arena,
            Reference::NULL,
            b"db",
            &super::super::catalog_codec::encode(&db),
        )
        .unwrap();
        db.ordering.keys = OrderingSpec::Custom;
        let custom = ulmdb_kv::put(
            &mut arena,
            Reference::NULL,
            b"db",
            &super::super::catalog_codec::encode(&db),
        )
        .unwrap();
        let view = arena.freeze().unwrap().activate();
        assert!(super::super::audit::audit(&view, bad, 2, 1_000_000).is_err());
        super::super::audit::audit(&view, custom, 2, 1_000_000).unwrap();
    }
}

impl ReadTxn {
    pub fn catalog_capacity(&self) -> Result<crate::CatalogCapacity> {
        crate::catalog_profile::read(&self.snapshot.image, self.snapshot.root)
    }
}
impl WriteTxn<'_> {
    pub fn catalog_capacity(&self) -> crate::CatalogCapacity {
        self.catalog_capacity
    }
    pub fn set_catalog_capacity(&mut self, capacity: crate::CatalogCapacity) -> Result<()> {
        self.live()?;
        if self.catalog.len() > capacity.get() {
            return Err(Error::LimitExceeded);
        }
        if self.catalog_capacity != capacity {
            self.catalog_capacity = capacity;
            self.catalog_profile_changed = true;
        }
        Ok(())
    }
}
