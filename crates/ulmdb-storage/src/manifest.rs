use crate::{Error, Reference, Result, SegmentEntry};
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};

#[derive(Clone, Debug)]
pub struct Manifest {
    pub(crate) transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
    pub page_size: ulmdb_core::PageSize,
    pub value_limit: ulmdb_core::ValueLimit,
    /// Preserve old descriptors during snapshot export; writers upgrade at commit.
    pub format_minor: u16,
    pub uuid: [u8; 16],
    pub generation: u128,
    pub epoch: u64,
    pub sequence: u64,
    pub commit: u128,
    pub parent: u128,
    pub previous: u128,
    pub catalog: Reference,
    pub next_database: u64,
    pub next_segment: u64,
    pub segments: crate::Inventory,
    pub(crate) index: Option<crate::metadata::Index>,
}
#[cfg(not(windows))]
pub fn new_identity() -> Result<u128> {
    // File implements Read for &File: the shared random device requires no
    // user-space mutex or seek state, and need not be reopened for every ID.
    static RANDOM: std::sync::OnceLock<File> = std::sync::OnceLock::new();
    if RANDOM.get().is_none() {
        let file = File::open("/dev/urandom")?;
        let _ = RANDOM.set(file);
    }
    let mut source = RANDOM.get().ok_or(Error::LimitExceeded)?;
    for _ in 0..16 {
        let mut bytes = [0; 16];
        source.read_exact(&mut bytes)?;
        let identity = u128::from_le_bytes(bytes);
        if identity != 0 {
            return Ok(identity);
        }
    }
    Err(Error::Io(std::io::Error::other(
        "random source returned repeated zero identities",
    )))
}
#[cfg(windows)]
pub fn new_identity() -> Result<u128> {
    identity_from(|bytes| ulmdb_platform::fill_random(bytes))
}
#[cfg(any(windows, test))]
fn identity_from(mut fill: impl FnMut(&mut [u8; 16]) -> std::io::Result<()>) -> Result<u128> {
    for _ in 0..16 {
        let mut bytes = [0; 16];
        fill(&mut bytes)?;
        let identity = u128::from_le_bytes(bytes);
        if identity != 0 {
            return Ok(identity);
        }
    }
    Err(Error::Io(std::io::Error::other(
        "random source returned repeated zero identities",
    )))
}

