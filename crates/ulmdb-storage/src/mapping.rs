//! Unsafe mapping boundary: committed mapped bytes remain immutable.
//! File backends seal whole files; range appends/heads are outside mapped pages.
use crate::{Error, Result, SegmentEntry};
use memmap2::{Mmap, MmapOptions};
use std::{fs::File, path::Path};

// Keep Linux's hot mapped-byte header unchanged. Other fixed-capable platforms
// can also own a read-only Mach/section view without retaining a second decoded
// allocation or manufacturing a file handle for an anonymous kernel object.
#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) type MappingBytes = Mmap;
#[cfg(any(target_os = "macos", windows))]
pub(crate) enum MappingBytes {
    File(Mmap),
    Decoded(crate::transform::OwnedReadView),
}
#[cfg(any(target_os = "macos", windows))]
impl From<Mmap> for MappingBytes {
    fn from(value: Mmap) -> Self {
        Self::File(value)
    }
}
#[cfg(any(target_os = "macos", windows))]
impl From<crate::transform::OwnedReadView> for MappingBytes {
    fn from(value: crate::transform::OwnedReadView) -> Self {
        Self::Decoded(value)
    }
}
#[cfg(any(target_os = "macos", windows))]
impl std::ops::Deref for MappingBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::File(bytes) => bytes,
            Self::Decoded(bytes) => bytes,
        }
    }
}
#[cfg(any(target_os = "macos", windows))]
impl AsRef<[u8]> for MappingBytes {
    fn as_ref(&self) -> &[u8] {
        self
    }
}
#[cfg(target_os = "macos")]
impl MappingBytes {
    #[allow(unsafe_code)]
    pub(crate) fn lock(&self) -> std::io::Result<()> {
        // SAFETY: the variant owns the complete immutable view for this call.
        unsafe { ulmdb_platform::lock_memory(self.as_ptr(), self.len()) }
    }
    #[allow(unsafe_code)]
    pub(crate) fn unlock(&self) -> std::io::Result<()> {
        // SAFETY: the view remains mapped until after residency teardown.
        unsafe { ulmdb_platform::unlock_memory(self.as_ptr(), self.len()) }
    }
}

#[cfg(any(target_os = "macos", windows))]
fn advise_decoded(policy: crate::ReadAhead, bytes: &crate::transform::OwnedReadView) {
    #[cfg(target_os = "macos")]
    if policy == crate::ReadAhead::Random {
        // SAFETY: an owned Mach view starts at an OS-page boundary and retains
        // its complete mapping. Advice is best effort, as for ordinary mappings.
        #[allow(unsafe_code)]
        unsafe {
            libc::madvise(
                bytes.as_ptr().cast_mut().cast(),
                bytes.len(),
                libc::MADV_RANDOM,
            );
        }
    }
    #[cfg(windows)]
    let _ = (policy, bytes); // Pinned LMDB accepts and ignores this Windows hint.
}

// Keep the hot mapped-byte header before cold, 16-byte-aligned origin metadata.
#[repr(C)]
pub(crate) struct Mapping {
    pub bytes: MappingBytes,
    pub entry: SegmentEntry,
    pub(crate) read_ahead: crate::ReadAhead,
    pub(crate) range: Option<Box<crate::range::RangeIdentity>>,
    // Drop order is intentional: bytes unmaps before this mapping-owned lease
    // releases its controller charge. Never run munlock from a destructor.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fixed: Option<crate::fixed_address::ByteLease>,
    pub(crate) residency:
        crate::creator_owned::CreatorOwned<std::sync::Mutex<Option<crate::residency::LockState>>>,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fixed_backing: Option<std::sync::Arc<crate::transform::FixedDecodedBacking>>,
    decoded: Option<crate::transform::DecodedLease>,
}

