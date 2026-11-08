//! Borrowed nested writer scopes. The guard owns transaction metadata and an
//! arena checkpoint; dropping a guard restores both, including failed state.
use crate::{Error, Result, WriteTxn, catalog::Database};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::{Deref, DerefMut},
    sync::Arc,
};
use ulmdb_storage::ArenaCheckpoint;

struct Metadata {
    catalog: BTreeMap<Vec<u8>, Database>,
    detached_roots: BTreeMap<u64, Arc<crate::detached::DetachedRoot>>,
    database_views: Option<crate::database_view::ViewRegistry>,
    created_database_ids: BTreeSet<u64>,
    changed: BTreeSet<Vec<u8>>,
    failed: bool,
    local_durability: crate::Durability,
    value_limit: crate::ValueLimit,
    catalog_capacity: crate::CatalogCapacity,
}

/// A nested writer scope. `release` accepts its changes; `abort` or ordinary
/// drop restores the state at entry. The parent cannot be moved into `commit`
/// while borrowed by this guard. Use `scope.savepoint()` to nest another guard.
pub struct Savepoint<'scope, 'env> {
    txn: &'scope mut WriteTxn<'env>,
    checkpoint: Option<ArenaCheckpoint>,
    metadata: Option<Metadata>,
}
impl<'env> WriteTxn<'env> {
    pub fn savepoint(&mut self) -> Result<Savepoint<'_, 'env>> {
        self.live()?;
        let checkpoint = self.arena.checkpoint()?;
        let metadata = Metadata {
            catalog: self.catalog.clone(),
            detached_roots: self.detached_roots.clone(),
            database_views: self.database_views.clone(),
            created_database_ids: self.created_database_ids.clone(),
            changed: self.changed.clone(),
            failed: self.failed,
            local_durability: self.local_durability,
            value_limit: self.value_limit,
            catalog_capacity: self.catalog_capacity,
        };
        Ok(Savepoint {
            txn: self,
            checkpoint: Some(checkpoint),
            metadata: Some(metadata),
        })
    }
}
impl Savepoint<'_, '_> {
    /// Accept changes into the parent. A failed child cannot be released: this
    /// returns its error and automatically rolls the failed child back.
    pub fn release(mut self) -> Result<()> {
        self.txn.live()?;
        if let Some(checkpoint) = &self.checkpoint {
            self.txn.arena.release_checkpoint(checkpoint)?;
        }
        self.checkpoint = None;
        self.metadata = None;
        Ok(())
    }
    pub fn abort(mut self) -> Result<()> {
        self.restore()
    }
    /// Restore entry state and retain this scope for another attempt. This also
    /// clears a failed child's state. Database identities remain monotonic.
    pub fn rollback(&mut self) -> Result<()> {
        let checkpoint = self
            .checkpoint
            .as_ref()
            .ok_or(Error::InvalidInput("inactive savepoint"))?;
        if let Err(error) = self.txn.arena.rollback_checkpoint(checkpoint) {
            self.txn.failed = true;
            return Err(error);
        }
        self.checkpoint = None;
        if let Some(metadata) = &self.metadata {
            self.txn.catalog = metadata.catalog.clone();
            self.txn.detached_roots = metadata.detached_roots.clone();
            self.txn.database_views = metadata.database_views.clone();
            self.txn.created_database_ids = metadata.created_database_ids.clone();
            self.txn.changed = metadata.changed.clone();
            self.txn.failed = metadata.failed;
            self.txn.local_durability = metadata.local_durability;
            self.txn.catalog_capacity = metadata.catalog_capacity;
            self.txn.value_limit = metadata.value_limit;
            self.txn.arena.set_value_limit(metadata.value_limit);
        }
        match self.txn.arena.checkpoint() {
            Ok(checkpoint) => {
                self.checkpoint = Some(checkpoint);
                Ok(())
            }
            Err(error) => {
                self.txn.failed = true;
                Err(error)
            }
        }
    }
    fn restore(&mut self) -> Result<()> {
        if let Some(checkpoint) = &self.checkpoint {
            self.txn.arena.rollback_checkpoint(checkpoint)?;
            self.checkpoint = None;
            if let Some(metadata) = self.metadata.take() {
                self.txn.catalog = metadata.catalog;
                self.txn.detached_roots = metadata.detached_roots;
                self.txn.database_views = metadata.database_views;
                self.txn.created_database_ids = metadata.created_database_ids;
                self.txn.changed = metadata.changed;
                self.txn.failed = metadata.failed;
                self.txn.local_durability = metadata.local_durability;
                self.txn.catalog_capacity = metadata.catalog_capacity;
                self.txn.value_limit = metadata.value_limit;
                self.txn.arena.set_value_limit(metadata.value_limit);
            }
        }
        Ok(())
    }
}
impl<'scope, 'env> Deref for Savepoint<'scope, 'env> {
    type Target = WriteTxn<'env>;
    fn deref(&self) -> &Self::Target {
        self.txn
    }
}
impl DerefMut for Savepoint<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.txn
    }
}
impl Drop for Savepoint<'_, '_> {
    fn drop(&mut self) {
        if self.restore().is_err() {
            self.txn.failed = true;
        }
    }
}