pub fn create_layout(directory: &Path) -> Result<()> {
    crate::access::create_layout(directory, crate::access::FileAccess::PRIVATE)
}
pub(crate) fn number<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N]> {
    bytes
        .get(
            offset
                ..offset
                    .checked_add(N)
                    .ok_or(Error::Corrupt("descriptor offset overflow"))?,
        )
        .ok_or(Error::Corrupt("short descriptor"))?
        .try_into()
        .map_err(|_| Error::Corrupt("descriptor field"))
}
impl Manifest {
    pub fn transform_context(&self) -> Option<&std::sync::Arc<crate::transform::TransformContext>> {
        self.transform.as_ref()
    }
    pub fn with_transform_context(
        mut self,
        context: Option<std::sync::Arc<crate::transform::TransformContext>>,
    ) -> Self {
        self.transform = context;
        self
    }
    pub fn empty() -> Result<Self> {
        let identity = new_identity()?;
        Ok(Self {
            transform: None,
            format_minor: 2,
            page_size: Default::default(),
            value_limit: Default::default(),
            uuid: new_identity()?.to_le_bytes(),
            generation: identity,
            epoch: 1,
            sequence: 0,
            commit: identity,
            parent: 0,
            previous: 0,
            catalog: Reference::NULL,
            next_database: 1,
            next_segment: 1,
            segments: Vec::new().into(),
            index: None,
        })
    }
    /// Copy scalar snapshot metadata without cloning an inventory that the
    /// next builder will replace. Caller must install its complete inventory.
    pub fn clone_metadata(&self) -> Self {
        Self {
            transform: self.transform.clone(),
            format_minor: self.format_minor,
            page_size: self.page_size,
            value_limit: self.value_limit,
            uuid: self.uuid,
            generation: self.generation,
            epoch: self.epoch,
            sequence: self.sequence,
            commit: self.commit,
            parent: self.parent,
            previous: self.previous,
            catalog: self.catalog,
            next_database: self.next_database,
            next_segment: self.next_segment,
            segments: Vec::new().into(),
            index: None,
        }
    }
    /// Build a persistent metadata directory, sharing immutable unchanged paths
    /// from a prepared indexed base. Public inventory changes require reprepare.
    pub fn prepare_indexed(&mut self, base: Option<&Manifest>) -> Result<()> {
        let mut index = crate::metadata::prepare(self, base)?;
        self.segments = index.adopt_inventory(std::mem::take(&mut self.segments))?;
        self.format_minor = 4;
        self.index = Some(index);
        Ok(())
    }
    pub fn certify_indexed_table(&mut self, table: &crate::SegmentTable) -> Result<()> {
        if self.format_minor != 4 {
            return Ok(());
        }
        let index = self
            .index
            .as_ref()
            .ok_or(Error::InvalidInput("unprepared indexed manifest"))?;
        if index.append_eligible(self, table.residency_identity()) {
            return Ok(());
        }
        index.root_bytes(self)?;
        if !table.matches_inventory(&self.segments) {
            return Err(Error::Corrupt("indexed table inventory mismatch"));
        }
        self.index.as_mut().unwrap().certified_table = Some(table.residency_identity());
        Ok(())
    }
    /// Reachable metadata cohorts, not historical footer inventories. An
    /// unprepared format4 manifest cannot be encoded or activated.
    /// Owned resumable traversal; no lazy full origins-set materialization.
    pub fn metadata_origin_cursor(&self) -> Result<crate::MetadataOriginCursor> {
        crate::MetadataOriginCursor::new(self)
    }
    pub fn metadata_origins(&self) -> std::collections::BTreeSet<u128> {
        self.metadata_origins_ref().clone()
    }
    /// Borrow the lazily cached reachable origins without allocating a temporary
    /// owned set. Legacy and unprepared manifests share a static empty set.
    pub fn metadata_origins_ref(&self) -> &std::collections::BTreeSet<u128> {
        static EMPTY: std::collections::BTreeSet<u128> = std::collections::BTreeSet::new();
        if self.format_minor == 4
            && let Some(index) = &self.index
        {
            return index.origins();
        }
        &EMPTY
    }
    pub(crate) fn page_size_from_header(b: &[u8]) -> Result<ulmdb_core::PageSize> {
        if b.len() < 176 || b[152..176].iter().any(|n| *n != 0) {
            return Err(Error::Corrupt("invalid page geometry header"));
        }
        let declared = u64::from_le_bytes(number(b, 120)?);
        if declared & 16 != 0 {
            let value = ulmdb_core::PageSize::new(u32::from_le_bytes(number(b, 144)?) as usize)
                .map_err(|_| Error::Corrupt("invalid persisted page size"))?;
            if value == ulmdb_core::PageSize::DEFAULT {
                return Err(Error::Corrupt("noncanonical page size feature"));
            }
            Ok(value)
        } else {
            if b[144..148] != [0; 4] {
                return Err(Error::Corrupt("undeclared page size"));
            }
            Ok(ulmdb_core::PageSize::DEFAULT)
        }
    }
    pub(crate) fn value_limit_from_header(b: &[u8]) -> Result<ulmdb_core::ValueLimit> {
        let declared = u64::from_le_bytes(number(b, 120)?);
        if declared & 32 != 0 {
            let limit = ulmdb_core::ValueLimit::new(u32::from_le_bytes(number(b, 148)?) as usize)
                .map_err(|_| Error::Corrupt("invalid value limit"))?;
            if limit == ulmdb_core::ValueLimit::DEFAULT {
                return Err(Error::Corrupt("noncanonical value limit"));
            }
            Ok(limit)
        } else {
            if number::<4>(b, 148)? != [0; 4] {
                return Err(Error::Corrupt("undeclared value limit"));
            }
            Ok(ulmdb_core::ValueLimit::DEFAULT)
        }
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        let mut features = match self.format_minor {
            1 => 1u64,
            2 => 3u64,
            3 => 7u64,
            4 => 15u64,
            _ => return Err(Error::InvalidInput("unsupported descriptor minor version")),
        };
        if self.page_size != ulmdb_core::PageSize::DEFAULT {
            features |= 16;
        }
        if self.value_limit != ulmdb_core::ValueLimit::DEFAULT {
            features |= 32;
        }
        if self.segments.len() > 1_048_575 {
            return Err(Error::LimitExceeded);
        }
        let (header, root, local, blob) = if self.format_minor == 4 {
            let index = self.index.as_ref().ok_or(Error::InvalidInput(
                "indexed manifest requires prepare_indexed",
            ))?;
            (
                256,
                index.root_bytes(self)?,
                index.local_nodes,
                index.blob.as_ref(),
            )
        } else {
            (192, [0; 64], 0, &[][..])
        };
        let length = if self.format_minor == 4 {
            header + blob.len()
        } else {
            header + self.segments.len() * 64
        };
        let mut b = vec![0; length];
        b[..8].copy_from_slice(b"ULMDBROT");
        b[10..12].copy_from_slice(&self.format_minor.to_le_bytes());
        b[12..16].copy_from_slice(&(header as u32).to_le_bytes());
        b[16..32].copy_from_slice(&self.uuid);
        b[32..48].copy_from_slice(&self.generation.to_le_bytes());
        b[48..56].copy_from_slice(&self.epoch.to_le_bytes());
        b[56..64].copy_from_slice(&self.sequence.to_le_bytes());
        b[64..80].copy_from_slice(&self.commit.to_le_bytes());
        b[80..96].copy_from_slice(&self.parent.to_le_bytes());
        b[96..112].copy_from_slice(&self.catalog.encode());
        b[112..116].copy_from_slice(&(self.segments.len() as u32).to_le_bytes());
        b[120..128].copy_from_slice(&features.to_le_bytes());
        if self.value_limit != ulmdb_core::ValueLimit::DEFAULT {
            b[148..152].copy_from_slice(&(self.value_limit.get() as u32).to_le_bytes());
        }
        b[128..144].copy_from_slice(&self.previous.to_le_bytes());
        if self.page_size != ulmdb_core::PageSize::DEFAULT {
            b[144..148].copy_from_slice(&(self.page_size.get() as u32).to_le_bytes());
        }
        b[176..184].copy_from_slice(&self.next_database.to_le_bytes());
        b[184..192].copy_from_slice(&self.next_segment.to_le_bytes());
        if self.format_minor == 4 {
            b[116..120].copy_from_slice(&local.to_le_bytes());
            b[192..256].copy_from_slice(&root);
            b[256..].copy_from_slice(blob);
            return Ok(b);
        }
        for (i, s) in self.segments.iter().enumerate() {
            let o = 192 + i * 64;
            b[o..o + 8].copy_from_slice(&s.id.to_le_bytes());
            b[o + 8..o + 16].copy_from_slice(&s.length.to_le_bytes());
            b[o + 16..o + 48].copy_from_slice(&s.hash);
            if self.format_minor >= 3 {
                b[o + 48..o + 64].copy_from_slice(&s.cohort.to_le_bytes());
            } else {
                if s.cohort != 0 {
                    return Err(Error::InvalidInput("legacy descriptor has cohort origin"));
                }
                b[o + 48..o + 56].copy_from_slice(&1u64.to_le_bytes());
            }
        }
        Ok(b)
    }
    pub(crate) fn decode_prefix(b: &[u8]) -> Result<(u16, ulmdb_core::PageSize, ulmdb_core::ValueLimit, usize)> {
        Self::decode_prefix_with_length(b, b.len())
    }
    pub(crate) fn decode_prefix_with_length(b: &[u8], length: usize) -> Result<(u16, ulmdb_core::PageSize, ulmdb_core::ValueLimit, usize)> {
        let format_minor = u16::from_le_bytes(number(b, 10)?);
        let features = match format_minor {
            1 => 1u64,
            2 => 3u64,
            3 => 7u64,
            4 => 15u64,
            _ => return Err(Error::Corrupt("unsupported descriptor minor version")),
        };
        let header = if format_minor == 4 { 256 } else { 192 };
        if b.len() < header
            || &b[..8] != b"ULMDBROT"
            || b[8..10] != 0u16.to_le_bytes()
            || b[12..16] != (header as u32).to_le_bytes()
            || (format_minor != 4 && b[116..120] != [0; 4])
            || ![features, features | 16, features | 32, features | 48]
                .contains(&u64::from_le_bytes(number(b, 120)?))
            || b[152..176].iter().any(|n| *n != 0)
        {
            return Err(Error::Corrupt("unsupported root descriptor"));
        }
        let page_size = Self::page_size_from_header(b)?;
        let value_limit = Self::value_limit_from_header(b)?;
        let count = u32::from_le_bytes(number(b, 112)?) as usize;
        if count >= 1_048_576 || (format_minor != 4 && length != 192 + count * 64) {
            return Err(Error::Corrupt("invalid root inventory size"));
        }
        Ok((format_minor, page_size, value_limit, count))
    }
    pub(crate) fn decode(b: &[u8]) -> Result<Self> {
        Self::decode_resolved(b, None)
    }
    pub(crate) fn decode_resolved(
        b: &[u8],
        resolver: Option<&mut crate::metadata::Resolver>,
    ) -> Result<Self> {
        let (format_minor, page_size, value_limit, count) = Self::decode_prefix(b)?;
        let (segments, index) = if format_minor == 4 {
            let (index, entries) = crate::metadata::decode(
                b,
                u128::from_le_bytes(number(b, 32)?),
                count,
                u32::from_le_bytes(number(b, 116)?),
                &b[192..256],
                resolver,
            )?;
            (entries, Some(index))
        } else {
            let mut segments = Vec::with_capacity(count);
            let mut prior = 0;
            let mut first_chunk = None;
            for i in 0..count {
                let o = 192 + i * 64;
                let id = u64::from_le_bytes(number(b, o)?);
                if id <= prior
                    || id == u64::MAX
                    || (format_minor < 3
                        && (b[o + 48..o + 56] != 1u64.to_le_bytes() || b[o + 56..o + 64] != [0; 8]))
                {
                    return Err(Error::Corrupt("invalid root segment inventory"));
                }
                let first = *first_chunk.get_or_insert(id >> 8);
                if (id >> 8) - first >= crate::table::MAX_DIRECTORY_CHUNKS {
                    return Err(Error::Corrupt("segment inventory exceeds directory window"));
                }
                segments.push(SegmentEntry {
                    id,
                    cohort: if format_minor >= 3 {
                        u128::from_le_bytes(number(b, o + 48)?)
                    } else {
                        0
                    },
                    length: u64::from_le_bytes(number(b, o + 8)?),
                    hash: number(b, o + 16)?,
                });
                prior = id;
            }
            (segments, None)
        };
        if segments
            .iter()
            .any(|e| !(4096..=value_limit.segment_bytes() as u64).contains(&e.length))
        {
            return Err(Error::Corrupt("segment exceeds declared value profile"));
        }
        let prior = segments.last().map_or(0, |entry| entry.id);
        let mut m = Self {
            transform: None,
            page_size,
            value_limit,
            format_minor,
            uuid: number(b, 16)?,
            generation: u128::from_le_bytes(number(b, 32)?),
            epoch: u64::from_le_bytes(number(b, 48)?),
            sequence: u64::from_le_bytes(number(b, 56)?),
            commit: u128::from_le_bytes(number(b, 64)?),
            parent: u128::from_le_bytes(number(b, 80)?),
            previous: u128::from_le_bytes(number(b, 128)?),
            catalog: Reference::decode(&b[96..112])?,
            next_database: u64::from_le_bytes(number(b, 176)?),
            next_segment: u64::from_le_bytes(number(b, 184)?),
            segments: segments.into(),
            index,
        };
        if m.uuid == [0; 16]
            || m.generation == 0
            || m.generation == m.previous
            || m.commit == 0
            || m.epoch == 0
            || m.next_database == 0
            || m.next_segment <= prior
        {
            return Err(Error::Corrupt("invalid root identities/counters"));
        }
        if let Some(index) = &mut m.index {
            m.segments = index.adopt_inventory(std::mem::take(&mut m.segments))?;
        }
        Ok(m)
    }
}
pub use crate::publication::{
    checkpoint_bytes, install_snapshot, publish, read_checkpoint, read_current, read_previous,
    read_roots, read_roots_checked,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publication::{crc, decode_pointer, pointer};
    #[test]
    fn injected_identity_retries_zero_and_preserves_little_endian_value() {
        let mut calls = 0;
        let id = identity_from(|bytes| {
            calls += 1;
            if calls == 2 { *bytes = 42u128.to_le_bytes(); }
            Ok(())
        }).unwrap();
        assert_eq!(id, 42);
        assert_eq!(calls, 2);
    }
    #[test]
    fn injected_identity_errors_do_not_retry_or_return_partial_bytes() {
        let mut calls = 0;
        let result = identity_from(|bytes| {
            calls += 1;
            bytes.fill(0xa7);
            Err(std::io::Error::from_raw_os_error(5))
        });
        assert!(matches!(result, Err(Error::Io(e)) if e.raw_os_error() == Some(5)));
        assert_eq!(calls, 1);
    }
    #[test]
    fn injected_identity_repeated_zero_is_bounded_and_fails_closed() {
        let mut calls = 0;
        let result = identity_from(|_| { calls += 1; Ok(()) });
        assert!(matches!(result, Err(Error::Io(_))));
        assert_eq!(calls, 16);
    }
    #[test]
    fn checksum_known_vector_and_pointer_corruption() {
        assert_eq!(crc(b"123456789"), 0xe3069283);
        let mut p = pointer(7, [8; 32]);
        assert_eq!(decode_pointer(&p).unwrap(), (7, [8; 32]));
        p[20] ^= 1;
        assert!(decode_pointer(&p).is_err());
    }

    #[test]
    fn format_versions_round_trip_and_reject_undeclared_capabilities() {
        for (minor, features) in [(1, 1u64), (2, 3u64), (3, 7u64)] {
            let mut manifest = Manifest::empty().unwrap();
            manifest.format_minor = minor;
            let bytes = manifest.encode().unwrap();
            assert_eq!(&bytes[120..128], &features.to_le_bytes());
            let decoded = Manifest::decode(&bytes).unwrap();
            assert_eq!(decoded.format_minor, minor);
            assert_eq!(decoded.encode().unwrap(), bytes);
            let mut corrupt = bytes;
            corrupt[120..128].copy_from_slice(&15u64.to_le_bytes());
            assert!(Manifest::decode(&corrupt).is_err());
        }
        let mut manifest = Manifest::empty().unwrap();
        manifest.format_minor = 5;
        assert!(manifest.encode().is_err());
    }

    #[test]
    fn installing_snapshot_preserves_identity_and_rejects_reinitialization() {
        let directory = std::env::temp_dir().join(format!(
            "ulmdb-install-{}-{}",
            std::process::id(),
            new_identity().unwrap()
        ));
        create_layout(&directory).unwrap();
        let mut manifest = Manifest::empty().unwrap();
        manifest.sequence = 9;
        manifest.parent = 7;
        manifest.previous = 8;
        let mut invalid = manifest.clone();
        invalid.next_segment = 0;
        assert!(install_snapshot(&directory, &invalid).is_err());
        assert!(!directory.join("CURRENT").exists());
        install_snapshot(&directory, &manifest).unwrap();
        let current = read_current(&directory).unwrap();
        assert_eq!(current.sequence, 9);
        assert_eq!(current.uuid, manifest.uuid);
        assert_eq!(current.generation, manifest.generation);
        assert_eq!(current.commit, manifest.commit);
        let before = fs::read(directory.join("CURRENT")).unwrap();
        assert!(install_snapshot(&directory, &Manifest::empty().unwrap()).is_err());
        assert_eq!(fs::read(directory.join("CURRENT")).unwrap(), before);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn high_id_inventories_round_trip_and_reject_oversized_windows() {
        let mut manifest = Manifest::empty().unwrap();
        manifest.next_segment = u64::MAX;
        manifest.segments = (vec![SegmentEntry {
            cohort: 0,
            id: u64::MAX - 1,
            length: 4096,
            hash: [5; 32],
        }])
        .into();
        let bytes = manifest.encode().unwrap();
        assert_eq!(
            Manifest::decode(&bytes).unwrap().segments[0].id,
            u64::MAX - 1
        );
        for invalid in [0, u64::MAX] {
            let mut corrupt = bytes.clone();
            corrupt[192..200].copy_from_slice(&invalid.to_le_bytes());
            assert!(Manifest::decode(&corrupt).is_err());
        }
        let first = (1u64 << 40) + 255;
        let last = first + 1_048_575;
        manifest.segments = (vec![
            SegmentEntry {
                cohort: 0,
                id: first,
                length: 4096,
                hash: [1; 32],
            },
            SegmentEntry {
                cohort: 0,
                id: last,
                length: 4096,
                hash: [2; 32],
            },
        ])
        .into();
        assert!(Manifest::decode(&manifest.encode().unwrap()).is_ok());
        let mut corrupt = manifest.encode().unwrap();
        let too_far = ((last >> 8) + 1) * 256;
        corrupt[256..264].copy_from_slice(&too_far.to_le_bytes());
        assert!(Manifest::decode(&corrupt).is_err());
    }
    #[test]
    fn cohort_origin_round_trips_only_in_declared_format() {
        let mut manifest = Manifest::empty().unwrap();
        manifest.next_segment = 18;
        manifest.segments = (vec![SegmentEntry {
            id: 17,
            length: 8192,
            hash: [5; 32],
            cohort: (1u128 << 120) + 7,
        }])
        .into();
        assert!(manifest.encode().is_err());
        manifest.format_minor = 3;
        let bytes = manifest.encode().unwrap();
        assert_eq!(&bytes[240..256], &manifest.segments[0].cohort.to_le_bytes());
        let decoded = Manifest::decode(&bytes).unwrap();
        assert_eq!(decoded.segments[0].cohort, manifest.segments[0].cohort);
        assert_eq!(decoded.encode().unwrap(), bytes);
    }
    #[test]
    fn borrowed_metadata_origins_reuse_cache_and_owned_results_stay_independent() {
        let legacy = Manifest::empty().unwrap();
        let other = Manifest::empty().unwrap();
        assert!(std::ptr::eq(
            legacy.metadata_origins_ref(),
            other.metadata_origins_ref()
        ));
        let mut indexed = Manifest::empty().unwrap();
        indexed.segments = (vec![SegmentEntry {
            id: 1,
            length: 4096,
            hash: [7; 32],
            cohort: 0,
        }])
        .into();
        indexed.next_segment = 2;
        indexed.prepare_indexed(None).unwrap();
        assert!(indexed.index.as_ref().unwrap().origins.get().is_none());
        let borrowed = indexed.metadata_origins_ref();
        assert_eq!(
            borrowed,
            &std::collections::BTreeSet::from([indexed.generation])
        );
        assert!(std::ptr::eq(borrowed, indexed.metadata_origins_ref()));
        let mut owned = indexed.metadata_origins();
        owned.clear();
        assert!(owned.is_empty());
        assert!(borrowed.contains(&indexed.generation));
        let unprepared = indexed.clone_metadata();
        assert!(unprepared.metadata_origins_ref().is_empty());
        assert!(std::ptr::eq(
            unprepared.metadata_origins_ref(),
            legacy.metadata_origins_ref()
        ));
    }
}

#[cfg(test)]
mod page_geometry_tests {
    use super::*;
    #[test]
    fn geometry_roundtrip_default_wire_and_nondefault_authentication() {
        let initial = Manifest::empty().unwrap();
        let old = initial.encode().unwrap();
        assert_eq!(&old[144..148], &[0; 4]);
        assert_eq!(
            u64::from_le_bytes(old[120..128].try_into().unwrap()) & 16,
            0
        );
        for bytes in [256, 512, 8192, 65536] {
            let mut m = initial.clone_metadata();
            m.page_size = ulmdb_core::PageSize::new(bytes).unwrap();
            let encoded = m.encode().unwrap();
            let decoded = Manifest::decode(&encoded).unwrap();
            assert_eq!(decoded.page_size, m.page_size);
            let mut broken = encoded.clone();
            broken[144] ^= 1;
            assert!(Manifest::decode(&broken).is_err());
        }
    }
}

#[cfg(test)]
mod value_limit_tests {
    use super::*;
    #[test]
    fn default_bytes_stay_legacy_and_extended_profile_is_authenticated() {
        let mut m = Manifest::empty().unwrap();
        let old = m.encode().unwrap();
        assert_eq!(&old[148..152], &[0; 4]);
        assert_eq!(
            u64::from_le_bytes(old[120..128].try_into().unwrap()) & 32,
            0
        );
        for minor in [2, 3, 4] {
            m.format_minor = minor;
            m.value_limit = ulmdb_core::ValueLimit::EXTENDED;
            if minor == 4 {
                m.prepare_indexed(None).unwrap();
            }
            let wire = m.encode().unwrap();
            let decoded = Manifest::decode(&wire).unwrap();
            assert_eq!(decoded.value_limit, ulmdb_core::ValueLimit::EXTENDED);
            let mut bad = wire;
            bad[148] ^= 1;
            assert!(Manifest::decode(&bad).is_err());
        }
    }
}

#[cfg(test)]
mod jumbo_profile_tests {
    use super::*;
    #[test]
    fn jumbo_inventory_requires_exact_authenticated_profile() {
        for minor in [2, 3, 4] {
            let mut manifest = Manifest::empty().unwrap();
            manifest.format_minor = minor;
            manifest.value_limit = ulmdb_core::ValueLimit::LARGE;
            manifest.segments = vec![SegmentEntry {
                id: 1,
                length: 300 << 20,
                hash: [7; 32],
                cohort: if minor >= 3 { manifest.generation } else { 0 },
            }]
            .into();
            manifest.next_segment = 2;
            if minor == 4 {
                manifest.prepare_indexed(None).unwrap();
            }
            let bytes = manifest.encode().unwrap();
            assert_eq!(
                Manifest::decode(&bytes).unwrap().value_limit,
                ulmdb_core::ValueLimit::LARGE
            );
            let mut bad = bytes.clone();
            bad[148..152]
                .copy_from_slice(&(ulmdb_core::ValueLimit::EXTENDED.get() as u32).to_le_bytes());
            assert!(Manifest::decode(&bad).is_err());
            bad[148..152].copy_from_slice(&123u32.to_le_bytes());
            assert!(Manifest::decode(&bad).is_err());
            manifest.value_limit = ulmdb_core::ValueLimit::DEFAULT;
            assert!(Manifest::decode(&manifest.encode().unwrap()).is_err());
        }
    }
}

#[cfg(test)]
mod maximum_profile_tests {
    use super::*;
    #[test]
    fn maximum_wire_width_is_exact_and_512_profile_stays_closed() {
        for minor in [2, 3, 4] {
            let mut m = Manifest::empty().unwrap();
            m.format_minor = minor;
            m.value_limit = ulmdb_core::ValueLimit::MAXIMUM;
            m.segments = vec![SegmentEntry {
                id: 1,
                length: 5u64 << 30,
                hash: [7; 32],
                cohort: if minor >= 3 { m.generation } else { 0 },
            }]
            .into();
            m.next_segment = 2;
            if minor == 4 {
                m.prepare_indexed(None).unwrap();
            }
            let bytes = m.encode().unwrap();
            assert_eq!(&bytes[148..152], &u32::MAX.to_le_bytes());
            assert_eq!(Manifest::decode(&bytes).is_ok(), usize::BITS > 32);
            let mut old = bytes.clone();
            old[148..152]
                .copy_from_slice(&(ulmdb_core::ValueLimit::LARGE.get() as u32).to_le_bytes());
            assert!(Manifest::decode(&old).is_err());
            old[148..152].copy_from_slice(&(u32::MAX - 1).to_le_bytes());
            assert!(Manifest::decode(&old).is_err());
        }
    }
}
