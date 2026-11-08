//! Atomic catalog/KV/radix writer; all allocation and checks precede selection.
use super::{
    Environment, SharedSnapshot, Snapshot,
    environment::Writer,
    provenance::{Allocation, Pending, Sources},
};
use crate::{Address, DatabaseKind, Error, Prefix, Result, catalog::Database, read::check_family};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, MutexGuard},
};
use ulmdb_storage::{
    Reference,
    hybrid::{
        ImageArena, ImageTable,
        redo::{
            self, Authority, ChunkDelta, DestinationResolver, Extent, Record, RootState,
            SourceReceipt,
        },
    },
};

pub struct WriteTxn<'a> {
    pub(super) env: &'a Environment,
    pub(super) guard: MutexGuard<'a, Writer>,
    pub(super) base: SharedSnapshot,
    pub(super) arena: ImageArena<ImageTable>,
    pub(super) catalog: BTreeMap<Vec<u8>, Database>,
    pub(super) changed: BTreeSet<Vec<u8>>,
    pub(super) failed: bool,
}
impl WriteTxn<'_> {
    fn note_changed(&mut self, name: &[u8]) {
        if !self.changed.contains(name) {
            self.changed.insert(name.to_vec());
        }
    }

    pub(super) fn live(&self) -> Result<()> {
        if self.failed {
            Err(Error::InvalidInput("failed hybrid transaction must abort"))
        } else {
            Ok(())
        }
    }
    pub fn create_database(&mut self, name: &[u8], kind: DatabaseKind) -> Result<()> {
        self.live()?;
        if name.is_empty() || name.len() > 255 || self.catalog.contains_key(name) {
            return Err(Error::InvalidInput("invalid/existing database name"));
        }
        if matches!(kind, DatabaseKind::Duplicates { .. }) {
            return Err(Error::InvalidInput(
                "hybrid duplicate API integration pending",
            ));
        }
        if self.catalog.len() >= 4096 {
            return Err(Error::LimitExceeded);
        }
        let id = self.guard.next_database;
        self.guard.next_database = id.checked_add(1).ok_or(Error::LimitExceeded)?;
        self.catalog.insert(
            name.to_vec(),
            Database {
                ordering: crate::ordering::DatabaseOrdering::default(),
                id,
                kind,
                root: Reference::NULL,
            },
        );
        self.note_changed(name);
        Ok(())
    }
    pub fn drop_database(&mut self, name: &[u8]) -> Result<()> {
        self.live()?;
        self.catalog.remove(name).ok_or(Error::MissingDatabase)?;
        self.note_changed(name);
        Ok(())
    }
    pub fn get(&self, name: &[u8], key: &[u8]) -> Result<Option<&[u8]>> {
        self.live()?;
        if key.len() > 4096 {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        ulmdb_kv::get(&self.arena, db.root, key)
    }
    pub fn put(&mut self, name: &[u8], key: &[u8], value: &[u8]) -> Result<()> {
        self.live()?;
        if key.len() > 4096 || value.len() > 67_108_864 {
            return Err(Error::InvalidInput("KV size limit exceeded"));
        }
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        let prior = (db.root, self.arena.revision());
        self.failed = true;
        let root = ulmdb_kv::put(&mut self.arena, db.root, key, value)?;
        self.failed = false;
        if prior != (root, self.arena.revision()) {
            self.catalog
                .get_mut(name)
                .ok_or(Error::MissingDatabase)?
                .root = root;
            self.note_changed(name);
        }
        Ok(())
    }
    pub fn delete(&mut self, name: &[u8], key: &[u8]) -> Result<bool> {
        self.live()?;
        let exists = self.get(name, key)?.is_some();
        if !exists {
            return Ok(false);
        }
        let root = self.catalog.get(name).ok_or(Error::MissingDatabase)?.root;
        self.failed = true;
        let root = ulmdb_kv::delete(&mut self.arena, root, key)?;
        self.failed = false;
        self.catalog
            .get_mut(name)
            .ok_or(Error::MissingDatabase)?
            .root = root;
        self.note_changed(name);
        Ok(true)
    }
    pub fn insert_prefix(&mut self, name: &[u8], prefix: Prefix, value: &[u8]) -> Result<()> {
        self.live()?;
        if value.len() > 67_108_864 {
            return Err(Error::InvalidInput("prefix value too large"));
        }
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        check_family(db.kind, prefix.network())?;
        let prior = (db.root, self.arena.revision());
        self.failed = true;
        let root = ulmdb_prefix::insert(&mut self.arena, db.root, prefix, value)?;
        self.failed = false;
        if prior != (root, self.arena.revision()) {
            self.catalog
                .get_mut(name)
                .ok_or(Error::MissingDatabase)?
                .root = root;
            self.note_changed(name);
        }
        Ok(())
    }
    pub fn delete_prefix(&mut self, name: &[u8], prefix: Prefix) -> Result<()> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        check_family(db.kind, prefix.network())?;
        let prior = (db.root, self.arena.revision());
        self.failed = true;
        let root = ulmdb_prefix::delete(&mut self.arena, db.root, prefix)?;
        self.failed = false;
        if prior != (root, self.arena.revision()) {
            self.catalog
                .get_mut(name)
                .ok_or(Error::MissingDatabase)?
                .root = root;
            self.note_changed(name);
        }
        Ok(())
    }
    pub fn longest_match(&self, name: &[u8], address: Address) -> Result<Option<(Prefix, &[u8])>> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        check_family(db.kind, address)?;
        ulmdb_prefix::get(&self.arena, db.root, address)
    }
    pub fn commit(mut self) -> Result<u64> {
        self.live()?;
        if self.changed.is_empty() {
            return Ok(self.base.receipt.sequence);
        }
        let mut catalog_root = self.base.root.catalog;
        for name in &self.changed {
            catalog_root = if let Some(db) = self.catalog.get(name) {
                ulmdb_kv::put(&mut self.arena, catalog_root, name, &db.encode())?
            } else {
                ulmdb_kv::delete(&mut self.arena, catalog_root, name)?
            };
        }
        if catalog_root == self.base.root.catalog && self.arena.private_bytes() == 0 {
            return Ok(self.base.receipt.sequence);
        }
        let frozen = self.arena.freeze()?;
        // An allocation-free metadata change retains the selected image. It
        // must not publish an unused rotated chunk or previously burned tail.
        // Current KV deletion still allocates its resulting empty leaf.
        let has_allocations = frozen.allocation_count() != 0;
        let image = if has_allocations {
            self.base.image.with_active(frozen.view())?
        } else {
            self.base.image.clone()
        };
        let root = RootState {
            catalog: catalog_root,
            next_chunk: if has_allocations {
                frozen.chunk_id() + 1
            } else {
                self.base.root.next_chunk
            },
            next_database: self.guard.next_database,
        };
        let header = self.guard.journal.bootstrap();
        let lsn = self
            .base
            .receipt
            .lsn
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        let sequence = self
            .base
            .receipt
            .sequence
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        let commit = header
            .commit
            .checked_add(u128::from(lsn))
            .ok_or(Error::LimitExceeded)?;
        let generation = self.guard.generation;
        let before = self
            .base
            .image
            .chunk(frozen.chunk_id())
            .map_or(4096, |s| s.end()) as u64;
        let record = Record {
            uuid: header.uuid,
            epoch: header.epoch,
            lsn,
            sequence,
            commit,
            parent_commit: self.base.commit,
            parent: self.base.receipt,
            root,
            chunks: if has_allocations {
                vec![ChunkDelta {
                    chunk: frozen.chunk_id(),
                    logical_generation: generation,
                    before,
                    after: frozen.end() as u64,
                    capacity: image.active().unwrap().capacity() as u64,
                }]
            } else {
                Vec::new()
            },
            objects: Vec::new(),
        };
        let Writer {
            journal,
            ledger,
            recipe_workspace,
            ..
        } = &mut *self.guard;
        let workspace_bound = usize::try_from(self.env.options.max_wal_bytes.min(
            match self.env.options.wal_layout {
                ulmdb_storage::hybrid::journal::WalLayout::Dense => {
                    ulmdb_storage::hybrid::codec::MAX_RECORD_BYTES as u64
                }
                ulmdb_storage::hybrid::journal::WalLayout::PreSized { capacity } => capacity,
            },
        ))
        .map_err(|_| Error::LimitExceeded)?;
        let record = super::live_recipe::prepare(
            record,
            &frozen,
            &self.base.image,
            ledger,
            generation,
            self.env.options.redo,
            self.env.options.live_redo,
            recipe_workspace,
            workspace_bound,
        )?;
        let used_mode = record.mode();
        struct Destinations<'a>(&'a ulmdb_storage::hybrid::FrozenExtent);
        impl DestinationResolver for Destinations<'_> {
            fn resolve_prepared(&self, e: &Extent) -> Result<&[u8]> {
                if e.at.segment != self.0.chunk_id() {
                    return Err(Error::Corrupt("hybrid destination chunk mismatch"));
                }
                let at = usize::try_from(
                    e.at.offset
                        .checked_sub(self.0.start() as u64)
                        .ok_or(Error::Corrupt("hybrid destination before frozen tail"))?,
                )
                .map_err(|_| Error::LimitExceeded)?;
                let end = at
                    .checked_add(e.length as usize)
                    .ok_or(Error::LimitExceeded)?;
                self.0
                    .bytes()?
                    .get(at..end)
                    .ok_or(Error::Corrupt("hybrid destination outside frozen tail"))
            }
        }
        let mut prepared = None;
        // Future authority is private under the writer lock and old-sequence
        // Sources rejects it. Every BTreeMap node is allocated before journal IO.
        let mut pending = Pending::new(ledger, record.object_count())?;
        let sources = Sources::for_snapshot(&self.base, ledger);
        let ready = |receipt: ulmdb_storage::hybrid::redo::WalReceipt| -> Result<()> {
            // Existing checked mutators maintain routing. Prepare all root readers
            // before the durable decision; cold open additionally audits whole graphs.
            ulmdb_kv::Reader::new(&image, root.catalog)?;
            for db in self
                .changed
                .iter()
                .filter_map(|name| self.catalog.get(name))
            {
                match db.kind {
                    DatabaseKind::Kv => {
                        ulmdb_kv::Reader::new(&image, db.root)?;
                    }
                    DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => {
                        let reader = ulmdb_prefix::Reader::new(&image, db.root)?;
                        if let Some(p) = reader.root_prefix() {
                            check_family(db.kind, p.network())?;
                        }
                    }
                    DatabaseKind::Duplicates { .. } => return Err(Error::WrongKind),
                }
            }

            if receipt.end()? > self.env.options.max_wal_bytes {
                return Err(Error::LimitExceeded);
            }
            record.each_destination(|dest| {
                pending.insert(Allocation {
                    extent: dest,
                    receipt: SourceReceipt {
                        uuid: header.uuid,
                        epoch: header.epoch,
                        creation_lsn: lsn,
                        creation_sequence: sequence,
                        creation_commit: commit,
                        provenance_digest: receipt.digest,
                        authority: Authority::Wal {
                            lsn,
                            sequence,
                            digest: receipt.digest,
                        },
                    },
                })?;
                Ok(())
            })?;
            prepared = Some(Arc::new(Snapshot {
                image,
                catalog: self.catalog,
                root,
                receipt,
                commit,
            }));
            Ok(())
        };
        match &record {
            super::live_recipe::Prepared::Owned(r) => {
                redo::verify_materialized(r, &sources, &Destinations(&frozen))?;
                journal.commit_checked_with_receipt(r, &sources, |_, receipt| ready(receipt))?;
            }
            super::live_recipe::Prepared::Borrowed(r) => {
                journal.commit_borrowed_checked_with_receipt(
                    r,
                    &sources,
                    &Destinations(&frozen),
                    workspace_bound,
                    |_, receipt| ready(receipt),
                )?;
            }
        }
        // No authority insertion/allocation after the selected witness.
        pending.accept();
        self.guard.last_live_redo = Some(used_mode);
        if has_allocations {
            frozen.accept();
        }
        self.env
            .current
            .store(prepared.expect("prepared before durable selection"));
        Ok(sequence)
    }
}
