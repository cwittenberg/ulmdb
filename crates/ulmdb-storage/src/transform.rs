//! Per-owner persisted transforms. Normal mappings bypass this module entirely.
//! Callbacks and copied keys are process-local Arc owners, never wire pointers.
use crate::{Error, Result};
use memmap2::{Mmap, MmapMut};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use ulmdb_platform::FileExt;
const HEADER: usize = 128;
const PAGE: usize = 4096;
const BIND: usize = 32;
const MAGIC: &[u8; 8] = b"ULMDBTX1";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Encrypt,
    Decrypt,
}
pub trait Checksum: Send + Sync {
    fn compute(&self, source: &[u8], destination: &mut [u8], key: Option<&[u8]>) -> Result<()>;
}
pub trait Encryption: Send + Sync {
    /// IV is always16 bytes. authentication is writable MAC output on encrypt,
    /// stored MAC input on decrypt; input/output lengths are always identical.
    fn transform(
        &self,
        source: &[u8],
        destination: &mut [u8],
        key: &[u8],
        iv: &[u8],
        authentication: &mut [u8],
        direction: Direction,
    ) -> Result<()>;
}
pub struct ChecksumConfig {
    pub callback: Arc<dyn Checksum>,
    pub bytes: usize,
}
pub struct EncryptionConfig {
    pub callback: Arc<dyn Encryption>,
    pub key: Vec<u8>,
    pub authentication_bytes: usize,
}
struct Secret(Vec<u8>);
impl Drop for Secret {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // SAFETY: every pointer is an exclusive live element.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(Ordering::SeqCst);
    }
}
struct EncryptionState {
    callback: Arc<dyn Encryption>,
    key: Secret,
    authentication_bytes: usize,
}
pub struct TransformContext {
    checksum: Option<ChecksumConfig>,
    encryption: Option<EncryptionState>,
    limit: AtomicU64,
    decoded: AtomicU64,
}
impl std::fmt::Debug for TransformContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransformContext")
            .field("checksum_bytes", &self.checksum_bytes())
            .field("authentication_bytes", &self.authentication_bytes())
            .field("encrypted", &self.encryption.is_some())
            .field("decoded_byte_limit", &self.decoded_byte_limit())
            .finish()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum ObjectKind {
    Segment = 1,
    Configuration = 2,
    Descriptor = 3,
    Metadata = 4,
    ResidentCheckpoint = 5,
    ResidentWal = 6,
}
/// Mapping owns this lease after its decoded mmap field, so unmapping precedes
/// quota release and callback/module/key destruction.
pub(crate) struct DecodedLease {
    context: Arc<TransformContext>,
    bytes: u64,
}
impl Drop for DecodedLease {
    fn drop(&mut self) {
        self.context.decoded.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
impl TransformContext {
    pub fn new(
        checksum: Option<ChecksumConfig>,
        encryption: Option<EncryptionConfig>,
        decoded_byte_limit: u64,
    ) -> Result<Self> {
        if checksum
            .as_ref()
            .is_some_and(|s| s.bytes == 0 || s.bytes > 256)
            || encryption
                .as_ref()
                .is_some_and(|e| e.key.len() > 1 << 20 || e.authentication_bytes > 256)
            || (checksum.is_none() && encryption.is_none())
            || decoded_byte_limit < PAGE as u64
        {
            return Err(Error::InvalidInput("transform configuration/limit"));
        }
        Ok(Self {
            checksum,
            encryption: encryption.map(|e| EncryptionState {
                callback: e.callback,
                key: Secret(e.key),
                authentication_bytes: e.authentication_bytes,
            }),
            limit: AtomicU64::new(decoded_byte_limit),
            decoded: AtomicU64::new(0),
        })
    }
    pub fn checksum_bytes(&self) -> usize {
        self.checksum.as_ref().map_or(0, |s| s.bytes)
    }
    pub fn authentication_bytes(&self) -> usize {
        self.encryption
            .as_ref()
            .map_or(0, |e| e.authentication_bytes)
    }
    pub fn encrypted(&self) -> bool {
        self.encryption.is_some()
    }
    pub fn decoded_bytes(&self) -> u64 {
        self.decoded.load(Ordering::Acquire)
    }
    pub fn decoded_byte_limit(&self) -> u64 {
        self.limit.load(Ordering::Acquire)
    }
    /// Raise an owner's decode allowance after explicit capacity growth or
    /// adoption. Existing mapping leases remain charged; this never lowers a
    /// limit under concurrent reservations or invalidates borrowed bytes.
    pub fn raise_decoded_byte_limit(&self, bytes: u64) {
        self.limit.fetch_max(bytes, Ordering::AcqRel);
    }
    fn key(&self) -> Option<&[u8]> {
        self.encryption.as_ref().map(|e| e.key.0.as_slice())
    }
    fn frame_bytes(&self) -> usize {
        BIND + PAGE + self.checksum_bytes() + self.authentication_bytes()
    }
    pub fn encoded_length(&self, length: u64) -> Result<u64> {
        length
            .div_ceil(PAGE as u64)
            .checked_mul(self.frame_bytes() as u64)
            .and_then(|n| n.checked_add(HEADER as u64))
            .ok_or(Error::LimitExceeded)
    }
    fn header(
        &self,
        kind: ObjectKind,
        id: u128,
        length: u64,
        nonce: [u8; 16],
    ) -> Result<[u8; HEADER]> {
        if length == 0 {
            return Err(Error::InvalidInput("empty transformed object"));
        }
        let mut b = [0; HEADER];
        b[..8].copy_from_slice(MAGIC);
        b[8..10].copy_from_slice(&1u16.to_le_bytes());
        b[10..12].copy_from_slice(&(kind as u16).to_le_bytes());
        b[12..16].copy_from_slice(&(PAGE as u32).to_le_bytes());
        b[16..32].copy_from_slice(&id.to_le_bytes());
        b[32..40].copy_from_slice(&length.to_le_bytes());
        b[40..42].copy_from_slice(&(self.checksum_bytes() as u16).to_le_bytes());
        b[42..44].copy_from_slice(&(self.authentication_bytes() as u16).to_le_bytes());
        b[44] = u8::from(self.encrypted());
        b[48..64].copy_from_slice(&nonce);
        b[64..72].copy_from_slice(&self.encoded_length(length)?.to_le_bytes());
        let digest: [u8; 32] = Sha256::digest(&b[..96]).into();
        b[96..].copy_from_slice(&digest);
        Ok(b)
    }
    fn iv(header: &[u8; HEADER], index: u64) -> [u8; 16] {
        let mut iv: [u8; 16] = header[48..64].try_into().unwrap();
        let first = u64::from_le_bytes(iv[..8].try_into().unwrap()) ^ index;
        iv[..8].copy_from_slice(&first.to_le_bytes());
        iv
    }
    fn binding(header: &[u8; HEADER], index: u64) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"ULMDB/transform/frame/v1\0");
        h.update(header);
        h.update(index.to_le_bytes());
        h.finalize().into()
    }
    /// Encode directly into the caller's private file. No plaintext temporary
    /// file or whole-object ciphertext staging allocation is created.
    pub fn write_encoded(
        &self,
        writer: &mut impl Write,
        kind: ObjectKind,
        id: u128,
        source: &[u8],
    ) -> Result<u64> {
        let header = self.header(
            kind,
            id,
            source.len() as u64,
            crate::new_identity()?.to_le_bytes(),
        )?;
        writer.write_all(&header)?;
        let size = BIND + PAGE + self.checksum_bytes();
        let mut plain = vec![0; size];
        let mut cipher = vec![0; size];
        let mut auth = vec![0; self.authentication_bytes()];
        for (index, page) in source.chunks(PAGE).enumerate() {
            plain.fill(0);
            auth.fill(0);
            plain[..BIND].copy_from_slice(&Self::binding(&header, index as u64));
            plain[BIND..BIND + page.len()].copy_from_slice(page);
            if let Some(sum) = &self.checksum {
                let (data, out) = plain.split_at_mut(BIND + PAGE);
                sum.callback.compute(data, out, self.key())?;
            }
            if let Some(enc) = &self.encryption {
                enc.callback
                    .transform(
                        &plain,
                        &mut cipher,
                        &enc.key.0,
                        &Self::iv(&header, index as u64),
                        &mut auth,
                        Direction::Encrypt,
                    )
                    .map_err(|_| Error::CryptoFailed)?;
                writer.write_all(&cipher)?;
            } else {
                writer.write_all(&plain)?;
            }
            writer.write_all(&auth)?;
        }
        Ok(self.encoded_length(source.len() as u64)?)
    }
    fn reserve(self: &Arc<Self>, bytes: u64) -> Result<DecodedLease> {
        self.decoded
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(bytes)
                    .filter(|n| *n <= self.decoded_byte_limit())
            })
            .map_err(|_| Error::LimitExceeded)?;
        Ok(DecodedLease {
            context: Arc::clone(self),
            bytes,
        })
    }
    pub(crate) fn decode_mapping(
        self: &Arc<Self>,
        file: &File,
        kind: ObjectKind,
        id: u128,
        length: u64,
    ) -> Result<(Mmap, DecodedLease)> {
        self.decode_mapping_at(file, 0, kind, id, length)
    }
    pub(crate) fn decode_mapping_at(
        self: &Arc<Self>,
        file: &File,
        base: u64,
        kind: ObjectKind,
        id: u128,
        length: u64,
    ) -> Result<(Mmap, DecodedLease)> {
        if length == 0
            || length > isize::MAX as u64
            || length > ulmdb_core::ValueLimit::MAXIMUM.segment_limit()
        {
            return Err(Error::LimitExceeded);
        }
        let charge = Self::mapped_charge(length)?;
        let lease = self.reserve(charge)?;
        let header = self.checked_object_header(file, base, kind, id, length)?;
        let mut mapping =
            MmapMut::map_anon(usize::try_from(length).map_err(|_| Error::LimitExceeded)?)?;
        let mut scratch = FrameScratch::new(self);
        for index in 0..length.div_ceil(PAGE as u64) {
            let start = index as usize * PAGE;
            let count = (length as usize - start).min(PAGE);
            decode_frame(
                self,
                file,
                base,
                &header,
                index,
                &mut mapping[start..start + count],
                &mut scratch,
            )?;
        }
        Ok((mapping.make_read_only()?, lease))
    }
}

