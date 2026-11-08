//! Crate-private v7 metadata codecs. A decoded receipt is not selected authority.
//! No v5/v6 block, domain or selector is accepted here.
mod control;
mod frame;
use super::{
    geometry::{self, FrameDescriptor},
    header::{ChunkBinding, StreamId},
};
use crate::{Error, Result};
pub(crate) use control::{Global, decode_global, decode_head, encode_global, encode_head};
pub(crate) use frame::{
    Frame, decode_frame_footer, decode_frame_header, decode_frame_header_bound,
    encode_frame_footer, encode_frame_header, validate_frame_payload, validate_payload_with,
};
use sha2::{Digest, Sha256};
pub(crate) const CONTROL_FILE: &str = "RANGE7.head";
pub(crate) const BLOCK: usize = 4096;
pub(crate) const CONTROL_LEN: u64 = 196608;
pub(crate) const HEAD_OFFSETS: [u64; 2] = [65536, 131072];
pub(crate) type Block = [u8; BLOCK];
const CHECKSUM: usize = BLOCK - 32;
const RECEIPT_BYTES: usize = 96;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Receipt {
    descriptor: FrameDescriptor,
    payload_length: u64,
    binding: ChunkBinding,
}
impl Receipt {
    fn checked(
        stream: StreamId,
        start: u64,
        end: u64,
        ordinal: u64,
        payload_length: u64,
        hash: [u8; 32],
        chunk_hash: [u8; 32],
    ) -> Result<Self> {
        let descriptor = FrameDescriptor::new(start, end, ordinal, hash)?;
        let length =
            geometry::frame_length(payload_length).ok_or(Error::Corrupt("v7 payload capacity"))?;
        let index = geometry::locate(start).0;
        if geometry::frame_end(start, length) != Some(end)
            || index
                .checked_mul(geometry::DATA_BYTES)
                .and_then(|base| base.checked_add(geometry::DATA_BYTES))
                .is_none()
            || (ordinal == 0 && payload_length != 0)
        {
            return Err(Error::Corrupt("v7 receipt geometry"));
        }
        Ok(Self {
            descriptor,
            payload_length,
            binding: ChunkBinding {
                stream,
                index,
                header_hash: chunk_hash,
            },
        })
    }
    pub(crate) fn descriptor(self) -> FrameDescriptor {
        self.descriptor
    }
    pub(crate) fn frame_descriptor(self) -> FrameDescriptor {
        self.descriptor
    }
    pub(crate) fn start(self) -> u64 {
        self.descriptor.start()
    }
    pub(crate) fn end(self) -> u64 {
        self.descriptor.end()
    }
    pub(crate) fn ordinal(self) -> u64 {
        self.descriptor.ordinal()
    }
    pub(crate) fn hash(self) -> [u8; 32] {
        self.descriptor.hash()
    }
    pub(crate) fn payload_length(self) -> u64 {
        self.payload_length
    }
    pub(crate) fn chunk_hash(self) -> [u8; 32] {
        self.binding.header_hash
    }
    pub(crate) fn binding(self) -> ChunkBinding {
        self.binding
    }
}
fn number<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N]> {
    bytes
        .get(
            offset
                ..offset
                    .checked_add(N)
                    .ok_or(Error::Corrupt("v7 field overflow"))?,
        )
        .ok_or(Error::Corrupt("short v7 field"))?
        .try_into()
        .map_err(|_| Error::Corrupt("v7 field width"))
}
fn digest(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(bytes);
    hash.finalize().into()
}
fn seal(domain: &[u8], bytes: &mut Block) -> [u8; 32] {
    let hash = digest(domain, &bytes[..CHECKSUM]);
    bytes[CHECKSUM..].copy_from_slice(&hash);
    hash
}
fn block<'a>(bytes: &'a [u8], domain: &[u8]) -> Result<&'a Block> {
    let bytes: &Block = bytes
        .try_into()
        .map_err(|_| Error::Corrupt("v7 block size"))?;
    if digest(domain, &bytes[..CHECKSUM]) != bytes[CHECKSUM..] {
        return Err(Error::Corrupt("v7 block checksum"));
    }
    Ok(bytes)
}
fn base(bytes: &mut Block, magic: &[u8; 8], stream: StreamId) {
    bytes[..8].copy_from_slice(magic);
    bytes[8..10].copy_from_slice(&7u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&(BLOCK as u16).to_le_bytes());
    bytes[16..32].copy_from_slice(&stream.bytes());
}
fn check_base(bytes: &Block, magic: &[u8; 8], expected: Option<StreamId>) -> Result<StreamId> {
    let stream = StreamId::new(number(bytes, 16)?).map_err(|_| Error::Corrupt("v7 zero stream"))?;
    if &bytes[..8] != magic
        || bytes[8..10] != 7u16.to_le_bytes()
        || bytes[10..12] != (BLOCK as u16).to_le_bytes()
        || bytes[12..16] != [0; 4]
        || expected.is_some_and(|id| id != stream)
    {
        return Err(Error::Corrupt("v7 block type/version/stream"));
    }
    Ok(stream)
}
fn receipt_write(bytes: &mut [u8], at: usize, receipt: Receipt) {
    for (offset, value) in [
        (0, receipt.start()),
        (8, receipt.end()),
        (16, receipt.ordinal()),
        (24, receipt.payload_length()),
    ] {
        bytes[at + offset..at + offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes[at + 32..at + 64].copy_from_slice(&receipt.hash());
    bytes[at + 64..at + 96].copy_from_slice(&receipt.chunk_hash());
}
fn receipt_read(bytes: &[u8], at: usize, stream: StreamId) -> Result<Receipt> {
    Receipt::checked(
        stream,
        u64::from_le_bytes(number(bytes, at)?),
        u64::from_le_bytes(number(bytes, at + 8)?),
        u64::from_le_bytes(number(bytes, at + 16)?),
        u64::from_le_bytes(number(bytes, at + 24)?),
        number(bytes, at + 32)?,
        number(bytes, at + 64)?,
    )
}
#[cfg(test)]
mod tests;
