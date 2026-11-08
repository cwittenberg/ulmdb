//! Explicit opt-in format6 persistent physical locators. No format4 encoder.
#[cfg(test)]
mod admission_tests;
mod codec;
mod read;
mod reference;
#[cfg(test)]
mod tests;
mod tree;
use super::{FILE, LocatorWork, MARKER, PriorAdmission, RangeIdentity, RangeSnapshot, Seal, pack};
use crate::{
    Arena, Error, Inventory, Manifest, ResidencyController, Result, SegmentEntry, SegmentTable,
};
use reference::PhysicalRef;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::Arc,
};
use tree::Node;
pub(super) struct Store {
    writer: pack::Writer,
    #[cfg(feature = "range-lifecycle")]
    checkpoint_reader: Arc<File>,
    uuid: [u8; 16],
    root: Option<Arc<Node>>,
    inventory: Inventory,
    scalar_hash: [u8; 32],
    table_identity: u64,
    work: LocatorWork,
}
fn marker(file: [u8; 16], uuid: [u8; 16]) -> [u8; 96] {
    let mut b = [0; 96];
    b[..8].copy_from_slice(b"ULMDBB06");
    b[10..12].copy_from_slice(&6u16.to_le_bytes());
    b[12..16].copy_from_slice(&96u32.to_le_bytes());
    b[16..32].copy_from_slice(&file);
    b[32..48].copy_from_slice(&uuid);
    let h = Sha256::digest(&b[..64]);
    b[64..].copy_from_slice(&h);
    b
}
impl Store {
    pub fn create(
        directory: &Path,
        controller: &ResidencyController,
    ) -> Result<(Self, RangeSnapshot)> {
        for name in ["CURRENT", "PREVIOUS", MARKER, FILE] {
            if directory.join(name).try_exists()? {
                return Err(Error::InvalidInput(
                    "direct range create requires unpublished layout",
                ));
            }
        }
        let writer = pack::Writer::create(&directory.join(FILE))?;
        let mut manifest = Manifest::empty()?;
        manifest.format_minor = 6;
        manifest.segments = Inventory::range_from_entries(Vec::new())?;
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(MARKER))?;
        f.write_all(&marker(writer.global.file_id, manifest.uuid))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(std::fs::Permissions::from_mode(0o400))?;
        }
        ulmdb_platform::sync_file(&f)?;
        ulmdb_platform::sync_path(directory)?;
        #[cfg(feature = "range-lifecycle")]
        let checkpoint_reader = Arc::new(writer.reader.try_clone()?);
        let mut s = Self {
            writer,
            #[cfg(feature = "range-lifecycle")]
            checkpoint_reader,
            uuid: manifest.uuid,
            root: None,
            inventory: manifest.segments.clone(),
            scalar_hash: [0; 32],
            table_identity: 0,
            work: LocatorWork::default(),
        };
        let state = s.commit_parts(
            SegmentTable::default(),
            None,
            Vec::new(),
            manifest,
            controller,
            None,
        )?;
        Ok((s, state))
    }
    pub fn open_checked<F: FnOnce(&dyn crate::ReadStore, &Manifest) -> Result<()>>(
        directory: &Path,
        controller: &ResidencyController,
        validate: F,
    ) -> Result<(Self, RangeSnapshot)> {
        let mut bytes = Vec::new();
        File::open(directory.join(MARKER))?
            .take(97)
            .read_to_end(&mut bytes)?;
        if bytes.len() != 96 {
            return Err(Error::Corrupt("direct marker length"));
        }
        let file: [u8; 16] = bytes[16..32].try_into().unwrap();
        let uuid: [u8; 16] = bytes[32..48].try_into().unwrap();
        #[cfg(feature = "range-lifecycle")]
        let marker_valid = super::rotation::selection(directory).is_ok();
        #[cfg(not(feature = "range-lifecycle"))]
        let marker_valid = bytes == marker(file, uuid);
        if file == [0; 16] || uuid == [0; 16] || !marker_valid {
            return Err(Error::Corrupt("direct marker checksum/version"));
        }
        #[cfg(feature = "range-lifecycle")]
        let chosen = super::rotation::selection(directory)?;
        #[cfg(feature = "range-lifecycle")]
        let (file, uuid, path) = (
            chosen.file,
            chosen.uuid,
            super::rotation::asset(directory, chosen.file)?,
        );
        #[cfg(not(feature = "range-lifecycle"))]
        let path = directory.join(FILE);
        let (writer, loaded) = pack::Writer::open_checked(&path, |reader, id, receipt| {
            if id != file {
                return Err(Error::Corrupt("direct physical file identity"));
            }
            let result = read::load(reader, id, uuid, receipt, controller)?;
            validate(&result.snapshot.segments, &result.snapshot.manifest)?;
            Ok(result)
        })?;
        let read::Loaded { snapshot, root } = loaded;
        #[cfg(feature = "range-lifecycle")]
        let checkpoint_reader = snapshot
            .checkpoint
            .as_ref()
            .ok_or(Error::Corrupt("missingdirectcheckpoint"))?
            .reader();
        let s = Self {
            writer,
            #[cfg(feature = "range-lifecycle")]
            checkpoint_reader,
            uuid,
            root,
            inventory: snapshot.manifest.segments.clone(),
            scalar_hash: codec::scalar_hash(&snapshot.manifest),
            table_identity: snapshot.segments.residency_identity(),
            work: LocatorWork::default(),
        };
        Ok((s, snapshot))
    }
    pub fn is_poisoned(&self) -> bool {
        self.writer.poisoned
    }
    pub fn work(&self) -> LocatorWork {
        self.work
    }
    pub fn commit_arena(
        &mut self,
        arena: Arena,
        manifest: Manifest,
        base: &Manifest,
        controller: &ResidencyController,
        prior: Option<PriorAdmission<'_>>,
    ) -> Result<RangeSnapshot> {
        if self.writer.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        // The scalar identity, private canonical inventory stamp and exact owner table
        // jointly bind the base; public mutation cannot forge that private stamp.
        if base.format_minor != 6
            || manifest.format_minor != 6
            || base.segments.stamp() != self.inventory.stamp()
            || base.segments.stamp() == 0
            || arena.base().residency_identity() != self.table_identity
            || codec::scalar_hash(base) != self.scalar_hash
            || manifest.uuid != self.uuid
            || manifest.generation == 0
            || manifest.generation == base.generation
            || manifest.previous != base.generation
            || manifest.parent != base.commit
            || manifest.sequence != base.sequence.checked_add(1).ok_or(Error::LimitExceeded)?
            || manifest.epoch != base.epoch
            || manifest.commit != manifest.generation
            || manifest.next_segment != base.next_segment
            || manifest.next_database < base.next_database
        {
            return Err(Error::Corrupt(
                "direct range base/scalar/inventory ownership",
            ));
        }
        if matches!(
            controller.policy(),
            crate::ResidencyPolicy::RequiredLocked { .. }
        ) && prior
            .is_some_and(|p| p.base.residency_identity() != arena.base().residency_identity())
        {
            return Err(Error::InvalidInput(
                "range prior admission has foreign base table",
            ));
        }
        let (table, entry, data) = arena.range_data()?;
        if entry.as_ref().is_some_and(|e| e.id != base.next_segment) {
            return Err(Error::Corrupt("direct range allocation counter"));
        }
        self.commit_parts(table, entry, data, manifest, controller, prior)
    }
    fn commit_parts(
        &mut self,
        mut table: SegmentTable,
        entry: Option<SegmentEntry>,
        data: Vec<u8>,
        mut manifest: Manifest,
        controller: &ResidencyController,
        prior: Option<PriorAdmission<'_>>,
    ) -> Result<RangeSnapshot> {
        let ordinal = self
            .writer
            .selected
            .ordinal
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        manifest.generation = ulmdb_core::range_generation::physical_generation(
            u128::from_le_bytes(self.writer.global.file_id),
            ordinal,
        )
        .ok_or(Error::Corrupt("invalid physical generation namespace"))?;
        let start = self.writer.selected.frame_end()?;
        let file = self.writer.global.file_id;
        let mut inventory = self.inventory.clone();
        let mut root = self.root.clone();
        let mut encoded = tree::Encoded::default();
        if let Some(e) = &entry {
            if e.cohort != 0 || data.len() as u64 != e.length {
                return Err(Error::InvalidInput("direct range segment provenance"));
            }
            let physical = PhysicalRef {
                file,
                offset: start.checked_add(8192).ok_or(Error::LimitExceeded)?,
                length: e.length,
                hash: e.hash,
            };
            inventory = inventory.range_append(e.clone())?;
            root = Some(tree::insert(root, e.clone(), physical)?);
            manifest.next_segment = e.id.checked_add(1).ok_or(Error::LimitExceeded)?;
            root = Some(tree::serialize(
                root.unwrap(),
                file,
                codec::blob_offset(start, e.length)?,
                &mut encoded,
            )?);
        }
        if encoded.written > 65 || encoded.visited > 129 {
            return Err(Error::Corrupt("direct locator update path exceeded64bits"));
        }
        manifest.format_minor = 6;
        manifest.index = None;
        manifest.segments = inventory;
        let payload = codec::encode(codec::EncodeInput {
            file,
            frame_start: start,
            manifest: &manifest,
            data: &data,
            root: root.as_ref(),
            blob: &encoded.bytes,
            local: encoded.written,
            own_id: entry.as_ref().map_or(0, |e| e.id),
        })?;
        let work = LocatorWork {
            nodes_visited: encoded.visited,
            nodes_written: encoded.written,
            node_bytes: encoded.bytes.len() as u64,
            data_bytes: data.len() as u64,
            inventory_materialized: manifest.segments.is_materialized(),
        };
        let next_hash = codec::scalar_hash(&manifest);
        let next_inventory = manifest.segments.clone();
        let frame = self.writer.prepare(&payload)?;
        let candidate = (|| {
            if let Some(e) = entry {
                let identity = RangeIdentity {
                    file_id: file,
                    seal: Seal::Prefix {
                        committed_end: frame.receipt.frame_end()?,
                        data_hash: e.hash,
                    },
                    data_offset: start + 8192,
                    length: e.length,
                };
                let mapped = crate::mapping::Mapping::from_range(
                    &self.writer.reader,
                    e,
                    identity,
                    Some(&data),
                )?;
                table.insert_mapping(mapped)?;
            }
            let admission = match prior {
                Some(p)
                    if matches!(
                        controller.policy(),
                        crate::ResidencyPolicy::RequiredLocked { .. }
                    ) =>
                {
                    controller.admit_append(&table, p.base, p.ready)?
                }
                _ => controller.admit(&table)?,
            };
            #[cfg(feature = "range-lifecycle")]
            let checkpoint = super::checkpoint::RangeCheckpoint::mint(
                Arc::clone(&self.checkpoint_reader),
                file,
                &manifest,
                frame.receipt,
                frame.parent,
                next_hash,
            )?;
            Ok::<_, Error>(RangeSnapshot {
                #[cfg(feature = "range-lifecycle")]
                checkpoint: Some(checkpoint),
                manifest,
                segments: table,
                admission,
            })
        })();
        let state = match candidate {
            Ok(s) => s,
            Err(e) => {
                self.writer.abort_prepared()?;
                return Err(e);
            }
        };
        self.writer.publish_prepared(&frame)?;
        self.root = root;
        self.inventory = next_inventory;
        self.scalar_hash = next_hash;
        self.table_identity = state.segments.residency_identity();
        self.work = work;
        Ok(state)
    }
}

