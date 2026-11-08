//! Persisted ordering descriptions and process-local callback bindings.
//! Descriptors contain only stable tags, never function pointers or addresses.
use crate::{DatabaseKind, Error, ReadTxn, Result, WriteTxn};
use std::{cmp::Ordering, ops::Bound, sync::Arc};
use ulmdb_core::storage::ReadStore;
use ulmdb_kv::ordering::Comparator;

pub use ulmdb_kv::ordering::{IntegerEndian as Endian, IntegerWidth as Width};
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OrderingSpec {
    #[default]
    Bytewise,
    ReverseBytes,
    UnsignedInteger {
        width: Width,
        endian: Endian,
    },
    Custom,
}
impl OrderingSpec {
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Bytewise => 0,
            Self::ReverseBytes => 1,
            Self::Custom => 6,
            Self::UnsignedInteger {
                width: Width::U32,
                endian: Endian::Little,
            } => 2,
            Self::UnsignedInteger {
                width: Width::U32,
                endian: Endian::Big,
            } => 3,
            Self::UnsignedInteger {
                width: Width::U64,
                endian: Endian::Little,
            } => 4,
            Self::UnsignedInteger {
                width: Width::U64,
                endian: Endian::Big,
            } => 5,
        }
    }
    pub(crate) fn decode(value: u8) -> Result<Self> {
        Ok(match value {
            0 => Self::Bytewise,
            1 => Self::ReverseBytes,
            6 => Self::Custom,
            2 => Self::UnsignedInteger {
                width: Width::U32,
                endian: Endian::Little,
            },
            3 => Self::UnsignedInteger {
                width: Width::U32,
                endian: Endian::Big,
            },
            4 => Self::UnsignedInteger {
                width: Width::U64,
                endian: Endian::Little,
            },
            5 => Self::UnsignedInteger {
                width: Width::U64,
                endian: Endian::Big,
            },
            _ => return Err(Error::Corrupt("unknown database ordering")),
        })
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DatabaseOrdering {
    pub keys: OrderingSpec,
    pub duplicates: OrderingSpec,
}
impl DatabaseOrdering {
    pub(crate) fn validate(self, kind: DatabaseKind) -> Result<()> {
        if !matches!(kind, DatabaseKind::Kv | DatabaseKind::Duplicates { .. })
            && self != Self::default()
        {
            return Err(Error::InvalidInput(
                "prefix indexes do not accept key comparators",
            ));
        }
        if !matches!(kind, DatabaseKind::Duplicates { .. })
            && self.duplicates != OrderingSpec::Bytewise
        {
            return Err(Error::InvalidInput(
                "duplicate ordering requires a duplicate database",
            ));
        }
        if let (
            DatabaseKind::Duplicates {
                width: Some(actual),
            },
            OrderingSpec::UnsignedInteger { width, .. },
        ) = (kind, self.duplicates)
        {
            if actual
                != match width {
                    Width::U32 => 4,
                    Width::U64 => 8,
                }
            {
                return Err(Error::InvalidInput(
                    "integer duplicate ordering width mismatch",
                ));
            }
        }
        Ok(())
    }
}
pub type CustomComparator = Arc<dyn Fn(&[u8], &[u8]) -> Ordering + Send + Sync>;
#[derive(Clone, Default)]
pub struct DatabaseComparators {
    pub keys: Option<CustomComparator>,
    pub duplicates: Option<CustomComparator>,
}
#[derive(Clone)]
pub enum BoundComparator {
    Bytewise,
    ReverseBytes,
    UnsignedInteger(ulmdb_kv::ordering::UnsignedInteger),
    Custom(CustomComparator),
    /// Metadata capability may be opened before application registration.
    /// Data APIs reject this authority before comparing a query or mutating.
    UnregisteredCustom,
}
impl BoundComparator {
    pub(crate) fn deferred(
        spec: OrderingSpec,
        callback: Option<&CustomComparator>,
    ) -> Result<Self> {
        if spec == OrderingSpec::Custom && callback.is_none() {
            Ok(Self::UnregisteredCustom)
        } else {
            Self::bind(spec, callback)
        }
    }
    pub(crate) fn bind(spec: OrderingSpec, callback: Option<&CustomComparator>) -> Result<Self> {
        Ok(match spec {
            OrderingSpec::Bytewise => Self::Bytewise,
            OrderingSpec::ReverseBytes => Self::ReverseBytes,
            OrderingSpec::UnsignedInteger { width, endian } => {
                Self::UnsignedInteger(ulmdb_kv::ordering::UnsignedInteger { width, endian })
            }
            OrderingSpec::Custom => Self::Custom(
                callback
                    .ok_or(Error::InvalidInput("custom comparator must be registered"))?
                    .clone(),
            ),
        })
    }
}
impl Comparator for BoundComparator {
    fn check_ready(&self) -> Result<()> {
        if matches!(self, Self::UnregisteredCustom) {
            Err(Error::InvalidInput("custom comparator must be registered"))
        } else {
            Ok(())
        }
    }
    fn compare(&self, a: &[u8], b: &[u8]) -> Ordering {
        match self {
            Self::Bytewise => a.cmp(b),
            Self::ReverseBytes => a.iter().rev().cmp(b.iter().rev()),
            Self::UnsignedInteger(integer) => integer.compare(a, b),
            Self::Custom(callback) => callback(a, b),
            Self::UnregisteredCustom => panic!("unregistered custom comparison authority"),
        }
    }
    fn validate(&self, key: &[u8]) -> Result<()> {
        match self {
            Self::UnsignedInteger(integer) => integer.validate(key),
            Self::UnregisteredCustom => {
                Err(Error::InvalidInput("custom comparator must be registered"))
            }
            _ => Ok(()),
        }
    }
    fn is_bytewise(&self) -> bool {
        matches!(self, Self::Bytewise)
    }
}

/// A root plus explicit process-local comparison authority. Binding calls no
/// application comparator, so registration can follow environment/DBI opening.
pub struct OrderedKvRead<'a> {
    pub(crate) store: &'a ulmdb_storage::SegmentTable,
    pub(crate) root: ulmdb_storage::Reference,
    pub(crate) order: BoundComparator,
}
impl<'a> OrderedKvRead<'a> {
    pub fn get(&self, key: &[u8]) -> Result<Option<&'a [u8]>> {
        if key.len() > self.store.max_key_size() {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        if matches!(self.order, BoundComparator::Bytewise) {
            return ulmdb_kv::get(self.store, self.root, key);
        }
        ulmdb_kv::get_with(self.store, self.root, key, &self.order)
    }
    pub fn cursor(
        &self,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<ulmdb_kv::Cursor<'a, ulmdb_storage::SegmentTable, BoundComparator>> {
        ulmdb_kv::Cursor::with_order(
            self.store,
            self.root,
            lower,
            upper,
            reverse,
            self.order.clone(),
        )
    }
}
impl ReadTxn {
    pub fn database_ordering(&self, name: &[u8]) -> Result<DatabaseOrdering> {
        Ok(self.database(name)?.ordering)
    }
    pub fn kv_with_ordering(
        &self,
        name: &[u8],
        callbacks: &DatabaseComparators,
    ) -> Result<OrderedKvRead<'_>> {
        let db = self.database(name)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        Ok(OrderedKvRead {
            store: &self.snapshot.segments,
            root: db.root,
            order: BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?,
        })
    }
}
impl WriteTxn<'_> {
    pub fn database_ordering(&self, name: &[u8]) -> Result<DatabaseOrdering> {
        self.live()?;
        Ok(self
            .catalog
            .get(name)
            .ok_or(Error::MissingDatabase)?
            .ordering)
    }
    pub fn get_with_ordering(
        &self,
        name: &[u8],
        key: &[u8],
        callbacks: &DatabaseComparators,
    ) -> Result<Option<&[u8]>> {
        self.live()?;
        if key.len() > self.arena.max_key_size() {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if db.ordering.keys == OrderingSpec::Bytewise {
            return ulmdb_kv::get(&self.arena, db.root, key);
        }
        let order = BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?;
        ulmdb_kv::get_with(&self.arena, db.root, key, &order)
    }
    pub fn put_with_ordering(
        &mut self,
        name: &[u8],
        key: &[u8],
        value: &[u8],
        callbacks: &DatabaseComparators,
    ) -> Result<()> {
        self.live()?;
        if key.len() > self.arena.max_key_size() || value.len() > self.writable_value_limit() {
            return Err(Error::InvalidInput("KV size limit exceeded"));
        }
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        let order = BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?;
        order.validate(key)?;
        let prior = db.root;
        self.enable_value_limit(value.len())?;
        let revision = self.arena.revision();
        self.failed = true;
        let root = ulmdb_kv::put_with(&mut self.arena, prior, key, value, &order)?;
        self.catalog
            .get_mut(name)
            .ok_or(Error::MissingDatabase)?
            .root = root;
        if root != prior || self.arena.revision() != revision {
            self.mark_changed(name);
        }
        self.failed = false;
        Ok(())
    }
    pub fn delete_with_ordering(
        &mut self,
        name: &[u8],
        key: &[u8],
        callbacks: &DatabaseComparators,
    ) -> Result<()> {
        self.live()?;
        if key.len() > self.arena.max_key_size() {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        let order = BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?;
        order.validate(key)?;
        let prior = db.root;
        let revision = self.arena.revision();
        self.failed = true;
        let root = ulmdb_kv::delete_with(&mut self.arena, prior, key, &order)?;
        self.catalog
            .get_mut(name)
            .ok_or(Error::MissingDatabase)?
            .root = root;
        if root != prior || self.arena.revision() != revision {
            self.mark_changed(name);
        }
        self.failed = false;
        Ok(())
    }
}

impl WriteTxn<'_> {
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
    pub(crate) fn mutate_duplicates_with_ordering(
        &mut self,
        name: &[u8],
        key: &[u8],
        mutation: crate::duplicates::Mutation<'_>,
        callbacks: &DatabaseComparators,
    ) -> Result<u64> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        let keys = BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?;
        let values = BoundComparator::bind(db.ordering.duplicates, callbacks.duplicates.as_ref())?;
        keys.validate(key)?;
        let prior = db.root;
        let kind = db.kind;
        let revision = self.arena.revision();
        self.failed = true;
        let (root, affected) = crate::duplicates::ordered_write::mutate_root_with(
            &mut self.arena,
            prior,
            kind,
            key,
            mutation,
            &keys,
            &values,
        )?;
        self.catalog
            .get_mut(name)
            .ok_or(Error::MissingDatabase)?
            .root = root;
        if root != prior || self.arena.revision() != revision {
            self.mark_changed(name);
        }
        self.failed = false;
        Ok(affected)
    }
}

