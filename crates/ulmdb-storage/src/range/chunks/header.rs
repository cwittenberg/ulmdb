//! Explicit new v7 chunk envelope. No format5/6 bytes are reinterpreted.
use super::geometry::{self, DATA_BYTES, FrameDescriptor, HEADER_BYTES};
use crate::{Error, Result};
use sha2::{Digest, Sha256};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamId([u8; 16]);
impl StreamId {
    pub fn new(bytes: [u8; 16]) -> Result<Self> {
        if bytes == [0; 16] {
            Err(Error::InvalidInput("zero chunk stream ID"))
        } else {
            Ok(Self(bytes))
        }
    }
    pub fn bytes(self) -> [u8; 16] {
        self.0
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Continuation {
    pub prior_header_hash: [u8; 32],
    pub parent: FrameDescriptor,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkHeader {
    stream: StreamId,
    index: u64,
    base: u64,
    continuation: Option<Continuation>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChunkBinding {
    pub stream: StreamId,
    pub index: u64,
    pub header_hash: [u8; 32],
}
const DOMAIN: &[u8] = b"ULMDB/chunk-envelope/v7\0";
fn hash(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(bytes);
    h.finalize().into()
}
impl ChunkHeader {
    pub fn new(stream: StreamId, index: u64, continuation: Option<Continuation>) -> Result<Self> {
        let base = index.checked_mul(DATA_BYTES).ok_or(Error::LimitExceeded)?;
        base.checked_add(DATA_BYTES).ok_or(Error::LimitExceeded)?;
        if (index == 0) != continuation.is_none() {
            return Err(Error::Corrupt("chunk continuation presence"));
        }
        if let Some(c) = continuation {
            if geometry::locate(c.parent.start()).0.checked_add(1) != Some(index)
                || c.parent.end() > base
                || c.parent.end() <= base - DATA_BYTES
            {
                return Err(Error::Corrupt("chunk continuation parent index"));
            }
        }
        Ok(Self {
            stream,
            index,
            base,
            continuation,
        })
    }
    pub fn stream(&self) -> StreamId {
        self.stream
    }
    pub fn index(&self) -> u64 {
        self.index
    }
    pub fn base(&self) -> u64 {
        self.base
    }
    pub fn continuation(&self) -> Option<Continuation> {
        self.continuation
    }
    pub fn encode(&self) -> [u8; HEADER_BYTES as usize] {
        let mut b = [0; HEADER_BYTES as usize];
        b[..8].copy_from_slice(b"ULMDBCH7");
        b[8..10].copy_from_slice(&7u16.to_le_bytes());
        b[12..16].copy_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
        b[16..32].copy_from_slice(&self.stream.0);
        b[32..40].copy_from_slice(&self.index.to_le_bytes());
        b[40..48].copy_from_slice(&self.base.to_le_bytes());
        b[48..56].copy_from_slice(&DATA_BYTES.to_le_bytes());
        if let Some(c) = self.continuation {
            b[56..64].copy_from_slice(&1u64.to_le_bytes());
            b[64..72].copy_from_slice(&c.parent.start().to_le_bytes());
            b[72..80].copy_from_slice(&c.parent.end().to_le_bytes());
            b[80..88].copy_from_slice(&c.parent.ordinal().to_le_bytes());
            b[88..120].copy_from_slice(&c.parent.hash());
            b[120..152].copy_from_slice(&c.prior_header_hash);
        }
        let end = b.len() - 32;
        let digest = hash(&b[..end]);
        b[end..].copy_from_slice(&digest);
        b
    }
    pub fn decode(b: &[u8]) -> Result<Self> {
        if b.len() != HEADER_BYTES as usize
            || &b[..8] != b"ULMDBCH7"
            || b[8..10] != 7u16.to_le_bytes()
            || b[10..12] != [0; 2]
            || b[12..16] != (HEADER_BYTES as u32).to_le_bytes()
        {
            return Err(Error::Corrupt("chunk envelope header/version"));
        }
        let end = b.len() - 32;
        if hash(&b[..end]) != b[end..] || b[152..end].iter().any(|byte| *byte != 0) {
            return Err(Error::Corrupt("chunk envelope checksum/reserved"));
        }
        let u64_at = |at| u64::from_le_bytes(b[at..at + 8].try_into().unwrap());
        if u64_at(48) != DATA_BYTES {
            return Err(Error::Corrupt("chunk span geometry"));
        }
        let stream = StreamId::new(b[16..32].try_into().unwrap())
            .map_err(|_| Error::Corrupt("chunk zero stream"))?;
        let continuation = match u64_at(56) {
            0 if b[64..152].iter().all(|x| *x == 0) => None,
            1 => Some(Continuation {
                parent: FrameDescriptor::new(
                    u64_at(64),
                    u64_at(72),
                    u64_at(80),
                    b[88..120].try_into().unwrap(),
                )?,
                prior_header_hash: b[120..152].try_into().unwrap(),
            }),
            _ => return Err(Error::Corrupt("chunk continuation flag/reserved")),
        };
        let header = Self::new(stream, u64_at(32), continuation)?;
        if header.base != u64_at(40) {
            return Err(Error::Corrupt("chunk logical base"));
        }
        Ok(header)
    }
    pub fn binding(&self) -> ChunkBinding {
        let bytes = self.encode();
        ChunkBinding {
            stream: self.stream,
            index: self.index,
            header_hash: bytes[bytes.len() - 32..].try_into().unwrap(),
        }
    }
    pub fn filename(&self) -> String {
        filename(self.stream, self.index)
    }
}
pub fn filename(stream: StreamId, index: u64) -> String {
    let mut id = String::with_capacity(32);
    use std::fmt::Write;
    for byte in stream.0 {
        write!(&mut id, "{byte:02x}").unwrap();
    }
    format!("{id}-{index:016x}.chunk")
}
/// Compare a verified predecessor frame and exact old physical EOF with the
/// next envelope. This checks continuation metadata; it does not validate the
/// frame payload/hash itself or mint a selected-snapshot capability.
pub fn validate_continuation(
    previous: &ChunkHeader,
    parent: FrameDescriptor,
    physical_eof: u64,
    next: &ChunkHeader,
    new_start: u64,
    new_length: u64,
) -> Result<()> {
    let expected = Continuation {
        prior_header_hash: previous.binding().header_hash,
        parent,
    };
    let used = parent
        .end()
        .checked_sub(previous.base)
        .ok_or(Error::Corrupt("chunk parent below base"))?;
    if next.stream != previous.stream
        || previous.index.checked_add(1) != Some(next.index)
        || next.continuation != Some(expected)
        || new_start != next.base
        || geometry::next_frame(parent.start(), parent.end(), new_length) != Some(new_start)
        || geometry::locate(parent.start()).0 != previous.index
        || used > DATA_BYTES
        || physical_eof != HEADER_BYTES.checked_add(used).ok_or(Error::LimitExceeded)?
    {
        return Err(Error::Corrupt("chunk continuation/header/EOF mismatch"));
    }
    Ok(())
}
