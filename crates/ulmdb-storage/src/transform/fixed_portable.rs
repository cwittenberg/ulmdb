//! Anonymous kernel-backed checksum bytes for non-Linux fixed placement.
//! The only factory decodes/authenticates every frame before issuing a token.
//! Neither a plaintext filename nor a mutable/full-object Vec is created.
#![allow(unsafe_code)]
use super::*;
use crate::creator_owned::CreatorOwned;
use std::ops::Deref;
pub(super) type Charge = Arc<CreatorOwned<DecodedLease>>;

#[cfg(target_os = "macos")]
mod darwin;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "macos")]
use darwin as backend;
#[cfg(windows)]
use windows as backend;

/// Opaque read-only kernel capability. Arc identity denotes the same object;
/// no caller-provided integer or raw handle can manufacture another one.
pub(crate) struct ReadOnlyDecodedObject {
    // Destroy kernel resources before releasing their decoded-memory charge.
    backend: backend::Object,
    length: usize,
    creator: u32,
    _lease: Charge,
}
pub(crate) struct OwnedReadView {
    // The view unmaps before the final object/quota owner can disappear.
    view: backend::View,
    owner: Arc<ReadOnlyDecodedObject>,
}
impl OwnedReadView {
    pub(crate) fn bytes(&self) -> Result<&[u8]> {
        self.owner.check_creator()?;
        Ok(self.view.bytes())
    }
}
impl Deref for OwnedReadView {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.bytes().expect("inherited decoded view")
    }
}
impl AsRef<[u8]> for OwnedReadView {
    fn as_ref(&self) -> &[u8] {
        self
    }
}
impl std::fmt::Debug for OwnedReadView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedReadView")
            .field("length", &self.owner.len())
            .finish_non_exhaustive()
    }
}
impl ReadOnlyDecodedObject {
    pub(crate) fn len(&self) -> usize {
        self.length
    }
    /// Call before taking a destination reservation's registry mutex.
    pub(crate) fn check_creator(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited fixed decoded object"));
        }
        Ok(())
    }
    pub(crate) fn map_readonly(self: &Arc<Self>) -> Result<OwnedReadView> {
        self.check_creator()?;
        Ok(OwnedReadView {
            view: self.backend.map()?,
            owner: self.clone(),
        })
    }
    /// Replace only an already-owned destination reservation. This never
    /// releases that range before the atomic Mach mapping operation.
    #[cfg(target_os = "macos")]
    pub(crate) unsafe fn map_at(&self, address: usize, length: usize) -> Result<()> {
        self.check_creator()?;
        if length != self.length {
            return Err(Error::InvalidInput("decoded map extent"));
        }
        // SAFETY: caller owns the entire rounded destination reservation.
        unsafe { self.backend.map_at(address) }
    }
    /// Trusted internal loan for MapViewOfFile3(REPLACE_PLACEHOLDER).
    #[cfg(windows)]
    pub(crate) fn section(&self) -> Result<std::os::windows::io::BorrowedHandle<'_>> {
        self.check_creator()?;
        Ok(self.backend.section())
    }
}

pub(crate) struct FixedDecodedBacking {
    object: Arc<ReadOnlyDecodedObject>,
    length: u64,
    hash: [u8; 32],
}
impl FixedDecodedBacking {
    pub(crate) fn object(&self) -> Arc<ReadOnlyDecodedObject> {
        self.object.clone()
    }
    pub(crate) fn map(&self) -> Result<OwnedReadView> {
        self.object.map_readonly()
    }
}
impl crate::fixed_address::DecodedBacking for FixedDecodedBacking {
    fn object(&self) -> Arc<ReadOnlyDecodedObject> {
        self.object.clone()
    }
}

impl DecodedLease {
    pub(crate) fn context(&self) -> Arc<TransformContext> {
        self.context.clone()
    }
}
impl TransformContext {
    /// Cached objects never bypass authentication of the actual encoded source.
    pub(crate) fn fixed_backing(
        self: &Arc<Self>,
        file: &File,
        kind: ObjectKind,
        id: u128,
        length: u64,
        hash: [u8; 32],
        cached: Option<Arc<FixedDecodedBacking>>,
    ) -> Result<(Arc<FixedDecodedBacking>, Option<DecodedLease>)> {
        let creator = std::process::id();
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
        if let Some(backing) = &cached {
            backing.object.check_creator()?;
        }
        // A checksum callback can fork. Its child must never destroy a copied
        // quota ledger while another parent thread held that ledger's mutex.
        let mut lease = CreatorOwned::new(Some(self.reserve_cache_mapping(length)?));
        let header = self.checked_object_header(file, 0, kind, id, length)?;
        if creator != std::process::id() {
            return Err(Error::InvalidInput("inherited fixed decoded preparation"));
        }
        let charge = if cached.is_none() {
            Some(Arc::new(CreatorOwned::new(lease.take().unwrap())))
        } else {
            None
        };
        let mut target = if cached.is_none() {
            Some(backend::Writer::new(
                usize::try_from(length).map_err(|_| Error::LimitExceeded)?,
                charge.as_ref().unwrap().clone(),
            )?)
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
            if creator != std::process::id() {
                return Err(Error::InvalidInput("inherited fixed decoded preparation"));
            }
            digest.update(&*bytes);
            if let Some(target) = &mut target {
                target.write_at(start as usize, bytes)?;
            }
        }
        if <[u8; 32]>::from(digest.finalize()) != hash {
            return Err(Error::Corrupt("fixed decoded segment hash"));
        }
        if let Some(cached) = cached {
            return Ok((cached, lease.take()));
        }
        let backend = target.unwrap().seal()?;
        Ok((
            Arc::new(FixedDecodedBacking {
                object: Arc::new(ReadOnlyDecodedObject {
                    backend,
                    length: length as usize,
                    creator: std::process::id(),
                    _lease: charge.unwrap(),
                }),
                length,
                hash,
            }),
            None,
        ))
    }
}