impl ReadTxn {
    /// Physical inorder only: no callbacks, bounds, equality lookup, or order certificate.
    /// Intended for logical quota accounting and structurally authenticated copying.
    pub fn structural_pairs(
        &self,
        name: &[u8],
    ) -> Result<crate::duplicates::StructuralPairs<'_, ulmdb_storage::SegmentTable>> {
        let db = self.database(name)?;
        crate::duplicates::StructuralPairs::new(&self.snapshot.segments, db.root, db.kind)
    }
}
impl WriteTxn<'_> {
    pub fn set_database_ordering(&mut self, name: &[u8], ordering: DatabaseOrdering) -> Result<()> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        ordering.validate(db.kind)?;
        if db.ordering == ordering {
            return Ok(());
        }
        if !db.root.is_null() {
            return Err(Error::InvalidInput(
                "ordering change requires empty database",
            ));
        }
        if self
            .database_views
            .as_ref()
            .is_some_and(|v| v.has_identity(db.id))
        {
            return Err(Error::InvalidInput("ordering change with bound views"));
        }
        self.catalog.get_mut(name).unwrap().ordering = ordering;
        self.mark_changed(name);
        Ok(())
    }
}

impl ReadTxn {
    pub fn client_seek_pair_with_ordering(
        &self,
        name: &[u8],
        bound: Option<(&[u8], Option<&[u8]>)>,
        inclusive: bool,
        reverse: bool,
        callbacks: &DatabaseComparators,
    ) -> Result<Option<(&[u8], &[u8])>> {
        let db = self.database(name)?;
        let keys = BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?;
        let values = BoundComparator::bind(db.ordering.duplicates, callbacks.duplicates.as_ref())?;
        crate::duplicates::seek_pair_with(
            &self.snapshot.segments,
            db.root,
            db.kind,
            bound,
            inclusive,
            reverse,
            &keys,
            &values,
        )
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
    pub fn client_seek_pair_with_ordering(
        &self,
        name: &[u8],
        bound: Option<(&[u8], Option<&[u8]>)>,
        inclusive: bool,
        reverse: bool,
        callbacks: &DatabaseComparators,
    ) -> Result<Option<(&[u8], &[u8])>> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        let keys = BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?;
        let values = BoundComparator::bind(db.ordering.duplicates, callbacks.duplicates.as_ref())?;
        crate::duplicates::seek_pair_with(
            &self.arena,
            db.root,
            db.kind,
            bound,
            inclusive,
            reverse,
            &keys,
            &values,
        )
    }
}

