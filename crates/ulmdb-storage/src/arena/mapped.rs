//! Genuine writer-private file mappings. No published immutable inode is ever
//! mapped writable; all mutation/undo/reserve borrows target this unique file.
use crate::{Error, Result};
use memmap2::{MmapMut, MmapOptions};
use std::{
    fs::{self, File},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappedFlush {
    Synchronous,
    Asynchronous,
    None,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MappedAllocation {
    pub address: usize,
    pub used: usize,
    pub capacity: usize,
    #[cfg(not(windows))]
    pub file_inode: u64,
    #[cfg(windows)]
    pub file_inode: u128,
    pub file_device: u64,
    pub flush: MappedFlush,
}
enum WriterMapping {
    Ordinary(MmapMut),
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    Fixed(crate::fixed_address::WritableByteLease),
}
impl Deref for WriterMapping {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Ordinary(m) => m,
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            Self::Fixed(m) => m.bytes().expect("live fixed writer"),
        }
    }
}
impl DerefMut for WriterMapping {
    fn deref_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Ordinary(m) => m,
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            Self::Fixed(m) => m.bytes_mut().expect("live fixed writer"),
        }
    }
}
impl WriterMapping {
    fn flush_range(&self, offset: usize, length: usize) -> std::io::Result<()> {
        match self {
            Self::Ordinary(m) => m.flush_range(offset, length),
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            Self::Fixed(m) => {
                if offset != 0 {
                    return Err(std::io::Error::other("fixed flush offset"));
                }
                m.flush_range(length, false)
            }
        }
    }
    fn flush_async_range(&self, offset: usize, length: usize) -> std::io::Result<()> {
        match self {
            Self::Ordinary(m) => m.flush_async_range(offset, length),
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            Self::Fixed(m) => {
                if offset != 0 {
                    return Err(std::io::Error::other("fixed flush offset"));
                }
                m.flush_range(length, true)
            }
        }
    }
}
pub(super) struct MappedBytes {
    map: Option<WriterMapping>,
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    fixed: Option<(
        std::sync::Arc<crate::fixed_map::FixedMap>,
        usize,
        Option<crate::external::ExternalSynchronization>,
    )>,
    file: File,
    path: PathBuf,
    length: usize,
    capacity: usize,
    // Monotonic across truncate/savepoint rollback: a previously exposed suffix
    // can contain private writes and must never be trusted as fresh zero-fill.
    exposed_end: usize,
    no_mem_init: bool,
    flush: MappedFlush,
}
pub(super) enum ArenaBytes {
    Heap(Vec<u8>),
    File(MappedBytes),
}
impl ArenaBytes {
    pub fn heap() -> Self {
        Self::Heap(vec![0; 4096])
    }
    pub fn mapped(directory: &Path, capacity: usize, flush: MappedFlush) -> Result<Self> {
        if !(4096..=ulmdb_core::ValueLimit::platform_maximum().segment_bytes()).contains(&capacity)
            || !capacity.is_multiple_of(4096)
        {
            return Err(Error::InvalidInput(
                "mapped arena capacity must be4096-aligned and <=8GiB on64-bit",
            ));
        }
        let (path, initial) =
            crate::temporary::create(&directory.join("transactions"), "writemap")?;
        let original = ulmdb_platform::file_identity(&initial)?;
        if !original.is_file() || original.links != 1 {
            return Err(Error::Corrupt("mapped private file kind/links"));
        }
        drop(initial);
        let opened = (|| {
            let mut options = fs::OpenOptions::new();
            options.read(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS);
            }
            let file = options.open(&path)?;
            let opened = ulmdb_platform::file_identity(&file)?;
            if !opened.is_file() || opened.links != 1 || !opened.same_object(&original) {
                return Err(Error::Corrupt("mapped private object replaced"));
            }
            ulmdb_platform::initialize_zeroed_file(&file, capacity as u64)?;
            // SAFETY: the unique engine-owned staging inode is never published
            // or concurrently changed. Its extent is established before mmap;
            // capacity remains fixed until the mapping is consumed/unmapped.
            #[allow(unsafe_code)]
            let map = unsafe { MmapOptions::new().len(capacity).map_mut(&file)? };
            Ok(Self::File(MappedBytes {
                map: Some(WriterMapping::Ordinary(map)),
                #[cfg(any(target_os = "linux", target_os = "macos", windows))]
                fixed: None,
                file,
                path: path.clone(),
                length: 4096,
                capacity,
                exposed_end: 4096,
                no_mem_init: false,
                flush,
            }))
        })();
        if opened.is_err() {
            let _ = fs::remove_file(path);
        }
        opened
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub fn bind_fixed_writer(
        &mut self,
        fixed: std::sync::Arc<crate::fixed_map::FixedMap>,
        external: Option<crate::external::ExternalSynchronization>,
    ) -> Result<()> {
        let Self::File(m) = self else {
            return Err(Error::InvalidInput("fixed writer requires mapped arena"));
        };
        if m.length != 4096 {
            return Err(Error::InvalidInput("fixed writer must be pristine"));
        }
        // No source borrow exists: constructor has not returned this Arena.
        drop(m.map.take());
        // SAFETY: this unique staging inode is unpublished. All mutable/read
        // source borrows are tied to the consuming Arena; no external mapping
        // or truncation is allowed until its unique lease has been dropped.
        #[allow(unsafe_code)]
        let lease = unsafe { fixed.map_writer(&m.file, m.capacity)? };
        m.capacity = lease.len();
        m.fixed = Some((fixed, lease.region_offset(), external));
        m.map = Some(WriterMapping::Fixed(lease));
        Ok(())
    }
    pub fn register_fixed_entry(
        &self,
        entry: Option<&crate::SegmentEntry>,
        policy: crate::Durability,
    ) -> Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let (Self::File(m), Some(entry)) = (self, entry) {
            if let Some((fixed, offset, external)) = &m.fixed {
                fixed.register_writer_entry(entry, *offset, policy, external.clone())?;
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let _ = entry;
        Ok(())
    }
    pub fn mapping(&self) -> Result<Option<MappedAllocation>> {
        match self {
            Self::Heap(_) => Ok(None),
            Self::File(m) => {
                let meta = ulmdb_platform::file_identity(&m.file)?;
                Ok(Some(MappedAllocation {
                    address: m.map.as_ref().unwrap().as_ptr() as usize,
                    used: m.length,
                    capacity: m.capacity,
                    #[cfg(not(windows))]
                    file_inode: u64::try_from(meta.file_id).map_err(|_| Error::LimitExceeded)?,
                    #[cfg(windows)]
                    file_inode: meta.file_id,
                    file_device: meta.volume,
                    flush: m.flush,
                }))
            }
        }
    }
    pub fn set_no_mem_init(&mut self, enabled: bool) {
        if let Self::File(m) = self {
            m.no_mem_init = enabled;
        }
    }
    pub fn set_flush(&mut self, flush: MappedFlush) {
        if let Self::File(m) = self {
            m.flush = flush;
        }
    }
    pub fn effective_policy(&self, policy: crate::Durability) -> crate::Durability {
        match self {
            Self::File(m) if m.flush != MappedFlush::Synchronous => crate::Durability::NoSync,
            _ => policy,
        }
    }
    /// Schedule the actual finalized encoded inode, not its plaintext staging
    /// map. The regular sealer has authenticated/closed the immutable extent.
    pub fn flush_encoded_if_async(&self, path: &Path) -> Result<()> {
        if !matches!(self,Self::File(m)if m.flush==MappedFlush::Asynchronous) {
            return Ok(());
        }
        let mode = crate::read_mode::ReadMode::Sync;
        let file = mode.open(path)?;
        let length = file.metadata()?.len();
        if length == 0 || length > crate::MAX_COHORT_BYTES {
            return Err(Error::Corrupt("encoded async mapped extent"));
        }
        // SAFETY: this is the engine's complete, sealed immutable output. No
        // mutable mapping of this encoded inode exists. Sync mode checks its
        // regular inode/name before and after the bounded read-only mapping.
        #[allow(unsafe_code)]
        let map = unsafe {
            MmapOptions::new()
                .len(usize::try_from(length).map_err(|_| Error::LimitExceeded)?)
                .map(&file)?
        };
        crate::fault_point!("mapped.encoded.before_msync");
        #[allow(unsafe_code)]
        unsafe {
            ulmdb_platform::flush_view(map.as_ptr().cast_mut(), map.len())?
        };
        crate::fault_point!("mapped.encoded.after_msync");
        mode.check(path, &file)?;
        Ok(())
    }
    pub fn is_mapped(&self) -> bool {
        matches!(self, Self::File(_))
    }
    pub fn capacity_limit(&self) -> usize {
        match self {
            Self::Heap(_) => ulmdb_core::ValueLimit::platform_maximum().segment_bytes(),
            Self::File(m) => m.capacity,
        }
    }
    pub fn try_reserve_profile(
        &mut self,
        additional: usize,
        limit: ulmdb_core::ValueLimit,
    ) -> Result<()> {
        if limit <= ulmdb_core::ValueLimit::EXTENDED {
            return self.try_reserve(additional);
        }
        match self {
            Self::Heap(v) => {
                let end = v
                    .len()
                    .checked_add(additional)
                    .ok_or(Error::LimitExceeded)?;
                if end > limit.segment_bytes() {
                    return Err(Error::LimitExceeded);
                }
                if end > v.capacity() {
                    // Fixed16MiB growth headroom avoids both geometric over-allocation
                    // and copying a jumbo extent again for every subsequent tree page.
                    let step = 16 << 20;
                    let target =
                        end.checked_add(step - 1).ok_or(Error::LimitExceeded)? / step * step;
                    v.try_reserve_exact(target.min(limit.segment_bytes()) - v.len())
                        .map_err(|_| Error::LimitExceeded)?;
                }
                Ok(())
            }
            Self::File(_) => self.try_reserve(additional),
        }
    }
    pub fn try_reserve(&mut self, additional: usize) -> Result<()> {
        match self {
            Self::Heap(v) => v.try_reserve(additional).map_err(|_| Error::LimitExceeded),
            Self::File(m) => {
                if m.length
                    .checked_add(additional)
                    .is_some_and(|n| n <= m.capacity)
                {
                    Ok(())
                } else {
                    Err(Error::LimitExceeded)
                }
            }
        }
    }
    pub fn resize(&mut self, length: usize, value: u8) -> Result<()> {
        self.try_reserve(length.saturating_sub(self.len()))?;
        match self {
            Self::Heap(v) => v.resize(length, value),
            Self::File(m) => {
                if length > m.length {
                    // New set_len-backed bytes in our unique fresh inode are
                    // already initialized zeroes. Skip only redundant stores,
                    // never initialization of reused rollback bytes or gaps.
                    let clear_end = if m.no_mem_init && value == 0 {
                        length.min(m.exposed_end)
                    } else {
                        length
                    };
                    if clear_end > m.length {
                        m.map.as_mut().unwrap()[m.length..clear_end].fill(value);
                    }
                    m.exposed_end = m.exposed_end.max(length);
                }
                m.length = length;
            }
        }
        Ok(())
    }
    pub fn extend_from_slice(&mut self, bytes: &[u8]) -> Result<()> {
        self.try_reserve(bytes.len())?;
        match self {
            Self::Heap(v) => v.extend_from_slice(bytes),
            Self::File(m) => {
                let end = m.length + bytes.len();
                m.map.as_mut().unwrap()[m.length..end].copy_from_slice(bytes);
                m.exposed_end = m.exposed_end.max(end);
                m.length = end;
            }
        }
        Ok(())
    }
    pub fn truncate(&mut self, length: usize) {
        match self {
            Self::Heap(v) => v.truncate(length),
            Self::File(m) => m.length = m.length.min(length),
        }
    }
    pub fn as_slice(&self) -> &[u8] {
        self
    }
    pub fn into_vec(self) -> Vec<u8> {
        match self {
            Self::Heap(v) => v,
            Self::File(m) => m.deref().to_vec(),
        }
    }
    #[cfg(test)]
    pub fn clone(&self) -> Vec<u8> {
        self.to_vec()
    }
    pub fn take_mapped(&mut self) -> Option<MappedBytes> {
        if self.is_mapped() {
            match std::mem::replace(self, Self::Heap(Vec::new())) {
                Self::File(m) => Some(m),
                _ => unreachable!(),
            }
        } else {
            None
        }
    }
}
impl Deref for ArenaBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Heap(v) => v,
            Self::File(m) => m,
        }
    }
}
impl DerefMut for ArenaBytes {
    fn deref_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Heap(v) => v,
            Self::File(m) => m,
        }
    }
}
impl AsRef<[u8]> for ArenaBytes {
    fn as_ref(&self) -> &[u8] {
        self
    }
}
impl Deref for MappedBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.map.as_ref().unwrap()[..self.length]
    }
}
impl DerefMut for MappedBytes {
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.map.as_mut().unwrap()[..self.length]
    }
}
impl MappedBytes {
    pub(super) fn publish_cohort(
        mut self,
        directory: &Path,
        manifest: &crate::Manifest,
        payload: usize,
        policy: crate::Durability,
    ) -> Result<[u8; 32]> {
        use sha2::{Digest, Sha256};
        use std::io::{Seek, SeekFrom, Write};
        let (descriptor, trailer) = crate::cohort::mapped_tail(manifest, payload as u64)?;
        let hash = Sha256::digest(&descriptor).into();
        let legacy = directory
            .join("commits")
            .join(format!("{:032x}.root", manifest.generation));
        crate::cohort::reject_existing_generation(&legacy)?;
        let target = crate::cohort::path(directory, manifest.generation);
        crate::cohort::reject_existing_generation(&target)?;
        // End source reserve/slice authority before changing the mapped extent.
        drop(self.map.take());
        self.file.set_len(payload as u64)?;
        self.file.seek(SeekFrom::Start(payload as u64))?;
        crate::fault_point!("mapped.cohort.before_tail");
        self.file.write_all(&descriptor)?;
        self.file.write_all(&trailer)?;
        let physical = self.file.metadata()?.len();
        // SAFETY: still our unique unpublished writable inode, now at its exact
        // final extent; no live caller holds a mutable/source mapping anymore.
        #[allow(unsafe_code)]
        let mapped = unsafe {
            MmapOptions::new()
                .len(physical as usize)
                .map_mut(&self.file)?
        };
        crate::fault_point!("mapped.before_flush");
        match self.flush {
            MappedFlush::Asynchronous => mapped.flush_async()?,
            MappedFlush::Synchronous if policy.sync_data() => mapped.flush()?,
            _ => {}
        }
        crate::fault_point!("mapped.after_flush");
        drop(mapped);
        #[cfg(not(windows))]
        {
            let mut permissions = self.file.metadata()?.permissions();
            permissions.set_readonly(true);
            self.file.set_permissions(permissions)?;
        }
        #[cfg(windows)]
        ulmdb_platform::set_private_permissions(&self.file, false)?;
        if policy.sync_data() {
            ulmdb_platform::sync_file(&self.file)?;
        }
        crate::fault_point!("mapped.before_rename");
        ulmdb_platform::rename_replace(&self.path, &target)
            .map_err(ulmdb_platform::rename_original_error)?;
        crate::fault_point!("mapped.after_rename");
        if policy.sync_data() {
            ulmdb_platform::sync_directory(&directory.join("cohorts"))?;
        }
        Ok(hash)
    }
    pub(super) fn publish(mut self, final_path: &Path, policy: crate::Durability) -> Result<()> {
        crate::fault_point!("mapped.before_flush");
        match self.flush {
            MappedFlush::Asynchronous => self
                .map
                .as_ref()
                .unwrap()
                .flush_async_range(0, self.length)?,
            MappedFlush::Synchronous if policy.sync_data() => {
                self.map.as_ref().unwrap().flush_range(0, self.length)?
            }
            _ => {}
        }
        crate::fault_point!("mapped.after_flush");
        // End every mutable mapping before shortening/sealing the backing file.
        drop(self.map.take());
        self.file.set_len(self.length as u64)?;
        #[cfg(not(windows))]
        {
            let mut permissions = self.file.metadata()?.permissions();
            permissions.set_readonly(true);
            self.file.set_permissions(permissions)?;
        }
        #[cfg(windows)]
        ulmdb_platform::set_private_permissions(&self.file, false)?;
        if policy.sync_data() {
            ulmdb_platform::sync_file(&self.file)?;
        }
        crate::fault_point!("mapped.before_rename");
        ulmdb_platform::rename_replace(&self.path, final_path)
            .map_err(ulmdb_platform::rename_original_error)?;
        crate::fault_point!("mapped.after_rename");
        if policy.sync_data() {
            ulmdb_platform::sync_directory(
                final_path
                    .parent()
                    .ok_or(Error::Corrupt("mapped segment parent"))?,
            )?;
        }
        Ok(())
    }
}
impl Drop for MappedBytes {
    fn drop(&mut self) {
        // Never unlink another inode placed at our old private name.
        if let (Ok(named), Ok(owned)) = (
            ulmdb_platform::path_identity(&self.path),
            ulmdb_platform::file_identity(&self.file),
        ) {
            if named.is_file() && owned.is_file() && named.same_object(&owned) {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}

impl std::fmt::Debug for ArenaBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_slice().fmt(f)
    }
}
impl PartialEq<ArenaBytes> for [u8] {
    fn eq(&self, other: &ArenaBytes) -> bool {
        self == other.as_slice()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod no_mem_init_tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    #[test]
    fn sparse_suffix_and_reused_gaps_follow_monotonic_exposure() {
        let directory = std::env::temp_dir().join(format!(
            "arena-no-mem-init-{}",
            crate::new_identity().unwrap()
        ));
        crate::create_layout(&directory).unwrap();
        let mut bytes = ArenaBytes::mapped(&directory, 2 << 20, MappedFlush::None).unwrap();
        bytes.set_no_mem_init(true);
        bytes.resize(1 << 20, 0).unwrap();
        let ArenaBytes::File(mapped) = &bytes else {
            unreachable!()
        };
        assert_eq!(
            mapped.file.metadata().unwrap().blocks(),
            0,
            "fresh zero pages must not be written"
        );
        assert!(bytes[4096..].iter().all(|b| *b == 0));
        bytes[8192..].fill(0xac);
        bytes.truncate(4096);
        bytes.resize(65539, 0).unwrap();
        assert!(bytes[4096..].iter().all(|b| *b == 0));
        bytes.extend_from_slice(&vec![0xbd; 65536]).unwrap();
        bytes.truncate(65539);
        bytes.resize(131075, 0).unwrap();
        assert!(bytes[65539..].iter().all(|b| *b == 0));
        bytes.truncate(4096);
        bytes.resize((1 << 20) + 17, 0x5c).unwrap();
        assert!(bytes[4096..].iter().all(|b| *b == 0x5c));
        drop(bytes);
        // Isolate extension as the sole exposure path: no later resize may
        // accidentally cover for a missing extend_from_slice watermark.
        let mut extended = ArenaBytes::mapped(&directory, 2 << 20, MappedFlush::None).unwrap();
        extended.set_no_mem_init(true);
        extended.extend_from_slice(&vec![0xde; 65536]).unwrap();
        extended.truncate(4096);
        extended.resize(4096 + 65536, 0).unwrap();
        assert!(extended[4096..].iter().all(|b| *b == 0));
        drop(extended);
        fs::remove_dir_all(directory).unwrap();
    }
}
