//! Authenticated, quota-owned decoded backing for explicit fixed placement.
//! Decode one frame at a time into an anonymous sealed file; no plaintext path
//! or full-object Vec is created. The backing owns its original decoded charge.
use super::*;
use std::os::fd::{AsRawFd, FromRawFd};

pub(crate) struct FixedDecodedBacking {
    file: Arc<File>,
    length: u64,
    hash: [u8; 32],
    _lease: DecodedLease,
}
impl crate::fixed_address::DecodedBacking for FixedDecodedBacking {
    fn file(&self) -> Arc<File> {
        self.file.clone()
    }
}
impl FixedDecodedBacking {
    pub(crate) fn file(&self) -> Arc<File> {
        self.file.clone()
    }
    #[allow(unsafe_code)]
    pub(crate) fn map(&self) -> Result<Mmap> {
        // SAFETY: this anonymous file has WRITE/GROW/SHRINK/SEAL seals. No mutable
        // map was ever issued, and its owner keeps the quota/context alive.
        Ok(unsafe {
            memmap2::MmapOptions::new()
                .len(self.length as usize)
                .map(&self.file)?
        })
    }
}
impl DecodedLease {
    pub(crate) fn context(&self) -> Arc<TransformContext> {
        self.context.clone()
    }
}
#[allow(unsafe_code)]
fn anonymous_file(length: u64) -> Result<File> {
    let fd = unsafe {
        libc::memfd_create(
            c"ulmdb-fixed-decoded".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    file.set_len(length)?;
    Ok(file)
}
impl TransformContext {
    /// Always authenticate the actual encoded source before returning cached
    /// bytes. Reuse saves allocation; it never bypasses checksum/header checks.
    pub(crate) fn fixed_backing(
        self: &Arc<Self>,
        file: &File,
        kind: ObjectKind,
        id: u128,
        length: u64,
        hash: [u8; 32],
        cached: Option<Arc<FixedDecodedBacking>>,
    ) -> Result<(Arc<FixedDecodedBacking>, Option<DecodedLease>)> {
        if self.encrypted() {
            return Err(Error::InvalidInput("encrypted fixed placement unsupported"));
        }
        if length > isize::MAX as u64
            || !(4096..=ulmdb_core::ValueLimit::MAXIMUM.segment_limit()).contains(&length)
        {
            return Err(Error::LimitExceeded);
        }
        if cached
            .as_ref()
            .is_some_and(|b| b.length != length || b.hash != hash)
        {
            return Err(Error::Corrupt("fixed decoded identity"));
        }
        let lease = self.reserve_cache_mapping(length)?;
        let header = self.checked_object_header(file, 0, kind, id, length)?;
        let target = if cached.is_none() {
            Some(anonymous_file(length)?)
        } else {
            None
        };
        let mut frame = [0u8; PAGE];
        let mut scratch = FrameScratch::new(self);
        let mut digest = Sha256::new();
        for index in 0..length.div_ceil(PAGE as u64) {
            let start = index * PAGE as u64;
            let count = (length - start).min(PAGE as u64) as usize;
            let bytes = &mut frame[..count];
            decode_frame(self, file, 0, &header, index, bytes, &mut scratch)?;
            digest.update(&*bytes);
            if let Some(target) = &target {
                target.write_all_at(bytes, start)?;
            }
        }
        if <[u8; 32]>::from(digest.finalize()) != hash {
            return Err(Error::Corrupt("fixed decoded segment hash"));
        }
        if let Some(cached) = cached {
            return Ok((cached, Some(lease)));
        }
        let target = target.unwrap();
        #[allow(unsafe_code)]
        let sealed = unsafe {
            libc::fcntl(
                target.as_raw_fd(),
                libc::F_ADD_SEALS,
                libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL,
            )
        };
        if sealed < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok((
            Arc::new(FixedDecodedBacking {
                file: Arc::new(target),
                length,
                hash,
                _lease: lease,
            }),
            None,
        ))
    }
}