#[allow(unsafe_code)]
fn immutable_map(file: &File, length: usize) -> std::io::Result<Mmap> {
    if length > isize::MAX as usize {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "mapping length exceeds Rust slice bound",
        ));
    }
    // SAFETY: engine-owned sealed files are immutable. Exclusive environment
    // ownership and read-only published files forbid writes/truncation through
    // this engine; length is prechecked against physical bounds and maps only
    // the segment prefix, never a cohort descriptor/footer. External mutation
    // of these protected files is out of scope.
    unsafe { MmapOptions::new().len(length).map(file) }
}

/// Covers all OS pages touched by memmap2's normalized unaligned slice.
fn checked_range_pages(identity: crate::range::RangeIdentity, page: u64) -> Result<(u64, u64)> {
    if page == 0 || !65536u64.is_multiple_of(page) {
        return Err(Error::InvalidInput("unsupported range OS page geometry"));
    }
    let end = identity
        .data_offset
        .checked_add(identity.length)
        .ok_or(Error::LimitExceeded)?;
    let first = identity.data_offset / page * page;
    let last = end.checked_add(page - 1).ok_or(Error::LimitExceeded)? / page * page;
    if identity.length == 0 || first < identity.bounds().0 || last > identity.bounds().1 {
        return Err(Error::Corrupt("range map exceeds immutable frame pages"));
    }
    Ok((first, last))
}