#[cfg(feature = "range-lifecycle")]
pub(super) fn checkpoint_marker(file: [u8; 16], uuid: [u8; 16]) -> [u8; 96] {
    marker(file, uuid)
}
#[cfg(feature = "range-lifecycle")]
pub(super) fn checkpoint_scalar(manifest: &Manifest) -> [u8; 32] {
    codec::scalar_hash(manifest)
}
#[cfg(feature = "range-lifecycle")]
pub(super) fn load_checkpoint(
    file: &File,
    id: [u8; 16],
    uuid: [u8; 16],
    receipt: pack::Receipt,
    controller: &ResidencyController,
) -> Result<RangeSnapshot> {
    Ok(read::load(file, id, uuid, receipt, controller)?.snapshot)
}

#[cfg(feature = "range-lifecycle")]
impl Store {
    pub(super) fn check_rotation_source(
        &self,
        source: &super::checkpoint::RangeCheckpoint,
    ) -> Result<()> {
        if self.writer.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        if !source.matches_physical(
            self.writer.global.file_id,
            self.writer.selected,
            self.scalar_hash,
        ) {
            return Err(Error::StaleHandle);
        }
        Ok(())
    }
    pub(super) fn poison(&mut self) {
        self.writer.poisoned = true;
    }
    pub(super) fn create_rotated<F: FnOnce(&SegmentTable, &Manifest) -> Result<()>>(
        path: &Path,
        file: File,
        id: [u8; 16],
        anchor: super::rotation::Anchor,
        arena: Arena,
        manifest: Manifest,
        controller: &ResidencyController,
        validate: F,
    ) -> Result<(Self, RangeSnapshot)> {
        if !anchor.matches(&manifest) {
            return Err(Error::Corrupt("range anchor candidate identity"));
        }
        anchor.validate()?;
        let writer = pack::Writer::initialize_created(
            path,
            pack::codec::Global {
                file_id: id,
                anchor: Some(anchor),
            },
            file,
        )?;
        let checkpoint_reader = Arc::new(writer.reader.try_clone()?);
        let (table, entry, data) = arena.range_data()?;
        if !table.entries().is_empty() {
            return Err(Error::InvalidInput("rotated graph must be independent"));
        }
        let mut s = Self {
            writer,
            checkpoint_reader,
            uuid: manifest.uuid,
            root: None,
            inventory: Inventory::range_from_entries(Vec::new())?,
            scalar_hash: [0; 32],
            table_identity: 0,
            work: LocatorWork::default(),
        };
        let state = s.commit_parts(table, entry, data, manifest, controller, None)?;
        validate(&state.segments, &state.manifest)?;
        Ok((s, state))
    }
}
