//! Direct bounded reads: no generation table or historical-envelope resolver.
use super::super::{RangeIdentity, RangeSnapshot, Seal, pack};
use super::{
    codec::{self, Header},
    reference::PhysicalRef,
    tree::{self, Kind, Node},
};
use crate::{Error, Inventory, ResidencyController, Result, SegmentEntry, SegmentTable};
use std::{collections::BTreeMap, fs::File, sync::Arc};
pub(super) struct Loaded {
    pub snapshot: RangeSnapshot,
    pub root: Option<Arc<Node>>,
}
struct Decode<'a> {
    file: &'a File,
    id: [u8; 16],
    end: u64,
    start: u64,
    blob_start: u64,
    blob_end: u64,
    own: u64,
    data_len: u64,
    next_id: u64,
    slots: BTreeMap<u64, u64>,
    budget: usize,
    local: u32,
    local_bytes: u64,
    own_seen: u32,
    entries: Vec<SegmentEntry>,
    table: SegmentTable,
}
impl Decode<'_> {
    fn extent(&mut self, r: PhysicalRef) -> Result<()> {
        let end = r.end()?;
        if self
            .slots
            .range(..=r.offset)
            .next_back()
            .is_some_and(|(start, len)| start.checked_add(*len).is_none_or(|n| n > r.offset))
            || self
                .slots
                .range(r.offset..)
                .next()
                .is_some_and(|(start, _)| *start < end)
        {
            return Err(Error::Corrupt("physical locator overlap/alias"));
        }
        self.slots.insert(r.offset, r.length);
        Ok(())
    }
    fn node(
        &mut self,
        r: PhysicalRef,
        parent_depth: Option<u8>,
        parent_offset: Option<u64>,
    ) -> Result<Arc<Node>> {
        if self.budget == 0 {
            return Err(Error::Corrupt("physical node visit budget"));
        }
        self.budget -= 1;
        r.checked(self.id, self.end, true)?;
        if parent_offset.is_some_and(|n| r.end().is_ok_and(|end| end > n)) {
            return Err(Error::Corrupt("physical child is not backward"));
        }
        if r.offset >= self.start {
            if r.offset < self.blob_start || r.end()? > self.blob_end {
                return Err(Error::Corrupt("physical local node outside blob"));
            }
            self.local += 1;
            self.local_bytes += r.length;
        } else if r.end()? > self.start {
            return Err(Error::Corrupt("old node crosses selected frame"));
        }
        self.extent(r)?;
        let mut b = [0; 160];
        let length = r.length as usize;
        pack::io::read_exact_at(self.file, &mut b[..length], r.offset)?;
        let b = &b[..length];
        if tree::hash(b) != r.hash || &b[..8] != b"ULMDBL06" || b[10..16] != [0; 6] {
            return Err(Error::Corrupt("physical node type/hash/reserved"));
        }
        let depth = b[9];
        let prefix = u64::from_le_bytes(b[16..24].try_into().unwrap());
        let expected = usize::try_from(u64::from_le_bytes(b[24..32].try_into().unwrap()))
            .map_err(|_| Error::LimitExceeded)?;
        if depth > 64
            || parent_depth.is_some_and(|d| depth <= d)
            || tree::prefix(prefix, depth) != prefix
            || expected == 0
            || expected >= 1_048_576
        {
            return Err(Error::Corrupt("physical node prefix/depth/count"));
        }
        let kind = match b[8] {
            0 if length == 96 && depth == 64 && expected == 1 => {
                let data =
                    PhysicalRef::decode(&b[32..96])?.ok_or(Error::Corrupt("null physical data"))?;
                data.checked(self.id, self.end, false)?;
                self.extent(data)?;
                if prefix == 0 || prefix == u64::MAX || prefix >= self.next_id {
                    return Err(Error::Corrupt("physical leaf logical ID"));
                }
                if data.offset >= self.start {
                    if data.offset != self.start + 8192
                        || data.length != self.data_len
                        || prefix != self.own
                    {
                        return Err(Error::Corrupt("physical current data identity"));
                    }
                    self.own_seen += 1;
                } else if data.end()? > self.start {
                    return Err(Error::Corrupt("old data crosses selected frame"));
                }
                let entry = SegmentEntry {
                    id: prefix,
                    length: data.length,
                    hash: data.hash,
                    cohort: 0,
                };
                self.table.check_insert_id(entry.id)?;
                let identity = RangeIdentity {
                    file_id: self.id,
                    seal: Seal::Prefix {
                        committed_end: self.end,
                        data_hash: data.hash,
                    },
                    data_offset: data.offset,
                    length: data.length,
                };
                let mapping =
                    crate::mapping::Mapping::from_range(self.file, entry.clone(), identity, None)?;
                self.table.insert_mapping(mapping)?;
                self.entries.push(entry.clone());
                Kind::Leaf { entry, data }
            }
            1 if length == 160 && depth < 64 => {
                let left_ref = PhysicalRef::decode(&b[32..96])?
                    .ok_or(Error::Corrupt("null left physical node"))?;
                let right_ref = PhysicalRef::decode(&b[96..160])?
                    .ok_or(Error::Corrupt("null right physical node"))?;
                let l = self.node(left_ref, Some(depth), Some(r.offset))?;
                let right = self.node(right_ref, Some(depth), Some(r.offset))?;
                if tree::prefix(l.prefix, depth) != prefix
                    || tree::prefix(right.prefix, depth) != prefix
                    || tree::bit(l.prefix, depth)
                    || !tree::bit(right.prefix, depth)
                    || l.count.checked_add(right.count) != Some(expected)
                {
                    return Err(Error::Corrupt("physical branch routing/count"));
                }
                Kind::Branch(l, right)
            }
            _ => return Err(Error::Corrupt("physical node tag/length")),
        };
        Ok(Arc::new(Node {
            prefix,
            depth,
            count: expected,
            location: Some(r),
            kind,
        }))
    }
}
pub(super) fn load(
    file: &File,
    id: [u8; 16],
    uuid: [u8; 16],
    receipt: pack::Receipt,
    controller: &ResidencyController,
) -> Result<Loaded> {
    if receipt.ordinal == 0 {
        return Err(Error::Corrupt("direct range root missing"));
    }
    let mut bytes =
        vec![0; usize::try_from(receipt.payload_length).map_err(|_| Error::LimitExceeded)?];
    pack::io::read_exact_at(file, &mut bytes, receipt.offset + 4096)?;
    let Header {
        mut manifest,
        root,
        data_len,
        blob_len,
        own_id,
        local,
        file: expected_file,
        count,
    } = codec::decode(&bytes, receipt.offset)?;
    if expected_file != id || manifest.uuid != uuid {
        return Err(Error::Corrupt("direct environment identity"));
    }
    #[cfg(feature = "range-lifecycle")]
    let anchor = crate::range::rotation::global(file)?.anchor;
    #[cfg(feature = "range-lifecycle")]
    let base_sequence = anchor.map_or(0, |a| a.sequence());
    #[cfg(not(feature = "range-lifecycle"))]
    let base_sequence = 0;
    if manifest
        .sequence
        .checked_sub(base_sequence)
        .and_then(|n| n.checked_add(1))
        != Some(receipt.ordinal)
    {
        return Err(Error::Corrupt("direct sequence/physical ordinal mismatch"));
    }
    if Some(manifest.generation)
        != ulmdb_core::range_generation::physical_generation(
            u128::from_le_bytes(id),
            receipt.ordinal,
        )
    {
        return Err(Error::Corrupt(
            "direct physical generation/ordinal mismatch",
        ));
    }
    let frame = pack::recovery::verify_envelope(file, id, receipt)?;
    if receipt.ordinal == 1 {
        #[cfg(feature = "range-lifecycle")]
        let anchored = anchor.map_or(manifest.sequence == 0 && manifest.parent == 0, |a| {
            a.matches(&manifest)
        });
        #[cfg(not(feature = "range-lifecycle"))]
        let anchored = manifest.sequence == 0 && manifest.parent == 0;
        if manifest.previous != 0 || !anchored {
            return Err(Error::Corrupt("direct bootstrap identity"));
        }
    } else {
        let previous = frame
            .parent
            .ok_or(Error::Corrupt("direct parent missing"))?;
        let mut old = [0; codec::HEADER];
        pack::io::read_exact_at(file, &mut old, previous.offset + 4096)?;
        if &old[..8] != b"ULMDBR06"
            || old[10..12] != 6u16.to_le_bytes()
            || old[16..32] != uuid
            || old[120..128] != codec::FEATURES.to_le_bytes()
            || old[64..80] != manifest.parent.to_le_bytes()
            || old[48..56] != manifest.epoch.to_le_bytes()
            || u64::from_le_bytes(old[56..64].try_into().unwrap()).checked_add(1)
                != Some(manifest.sequence)
            || u128::from_le_bytes(old[32..48].try_into().unwrap()) != manifest.previous
            || <[u8; 32]>::from(sha2::Sha256::digest(&old[..codec::HEADER - 32]))
                != old[codec::HEADER - 32..]
        {
            return Err(Error::Corrupt("direct logical predecessor mismatch"));
        }
    }
    let blob_start = codec::blob_offset(receipt.offset, data_len)?;
    let mut d = Decode {
        file,
        id,
        end: receipt.frame_end()?,
        start: receipt.offset,
        blob_start,
        blob_end: blob_start + blob_len,
        own: own_id,
        data_len,
        next_id: manifest.next_segment,
        slots: BTreeMap::new(),
        budget: count.saturating_mul(2).saturating_sub(1),
        local: 0,
        local_bytes: 0,
        own_seen: 0,
        entries: Vec::with_capacity(count),
        table: SegmentTable::default(),
    };
    let root = root.map(|r| d.node(r, None, None)).transpose()?;
    if d.entries.len() != count
        || d.local != local
        || d.local_bytes != blob_len
        || d.own_seen != u32::from(own_id != 0)
    {
        return Err(Error::Corrupt("direct local/inventory coverage"));
    }
    manifest.segments = Inventory::range_from_entries(d.entries)?;
    let admission = controller.admit(&d.table)?;
    Ok(Loaded {
        snapshot: RangeSnapshot {
            #[cfg(feature = "range-lifecycle")]
            checkpoint: Some(super::super::checkpoint::RangeCheckpoint::mint(
                Arc::new(file.try_clone()?),
                id,
                &manifest,
                receipt,
                frame.parent,
                codec::scalar_hash(&manifest),
            )?),
            manifest,
            segments: d.table,
            admission,
        },
        root,
    })
}
use sha2::Digest;
