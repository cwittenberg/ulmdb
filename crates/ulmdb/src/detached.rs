//! Transaction-local read authority for databases removed from the live catalog.
//!
//! Removal is the immutability boundary: mutators resolve only live catalog
//! entries, and recreating a name starts with a new identity and empty root.
//! Detached private objects therefore cannot be rewritten by later operations.
//! Arena rollback can truncate or restore those bytes, so savepoints restore the
//! capability map with the catalog and revoke every root issued after entry.
use crate::{DatabaseHandle, DatabaseKind, Error, Result, WriteTxn};
use std::sync::Arc;
use ulmdb_core::storage::Reference;

pub(crate) struct DetachedRoot {
    handle: DatabaseHandle,
    root: Reference,
    ordering: crate::ordering::DatabaseOrdering,
}

/// An opaque read capability issued when a writer removes a database.
///
/// Reads require the exact originating writer, including its active savepoint
/// state. The capability owns metadata, not an independent snapshot; it cannot
/// read after that writer ends or grant mutation authority. A child rollback
/// revokes capabilities issued by the child, even if its arena offsets are
/// reused. Releasing a child preserves its capabilities in the parent.
#[derive(Clone)]
pub struct DetachedDatabase(Arc<DetachedRoot>);

impl std::fmt::Debug for DetachedDatabase {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("DetachedDatabase")
            .field(&self.0.handle)
            .finish()
    }
}

impl DetachedDatabase {
    pub fn name(&self) -> &[u8] {
        self.0.handle.name()
    }
    pub fn id(&self) -> u64 {
        self.0.handle.id()
    }
    pub fn kind(&self) -> DatabaseKind {
        self.0.handle.kind()
    }
    /// Return the original identity. Ordinary handle operations still reject it
    /// after removal; this does not convert detached read authority into a writer.
    pub fn handle(&self) -> &DatabaseHandle {
        &self.0.handle
    }
}

impl WriteTxn<'_> {
    /// Maximum retained detached identities in one writer. Retained records
    /// contain only metadata; their existing arena bytes remain within the
    /// ordinary arena budget and no database rows are copied.
    pub const MAX_DETACHED_DATABASES: usize = 4096;

    /// Remove an exact database identity and retain read authority for its
    /// current private root. The identity is absent from the writable catalog
    /// before the capability is returned. Recreating the name never rebinds it.
    pub fn detach_database_handle(&mut self, handle: &DatabaseHandle) -> Result<DetachedDatabase> {
        self.live()?;
        let current = match self.open_database(handle.name()) {
            Err(Error::MissingDatabase) => return Err(Error::StaleHandle),
            result => result?,
        };
        if &current != handle {
            return Err(Error::StaleHandle);
        }
        if self.detached_roots.len() >= Self::MAX_DETACHED_DATABASES {
            return Err(Error::LimitExceeded);
        }
        let ordering = self
            .catalog
            .get(handle.name())
            .ok_or(Error::StaleHandle)?
            .ordering;
        let root = self
            .catalog
            .get(handle.name())
            .ok_or(Error::StaleHandle)?
            .root;
        let retained = Arc::new(DetachedRoot {
            handle: current,
            root,
            ordering,
        });
        // Mark first: all ordinary fallible checks precede catalog removal.
        self.mark_changed(handle.name());
        self.detached_roots.insert(handle.id(), retained.clone());
        self.catalog
            .remove(handle.name())
            .ok_or(Error::StaleHandle)?;
        self.created_database_ids.remove(&handle.id());
        self.remove_database_view_identity(handle.id());
        Ok(DetachedDatabase(retained))
    }

    fn detached_root(&self, database: &DetachedDatabase) -> Result<&DetachedRoot> {
        self.live()?;
        self.detached_roots
            .get(&database.id())
            .filter(|root| Arc::ptr_eq(root, &database.0))
            .map(Arc::as_ref)
            .ok_or(Error::StaleHandle)
    }

    /// Borrow a KV value or the first duplicate value from a retained root.
    /// Returned bytes borrow this writer and cannot survive a later mutation.
    pub fn detached_get(&self, database: &DetachedDatabase, key: &[u8]) -> Result<Option<&[u8]>> {
        self.detached_get_with_ordering(database, key, &Default::default())
    }
    pub fn detached_get_with_ordering(
        &self,
        database: &DetachedDatabase,
        key: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<Option<&[u8]>> {
        let root = self.detached_root(database)?;
        let keys =
            crate::ordering::BoundComparator::bind(root.ordering.keys, callbacks.keys.as_ref())?;
        let values = crate::ordering::BoundComparator::bind(
            root.ordering.duplicates,
            callbacks.duplicates.as_ref(),
        )?;
        if key.len() > self.base.manifest.page_size.native_key_limit() {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        use ulmdb_kv::ordering::Comparator;
        match database.kind() {
            DatabaseKind::Kv => ulmdb_kv::get_with(&self.arena, root.root, key, &keys),
            DatabaseKind::Duplicates { .. } => crate::duplicates::seek_pair_with(
                &self.arena,
                root.root,
                database.kind(),
                Some((key, None)),
                true,
                false,
                &keys,
                &values,
            )
            .map(|pair| {
                pair.filter(|(stored, _)| keys.compare(stored, key).is_eq())
                    .map(|(_, v)| v)
            }),
            _ => Err(Error::WrongKind),
        }
    }

    /// Seek one borrowed pair without materializing rows. Bounds and direction
    /// match [`Self::client_seek_pair_borrowed`], including duplicate values.
    pub fn detached_seek_pair_borrowed(
        &self,
        database: &DetachedDatabase,
        bound: Option<(&[u8], Option<&[u8]>)>,
        inclusive: bool,
        reverse: bool,
    ) -> Result<Option<(&[u8], &[u8])>> {
        self.detached_seek_pair_with_ordering(
            database,
            bound,
            inclusive,
            reverse,
            &Default::default(),
        )
    }
    pub fn detached_seek_pair_with_ordering(
        &self,
        database: &DetachedDatabase,
        bound: Option<(&[u8], Option<&[u8]>)>,
        inclusive: bool,
        reverse: bool,
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<Option<(&[u8], &[u8])>> {
        let r = self.detached_root(database)?;
        crate::duplicates::seek_pair_with(
            &self.arena,
            r.root,
            database.kind(),
            bound,
            inclusive,
            reverse,
            &crate::ordering::BoundComparator::bind(r.ordering.keys, callbacks.keys.as_ref())?,
            &crate::ordering::BoundComparator::bind(
                r.ordering.duplicates,
                callbacks.duplicates.as_ref(),
            )?,
        )
    }
}

impl WriteTxn<'_> {
    pub fn detached_database_stats(
        &self,
        database: &DetachedDatabase,
    ) -> Result<crate::DatabaseStats> {
        let r = self.detached_root(database)?;
        crate::physical_stats::measure(
            &self.arena,
            r.root,
            database.kind(),
            r.ordering,
            self.physical_budget()?,
        )
    }
}
