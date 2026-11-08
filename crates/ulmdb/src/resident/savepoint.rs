//! Child release is private only. The outer commit remains the sole WAL
//! publication. Drop or abort restores the catalog and touched private pages.
use super::{WriteTxn, catalog};
use crate::Result;
use std::ops::{Deref, DerefMut};
use ulmdb_storage::resident::ArenaCheckpoint;
struct Metadata {
    catalog: catalog::Edit,
    failed: bool,
    catalog_capacity: crate::CatalogCapacity,
    catalog_profile_changed: bool,
    overflow_databases: std::collections::BTreeSet<u64>,
}
/// Nested Resident writer. `release`/`commit` merge into the parent; ordinary
/// drop and `abort` undo child changes. Up to 64 scopes can be active.
pub struct Savepoint<'scope, 'env> {
    txn: &'scope mut WriteTxn<'env>,
    checkpoint: Option<ArenaCheckpoint>,
    metadata: Option<Metadata>,
}
impl<'env> WriteTxn<'env> {
    pub fn savepoint(&mut self) -> Result<Savepoint<'_, 'env>> {
        self.live()?;
        let metadata = Metadata {
            catalog: self.catalog.clone(),
            failed: self.failed,
            catalog_capacity: self.catalog_capacity,
            catalog_profile_changed: self.catalog_profile_changed,
            overflow_databases: self.overflow_databases.clone(),
        };
        let checkpoint = self.arena.as_mut().unwrap().checkpoint()?;
        Ok(Savepoint {
            txn: self,
            checkpoint: Some(checkpoint),
            metadata: Some(metadata),
        })
    }
}
impl Savepoint<'_, '_> {
    /// Accept private changes without publishing a WAL record.
    pub fn release(mut self) -> Result<()> {
        self.txn.live()?;
        if let Some(token) = &self.checkpoint {
            self.txn.arena.as_mut().unwrap().release_checkpoint(token)?;
        }
        self.checkpoint = None;
        self.metadata = None;
        Ok(())
    }
    /// Alias for `release`; only the outer transaction makes changes durable.
    pub fn commit(self) -> Result<()> {
        self.release()
    }
    pub fn abort(mut self) -> Result<()> {
        self.restore()
    }
    /// Undo this attempt and keep the scope for another attempt.
    pub fn rollback(&mut self) -> Result<()> {
        self.restore()?;
        let metadata = Metadata {
            catalog: self.txn.catalog.clone(),
            failed: self.txn.failed,
            catalog_capacity: self.txn.catalog_capacity,
            catalog_profile_changed: self.txn.catalog_profile_changed,
            overflow_databases: self.txn.overflow_databases.clone(),
        };
        match self.txn.arena.as_mut().unwrap().checkpoint() {
            Ok(token) => {
                self.checkpoint = Some(token);
                self.metadata = Some(metadata);
                Ok(())
            }
            Err(error) => {
                self.txn.failed = true;
                Err(error)
            }
        }
    }
    fn restore(&mut self) -> Result<()> {
        if let Some(token) = &self.checkpoint {
            self.txn
                .arena
                .as_mut()
                .unwrap()
                .rollback_checkpoint(token)?;
            self.checkpoint = None;
            if let Some(metadata) = self.metadata.take() {
                self.txn.catalog = metadata.catalog;
                self.txn.failed = metadata.failed;
                self.txn.catalog_capacity = metadata.catalog_capacity;
                self.txn.catalog_profile_changed = metadata.catalog_profile_changed;
                self.txn.overflow_databases = metadata.overflow_databases;
            }
        }
        // Database identities, like pool generations, remain monotonic.
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

#[cfg(test)]
mod tests {
    use crate::{
        DatabaseKind,
        resident::{Environment, Options},
    };
    #[test]
    fn aborted_child_database_identities_are_not_reallocated_by_parent() {
        let path = std::env::temp_dir().join(format!(
            "resident-child-ids-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let env = Environment::create(&path, Options::default()).unwrap();
        let mut parent = env.begin_write().unwrap();
        parent.create_database(b"keep", DatabaseKind::Kv).unwrap();
        let burned;
        {
            let mut child = parent.savepoint().unwrap();
            child
                .create_database(b"temporary", DatabaseKind::Kv)
                .unwrap();
            burned = child
                .catalog
                .get(&child.base.catalog, b"temporary")
                .unwrap()
                .id;
            child.abort().unwrap();
        }
        parent
            .create_database(b"replacement", DatabaseKind::Kv)
            .unwrap();
        let replacement = parent
            .catalog
            .get(&parent.base.catalog, b"replacement")
            .unwrap()
            .id;
        assert!(replacement > burned);
        parent.commit().unwrap();
        drop(env);
        let env = Environment::open(&path, Options::default()).unwrap();
        let reader = env.begin_read();
        assert_eq!(
            reader.snapshot.catalog.get(b"replacement").unwrap().id,
            replacement
        );
        assert!(reader.snapshot.next_database > replacement);
        drop(reader);
        drop(env);
        std::fs::remove_dir_all(path).unwrap();
    }
}