impl ReadTxn {
    pub fn structural_resume_pair(
        &self,
        name: &[u8],
        after: Option<(&[u8], &[u8])>,
    ) -> Result<Option<(&[u8], &[u8])>> {
        let db = self.database(name)?;
        crate::duplicates::structural::resume(&self.snapshot.segments, db.root, db.kind, after)
    }
}
impl WriteTxn<'_> {
    /// Callback-free physical traversal for quota accounting and export.
    pub fn structural_pairs(
        &self,
        name: &[u8],
    ) -> Result<crate::duplicates::StructuralPairs<'_, ulmdb_storage::Arena>> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        crate::duplicates::StructuralPairs::new(&self.arena, db.root, db.kind)
    }
    pub fn structural_resume_pair(
        &self,
        name: &[u8],
        after: Option<(&[u8], &[u8])>,
    ) -> Result<Option<(&[u8], &[u8])>> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        crate::duplicates::structural::resume(&self.arena, db.root, db.kind, after)
    }
}

impl WriteTxn<'_> {
    pub fn duplicate_count_with_ordering(
        &self,
        name: &[u8],
        key: &[u8],
        callbacks: &DatabaseComparators,
    ) -> Result<u64> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        let DatabaseKind::Duplicates { width } = db.kind else {
            return Err(Error::WrongKind);
        };
        let keys = BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?;
        let values = BoundComparator::bind(db.ordering.duplicates, callbacks.duplicates.as_ref())?;
        Ok(crate::duplicates::bucket::Bucket::get_with(
            &self.arena,
            db.root,
            key,
            width,
            &keys,
            &values,
        )?
        .map_or(0, |b| b.count()))
    }
}
