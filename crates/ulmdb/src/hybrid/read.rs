//! Direct ordinary B+tree/radix reads: no WAL or allocation-ledger lookup.
use super::SharedSnapshot;
use crate::{Address, DatabaseKind, Error, Prefix, Result, catalog::Database, read::check_family};
use std::ops::Bound;
use ulmdb_storage::{Reference, hybrid::ImageTable};

pub struct ReadTxn {
    pub(super) snapshot: SharedSnapshot,
}
impl ReadTxn {
    pub fn sequence(&self) -> u64 {
        self.snapshot.receipt.sequence
    }
    pub fn commit_identity(&self) -> u128 {
        self.snapshot.commit
    }
    fn database(&self, name: &[u8]) -> Result<&Database> {
        self.snapshot
            .catalog
            .get(name)
            .ok_or(Error::MissingDatabase)
    }
    pub fn get(&self, name: &[u8], key: &[u8]) -> Result<Option<&[u8]>> {
        self.kv(name)?.get(key)
    }
    pub fn kv(&self, name: &[u8]) -> Result<KvRead<'_>> {
        let db = self.database(name)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        let store = &self.snapshot.image;
        Ok(KvRead {
            store,
            root: db.root,
            reader: ulmdb_kv::Reader::new(store, db.root)?,
        })
    }
    pub fn prefix_index(&self, name: &[u8]) -> Result<PrefixRead<'_>> {
        let db = self.database(name)?;
        if !matches!(db.kind, DatabaseKind::Ipv4 | DatabaseKind::Ipv6) {
            return Err(Error::WrongKind);
        }
        let store = &self.snapshot.image;
        let reader = ulmdb_prefix::Reader::new(store, db.root)?;
        if let Some(p) = reader.root_prefix() {
            check_family(db.kind, p.network())?;
        }
        Ok(PrefixRead {
            store,
            root: db.root,
            kind: db.kind,
            reader,
        })
    }
    pub fn longest_match(&self, name: &[u8], address: Address) -> Result<Option<(Prefix, &[u8])>> {
        self.prefix_index(name)?.longest_match(address)
    }
    pub fn database_count(&self) -> usize {
        self.snapshot.catalog.len()
    }
}
pub struct KvRead<'a> {
    store: &'a ImageTable,
    root: Reference,
    reader: ulmdb_kv::Reader<'a, ImageTable>,
}
impl<'a> KvRead<'a> {
    pub fn get(&self, key: &[u8]) -> Result<Option<&'a [u8]>> {
        if key.len() > 4096 {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        self.reader.get(key)
    }
    pub fn cursor(
        &self,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<ulmdb_kv::Cursor<'a, ImageTable>> {
        ulmdb_kv::Cursor::new(self.store, self.root, lower, upper, reverse)
    }
}
pub struct PrefixRead<'a> {
    store: &'a ImageTable,
    root: Reference,
    kind: DatabaseKind,
    reader: ulmdb_prefix::Reader<'a, ImageTable>,
}
impl<'a> PrefixRead<'a> {
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
    pub fn ancestors(&self, prefix: Prefix) -> Result<ulmdb_prefix::Cursor<'a, ImageTable>> {
        check_family(self.kind, prefix.network())?;
        ulmdb_prefix::ancestors(self.store, self.root, prefix)
    }
    pub fn subnets(&self, prefix: Prefix) -> Result<ulmdb_prefix::Cursor<'a, ImageTable>> {
        check_family(self.kind, prefix.network())?;
        ulmdb_prefix::subnets(self.store, self.root, prefix)
    }
    pub fn overlaps(&self, prefix: Prefix) -> Result<ulmdb_prefix::Cursor<'a, ImageTable>> {
        check_family(self.kind, prefix.network())?;
        ulmdb_prefix::overlaps(self.store, self.root, prefix)
    }
}
