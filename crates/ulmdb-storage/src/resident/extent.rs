//! Versioned overflow: real immutable chunk owners plus one quota-owned
//! contiguous decoded mmap. The reference segment names its descriptor version;
//! byte offsets address this actual mapping, never adjacent pool slots.
use super::{
    Pool, VersionKey, View,
    pool::{PAGE, Version, unpack},
};
use crate::{Error, Reference, Result};
use memmap2::{Mmap, MmapOptions};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
pub const MAX_OVERFLOW_VALUE_BYTES: usize = 16 << 20;
pub(super) const ROOT_BYTES: usize = 80;
pub(super) const TABLE_HEADER: usize = 40;
pub(super) const TABLE_ITEMS: usize = (PAGE - TABLE_HEADER) / 16;
pub(super) const ROOT_MAGIC: &[u8; 8] = b"ULMDBRE1";
pub(super) const TABLE_MAGIC: &[u8; 8] = b"ULMDBRT1";
#[derive(Clone, Copy, Debug)]
pub struct ExtentSource {
    pub reference: Reference,
    pub length: usize,
    pub version: VersionKey,
    pub kind: u8,
}
#[derive(Clone, Copy, Debug)]
pub struct OverflowStats {
    /// Charged decoded overflow mappings, not process RSS or fixed pool maps.
    pub mapped_bytes: u64,
    /// Remaining mapped allowance after fixed maps, with a256MiB combined cap.
    pub mapped_budget_bytes: u64,
}
pub(super) struct Backing {
    pub map: Mmap,
    _context: Option<Arc<crate::transform::TransformContext>>,
    _decoded: Option<crate::transform::DecodedLease>,
    _mapped: super::private_quota::Reservation,
}
pub(super) struct Extent {
    pub backing: Arc<Backing>,
    pub sources: Vec<ExtentSource>,
    // Excludes the descriptor owner itself, avoiding an Arc cycle.
    _owners: Vec<Arc<Version>>,
    _metadata: super::private_quota::Reservation,
}
pub(super) struct Pending {
    pub backing: Arc<Backing>,
    pub refs: Vec<(Reference, usize, u8)>,
    pub serial: u64,
    pub metadata: super::private_quota::Reservation,
}
pub fn reference_version(at: Reference) -> Result<VersionKey> {
    let physical = if at.segment == 1 {
        at
    } else {
        Reference {
            segment: 1,
            offset: at.segment,
        }
    };
    let (slot, generation, offset) = unpack(physical)?;
    if at.segment != 1 && offset != 0 {
        return Err(Error::Corrupt("overflow descriptor alignment"));
    }
    Ok(VersionKey { slot, generation })
}
impl Pool {
    pub fn overflow_stats(&self) -> OverflowStats {
        OverflowStats {
            mapped_bytes: self.extent_quota.used(),
            mapped_budget_bytes: self.extent_quota.limit(),
        }
    }
    pub(super) fn extent_backing(
        self: &Arc<Self>,
        bytes: usize,
    ) -> Result<(
        memmap2::MmapMut,
        super::private_quota::Reservation,
        Option<crate::transform::DecodedLease>,
    )> {
        let rounded =
            bytes.div_ceil(crate::residency::os::page_size()?) * crate::residency::os::page_size()?;
        let mapped = self.extent_quota.reserve(rounded as u64)?;
        let decoded = self
            .transform_context()
            .map(|c| c.reserve_resident_pool(rounded as u64))
            .transpose()?;
        let map = MmapOptions::new().len(bytes).map_anon()?;
        Ok((map, mapped, decoded))
    }
}
impl Backing {
    pub(super) fn seal(
        pool: &Arc<Pool>,
        map: memmap2::MmapMut,
        mapped: super::private_quota::Reservation,
        decoded: Option<crate::transform::DecodedLease>,
    ) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            map: map.make_read_only()?,
            _context: pool.transform_context().cloned(),
            _decoded: decoded,
            _mapped: mapped,
        }))
    }
    pub(super) fn read(&self, offset: u64, len: usize) -> Result<&[u8]> {
        let start = usize::try_from(offset).map_err(|_| Error::Corrupt("overflow offset"))?;
        self.map
            .get(
                start
                    ..start
                        .checked_add(len)
                        .ok_or(Error::Corrupt("overflow span"))?,
            )
            .ok_or(Error::Corrupt("overflow span"))
    }
}
impl View {
    fn source(&self, at: Reference, len: usize) -> Result<(ExtentSource, Arc<Version>)> {
        if at.segment != 1 {
            return Err(Error::Corrupt("overflow physical source namespace"));
        }
        let key = self.allocation_key(at, len)?;
        Ok((
            ExtentSource {
                reference: at,
                length: len,
                version: key,
                kind: 2,
            },
            Arc::clone(self.version(key.slot).unwrap()),
        ))
    }
    pub(super) fn install_extents(&self, pending: BTreeMap<u64, Pending>) -> Result<()> {
        for (segment, p) in pending {
            let key = reference_version(Reference { segment, offset: 0 })?;
            let owner = self
                .version(key.slot)
                .filter(|v| v.key() == key)
                .ok_or(Error::Corrupt("overflow descriptor owner absent"))?;
            let mut sources = Vec::new();
            let mut owners = Vec::new();
            sources
                .try_reserve_exact(p.refs.len())
                .map_err(|_| Error::LimitExceeded)?;
            owners
                .try_reserve_exact(p.refs.len() - 1)
                .map_err(|_| Error::LimitExceeded)?;
            for (index, (at, len, kind)) in p.refs.into_iter().enumerate() {
                let (mut source, v) = self.source(at, len)?;
                source.kind = kind;
                sources.push(source);
                if index != 0 {
                    owners.push(v)
                }
            }
            owner
                .extent
                .set(Arc::new(Extent {
                    backing: p.backing,
                    sources,
                    _owners: owners,
                    _metadata: p.metadata,
                }))
                .map_err(|_| Error::Corrupt("duplicate overflow cache install"))?;
        }
        Ok(())
    }
    fn decode_extent(&self, segment: u64, limit: usize) -> Result<Arc<Extent>> {
        let root = Reference {
            segment: 1,
            offset: segment,
        };
        let (mut root_source, owner) = self.source(root, ROOT_BYTES)?;
        root_source.kind = 0;
        let h = owner.read(root_source.version.generation, 0, ROOT_BYTES)?;
        if &h[..8] != ROOT_MAGIC
            || h[8..12] != 1u32.to_le_bytes()
            || h[12..16] != (ROOT_BYTES as u32).to_le_bytes()
        {
            return Err(Error::Corrupt("overflow descriptor version"));
        }
        let length = u64::from_le_bytes(h[16..24].try_into().unwrap());
        if length <= 4080 || length > MAX_OVERFLOW_VALUE_BYTES as u64 {
            return Err(Error::Corrupt("overflow value length"));
        }
        let total = length as usize + 16;
        let chunks = total.div_ceil(PAGE);
        let tables = chunks.div_ceil(TABLE_ITEMS);
        if u32::from_le_bytes(h[24..28].try_into().unwrap()) as usize != chunks
            || u32::from_le_bytes(h[28..32].try_into().unwrap()) as usize != tables
        {
            return Err(Error::Corrupt("overflow source counts"));
        }
        if chunks + tables + 1 > limit {
            return Err(Error::LimitExceeded);
        }
        let mut next = Reference::decode(&h[32..48])?;
        let expected: [u8; 32] = h[48..80].try_into().unwrap();
        let metadata = self
            .pool
            .private_quota
            .reserve(((chunks + tables + 1) * 128 + 512) as u64)?;
        let mut sources = Vec::new();
        let mut owners = Vec::new();
        let mut data = Vec::new();
        sources
            .try_reserve_exact(chunks + tables + 1)
            .map_err(|_| Error::LimitExceeded)?;
        owners
            .try_reserve_exact(chunks + tables)
            .map_err(|_| Error::LimitExceeded)?;
        data.try_reserve_exact(chunks)
            .map_err(|_| Error::LimitExceeded)?;
        let mut seen = BTreeSet::new();
        seen.insert(root_source.version);
        sources.push(root_source);
        for ordinal in 0..tables {
            let (mut s, v) = self.source(next, PAGE)?;
            s.kind = 1;
            if !seen.insert(s.version) {
                return Err(Error::Corrupt("overflow source alias/cycle"));
            }
            let b = v.read(s.version.generation, 0, PAGE)?;
            let count = usize::min(TABLE_ITEMS, chunks - data.len());
            if &b[..8] != TABLE_MAGIC
                || u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize != count
                || u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize != ordinal
                || u64::from_le_bytes(b[16..24].try_into().unwrap()) as usize != chunks
                || b[TABLE_HEADER + count * 16..].iter().any(|&x| x != 0)
            {
                return Err(Error::Corrupt("overflow table contract"));
            }
            next = Reference::decode(&b[24..40])?;
            for i in 0..count {
                data.push(Reference::decode(
                    &b[TABLE_HEADER + i * 16..TABLE_HEADER + (i + 1) * 16],
                )?)
            }
            sources.push(s);
            owners.push(v);
        }
        if !next.is_null() || data.len() != chunks {
            return Err(Error::Corrupt("overflow table termination"));
        }
        let (mut map, mapped, decoded) = self.pool.extent_backing(total)?;
        for (index, at) in data.into_iter().enumerate() {
            let (s, v) = self.source(at, PAGE)?;
            if !seen.insert(s.version) {
                return Err(Error::Corrupt("overflow source alias/cycle"));
            }
            let b = v.read(s.version.generation, 0, PAGE)?;
            let used = usize::min(PAGE, total - index * PAGE);
            if b[used..].iter().any(|&x| x != 0) {
                return Err(Error::Corrupt("overflow final padding"));
            }
            map[index * PAGE..index * PAGE + used].copy_from_slice(&b[..used]);
            sources.push(s);
            owners.push(v);
        }
        if map[..2] != 1u16.to_le_bytes()
            || map[2..4] != [0; 2]
            || map[4..8] != 16u32.to_le_bytes()
            || map[8..16] != length.to_le_bytes()
            || Sha256::digest(&map[..]).as_slice() != expected
        {
            return Err(Error::Corrupt("overflow payload header/digest"));
        }
        Ok(Arc::new(Extent {
            backing: Backing::seal(&self.pool, map, mapped, decoded)?,
            sources,
            _owners: owners,
            _metadata: metadata,
        }))
    }
    fn extent(&self, at: Reference, limit: usize) -> Result<&Extent> {
        if limit == 0 {
            return Err(Error::LimitExceeded);
        }
        let key = reference_version(at)?;
        let owner = self
            .version(key.slot)
            .filter(|v| v.key() == key)
            .ok_or(Error::Corrupt("overflow descriptor generation absent"))?;
        if owner.extent.get().is_none() {
            self.pool.check_creator()?;
            let _guard = owner
                .extent_lock
                .lock()
                .map_err(|_| Error::Corrupt("overflow decode lock poisoned"))?;
            if owner.extent.get().is_none() {
                owner
                    .extent
                    .set(self.decode_extent(at.segment, limit)?)
                    .map_err(|_| Error::Corrupt("overflow concurrent cache install"))?;
            }
        }
        let extent = owner.extent.get().unwrap();
        if extent.sources.len() > limit {
            return Err(Error::LimitExceeded);
        }
        if extent
            .sources
            .iter()
            .any(|s| !self.contains_version(s.version))
        {
            return Err(Error::Corrupt("overflow source absent from exact view"));
        }
        Ok(extent)
    }
    pub(super) fn read_overflow(
        &self,
        at: Reference,
        len: usize,
    ) -> Result<(&[u8], VersionKey, &[ExtentSource])> {
        self.read_overflow_limited(at, len, usize::MAX)
    }
    pub(super) fn read_overflow_limited(
        &self,
        at: Reference,
        len: usize,
        limit: usize,
    ) -> Result<(&[u8], VersionKey, &[ExtentSource])> {
        let extent = self.extent(at, limit)?;
        Ok((
            extent.backing.read(at.offset, len)?,
            reference_version(at)?,
            &extent.sources,
        ))
    }
    pub(super) fn overflow_allocation(&self, at: Reference, len: usize) -> Result<VersionKey> {
        let extent = self.extent(at, usize::MAX)?;
        if at.offset != 0 || len != extent.backing.map.len() {
            return Err(Error::Corrupt("overflow object is not exact allocation"));
        }
        Ok(reference_version(at)?)
    }
}

