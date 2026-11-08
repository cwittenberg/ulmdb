//! The same borrowed-scope semantics over mapped-tail undo and catalog metadata.
use super::WriteTxn;
use crate::{Error, Result, catalog::Database};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::{Deref, DerefMut},
};
use ulmdb_storage::hybrid::ImageCheckpoint;
struct Metadata {
    catalog: BTreeMap<Vec<u8>, Database>,
    changed: BTreeSet<Vec<u8>>,
    failed: bool,
}
pub struct Savepoint<'scope, 'env> {
    txn: &'scope mut WriteTxn<'env>,
    checkpoint: Option<ImageCheckpoint>,
    metadata: Option<Metadata>,
}
impl<'env> WriteTxn<'env> {
    pub fn savepoint(&mut self) -> Result<Savepoint<'_, 'env>> {
        self.live()?;
        let checkpoint = self.arena.checkpoint()?;
        let metadata = Metadata {
            catalog: self.catalog.clone(),
            changed: self.changed.clone(),
            failed: self.failed,
        };
        Ok(Savepoint {
            txn: self,
            checkpoint: Some(checkpoint),
            metadata: Some(metadata),
        })
    }
}
impl Savepoint<'_, '_> {
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
            self.txn.changed = metadata.changed.clone();
            self.txn.failed = metadata.failed;
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
                self.txn.changed = metadata.changed;
                self.txn.failed = metadata.failed;
            }
        }
        Ok(())
    }
}
impl<'env> Deref for Savepoint<'_, 'env> {
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
