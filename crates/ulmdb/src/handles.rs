//! Checked process-local database identities. Prepared views bind once per batch.
use crate::catalog::Database;
use crate::{
    DatabaseKind, DuplicatesRead, Error, KvRead, Prefix, PrefixRead, ReadTxn, Result, WriteTxn,
};

/// A handle is scoped to one open environment instance. Dropping/recreating a
/// name never rebinds its previous handle; aborted provisional IDs are burned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatabaseHandle {
    name: Box<[u8]>,
    id: u64,
    owner: u128,
    kind: DatabaseKind,
}
impl DatabaseHandle {
    fn new(owner: u128, name: &[u8], database: &Database) -> Self {
        Self {
            name: name.into(),
            id: database.id,
            owner,
            kind: database.kind,
        }
    }
    pub fn name(&self) -> &[u8] {
        &self.name
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn kind(&self) -> DatabaseKind {
        self.kind
    }
    fn check(&self, owner: u128, database: Option<&Database>) -> Result<()> {
        if owner != self.owner
            || !database.is_some_and(|db| db.id == self.id && db.kind == self.kind)
        {
            return Err(Error::StaleHandle);
        }
        Ok(())
    }
}
impl ReadTxn {
    pub fn open_database(&self, name: &[u8]) -> Result<DatabaseHandle> {
        Ok(DatabaseHandle::new(
            self.snapshot.owner,
            name,
            self.database(name)?,
        ))
    }
    fn checked_handle(&self, handle: &DatabaseHandle) -> Result<&Database> {
        self.snapshot.check_external()?;
        let database = self.snapshot.catalog.get(handle.name());
        handle.check(self.snapshot.owner, database)?;
        database.ok_or(Error::StaleHandle)
    }
    pub fn kv_handle(&self, handle: &DatabaseHandle) -> Result<KvRead<'_>> {
        let database = self.checked_handle(handle)?;
        if database.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        KvRead::with_order(
            &self.snapshot.segments,
            database.root,
            crate::ordering::BoundComparator::bind(database.ordering.keys, None)?,
        )
    }
    pub fn prefix_handle(&self, handle: &DatabaseHandle) -> Result<PrefixRead<'_>> {
        let database = self.checked_handle(handle)?;
        if !matches!(database.kind, DatabaseKind::Ipv4 | DatabaseKind::Ipv6) {
            return Err(Error::WrongKind);
        }
        PrefixRead::new(&self.snapshot.segments, database.root, database.kind)
    }
    pub fn duplicates_handle(&self, handle: &DatabaseHandle) -> Result<DuplicatesRead<'_>> {
        self.checked_handle(handle)?;
        self.duplicates(handle.name())
    }
    pub fn get_handle(&self, handle: &DatabaseHandle, key: &[u8]) -> Result<Option<&[u8]>> {
        self.kv_handle(handle)?.get(key)
    }
}
impl WriteTxn<'_> {
    pub fn open_database(&self, name: &[u8]) -> Result<DatabaseHandle> {
        self.live()?;
        Ok(DatabaseHandle::new(
            self.base.owner,
            name,
            self.catalog.get(name).ok_or(Error::MissingDatabase)?,
        ))
    }
    pub(crate) fn check_handle(&self, handle: &DatabaseHandle) -> Result<()> {
        self.live()?;
        handle.check(self.base.owner, self.catalog.get(handle.name()))
    }
    pub fn get_handle(&self, handle: &DatabaseHandle, key: &[u8]) -> Result<Option<&[u8]>> {
        self.check_handle(handle)?;
        self.get(handle.name(), key)
    }
    pub fn put_handle(&mut self, handle: &DatabaseHandle, key: &[u8], value: &[u8]) -> Result<()> {
        self.check_handle(handle)?;
        self.put(handle.name(), key, value)
    }
    pub fn delete_handle(&mut self, handle: &DatabaseHandle, key: &[u8]) -> Result<()> {
        self.check_handle(handle)?;
        self.delete(handle.name(), key)
    }
    pub fn insert_prefix_handle(
        &mut self,
        handle: &DatabaseHandle,
        prefix: Prefix,
        value: &[u8],
    ) -> Result<()> {
        self.check_handle(handle)?;
        self.insert_prefix(handle.name(), prefix, value)
    }
    pub fn delete_prefix_handle(&mut self, handle: &DatabaseHandle, prefix: Prefix) -> Result<()> {
        self.check_handle(handle)?;
        self.delete_prefix(handle.name(), prefix)
    }
    pub fn insert_duplicate_handle(
        &mut self,
        handle: &DatabaseHandle,
        key: &[u8],
        value: &[u8],
    ) -> Result<bool> {
        self.check_handle(handle)?;
        self.insert_duplicate(handle.name(), key, value)
    }
    pub fn delete_duplicate_handle(
        &mut self,
        handle: &DatabaseHandle,
        key: &[u8],
        value: &[u8],
    ) -> Result<bool> {
        self.check_handle(handle)?;
        self.delete_duplicate(handle.name(), key, value)
    }
    pub fn drop_database_handle(&mut self, handle: &DatabaseHandle) -> Result<()> {
        self.check_handle(handle)?;
        self.drop_database(handle.name())
    }
}
