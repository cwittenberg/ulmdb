//! Read-only archived prefixes. Their old physical headers are NEVER fresh
//! image-owner identities or mapping lengths; only checked real file geometry is used.
use super::{CheckpointDescriptor, CheckpointLimits, verify_artifact};
use crate::hybrid::provenance::{self, ChunkProvenance};
use crate::{Error, ReadStore, Reference, Result};
use memmap2::{Mmap, MmapOptions};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::{collections::BTreeMap, fs::OpenOptions, io::Read, path::Path, sync::Arc};
pub struct ArchivedChunk {
    bytes: Mmap,
    table: Arc<ChunkProvenance>,
}
impl ArchivedChunk {
    pub fn table(&self) -> &Arc<ChunkProvenance> {
        &self.table
    }
    pub fn read(&self, offset: usize, length: usize) -> Result<&[u8]> {
        let end = offset
            .checked_add(length)
            .ok_or(Error::Corrupt("checkpoint archive extent overflow"))?;
        self.bytes
            .get(offset..end)
            .ok_or(Error::Corrupt("checkpoint archive extent outside prefix"))
    }
}
pub struct CheckpointArchive {
    descriptor: CheckpointDescriptor,
    chunks: BTreeMap<u64, ArchivedChunk>,
}
impl CheckpointArchive {
    pub fn open(directory: &Path, limits: CheckpointLimits) -> Result<Self> {
        let descriptor = verify_artifact(directory, limits)?;
        let mut chunks = BTreeMap::new();
        for c in &descriptor.chunks {
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS)
                .open(directory.join(format!("chunk-{:016x}.image", c.identity.chunk)))?;
            let length = usize::try_from(c.end).map_err(|_| Error::LimitExceeded)?;
            if length == 0
                || length > isize::MAX as usize
                || !file.metadata()?.is_file()
                || file.metadata()?.len() != c.end
            {
                return Err(Error::Corrupt("checkpoint archive mapping geometry"));
            }
            #[allow(unsafe_code)]
            // SAFETY: immutable engine-owned readonly file, positive checked
            // actual prefix length <=isize::MAX; no capacity/header inference.
            let bytes = unsafe { MmapOptions::new().len(length).map(&file)? };
            let mut h = Sha256::new();
            h.update(b"ULMDB/hybrid/checkpoint-image/v1\0");
            h.update(&bytes);
            if file.metadata()?.len() != c.end || h.finalize().as_slice() != c.image_digest {
                return Err(Error::Corrupt("checkpoint archive mapped digest/length"));
            }
            let mut origins = OpenOptions::new()
                .read(true)
                .custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS)
                .open(directory.join(format!("chunk-{:016x}.origins", c.identity.chunk)))?;
            if !origins.metadata()?.is_file()
                || origins.metadata()?.len() != c.provenance_bytes
                || c.provenance_bytes > limits.max_provenance_bytes
            {
                return Err(Error::Corrupt("checkpoint archive origins size"));
            }
            let length = usize::try_from(c.provenance_bytes).map_err(|_| Error::LimitExceeded)?;
            let mut encoded = Vec::new();
            encoded
                .try_reserve_exact(length)
                .map_err(|_| Error::LimitExceeded)?;
            encoded.resize(length, 0);
            origins.read_exact(&mut encoded)?;
            let table = provenance::decode(&encoded)?;
            if table.identity() != c.identity
                || table.end() != c.end
                || table.bounds() != c.bounds
                || table.len() as u64 != c.extent_count
                || encoded[encoded.len() - 32..] != c.provenance_digest
            {
                return Err(Error::Corrupt("checkpoint archive origin binding"));
            }
            chunks.insert(c.identity.chunk, ArchivedChunk { bytes, table });
        }
        Ok(Self { descriptor, chunks })
    }
    pub fn descriptor(&self) -> &CheckpointDescriptor {
        &self.descriptor
    }
    pub fn chunks(&self) -> impl ExactSizeIterator<Item = (&u64, &ArchivedChunk)> {
        self.chunks.iter()
    }
}
impl ReadStore for CheckpointArchive {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.offset < 4096 {
            return Err(Error::Corrupt(
                "checkpoint reference targets archived header",
            ));
        }
        self.chunks
            .get(&at.segment)
            .ok_or(Error::Corrupt("checkpoint archived chunk absent"))?
            .read(
                usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?,
                length,
            )
    }
}
