//! Prepared typed views bind catalog roots once per read transaction/batch.
use crate::{Address, DatabaseKind, Error, Prefix, Result, read::check_family};
use std::ops::Bound;
use ulmdb_core::storage::ReadStore;
use ulmdb_storage::{Reference, SegmentTable};

pub struct KvRead<'a> {
    pub(crate) store: &'a SegmentTable,
    pub(crate) root: Reference,
    reader: Option<ulmdb_kv::Reader<'a, SegmentTable>>,
    order: crate::ordering::BoundComparator,
}
impl<'a> KvRead<'a> {
    pub(crate) fn new(store: &'a SegmentTable, root: Reference) -> Result<Self> {
        Self::with_order(store, root, crate::ordering::BoundComparator::Bytewise)
    }
    pub(crate) fn with_order(
        store: &'a SegmentTable,
        root: Reference,
        order: crate::ordering::BoundComparator,
    ) -> Result<Self> {
        use ulmdb_kv::ordering::Comparator;
        let reader = if order.is_bytewise() {
            Some(ulmdb_kv::Reader::new(store, root)?)
        } else {
            None
        };
        Ok(Self {
            store,
            root,
            reader,
            order,
        })
    }
    pub fn get(&self, key: &[u8]) -> Result<Option<&'a [u8]>> {
        if key.len() > self.store.max_key_size() {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        match &self.reader {
            Some(reader) => reader.get(key),
            None => ulmdb_kv::get_with(self.store, self.root, key, &self.order),
        }
    }
    pub fn cursor(
        &self,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<ulmdb_kv::Cursor<'a, SegmentTable, crate::ordering::BoundComparator>> {
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
pub struct PrefixRead<'a> {
    pub(crate) store: &'a SegmentTable,
    pub(crate) root: Reference,
    pub(crate) kind: DatabaseKind,
    reader: ulmdb_prefix::Reader<'a, SegmentTable>,
}
impl<'a> PrefixRead<'a> {
    pub(crate) fn new(
        store: &'a SegmentTable,
        root: Reference,
        kind: DatabaseKind,
    ) -> Result<Self> {
        let reader = ulmdb_prefix::Reader::new(store, root)?;
        if let Some(prefix) = reader.root_prefix() {
            check_family(kind, prefix.network())
                .map_err(|_| Error::Corrupt("prefix root family mismatch"))?;
        }
        Ok(Self {
            store,
            root,
            kind,
            reader,
        })
    }
    pub fn longest_match(&self, address: Address) -> Result<Option<(Prefix, &'a [u8])>> {
        check_family(self.kind, address)?;
        self.reader.longest_match(address)
    }
    pub fn exact(&self, prefix: Prefix) -> Result<Option<(Prefix, &'a [u8])>> {
        check_family(self.kind, prefix.network())?;
        self.reader.exact(prefix)
    }
    pub fn covering(&self, prefix: Prefix) -> Result<Option<(Prefix, &'a [u8])>> {
        check_family(self.kind, prefix.network())?;
        self.reader.covering(prefix)
    }
    pub fn ancestors(&self, prefix: Prefix) -> Result<ulmdb_prefix::Cursor<'a, SegmentTable>> {
        check_family(self.kind, prefix.network())?;
        ulmdb_prefix::ancestors(self.store, self.root, prefix)
    }
    pub fn subnets(&self, prefix: Prefix) -> Result<ulmdb_prefix::Cursor<'a, SegmentTable>> {
        check_family(self.kind, prefix.network())?;
        ulmdb_prefix::subnets(self.store, self.root, prefix)
    }
    pub fn overlaps(&self, prefix: Prefix) -> Result<ulmdb_prefix::Cursor<'a, SegmentTable>> {
        check_family(self.kind, prefix.network())?;
        ulmdb_prefix::overlaps(self.store, self.root, prefix)
    }
}
