//! Direct ordinary B+tree/radix reads: no WAL or allocation-ledger lookup.
use super::SharedSnapshot;
use crate::{Address, DatabaseKind, Error, Prefix, Result, catalog::Database, read::check_family};
use std::ops::Bound;
use ulmdb_storage::{Reference, resident::View};

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
    pub(super) fn database(&self, name: &[u8]) -> Result<&Database> {
        self.snapshot
            .catalog
            .get(name)
            .ok_or(Error::MissingDatabase)
    }
    pub fn get(&self, name: &[u8], key: &[u8]) -> Result<Option<&[u8]>> {
        self.kv(name)?.get(key)
    }
    pub fn kv(&self, name: &[u8]) -> Result<KvRead<'_>> {
        self.kv_with_ordering(name, &Default::default())
    }
    pub fn kv_with_ordering(
        &self,
        name: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<KvRead<'_>> {
        let db = self.database(name)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        let store = &self.snapshot.image;
        Ok(KvRead {
            store,
            root: db.root,
            bytewise: if db.ordering.keys == crate::ordering::OrderingSpec::Bytewise {
                Some(ulmdb_kv::Reader::new(store, db.root)?)
            } else {
                None
            },
            order: crate::ordering::BoundComparator::bind(
                db.ordering.keys,
                callbacks.keys.as_ref(),
            )?,
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
    /// Declared payload profile in this pinned catalog. First overflow write
    /// promotes KV/IP from4080 to16MiB; old snapshots keep their prior profile.
    pub fn database_value_limit(&self, name: &[u8]) -> Result<usize> {
        self.database(name)?;
        super::catalog_codec::value_limit(
            ulmdb_kv::get(&self.snapshot.image, self.snapshot.root, name)?
                .ok_or(Error::Corrupt("resident descriptor absent"))?,
        )
    }
    pub fn database_count(&self) -> usize {
        self.snapshot.catalog.len()
    }
    /// Checked reachable allocations in this exact pinned pool view. WAL and
    /// checkpoint envelope bytes are separate from these decoded tree metrics.
    pub fn database_stats(&self, name: &[u8]) -> Result<crate::DatabaseStats> {
        let db = self.database(name)?;
        let bytes = (self.snapshot.image.member_count() as u64)
            .checked_mul(4096)
            .ok_or(Error::LimitExceeded)?;
        crate::physical_stats::measure(
            &self.snapshot.image,
            db.root,
            db.kind,
            db.ordering,
            crate::physical_stats::Budget::from_bytes(bytes, 4096)?,
        )
    }
}
pub struct KvRead<'a> {
    store: &'a View,
    root: Reference,
    order: crate::ordering::BoundComparator,
    // Retain the existing decoded-root/word fast path for default databases.
    bytewise: Option<ulmdb_kv::Reader<'a, View>>,
}
impl<'a> KvRead<'a> {
    pub fn get(&self, key: &[u8]) -> Result<Option<&'a [u8]>> {
        if key.len() > super::catalog_codec::MAX_KEY_BYTES {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        match &self.bytewise {
            Some(reader) => reader.get(key),
            None => ulmdb_kv::get_with(self.store, self.root, key, &self.order),
        }
    }
    pub fn cursor(
        &self,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<ulmdb_kv::Cursor<'a, View, crate::ordering::BoundComparator>> {
        for bound in [&lower, &upper] {
            if let std::ops::Bound::Included(k) | std::ops::Bound::Excluded(k) = bound {
                if k.len() > 4080 {
                    return Err(Error::InvalidInput("resident key size limit4080"));
                }
            }
        }
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
    store: &'a View,
    root: Reference,
    kind: DatabaseKind,
    reader: ulmdb_prefix::Reader<'a, View>,
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
    pub fn ancestors(&self, prefix: Prefix) -> Result<ulmdb_prefix::Cursor<'a, View>> {
        check_family(self.kind, prefix.network())?;
        ulmdb_prefix::ancestors(self.store, self.root, prefix)
    }
    pub fn subnets(&self, prefix: Prefix) -> Result<ulmdb_prefix::Cursor<'a, View>> {
        check_family(self.kind, prefix.network())?;
        ulmdb_prefix::subnets(self.store, self.root, prefix)
    }
    pub fn overlaps(&self, prefix: Prefix) -> Result<ulmdb_prefix::Cursor<'a, View>> {
        check_family(self.kind, prefix.network())?;
        ulmdb_prefix::overlaps(self.store, self.root, prefix)
    }
}
