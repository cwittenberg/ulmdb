//! Cold origin resolution never follows historical locator inventories.
use super::{
    codec::{self, HEADER, Header, Origin},
    pack,
};
use crate::{Error, Manifest, ResidencyController, Result, SegmentTable};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs::File, sync::Arc};
pub(super) fn header(
    file: &File,
    file_id: [u8; 16],
    uuid: [u8; 16],
    generation: u128,
    o: Origin,
) -> Result<Header> {
    pack::recovery::verify_envelope(file, file_id, o.receipt)?;
    let mut b = [0; HEADER];
    pack::io::read_exact_at(file, &mut b, o.receipt.offset + pack::BLOCK as u64)?;
    let h = codec::header(&b)?;
    if h.hash != o.header_hash
        || h.uuid != uuid
        || h.generation != generation
        || h.payload_len != o.receipt.payload_length
    {
        return Err(Error::Corrupt("range origin envelope identity"));
    }
    Ok(h)
}
pub(super) struct Source {
    pub file: Arc<File>,
    pub file_id: [u8; 16],
    pub uuid: [u8; 16],
    pub origins: BTreeMap<u128, Origin>,
    pub cache: BTreeMap<u128, Header>,
}
impl crate::metadata::resolver::NodeSource for Source {
    fn read(&mut self, generation: u128, offset: u64, length: u32, bytes: &mut [u8]) -> Result<()> {
        let o = *self
            .origins
            .get(&generation)
            .ok_or(Error::Corrupt("missing range metadata origin"))?;
        if !self.cache.contains_key(&generation) {
            if self.cache.len() == 64 {
                self.cache.clear();
            }
            self.cache.insert(
                generation,
                header(&self.file, self.file_id, self.uuid, generation, o)?,
            );
        }
        let h = self.cache.get(&generation).unwrap();
        if offset < 256
            || !offset.is_multiple_of(32)
            || ![96, 160].contains(&length)
            || offset
                .checked_add(u64::from(length))
                .is_none_or(|end| end > h.descriptor_len)
            || bytes.len() != length as usize
        {
            return Err(Error::Corrupt("range metadata node extent"));
        }
        let start = codec::data_offset(o)?
            .checked_add(h.data_len)
            .and_then(|n| n.checked_add(offset))
            .ok_or(Error::LimitExceeded)?;
        Ok(pack::io::read_exact_at(self.file.as_ref(), bytes, start)?)
    }
}
pub(super) struct Loaded {
    pub snapshot: super::RangeSnapshot,
    pub origins: BTreeMap<u128, Origin>,
    pub placements: BTreeMap<u64, u128>,
    pub manifest_hash: [u8; 32],
}
pub(super) fn load(
    file: &File,
    file_id: [u8; 16],
    uuid: [u8; 16],
    receipt: pack::Receipt,
    controller: &ResidencyController,
) -> Result<Loaded> {
    if receipt.ordinal == 0 {
        return Err(Error::Corrupt("range environment has no published root"));
    }
    let mut b = vec![0; usize::try_from(receipt.payload_length).map_err(|_| Error::LimitExceeded)?];
    pack::io::read_exact_at(file, &mut b, receipt.offset + pack::BLOCK as u64)?;
    let h = codec::header(b.get(..HEADER).ok_or(Error::Corrupt("short range root"))?)?;
    if h.uuid != uuid {
        return Err(Error::Corrupt("foreign range environment"));
    }
    let (origins, placements) = codec::directories(&b, &h, receipt)?;
    let start = HEADER + usize::try_from(h.data_len).map_err(|_| Error::LimitExceeded)?;
    let end = start + usize::try_from(h.descriptor_len).map_err(|_| Error::LimitExceeded)?;
    let descriptor = &b[start..end];
    let digest: [u8; 32] = Sha256::digest(descriptor).into();
    if digest != h.descriptor_hash {
        return Err(Error::Corrupt("range logical descriptor hash"));
    }
    let source = Source {
        file: Arc::new(file.try_clone()?),
        file_id,
        uuid,
        origins: origins.clone(),
        cache: BTreeMap::new(),
    };
    let mut resolver = crate::metadata::Resolver::from_source(Box::new(source));
    let mut manifest = Manifest::decode_resolved(descriptor, Some(&mut resolver))?;
    if manifest.format_minor != 4
        || manifest.generation != h.generation
        || manifest.uuid != uuid
        || manifest.segments.len() != placements.len()
    {
        return Err(Error::Corrupt("range logical root identity"));
    }
    if receipt.ordinal == 1 {
        if manifest.previous != 0 {
            return Err(Error::Corrupt("range bootstrap predecessor"));
        }
    } else {
        let previous = *origins
            .get(&manifest.previous)
            .ok_or(Error::Corrupt("range predecessor missing"))?;
        let frame = pack::recovery::verify_envelope(file, file_id, receipt)?;
        if frame.parent != Some(previous.receipt) {
            return Err(Error::Corrupt(
                "range logical/physical predecessor mismatch",
            ));
        }
        header(file, file_id, uuid, manifest.previous, previous)?;
    }
    let mut table = SegmentTable::default();
    for entry in &manifest.segments {
        if entry.cohort != 0 {
            return Err(Error::Corrupt("cohort identity in range logical inventory"));
        }
        let generation = *placements
            .get(&entry.id)
            .ok_or(Error::Corrupt("range segment placement missing"))?;
        let o = *origins
            .get(&generation)
            .ok_or(Error::Corrupt("range data origin missing"))?;
        let origin = header(file, file_id, uuid, generation, o)?;
        if origin.segment != entry.id || origin.data_len != entry.length {
            return Err(Error::Corrupt("range data identity/extent"));
        }
        let identity = super::RangeIdentity::new(file_id, o, entry.length)?;
        let mapped = crate::mapping::Mapping::from_range(file, entry.clone(), identity, None)?;
        table.insert_mapping(mapped)?;
    }
    if h.segment != 0
        && placements
            .get(&h.segment)
            .is_none_or(|g| *g != h.generation)
    {
        return Err(Error::Corrupt("range own data placement"));
    }
    manifest.certify_indexed_table(&table)?;
    let admission = controller.admit(&table)?;
    Ok(Loaded {
        snapshot: super::RangeSnapshot {
            #[cfg(feature = "range-lifecycle")]
            checkpoint: None,
            manifest,
            segments: table,
            admission,
        },
        origins,
        placements,
        manifest_hash: digest,
    })
}