impl Mapping {
    pub(crate) fn data_bytes(&self) -> Result<&[u8]> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let Some(fixed) = &self.fixed {
            return fixed.bytes();
        }
        Ok(&self.bytes)
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub(crate) fn fixed_lease(&self) -> Option<&crate::fixed_address::ByteLease> {
        self.fixed.as_ref()
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub(crate) fn bind_fixed(
        &mut self,
        fixed: &std::sync::Arc<crate::fixed_map::FixedMap>,
        register: bool,
        policy: crate::Durability,
        external: Option<crate::external::ExternalSynchronization>,
    ) -> Result<()> {
        if self.range.is_some() {
            return Err(Error::InvalidInput(
                "fixed placement requires plain file mappings",
            ));
        }
        let file = std::sync::Arc::new(ulmdb_platform::open_readonly(&crate::segment_path(
            fixed.directory(),
            &self.entry,
        ))?);
        let backing = if let Some(decoded) = &self.decoded {
            let (backing, lease) = fixed.decoded_backing(&file, &self.entry, &decoded.context())?;
            let bytes = backing.map()?;
            #[cfg(target_os = "linux")]
            self.read_ahead.apply(&bytes);
            #[cfg(any(target_os = "macos", windows))]
            advise_decoded(self.read_ahead, &bytes);
            if bytes.as_ref() != self.bytes.as_ref() {
                return Err(Error::Corrupt("fixed decoded bytes differ"));
            }
            // Unmap the old anonymous allocation before releasing its charge.
            self.bytes = bytes.into();
            self.fixed_backing = Some(backing.clone());
            self.decoded = lease;
            Some(backing)
        } else {
            None
        };
        #[cfg(target_os = "linux")]
        let file = backing.as_ref().map_or(file, |backing| backing.file());
        #[cfg(target_os = "linux")]
        let lease = fixed.map_entry(
            &self.entry,
            file,
            register,
            self.read_ahead,
            policy,
            external,
        )?;
        #[cfg(any(target_os = "macos", windows))]
        let lease = if let Some(backing) = &backing {
            fixed.map_decoded_entry(
                &self.entry,
                backing.object(),
                register,
                self.read_ahead,
                policy,
                external,
            )?
        } else {
            fixed.map_entry(
                &self.entry,
                file,
                register,
                self.read_ahead,
                policy,
                external,
            )?
        };
        if lease.bytes()? != self.bytes.as_ref() {
            return Err(Error::Corrupt(
                "fixed mapping differs from authenticated inode",
            ));
        }
        if let Some(backing) = &backing {
            lease.retain_decoded(backing.clone())?;
        }
        self.fixed = Some(lease);
        self.fixed_backing = backing;
        Ok(())
    }
    #[cfg(test)]
    pub fn open(directory: &Path, entry: SegmentEntry) -> Result<Self> {
        Self::open_with_read_ahead(directory, entry, crate::ReadAhead::Normal)
    }

    /// Only the arena that hashed and wrote these exact bytes may use this
    /// path. Comparing the mapped file with that buffer validates every byte
    /// without calculating SHA-256 a second time during the same commit.
    #[cfg(test)]
    pub(crate) fn open_written(
        directory: &Path,
        entry: SegmentEntry,
        written: &[u8],
    ) -> Result<Self> {
        Self::open_written_with_read_ahead(directory, entry, written, crate::ReadAhead::Normal)
    }
    pub(crate) fn open_with_read_ahead(
        directory: &Path,
        entry: SegmentEntry,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        Self::open_checked(directory, entry, None, read_ahead)
    }
    pub(crate) fn open_written_with_read_ahead(
        directory: &Path,
        entry: SegmentEntry,
        written: &[u8],
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        Self::open_checked(directory, entry, Some(written), read_ahead)
    }
    pub(crate) fn open_with_transform(
        directory: &Path,
        entry: SegmentEntry,
        written: Option<&[u8]>,
        read_ahead: crate::ReadAhead,
        context: Option<&std::sync::Arc<crate::transform::TransformContext>>,
    ) -> Result<Self> {
        let Some(context) = context else {
            return Self::open_checked(directory, entry, written, read_ahead);
        };
        let path = crate::segment_path(directory, &entry);
        let file = File::open(path)?;
        Self::from_transformed_file(&file, entry, written, read_ahead, context)
    }
    pub(crate) fn from_transformed_file(
        file: &File,
        entry: SegmentEntry,
        written: Option<&[u8]>,
        read_ahead: crate::ReadAhead,
        context: &std::sync::Arc<crate::transform::TransformContext>,
    ) -> Result<Self> {
        let encoded = context.encoded_length(entry.length)?;
        let physical = file.metadata()?.len();
        if (entry.cohort == 0 && physical != encoded)
            || (entry.cohort != 0
                && (physical < encoded + 224 || physical > crate::MAX_COHORT_BYTES))
        {
            return Err(Error::Corrupt("transformed segment extent"));
        }
        let (bytes, lease) = context.decode_mapping(
            file,
            crate::transform::ObjectKind::Segment,
            entry.id as u128,
            entry.length,
        )?;
        let mut mapping =
            Self::from_bytes_with_read_ahead(bytes, entry, written, None, read_ahead)?;
        mapping.decoded = Some(lease);
        Ok(mapping)
    }
    fn open_checked(
        directory: &Path,
        entry: SegmentEntry,
        written: Option<&[u8]>,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        read_ahead.validate()?;
        if entry.id == 0 || entry.id == u64::MAX {
            return Err(Error::LimitExceeded);
        }
        let path = crate::segment_path(directory, &entry);
        let file = File::open(path)?;
        let physical = file.metadata()?.len();
        Self::map_checked(&file, entry, written, physical, read_ahead)
    }

    #[allow(unsafe_code)]
    pub(crate) fn from_range(
        file: &File,
        entry: SegmentEntry,
        identity: crate::range::RangeIdentity,
        written: Option<&[u8]>,
    ) -> Result<Self> {
        let page = crate::residency::os::page_size()? as u64;
        if entry.cohort != 0 || identity.length != entry.length {
            return Err(Error::InvalidInput("range mapping origin"));
        }
        if let crate::range::Seal::Prefix { data_hash, .. } = identity.seal
            && data_hash != entry.hash
        {
            return Err(Error::Corrupt("range physical hash identity"));
        }
        checked_range_pages(identity, page)?;
        let start = identity.data_offset;
        if file.metadata()?.len() < identity.bounds().1 {
            return Err(Error::Corrupt("range file is truncated"));
        }
        if entry.length > isize::MAX as u64 {
            return Err(Error::LimitExceeded);
        }
        let length = usize::try_from(entry.length).map_err(|_| Error::LimitExceeded)?;
        // SAFETY: only range's private prepare/load code constructs this exact
        // immutable, hashed span on its independent read-only OFD. Rounded OS
        // pages are within the sealed frame/prefix, disjoint from heads/future tails.
        // Private failed candidates are dropped before any abort truncation;
        // published frames are never changed. Mmap normalizes unaligned offsets.
        let bytes = unsafe { MmapOptions::new().offset(start).len(length).map(file)? };
        Self::from_bytes(bytes, entry, written, Some(identity))
    }
    /// Only a just-sealed cohort supplies this private handle and metadata.
    /// Header and every mapped byte retain the same validation as open_written.
    #[cfg(test)]
    pub(crate) fn from_written_file(
        file: &File,
        entry: SegmentEntry,
        written: &[u8],
        physical: u64,
    ) -> Result<Self> {
        Self::from_written_file_with_read_ahead(
            file,
            entry,
            written,
            physical,
            crate::ReadAhead::Normal,
        )
    }
    pub(crate) fn from_written_file_with_read_ahead(
        file: &File,
        entry: SegmentEntry,
        written: &[u8],
        physical: u64,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        read_ahead.validate()?;
        Self::map_checked(file, entry, Some(written), physical, read_ahead)
    }

    fn map_checked(
        file: &File,
        entry: SegmentEntry,
        written: Option<&[u8]>,
        physical: u64,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        if entry.id == 0 || entry.id == u64::MAX {
            return Err(Error::LimitExceeded);
        }
        if entry.length < 4096 || entry.length > (ulmdb_core::ValueLimit::MAXIMUM.segment_limit()) {
            return Err(Error::Corrupt("invalid segment file length"));
        }
        if entry.cohort == 0 {
            if physical != entry.length {
                return Err(Error::Corrupt("invalid segment file length"));
            }
        } else {
            let minimum = entry
                .length
                .checked_add(224)
                .ok_or(Error::Corrupt("cohort prefix extent overflow"))?;
            if physical < minimum || physical > crate::MAX_COHORT_BYTES {
                return Err(Error::Corrupt("invalid cohort physical length"));
            }
        }
        let length = usize::try_from(entry.length).map_err(|_| Error::LimitExceeded)?;
        let bytes = immutable_map(file, length)?;
        Self::from_bytes_with_read_ahead(bytes, entry, written, None, read_ahead)
    }
    fn from_bytes(
        bytes: Mmap,
        entry: SegmentEntry,
        written: Option<&[u8]>,
        range: Option<crate::range::RangeIdentity>,
    ) -> Result<Self> {
        Self::from_bytes_with_read_ahead(bytes, entry, written, range, crate::ReadAhead::Normal)
    }
    fn from_bytes_with_read_ahead(
        bytes: Mmap,
        entry: SegmentEntry,
        written: Option<&[u8]>,
        range: Option<crate::range::RangeIdentity>,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        read_ahead.validate()?;
        read_ahead.apply(&bytes);
        if entry.id == 0
            || entry.id == u64::MAX
            || !(4096..=(ulmdb_core::ValueLimit::MAXIMUM.segment_limit())).contains(&entry.length)
            || bytes.len() as u64 != entry.length
        {
            return Err(Error::Corrupt("mapped segment extent/identity"));
        }
        let features = match &bytes[10..12] {
            [1, 0] => 1u64,
            [2, 0] => 3u64,
            [3, 0]
                if (268_435_456..=1_073_741_824).contains(&entry.length)
                    && entry.length != 268_435_456 =>
            {
                7u64
            }
            [4, 0] if entry.length > 1_073_741_824 => 15u64,
            _ => return Err(Error::Corrupt("unsupported segment minor version")),
        };
        let content_matches = match written {
            Some(expected) => crate::work::equal(&bytes[..], expected)?,
            None => crate::work::digest(&bytes[..])? == entry.hash,
        };
        if !content_matches
            || &bytes[..8] != b"ULMDBSEG"
            || bytes[8..10] != 0u16.to_le_bytes()
            || bytes[12..16] != 128u32.to_le_bytes()
            || bytes[16..24] != entry.id.to_le_bytes()
            || bytes[24..28] != 4096u32.to_le_bytes()
            || bytes[28] != 1
            || bytes[32..40] != 4096u64.to_le_bytes()
            || bytes[40..48] != (entry.length - 4096).to_le_bytes()
            || bytes[48..56] != entry.length.to_le_bytes()
            || bytes[56..64] != features.to_le_bytes()
            || bytes[29..32].iter().any(|b| *b != 0)
            || bytes[64..4096].iter().any(|b| *b != 0)
        {
            return Err(Error::Corrupt("segment hash/header validation failed"));
        }
        Ok(Self {
            bytes: bytes.into(),
            entry,
            read_ahead,
            range: range.map(Box::new),
            residency: crate::creator_owned::CreatorOwned::new(std::sync::Mutex::new(None)),
            decoded: None,
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            fixed_backing: None,
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            fixed: None,
        })
    }
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Arena, SegmentTable, WriteStore, create_layout};

