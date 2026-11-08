//! Actual KV/radix mutation; literal slot state is selected only after two fences.
use super::{
    Environment, SharedSnapshot, Snapshot, catalog_codec, environment::Writer, journal::Journal,
};
use crate::{Address, DatabaseKind, Error, Prefix, Result, catalog::Database, read::check_family};
use std::sync::{Arc, MutexGuard};
use ulmdb_storage::{
    Reference,
    resident::{
        Arena,
        wal_codec::{Kind, Record},
    },
};
pub struct WriteTxn<'a> {
    pub(super) env: &'a Environment,
    pub(super) guard: MutexGuard<'a, Writer>,
    pub(super) base: SharedSnapshot,
    pub(super) arena: Option<Arena>,
    pub(super) catalog: super::catalog::Edit,
    pub(super) next_database: u64,
    pub(super) failed: bool,
    pub(super) catalog_capacity: crate::CatalogCapacity,
    pub(super) catalog_profile_changed: bool,
    pub(super) overflow_databases: std::collections::BTreeSet<u64>,
}
impl WriteTxn<'_> {
    pub(super) fn live(&self) -> Result<()> {
        if self.guard.poisoned || self.guard.journal.is_poisoned() {
            return Err(Error::OutcomeUnknown);
        }
        if self.failed {
            return Err(Error::InvalidInput(
                "failed resident transaction requires abort",
            ));
        }
        Ok(())
    }
    fn changed(&mut self, name: &[u8], root: Reference) {
        self.catalog.set_root(&self.base.catalog, name, root);
    }
    pub fn create_database(&mut self, name: &[u8], kind: DatabaseKind) -> Result<()> {
        self.live()?;
        if name.is_empty()
            || name.len() > 255
            || self.catalog.contains_key(&self.base.catalog, name)
        {
            return Err(Error::InvalidInput(
                "invalid/existing resident database name",
            ));
        }
        if matches!(kind, DatabaseKind::Duplicates { width: Some(width) } if width > catalog_codec::MAX_DUPLICATE_VALUE_BYTES as u32)
        {
            return Err(Error::InvalidInput(
                "resident fixed duplicate width exceeds65536",
            ));
        }
        if self.catalog.len() >= self.catalog_capacity.get() {
            return Err(Error::LimitExceeded);
        }
        let id = self.next_database;
        self.next_database = id.checked_add(1).ok_or(Error::LimitExceeded)?;
        if matches!(kind,DatabaseKind::Duplicates {width:Some(w)} if w>4080) {
            self.overflow_databases.insert(id);
        }
        self.catalog.insert(
            &self.base.catalog,
            name.to_vec(),
            Database {
                ordering: crate::ordering::DatabaseOrdering::default(),
                id,
                kind,
                root: Reference::NULL,
            },
        );
        Ok(())
    }
    pub fn drop_database(&mut self, name: &[u8]) -> Result<()> {
        self.live()?;
        let dropped = self
            .catalog
            .remove(&self.base.catalog, name)
            .ok_or(Error::MissingDatabase)?;
        self.overflow_databases.remove(&dropped.id);
        Ok(())
    }
    pub fn get(&self, name: &[u8], key: &[u8]) -> Result<Option<&[u8]>> {
        self.get_with_ordering(name, key, &Default::default())
    }
    pub fn get_with_ordering(
        &self,
        name: &[u8],
        key: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<Option<&[u8]>> {
        self.live()?;
        if key.len() > catalog_codec::MAX_KEY_BYTES {
            return Err(Error::InvalidInput("resident key limit4080"));
        }
        let db = self
            .catalog
            .get(&self.base.catalog, name)
            .ok_or(Error::MissingDatabase)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        let order =
            crate::ordering::BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?;
        ulmdb_kv::get_with(self.arena.as_ref().unwrap(), db.root, key, &order)
    }
    pub fn put(&mut self, name: &[u8], key: &[u8], value: &[u8]) -> Result<()> {
        self.put_with_ordering(name, key, value, &Default::default())
    }
    pub fn put_with_ordering(
        &mut self,
        name: &[u8],
        key: &[u8],
        value: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<()> {
        self.live()?;
        if key.len() > catalog_codec::MAX_KEY_BYTES
            || value.len() > catalog_codec::OVERFLOW_VALUE_BYTES
        {
            return Err(Error::InvalidInput(
                "resident key limit4080/value limit16MiB",
            ));
        }
        let db = self
            .catalog
            .get(&self.base.catalog, name)
            .ok_or(Error::MissingDatabase)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if value.len() > catalog_codec::MAX_VALUE_BYTES
            && self.get_with_ordering(name, key, callbacks)? == Some(value)
        {
            return Ok(());
        }
        let id = db.id;
        let order =
            crate::ordering::BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?;
        ulmdb_kv::ordering::Comparator::validate(&order, key)?;
        let old = db.root;
        let revision = self.arena.as_ref().unwrap().revision();
        self.admit_overflow(value.len())?;
        self.failed = true;
        let root = ulmdb_kv::put_with(self.arena.as_mut().unwrap(), old, key, value, &order)?;
        self.failed = false;
        if value.len() > catalog_codec::MAX_VALUE_BYTES {
            self.overflow_databases.insert(id);
        }
        if (old, revision) != (root, self.arena.as_ref().unwrap().revision()) {
            self.changed(name, root);
        }
        Ok(())
    }
    pub fn delete(&mut self, name: &[u8], key: &[u8]) -> Result<bool> {
        self.delete_with_ordering(name, key, &Default::default())
    }
    pub fn delete_with_ordering(
        &mut self,
        name: &[u8],
        key: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<bool> {
        self.live()?;
        if self.get_with_ordering(name, key, callbacks)?.is_none() {
            return Ok(false);
        }
        let db = self.catalog.get(&self.base.catalog, name).unwrap();
        let root = db.root;
        let order =
            crate::ordering::BoundComparator::bind(db.ordering.keys, callbacks.keys.as_ref())?;
        self.failed = true;
        let root = ulmdb_kv::delete_with(self.arena.as_mut().unwrap(), root, key, &order)?;
        self.failed = false;
        self.changed(name, root);
        Ok(true)
    }
    pub fn insert_prefix(&mut self, name: &[u8], prefix: Prefix, value: &[u8]) -> Result<()> {
        self.live()?;
        if value.len() > catalog_codec::OVERFLOW_VALUE_BYTES {
            return Err(Error::InvalidInput("resident prefix value limit16MiB"));
        }
        let db = self
            .catalog
            .get(&self.base.catalog, name)
            .ok_or(Error::MissingDatabase)?;
        check_family(db.kind, prefix.network())?;
        if value.len() > catalog_codec::MAX_VALUE_BYTES
            && ulmdb_prefix::Reader::new(self.arena.as_ref().unwrap(), db.root)?
                .exact(prefix)?
                .map(|(_, bytes)| bytes)
                == Some(value)
        {
            return Ok(());
        }
        let id = db.id;
        let old = db.root;
        let revision = self.arena.as_ref().unwrap().revision();
        self.admit_overflow(value.len())?;
        self.failed = true;
        let root = ulmdb_prefix::insert(self.arena.as_mut().unwrap(), old, prefix, value)?;
        self.failed = false;
        if value.len() > catalog_codec::MAX_VALUE_BYTES {
            self.overflow_databases.insert(id);
        }
        if (old, revision) != (root, self.arena.as_ref().unwrap().revision()) {
            self.changed(name, root);
        }
        Ok(())
    }
    pub fn delete_prefix(&mut self, name: &[u8], prefix: Prefix) -> Result<()> {
        self.live()?;
        let db = self
            .catalog
            .get(&self.base.catalog, name)
            .ok_or(Error::MissingDatabase)?;
        check_family(db.kind, prefix.network())?;
        let old = db.root;
        let revision = self.arena.as_ref().unwrap().revision();
        self.failed = true;
        let root = ulmdb_prefix::delete(self.arena.as_mut().unwrap(), old, prefix)?;
        self.failed = false;
        if (old, revision) != (root, self.arena.as_ref().unwrap().revision()) {
            self.changed(name, root);
        }
        Ok(())
    }
    pub fn longest_match(&self, name: &[u8], address: Address) -> Result<Option<(Prefix, &[u8])>> {
        self.live()?;
        let db = self
            .catalog
            .get(&self.base.catalog, name)
            .ok_or(Error::MissingDatabase)?;
        check_family(db.kind, address)?;
        ulmdb_prefix::get(self.arena.as_ref().unwrap(), db.root, address)
    }
    pub(super) fn admit_overflow(&self, len: usize) -> Result<()> {
        if len > catalog_codec::MAX_VALUE_BYTES {
            let bound = self.arena.as_ref().unwrap().overflow_record_bound(len)?;
            if bound > ulmdb_storage::resident::wal_codec::MAX_RECORD_BYTES as u64
                || bound
                    .checked_add(ulmdb_storage::resident::wal_codec::DATA_START)
                    .is_none_or(|n| n > self.guard.journal.capacity())
            {
                return Err(Error::LimitExceeded);
            }
        }
        Ok(())
    }
    fn database_value_limit(&self, name: &[u8], db: &Database) -> Result<usize> {
        if self.overflow_databases.contains(&db.id) {
            return Ok(if matches!(db.kind, DatabaseKind::Duplicates { .. }) {
                catalog_codec::MAX_DUPLICATE_VALUE_BYTES
            } else {
                catalog_codec::OVERFLOW_VALUE_BYTES
            });
        }
        if self
            .base
            .catalog
            .get(name)
            .is_some_and(|old| old.id == db.id)
        {
            let bytes = ulmdb_kv::get(&self.base.image, self.base.root, name)?
                .ok_or(Error::Corrupt("resident base descriptor absent"))?;
            return catalog_codec::value_limit(bytes);
        }
        Ok(catalog_codec::MAX_VALUE_BYTES)
    }
    pub fn commit(mut self) -> Result<u64> {
        self.live()?;
        if self.catalog.is_unchanged() && !self.catalog_profile_changed {
            return Ok(self.base.receipt.sequence);
        }
        let mut root = self.base.root;
        if self.catalog_profile_changed {
            root = match self.catalog_capacity.marker() {
                Some(bytes) => ulmdb_kv::put(self.arena.as_mut().unwrap(), root, b"", &bytes)?,
                None => ulmdb_kv::delete(self.arena.as_mut().unwrap(), root, b"")?,
            };
        }
        for name in self.catalog.changed() {
            root = if let Some(db) = self.catalog.get(&self.base.catalog, name) {
                let bytes =
                    catalog_codec::encode_with_limit(db, self.database_value_limit(name, db)?);
                ulmdb_kv::put(self.arena.as_mut().unwrap(), root, name, &bytes)?
            } else {
                ulmdb_kv::delete(self.arena.as_mut().unwrap(), root, name)?
            };
        }
        if root == self.base.root
            && self.arena.as_ref().unwrap().revision() == 0
            && self.next_database == self.base.next_database
        {
            return Ok(self.base.receipt.sequence);
        }
        let catalog = self
            .catalog
            .prepare_with_capacity(&self.base.catalog, self.catalog_capacity)?;
        let candidate = self.arena.take().unwrap().freeze()?;
        let maximum = candidate
            .slots()
            .try_fold(512u64, |bytes, slot| {
                bytes
                    .checked_add(4096 + 2048 + slot.ranges.len() as u64 * 4)
                    .ok_or(Error::LimitExceeded)
            })?
            .checked_add(self.base.image.member_count() as u64 * 16)
            .ok_or(Error::LimitExceeded)?;
        let end = self.guard.journal.state().selected().end()?;
        if !self.guard.journal.can_append(maximum)?
            && end > ulmdb_storage::resident::wal_codec::DATA_START
        {
            let capacity = self.guard.journal.capacity();
            self.env.checkpoint_locked(&mut self.guard, capacity)?;
            self.base = self.env.current.load_full();
        }
        let boot = self.guard.journal.boot();
        let parent = self.guard.journal.state().selected();
        if parent != self.base.receipt
            || self.guard.journal.state().commit() != self.base.commit
            || self.guard.journal.state().root() != self.base.root
        {
            self.guard.poisoned = true;
            self.env.pool.quarantine();
            return Err(Error::Corrupt(
                "resident writer base differs from selected state",
            ));
        }
        // Only the opt-in writer builds graph deltas. Reads retain the same
        // materialized pages. Prior-dead versions are retired in bounded groups;
        // current-frame Copy sources remain members through a later commit.
        let mut pending = if let Some(certificate) = &self.guard.incremental {
            use super::graph_certificate::{RootChange, RootSpec};
            let spec = |name: &Vec<u8>, db: &Database, value_limit: usize| RootSpec {
                name: name.clone(),
                id: db.id,
                kind: db.kind,
                ordering: db.ordering,
                root: db.root,
                value_limit,
            };
            let mut changes = Vec::new();
            for name in self.catalog.changed() {
                let before = self
                    .base
                    .catalog
                    .get(name)
                    .map(|db| {
                        let bytes = ulmdb_kv::get(&self.base.image, self.base.root, name)?
                            .ok_or(Error::Corrupt("base graph descriptor absent"))?;
                        Ok::<_, Error>(spec(name, db, catalog_codec::value_limit(bytes)?))
                    })
                    .transpose()?;
                let after = self
                    .catalog
                    .get(&self.base.catalog, name)
                    .map(|db| {
                        self.database_value_limit(name, db)
                            .map(|limit| spec(name, db, limit))
                    })
                    .transpose()?;
                if before
                    .as_ref()
                    .zip(after.as_ref())
                    .is_some_and(|(a, b)| a.id != b.id)
                {
                    changes.push(RootChange {
                        before,
                        after: None,
                    });
                    changes.push(RootChange {
                        before: None,
                        after,
                    });
                } else if before.is_some() || after.is_some() {
                    changes.push(RootChange { before, after });
                }
            }
            Some(certificate.prepare(
                &self.base,
                &candidate,
                &changes,
                root,
                self.next_database,
                64,
            )?)
        } else {
            None
        };
        let retired = pending
            .as_ref()
            .map_or_else(Vec::new, |p| p.retired().to_vec());
        let image = pending
            .as_ref()
            .map_or_else(|| candidate.view().clone(), |p| p.image().clone());
        let metrics = pending.as_ref().map(|p| p.metrics());
        let sequence = parent.sequence.checked_add(1).ok_or(Error::LimitExceeded)?;
        let lsn = parent.lsn.checked_add(1).ok_or(Error::LimitExceeded)?;
        let commit = boot
            .commit
            .checked_add(sequence as u128)
            .ok_or(Error::LimitExceeded)?;
        // The exact selected base and frozen candidate remain pinned through
        // both fences. Private recipe construction precedes canonical encode.
        let mut ready = None;
        let mut ready_certificate = None;
        let ready_callback =
            |receipt, metadata: Option<&ulmdb_storage::resident::wal_codec_v2::Record>| {
                if let Some(pending) = pending.as_ref() {
                    pending.validate_record(
                        metadata.ok_or(Error::Corrupt("incremental publication requires RL2"))?,
                    )?;
                }
                ulmdb_kv::Reader::new(&image, root)?;
                for db in self
                    .catalog
                    .changed()
                    .filter_map(|n| self.catalog.get(&self.base.catalog, n))
                {
                    match db.kind {
                        DatabaseKind::Kv | DatabaseKind::Duplicates { .. } => {
                            ulmdb_kv::Reader::new(&image, db.root)?;
                        }
                        DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => {
                            let r = ulmdb_prefix::Reader::new(&image, db.root)?;
                            if let Some(p) = r.root_prefix() {
                                check_family(db.kind, p.network())?;
                            }
                        }
                    }
                }
                ready_certificate = pending
                    .take()
                    .map(|p| p.bind(receipt, commit))
                    .transpose()?;
                ready = Some(Arc::new(Snapshot {
                    image,
                    catalog,
                    root,
                    next_database: self.next_database,
                    receipt,
                    commit,
                }));
                Ok(())
            };
        let super::environment::Writer {
            journal,
            recipe_workspace,
            ..
        } = &mut *self.guard;
        let result = match journal {
            Journal::Literal(j) => {
                let record = Record {
                    uuid: boot.uuid,
                    epoch: boot.epoch,
                    lsn,
                    sequence,
                    commit,
                    parent_commit: self.base.commit,
                    parent,
                    root,
                    next_database: self.next_database,
                    kind: Kind::Write,
                    allocations: candidate.literals(j.state())?,
                    retired,
                };
                j.commit_checked(record, |_, receipt| ready_callback(receipt, None))
            }
            Journal::Compact(j) => {
                let mut record = ulmdb_storage::resident::wal_codec_v2::Record {
                    uuid: boot.uuid,
                    epoch: boot.epoch,
                    lsn,
                    sequence,
                    commit,
                    parent_commit: self.base.commit,
                    parent,
                    root,
                    next_database: self.next_database,
                    kind: Kind::Write,
                    allocations: Vec::new(),
                    retired,
                };
                match self.env.redo_encoding() {
                    super::RedoEncoding::Owned => {
                        record.allocations = super::recipe::allocations(
                            &self.base,
                            j.state(),
                            &candidate,
                            self.env.redo_format(),
                        )?;
                        j.commit_checked(record, |metadata, receipt| {
                            ready_callback(receipt, Some(metadata))
                        })
                    }
                    super::RedoEncoding::Borrowed => {
                        let borrowed = super::live_recipe::build(
                            &self.base,
                            j.state(),
                            &candidate,
                            record,
                            recipe_workspace,
                            self.env.redo_format(),
                        )?;
                        j.commit_borrowed(borrowed, |metadata, receipt| {
                            ready_callback(receipt, Some(metadata))
                        })
                    }
                }
            }
        };
        if let Err(error) = result {
            if self.guard.journal.is_poisoned() {
                self.guard.poisoned = true;
                self.env.pool.quarantine();
            }
            return Err(error);
        }
        drop(candidate.activate());
        if let Some(certificate) = ready_certificate {
            self.guard.incremental = Some(certificate.activate());
            self.guard.incremental_metrics = metrics.expect("incremental preparation has metrics");
        }
        self.env
            .current
            .store(ready.expect("resident write prepared before durable IO"));
        Ok(sequence)
    }
}
