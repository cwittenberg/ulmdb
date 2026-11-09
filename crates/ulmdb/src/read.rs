use crate::{
    Address, Error, Prefix, Result,
    catalog::{Database, DatabaseKind},
    environment::Snapshot,
};
use std::{
    ops::Bound,
    sync::{Arc, Weak},
};

#[cfg(test)]
#[path = "read_position_tests.rs"]
mod position_tests;

pub struct ReadTxn {
    pub(crate) snapshot: Arc<Snapshot>,
}

/// Owned traversal state for one immutable bytewise KV snapshot.
/// No mapping or physical pin is retained after the transaction ends.
pub struct KvPosition {
    snapshot: Weak<Snapshot>,
    position: ulmdb_kv::CursorPosition,
}
impl KvPosition {
    pub fn matches_snapshot(&self, read: &ReadTxn) -> bool {
        self.snapshot.as_ptr() == Arc::as_ptr(&read.snapshot)
    }
    fn store<'a>(&self, read: &'a ReadTxn) -> Result<&'a ulmdb_storage::SegmentTable> {
        if !self.matches_snapshot(read) {
            return Err(Error::StaleHandle);
        }
        read.snapshot.check_external()?;
        Ok(&read.snapshot.segments)
    }
    pub fn seek<'a>(
        &mut self,
        read: &'a ReadTxn,
        bound: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<Option<(&'a [u8], &'a [u8])>> {
        if let Bound::Included(key) | Bound::Excluded(key) = &bound
            && key.len() > read.snapshot.manifest.page_size.native_key_limit()
        {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        let store = self.store(read)?;
        self.position.seek(store, bound, reverse)
    }
    /// One fresh snapshot/store admission for a current-or-adjacent attempt.
    /// Only an unpositioned continuation seeks to the bound. Otherwise an
    /// out-of-bound adjacent row can move the position while returning None.
    /// No page, descriptor or owner is retained beyond this supplied borrow.
    pub fn current_or_step<'a>(
        &mut self,
        read: &'a ReadTxn,
        bound: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<Option<(&'a [u8], &'a [u8])>> {
        if let Bound::Included(key) | Bound::Excluded(key) = &bound
            && key.len() > read.snapshot.manifest.page_size.native_key_limit()
        {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        let store = self.store(read)?;
        self.position.current_or_step(store, bound, reverse)
    }
    pub fn current<'a>(&self, read: &'a ReadTxn) -> Result<Option<(&'a [u8], &'a [u8])>> {
        self.position.current(self.store(read)?)
    }
    pub fn next<'a>(&mut self, read: &'a ReadTxn) -> Result<Option<(&'a [u8], &'a [u8])>> {
        let store = self.store(read)?;
        self.position.next(store)
    }
    pub fn previous<'a>(&mut self, read: &'a ReadTxn) -> Result<Option<(&'a [u8], &'a [u8])>> {
        let store = self.store(read)?;
        self.position.previous(store)
    }
}
impl ReadTxn {
    /// Bind a checked bytewise database once; rows borrow each supplied transaction.
    pub fn kv_position(&self, handle: &crate::DatabaseHandle) -> Result<KvPosition> {
        let view = self.kv_handle(handle)?;
        if self.database_ordering(handle.name())?.keys != crate::ordering::OrderingSpec::Bytewise {
            return Err(Error::InvalidInput(
                "cursor position requires bytewise ordering",
            ));
        }
        Ok(KvPosition {
            snapshot: Arc::downgrade(&self.snapshot),
            position: ulmdb_kv::CursorPosition::new(view.root),
        })
    }
    /// Whether this snapshot resolves committed file data through bounded
    /// authenticated windows. Ordinary readers retain their original mapping path.
    pub fn uses_chunk_cache(&self) -> bool {
        self.snapshot.segments.is_chunked()
    }

    /// Authenticated immutable file descriptor metadata already held by this
    /// reader. No descriptor IO, mutable authority, or new graph certificate.
    pub fn file_manifest(&self) -> Result<&ulmdb_storage::Manifest> {
        self.snapshot.check_external()?;
        if self.snapshot.range_backed {
            return Err(Error::InvalidInput("Range file manifest unsupported"));
        }
        Ok(&self.snapshot.manifest)
    }
    /// Exact immutable file checkpoint of this reader. This is independent of
    /// later CURRENT changes; Range has a different authority and is unsupported.
    pub fn file_checkpoint(&self) -> Result<[u8; 64]> {
        self.snapshot.check_external()?;
        if self.snapshot.range_backed {
            return Err(Error::InvalidInput("Range file checkpoint unsupported"));
        }
        match &self.snapshot.physical_pin {
            Some(pin) => Ok(*pin.root().checkpoint()),
            None => ulmdb_storage::checkpoint_bytes(&self.snapshot.manifest),
        }
    }

