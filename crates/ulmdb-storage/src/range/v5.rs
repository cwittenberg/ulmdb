//! Preserved format5 reader AND writer; never upgrades implicitly.
use super::{FILE, MARKER, RangeIdentity, RangeSnapshot, codec, origin, pack};
use crate::{Arena, Error, Manifest, ResidencyController, Result, SegmentTable};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
pub(super) struct Store {
    writer: pack::Writer,
    uuid: [u8; 16],
    origins: BTreeMap<u128, codec::Origin>,
    placements: BTreeMap<u64, u128>,
    manifest_hash: [u8; 32],
    table_identity: u64,
}
fn marker(file_id: [u8; 16], uuid: [u8; 16]) -> [u8; 96] {
    let mut b = [0; 96];
    b[..8].copy_from_slice(b"ULMDBB05");
    b[10..12].copy_from_slice(&5u16.to_le_bytes());
    b[12..16].copy_from_slice(&96u32.to_le_bytes());
    b[16..32].copy_from_slice(&file_id);
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
        if directory.join("CURRENT").try_exists()?
            || directory.join("PREVIOUS").try_exists()?
            || directory.join(MARKER).try_exists()?
            || directory.join(FILE).try_exists()?
        {
            return Err(Error::InvalidInput(
                "range create requires unpublished layout",
            ));
        }
        let writer = pack::Writer::create(&directory.join(FILE))?;
        let mut manifest = Manifest::empty()?;
        manifest.prepare_indexed(None)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(MARKER))?;
        file.write_all(&marker(writer.global.file_id, manifest.uuid))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o400))?;
        }
        ulmdb_platform::sync_file(&file)?;
        ulmdb_platform::sync_path(directory)?;
        let mut store = Self {
            writer,
            uuid: manifest.uuid,
            origins: BTreeMap::new(),
            placements: BTreeMap::new(),
            manifest_hash: [0; 32],
            table_identity: 0,
        };
        let state = store.commit_parts(
            SegmentTable::default(),
            None,
            Vec::new(),
            manifest,
            controller,
        )?;
        Ok((store, state))
    }
    pub fn open_checked<F: FnOnce(&dyn crate::ReadStore, &Manifest) -> Result<()>>(
        directory: &Path,
        controller: &ResidencyController,
        validate: F,
    ) -> Result<(Self, RangeSnapshot)> {
        if !super::is_selected(directory)? {
            return Err(Error::InvalidInput("range backend not selected"));
        }
        let mut bytes = Vec::new();
        File::open(directory.join(MARKER))?
            .take(97)
            .read_to_end(&mut bytes)?;
        if bytes.len() != 96 {
            return Err(Error::Corrupt("range backend marker length"));
        }
        let file_id: [u8; 16] = bytes[16..32].try_into().unwrap();
        let uuid: [u8; 16] = bytes[32..48].try_into().unwrap();
        if uuid == [0; 16] || file_id == [0; 16] || bytes != marker(file_id, uuid) {
            return Err(Error::Corrupt("range backend marker checksum/version"));
        }
        let (writer, loaded) =
            pack::Writer::open_checked(&directory.join(FILE), |file, id, receipt| {
                if id != file_id {
                    return Err(Error::Corrupt("range file identity mismatch"));
                }
                let result = origin::load(file, id, uuid, receipt, controller)?;
                validate(&result.snapshot.segments, &result.snapshot.manifest)?;
                Ok(result)
            })?;
        let origin::Loaded {
            snapshot: state,
            origins,
            placements,
            manifest_hash,
        } = loaded;
        let table_identity = state.segments.residency_identity();
        Ok((
            Self {
                writer,
                uuid,
                origins,
                placements,
                manifest_hash,
                table_identity,
            },
            state,
        ))
    }
    pub fn is_poisoned(&self) -> bool {
        self.writer.poisoned
    }
    pub fn commit_arena(
        &mut self,
        arena: Arena,
        mut manifest: Manifest,
        base: &Manifest,
        controller: &ResidencyController,
    ) -> Result<RangeSnapshot> {
        if self.writer.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        if arena.base().residency_identity() != self.table_identity
            || manifest.uuid != self.uuid
            || <[u8; 32]>::from(Sha256::digest(base.encode()?)) != self.manifest_hash
            || manifest.previous != base.generation
            || self.origins.contains_key(&manifest.generation)
        {
            return Err(Error::Corrupt("range writer base/provenance mismatch"));
        }
        let (table, entry, data) = arena.range_parts(&mut manifest, base)?;
        self.commit_parts(table, entry, data, manifest, controller)
    }
    fn commit_parts(
        &mut self,
        mut table: SegmentTable,
        entry: Option<crate::SegmentEntry>,
        data: Vec<u8>,
        mut manifest: Manifest,
        controller: &ResidencyController,
    ) -> Result<RangeSnapshot> {
        let mut placements = self.placements.clone();
        if let Some(e) = &entry
            && placements.insert(e.id, manifest.generation).is_some()
        {
            return Err(Error::Corrupt("range logical ID rebind"));
        }
        let descriptor = manifest.encode()?;
        let manifest_hash = Sha256::digest(&descriptor).into();
        let (payload, header_hash) = codec::encode(
            self.uuid,
            manifest.generation,
            &data,
            &descriptor,
            &self.origins,
            &placements,
            entry.as_ref().map_or(0, |e| e.id),
        )?;
        let frame = self.writer.prepare(&payload)?;
        let own = codec::Origin {
            receipt: frame.receipt,
            header_hash,
        };
        // No private map or table is returned to a caller before head publication.
        let candidate = (|| {
            if let Some(entry) = entry {
                let identity = RangeIdentity::new(self.writer.global.file_id, own, entry.length)?;
                let mapping = crate::mapping::Mapping::from_range(
                    &self.writer.reader,
                    entry,
                    identity,
                    Some(&data),
                )?;
                table.insert_mapping(mapping)?;
            }
            manifest.certify_indexed_table(&table)?;
            let admission = controller.admit(&table)?;
            Ok::<_, Error>(RangeSnapshot {
                #[cfg(feature = "range-lifecycle")]
                checkpoint: None,
                manifest,
                segments: table,
                admission,
            })
        })();
        let state = match candidate {
            Ok(state) => state,
            Err(error) => {
                self.writer.abort_prepared()?;
                return Err(error);
            }
        };
        let mut next_origins = self.origins.clone();
        next_origins.insert(state.manifest.generation, own);
        self.writer.publish_prepared(&frame)?;
        self.origins = next_origins;
        self.placements = placements;
        self.manifest_hash = manifest_hash;
        self.table_identity = state.segments.residency_identity();
        Ok(state)
    }
}