    #[test]
    fn range_span_accounts_for_4k_and_64k_page_prefix_and_suffix() {
        let identity = crate::range::RangeIdentity {
            file_id: [1; 16],
            seal: crate::range::Seal::Frame {
                start: 196608,
                end: 327680,
                ordinal: 1,
                hash: [2; 32],
            },
            data_offset: 204800,
            length: 65537,
        };
        assert_eq!(
            checked_range_pages(identity, 4096).unwrap(),
            (204800, 274432)
        );
        assert_eq!(
            checked_range_pages(identity, 65536).unwrap(),
            (196608, 327680)
        );
        for page in [4096, 65536] {
            let mut bad = identity;
            bad.seal = crate::range::Seal::Frame {
                start: 262144,
                end: 327680,
                ordinal: 1,
                hash: [2; 32],
            };
            assert!(checked_range_pages(bad, page).is_err());
            let mut bad = identity;
            bad.seal = crate::range::Seal::Frame {
                start: 196608,
                end: 262144,
                ordinal: 1,
                hash: [2; 32],
            };
            assert!(checked_range_pages(bad, page).is_err());
        }
        assert!(checked_range_pages(identity, 131072).is_err());
        let mut overflow = identity;
        overflow.data_offset = u64::MAX - 1;
        overflow.length = 3;
        assert!(checked_range_pages(overflow, 4096).is_err());
    }
    #[test]
    fn mapping_hot_header_layout() {
        assert_eq!(std::mem::offset_of!(Mapping, bytes), 0);
        assert!(std::mem::offset_of!(Mapping, entry) >= std::mem::size_of::<Mmap>());
    }
    #[test]
    fn newly_written_mapping_compares_all_bytes_with_hashed_private_buffer() {
        let directory = std::env::temp_dir().join(format!(
            "ulmdb-map-{}-{}",
            std::process::id(),
            crate::new_identity().unwrap()
        ));
        create_layout(&directory).unwrap();
        let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
        arena.put_payload(b"verified payload").unwrap();
        let (table, entry) = arena.seal(&directory).unwrap();
        let entry = entry.unwrap();
        let bytes = std::fs::read(
            directory
                .join("segments")
                .join(format!("{}.seg", hex(&entry.hash))),
        )
        .unwrap();
        let mapping = Mapping::open_written(&directory, entry.clone(), &bytes).unwrap();
        assert_eq!(mapping.bytes[..], bytes);
        let mut wrong = bytes.clone();
        *wrong.last_mut().unwrap() ^= 1;
        assert!(Mapping::open_written(&directory, entry.clone(), &wrong).is_err());
        assert!(
            Mapping::open_written(&directory, entry.clone(), &bytes[..bytes.len() - 1]).is_err()
        );
        assert!(Mapping::open(&directory, entry).is_ok());
        drop(mapping);
        drop(table);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[cfg(test)]
#[test]
fn immutable_mapping_refuses_larger_than_rust_slice_before_mmap() {
    let file = File::open(std::env::current_exe().unwrap()).unwrap();
    let length = (isize::MAX as usize).checked_add(1).unwrap();
    let error = immutable_map(&file, length).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(error.to_string(), "mapping length exceeds Rust slice bound");
}