/// Metadata readers can locate a cohort descriptor without a callback or key.
/// This grants no plaintext/data authority; Mapping still verifies every frame.
pub fn encoded_geometry(file: &File) -> Result<(u64, u64)> {
    let mut h = [0; HEADER];
    file.read_exact_at(&mut h, 0)?;
    let logical = u64::from_le_bytes(h[32..40].try_into().unwrap());
    let encoded = u64::from_le_bytes(h[64..72].try_into().unwrap());
    let sum = u16::from_le_bytes(h[40..42].try_into().unwrap()) as u64;
    let auth = u16::from_le_bytes(h[42..44].try_into().unwrap()) as u64;
    if &h[..8] != MAGIC
        || h[8..10] != 1u16.to_le_bytes()
        || h[10..12] != (ObjectKind::Segment as u16).to_le_bytes()
        || h[12..16] != (PAGE as u32).to_le_bytes()
        || sum > 256
        || auth > 256
        || h[44] > 1
        || h[45..48] != [0; 3]
        || h[72..96] != [0; 24]
        || h[96..] != <[u8; 32]>::from(Sha256::digest(&h[..96]))
        || !(4096..=(ulmdb_core::ValueLimit::MAXIMUM.segment_limit())).contains(&logical)
        || encoded
            != HEADER as u64
                + logical.div_ceil(PAGE as u64) * (BIND as u64 + PAGE as u64 + sum + auth)
    {
        return Err(Error::Corrupt("transformed cohort header"));
    }
    Ok((logical, encoded))
}
const CONFIGURATION: &str = "TRANSFORM";
/// Must run under the environment's creator/writer fence before CURRENT exists.
/// The marker is transformed too, so wrong callbacks/keys fail even for an empty DB.
pub fn create_configuration(
    directory: &std::path::Path,
    uuid: [u8; 16],
    context: Option<&Arc<TransformContext>>,
) -> Result<()> {
    let Some(context) = context else {
        if directory.join(CONFIGURATION).try_exists()? {
            return Err(Error::CryptoMismatch);
        }
        return Ok(());
    };
    if directory.join(CONFIGURATION).try_exists()? {
        return Err(Error::InvalidInput(
            "transform configuration already exists",
        ));
    }
    let (temporary, mut file) = crate::temporary::create(directory, "transform")?;
    let result = (|| {
        context.write_encoded(&mut file, ObjectKind::Configuration, 0, &uuid)?;
        ulmdb_platform::sync_file(&file)?;
        ulmdb_platform::rename_replace(&temporary, &directory.join(CONFIGURATION))
            .map_err(ulmdb_platform::rename_original_error)?;
        ulmdb_platform::sync_path(directory)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
pub fn check_configuration(
    directory: &std::path::Path,
    context: Option<&Arc<TransformContext>>,
) -> Result<Option<[u8; 16]>> {
    let path = directory.join(CONFIGURATION);
    let exists = path.try_exists()?;
    match (exists, context) {
        (false, None) => Ok(None),
        (true, Some(context)) => {
            let file = crate::read_mode::ReadMode::Sync.open(&path)?;
            if file.metadata()?.len() != context.encoded_length(16)? {
                return Err(Error::CryptoMismatch);
            }
            let (bytes, _lease) =
                context.decode_mapping(&file, ObjectKind::Configuration, 0, 16)?;
            let uuid = bytes[..].try_into().unwrap();
            crate::read_mode::ReadMode::Sync.check(&path, &file)?;
            Ok(Some(uuid))
        }
        _ => Err(Error::CryptoMismatch),
    }
}

pub(crate) fn envelope_geometry(file: &File, offset: u64) -> Result<(u64, u64)> {
    let mut h = [0; HEADER];
    file.read_exact_at(&mut h, offset)?;
    let logical = u64::from_le_bytes(h[32..40].try_into().unwrap());
    let encoded = u64::from_le_bytes(h[64..72].try_into().unwrap());
    let sum = u16::from_le_bytes(h[40..42].try_into().unwrap()) as u64;
    let auth = u16::from_le_bytes(h[42..44].try_into().unwrap()) as u64;
    if &h[..8] != MAGIC
        || h[8..10] != 1u16.to_le_bytes()
        || h[12..16] != (PAGE as u32).to_le_bytes()
        || sum > 256
        || auth > 256
        || h[44] > 1
        || h[45..48] != [0; 3]
        || h[72..96] != [0; 24]
        || h[96..] != <[u8; 32]>::from(Sha256::digest(&h[..96]))
        || logical == 0
        || logical > (ulmdb_core::ValueLimit::MAXIMUM.segment_limit())
        || encoded
            != HEADER as u64
                + logical.div_ceil(PAGE as u64) * (BIND as u64 + PAGE as u64 + sum + auth)
    {
        return Err(Error::Corrupt("transformed object header"));
    }
    Ok((logical, encoded))
}

impl PartialEq for TransformContext {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
impl Eq for TransformContext {}

mod chunks;
mod resident;
pub(crate) use chunks::{FrameScratch, decode_frame};
mod stream;
pub(crate) use resident::ResidentFile;
pub(crate) use stream::{Reader as StreamReader, Writer as StreamWriter};

pub(crate) use resident::read_magic as resident_magic;
impl TransformContext {
    pub(crate) fn reserve_resident_pool(self: &Arc<Self>, bytes: u64) -> Result<DecodedLease> {
        self.reserve(bytes)
    }
}

impl TransformContext {
    pub(crate) fn mapped_charge(length: u64) -> Result<u64> {
        let page = crate::residency::os::page_size()? as u64;
        length
            .checked_add(page - 1)
            .map(|n| n / page * page)
            .ok_or(Error::LimitExceeded)
    }
}

#[cfg(target_os = "linux")]
mod fixed;
#[cfg(target_os = "linux")]
pub(crate) use fixed::FixedDecodedBacking;

#[cfg(any(target_os = "macos", windows))]
mod fixed_portable;
#[cfg(any(target_os = "macos", windows))]
pub(crate) use fixed_portable::{FixedDecodedBacking, OwnedReadView, ReadOnlyDecodedObject};