#[cfg(test)]
mod tests {
    use super::super::wal_codec::{Kind, LiteralSlot, Record};
    use super::*;
    use crate::{ReadStore, WriteStore};
    fn config() -> super::super::Config {
        super::super::Config {
            kv_pages: 512,
            tiny_pages: 16,
            max_reserved_bytes: 32 << 20,
            max_metadata_bytes: 32 << 20,
            max_views: 16,
        }
    }
    fn fixture() -> (Reference, Record) {
        let pool = Pool::create(config()).unwrap();
        let mut a = pool.begin(pool.empty().unwrap()).unwrap();
        let at = a.put_payload(&vec![7; (1 << 20) + 17]).unwrap();
        let prepared = a.freeze().unwrap();
        let allocations = prepared
            .slots()
            .map(|s| LiteralSlot {
                key: s.key,
                expected_previous_generation: 0,
                ranges: s.ranges.to_vec(),
                bytes: s.bytes.to_vec(),
            })
            .collect();
        (
            at,
            Record {
                uuid: [1; 16],
                epoch: 1,
                lsn: 1,
                sequence: 1,
                commit: 2,
                parent_commit: 1,
                parent: crate::hybrid::redo::WalReceipt {
                    lsn: 0,
                    sequence: 0,
                    offset: 196608,
                    length: 0,
                    digest: [1; 32],
                },
                root: Reference::NULL,
                next_database: 1,
                kind: Kind::Write,
                allocations,
                retired: vec![],
            },
        )
    }
    #[test]
    fn cold_extent_decode_rejects_malformed_lengths_tables_aliases_generations_and_digest() {
        let (at, original) = fixture();
        let root_key = reference_version(at).unwrap();
        let root = original
            .allocations
            .iter()
            .position(|a| a.key == root_key)
            .unwrap();
        let table_ref = Reference::decode(&original.allocations[root].bytes[32..48]).unwrap();
        let table_key = reference_version(table_ref).unwrap();
        let table = original
            .allocations
            .iter()
            .position(|a| a.key == table_key)
            .unwrap();
        let chunk_ref = Reference::decode(&original.allocations[table].bytes[40..56]).unwrap();
        let chunk_key = reference_version(chunk_ref).unwrap();
        let chunk = original
            .allocations
            .iter()
            .position(|a| a.key == chunk_key)
            .unwrap();
        for case in 0..7 {
            let mut record = original.clone();
            match case {
                1 => {
                    record.allocations[root].bytes[16..24].copy_from_slice(&u64::MAX.to_le_bytes())
                }
                2 => record.allocations[table].bytes[24..40].copy_from_slice(&table_ref.encode()),
                3 => {
                    let first = record.allocations[table].bytes[40..56].to_vec();
                    record.allocations[table].bytes[56..72].copy_from_slice(&first)
                }
                4 => record.allocations[chunk].bytes[100] ^= 1,
                5 => {
                    let mut bad = chunk_ref;
                    bad.offset += 1 << 28;
                    record.allocations[table].bytes[40..56].copy_from_slice(&bad.encode())
                }
                6 => {
                    record.allocations[root].bytes[24..28].copy_from_slice(&u32::MAX.to_le_bytes())
                }
                _ => (),
            }
            let pool = Pool::create(config()).unwrap();
            let view = pool
                .empty()
                .unwrap()
                .replay_literals(&record)
                .unwrap()
                .activate();
            assert!(matches!(
                view.read_with_sources_limited(at, 16, 0),
                Err(Error::LimitExceeded)
            ));
            assert_eq!(pool.overflow_stats().mapped_bytes, 0);
            let result = view.payload(at);
            if case == 0 {
                assert!(result.unwrap().iter().all(|&x| x == 7));
                let raw = Reference {
                    segment: 1,
                    offset: at.segment,
                };
                assert!(
                    view.read(raw, (1 << 20) + 33).is_err(),
                    "no fabricated contiguous physical span"
                );
                let missing = view.without_versions(&[chunk_key]).unwrap();
                assert!(
                    missing.payload(at).is_err(),
                    "cached bytes cannot hide absent source membership"
                );
            } else {
                assert!(matches!(result, Err(Error::Corrupt(_))), "case {case}");
                assert_eq!(pool.overflow_stats().mapped_bytes, 0);
            }
        }
    }
    #[test]
    fn mapped_budget_and_aborted_reference_admission_do_not_reuse_generations() {
        let mut c = config();
        c.max_reserved_bytes = ((c.kv_pages + c.tiny_pages) * PAGE) as u64;
        let p = Pool::create(c).unwrap();
        let mut a = p.begin(p.empty().unwrap()).unwrap();
        let before = p.stats().unwrap().allocations;
        assert!(matches!(
            a.put_payload(&vec![1; (1 << 20) + 1]),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(p.stats().unwrap().allocations, before);
        let p = Pool::create(config()).unwrap();
        let mut a = p.begin(p.empty().unwrap()).unwrap();
        let cp = a.checkpoint().unwrap();
        let old = a.put_payload(&vec![1; (1 << 20) + 1]).unwrap();
        let raw = Reference {
            segment: 1,
            offset: old.segment,
        };
        assert!(
            a.rewrite(raw, &[0; 80], 16).is_err(),
            "opaque descriptor cannot be rewritten through raw private API"
        );
        a.rollback_checkpoint(&cp).unwrap();
        assert_eq!(p.overflow_stats().mapped_bytes, 0);
        assert!(a.payload(old).is_err());
        let new = a.put_payload(&vec![2; (1 << 20) + 1]).unwrap();
        assert_ne!(old.segment, new.segment);
        assert!(a.payload(old).is_err());
        assert!(a.payload(new).unwrap().iter().all(|&x| x == 2));
    }
}
