use super::*;
use crate::range::pack::io::{ReadAt, read_exact_at};
const HEADER_MAGIC: &[u8; 8] = b"ULMDBF07";
const FOOTER_MAGIC: &[u8; 8] = b"ULMDBE07";
const HEADER_DOMAIN: &[u8] = b"ULMDB/chunks/frame-header/v7\0";
const FOOTER_DOMAIN: &[u8] = b"ULMDB/chunks/frame-footer/v7\0";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Frame {
    pub receipt: Receipt,
    pub parent: Option<Receipt>,
    pub payload_hash: [u8; 32],
}
impl Frame {
    pub(crate) fn length(self) -> u64 {
        self.receipt.end() - self.receipt.start()
    }
    fn validate(self) -> Result<()> {
        match self.parent {
            None if self.receipt.ordinal() == 0
                && self.receipt.start() == 0
                && self.receipt.payload_length() == 0
                && self.payload_hash == <[u8; 32]>::from(Sha256::digest([])) => {}
            Some(parent)
                if parent.binding().stream == self.receipt.binding().stream
                    && parent.ordinal().checked_add(1) == Some(self.receipt.ordinal())
                    && geometry::next_frame(parent.start(), parent.end(), self.length())
                        == Some(self.receipt.start()) =>
            {
                if parent.binding().index == self.receipt.binding().index
                    && parent.chunk_hash() != self.receipt.chunk_hash()
                {
                    return Err(Error::Corrupt("v7 same-chunk binding changed"));
                }
                if parent.binding().index != self.receipt.binding().index
                    && parent.binding().index.checked_add(1) != Some(self.receipt.binding().index)
                {
                    return Err(Error::Corrupt("v7 frame skipped chunk"));
                }
            }
            _ => return Err(Error::Corrupt("v7 frame parent chain")),
        }
        Ok(())
    }
}
fn fields(bytes: &mut Block, frame: &Frame) {
    for (at, value) in [
        (32, frame.receipt.start()),
        (40, frame.receipt.end()),
        (48, frame.receipt.ordinal()),
        (56, frame.receipt.payload_length()),
    ] {
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes[64..96].copy_from_slice(&frame.receipt.chunk_hash());
    if let Some(parent) = frame.parent {
        receipt_write(bytes, 96, parent);
    }
    bytes[192..224].copy_from_slice(&frame.payload_hash);
}
pub(crate) fn encode_frame_header(
    stream: StreamId,
    start: u64,
    ordinal: u64,
    payload_length: u64,
    parent: Option<Receipt>,
    chunk_hash: [u8; 32],
    payload_hash: [u8; 32],
) -> Result<(Frame, Block)> {
    let length = geometry::frame_length(payload_length).ok_or(Error::LimitExceeded)?;
    let end = geometry::frame_end(start, length).ok_or(Error::Corrupt("v7 frame extent"))?;
    let mut frame = Frame {
        receipt: Receipt::checked(
            stream,
            start,
            end,
            ordinal,
            payload_length,
            [0; 32],
            chunk_hash,
        )?,
        parent,
        payload_hash,
    };
    frame.validate()?;
    let mut bytes = [0; BLOCK];
    base(&mut bytes, HEADER_MAGIC, stream);
    fields(&mut bytes, &frame);
    let hash = seal(HEADER_DOMAIN, &mut bytes);
    frame.receipt = Receipt::checked(
        stream,
        start,
        end,
        ordinal,
        payload_length,
        hash,
        chunk_hash,
    )?;
    Ok((frame, bytes))
}
pub(crate) fn decode_frame_header(bytes: &[u8], stream: StreamId) -> Result<Frame> {
    let bytes = block(bytes, HEADER_DOMAIN)?;
    check_base(bytes, HEADER_MAGIC, Some(stream))?;
    if bytes[224..CHECKSUM].iter().any(|b| *b != 0) {
        return Err(Error::Corrupt("v7 frame reserved"));
    }
    let parent = if bytes[96..192] == [0; 96] {
        None
    } else {
        Some(receipt_read(bytes, 96, stream)?)
    };
    let frame = Frame {
        receipt: Receipt::checked(
            stream,
            u64::from_le_bytes(number(bytes, 32)?),
            u64::from_le_bytes(number(bytes, 40)?),
            u64::from_le_bytes(number(bytes, 48)?),
            u64::from_le_bytes(number(bytes, 56)?),
            number(bytes, CHECKSUM)?,
            number(bytes, 64)?,
        )?,
        parent,
        payload_hash: number(bytes, 192)?,
    };
    frame.validate()?;
    Ok(frame)
}
pub(crate) fn decode_frame_header_bound(bytes: &[u8], binding: ChunkBinding) -> Result<Frame> {
    let frame = decode_frame_header(bytes, binding.stream)?;
    if frame.receipt.binding() != binding {
        return Err(Error::Corrupt("v7 frame active chunk binding"));
    }
    Ok(frame)
}
pub(crate) fn encode_frame_footer(stream: StreamId, frame: &Frame) -> Block {
    let mut bytes = [0; BLOCK];
    base(&mut bytes, FOOTER_MAGIC, stream);
    fields(&mut bytes, frame);
    bytes[224..256].copy_from_slice(&frame.receipt.hash());
    seal(FOOTER_DOMAIN, &mut bytes);
    bytes
}
pub(crate) fn decode_frame_footer(bytes: &[u8], stream: StreamId, frame: &Frame) -> Result<()> {
    let bytes = block(bytes, FOOTER_DOMAIN)?;
    check_base(bytes, FOOTER_MAGIC, Some(stream))?;
    if stream != frame.receipt.binding().stream {
        return Err(Error::Corrupt("v7 footer stream"));
    }
    frame.validate()?;
    let expected = encode_frame_footer(stream, frame);
    if bytes[..CHECKSUM] != expected[..CHECKSUM] {
        return Err(Error::Corrupt("v7 footer/header mismatch"));
    }
    Ok(())
}
pub(crate) fn validate_frame_payload<R: ReadAt + ?Sized>(reader: &R, frame: &Frame) -> Result<()> {
    validate_payload_with(frame, |offset, buffer| {
        read_exact_at(reader, buffer, offset)?;
        Ok(())
    })
}
pub(crate) fn validate_payload_with(
    frame: &Frame,
    mut read: impl FnMut(u64, &mut [u8]) -> Result<()>,
) -> Result<()> {
    frame.validate()?;
    let mut buffer = [0; 131072];
    let mut hash = Sha256::new();
    let mut at = frame.receipt.start() + BLOCK as u64;
    let mut remaining = frame.receipt.payload_length();
    while remaining != 0 {
        let length = remaining.min(buffer.len() as u64) as usize;
        read(at, &mut buffer[..length])?;
        hash.update(&buffer[..length]);
        at += length as u64;
        remaining -= length as u64;
    }
    if <[u8; 32]>::from(hash.finalize()) != frame.payload_hash {
        return Err(Error::Corrupt("v7 payload hash"));
    }
    let footer = frame.receipt.end() - BLOCK as u64;
    while at < footer {
        let length = (footer - at).min(buffer.len() as u64) as usize;
        read(at, &mut buffer[..length])?;
        if buffer[..length].iter().any(|b| *b != 0) {
            return Err(Error::Corrupt("v7 nonzero frame padding"));
        }
        at += length as u64;
    }
    Ok(())
}