    pub fn kv(&self, name: &[u8]) -> Result<crate::KvRead<'_>> {
        let db = self.database(name)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        crate::KvRead::with_order(
            &self.snapshot.segments,
            db.root,
            crate::ordering::BoundComparator::bind(db.ordering.keys, None)?,
        )
    }
    pub fn prefix_index(&self, name: &[u8]) -> Result<crate::PrefixRead<'_>> {
        let db = self.database(name)?;
        if !matches!(db.kind, DatabaseKind::Ipv4 | DatabaseKind::Ipv6) {
            return Err(Error::WrongKind);
        }
        crate::PrefixRead::new(&self.snapshot.segments, db.root, db.kind)
    }
    pub fn transform_context(&self) -> Option<&Arc<ulmdb_storage::transform::TransformContext>> {
        self.snapshot.segments.transform()
    }
    pub fn sequence(&self) -> u64 {
        self.snapshot.manifest.sequence
    }
    pub fn generation(&self) -> u128 {
        self.snapshot.manifest.generation
    }
    pub fn commit_identity(&self) -> u128 {
        self.snapshot.manifest.commit
    }
    pub fn database_kind(&self, name: &[u8]) -> Result<DatabaseKind> {
        Ok(self.database(name)?.kind)
    }
    pub(crate) fn database(&self, name: &[u8]) -> Result<&Database> {
        self.snapshot.check_external()?;
        self.snapshot
            .catalog
            .get(name)
            .ok_or(Error::MissingDatabase)
    }
    #[inline]
    pub fn get(&self, name: &[u8], key: &[u8]) -> Result<Option<&[u8]>> {
        if key.len() > self.snapshot.manifest.page_size.native_key_limit() {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        let db = self.database(name)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if db.ordering.keys == crate::ordering::OrderingSpec::Bytewise {
            return ulmdb_kv::get(&self.snapshot.segments, db.root, key);
        }
        ulmdb_kv::get_with(
            &self.snapshot.segments,
            db.root,
            key,
            &crate::ordering::BoundComparator::bind(db.ordering.keys, None)?,
        )
    }
    pub fn range<'a>(
        &'a self,
        name: &[u8],
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<ulmdb_kv::Cursor<'a, ulmdb_storage::SegmentTable, crate::ordering::BoundComparator>>
    {
        let db = self.database(name)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        ulmdb_kv::Cursor::with_order(
            &self.snapshot.segments,
            db.root,
            lower,
            upper,
            reverse,
            crate::ordering::BoundComparator::bind(db.ordering.keys, None)?,
        )
    }
    pub fn longest_match(&self, name: &[u8], address: Address) -> Result<Option<(Prefix, &[u8])>> {
        let db = self.database(name)?;
        check_family(db.kind, address)?;
        ulmdb_prefix::get(&self.snapshot.segments, db.root, address)
    }
    pub fn exact_prefix(&self, name: &[u8], prefix: Prefix) -> Result<Option<(Prefix, &[u8])>> {
        let db = self.database(name)?;
        check_family(db.kind, prefix.network())?;
        ulmdb_prefix::exact(&self.snapshot.segments, db.root, prefix)
    }
    pub fn covering_prefix(&self, name: &[u8], prefix: Prefix) -> Result<Option<(Prefix, &[u8])>> {
        let db = self.database(name)?;
        check_family(db.kind, prefix.network())?;
        ulmdb_prefix::covering(&self.snapshot.segments, db.root, prefix)
    }
    pub fn subnets(
        &self,
        name: &[u8],
        prefix: Prefix,
    ) -> Result<ulmdb_prefix::Cursor<'_, ulmdb_storage::SegmentTable>> {
        let db = self.database(name)?;
        check_family(db.kind, prefix.network())?;
        ulmdb_prefix::subnets(&self.snapshot.segments, db.root, prefix)
    }
}
pub(crate) fn check_family(kind: DatabaseKind, address: Address) -> Result<()> {
    if matches!(
        (kind, address),
        (DatabaseKind::Ipv4, Address::V4(_)) | (DatabaseKind::Ipv6, Address::V6(_))
    ) {
        Ok(())
    } else {
        Err(Error::WrongKind)
    }
}
