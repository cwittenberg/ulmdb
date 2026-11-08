use super::*;
use crate::range::pack::io::read_exact_at;
use sha2::{Digest, Sha256};
use std::fs::File;
const HEADER_MAGIC: &[u8; 8] = b"ULMDBPF1";
const FOOTER_MAGIC: &[u8; 8] = b"ULMDBPE1";
const HEADER_DOMAIN: &[u8] = b"ULMDB/packed/frame-header/v1\0";
const FOOTER_DOMAIN: &[u8] = b"ULMDB/packed/frame-footer/v1\0";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Frame {
    pub receipt: Receipt,
    pub parent: Option<Receipt>,
    pub payload_hash: [u8; 32],
    pub frame_length: u64,
}
fn fields(bytes: &mut Block, frame: &Frame) {
    bytes[32..40].copy_from_slice(&frame.receipt.offset.to_le_bytes());
    bytes[40..48].copy_from_slice(&frame.receipt.ordinal.to_le_bytes());
    bytes[48..56].copy_from_slice(&frame.receipt.payload_length.to_le_bytes());
    bytes[56..64].copy_from_slice(&frame.frame_length.to_le_bytes());
    if let Some(parent) = frame.parent {
        receipt_write(bytes, 64, parent);
    }
    bytes[120..152].copy_from_slice(&frame.payload_hash);
}
fn check_frame(frame: &Frame) -> Result<()> {
    let end = receipt_end(frame.receipt)?;
    if end - frame.receipt.offset != frame.frame_length {
        return Err(Error::Corrupt("packed frame length"));
    }
    match frame.parent {
        None if frame.receipt.ordinal == 0
            && frame.receipt.payload_length == 0
            && frame.receipt.offset == crate::range::pack::DATA_START => {}
        Some(parent)
            if parent.ordinal.checked_add(1) == Some(frame.receipt.ordinal)
                && receipt_end(parent)? == frame.receipt.offset => {}
        _ => return Err(Error::Corrupt("packed frame parent chain")),
    }
    Ok(())
}
pub(crate) fn encode_frame_header(
    file_id: [u8; 16],
    offset: u64,
    ordinal: u64,
    payload_length: u64,
    parent: Option<Receipt>,
    payload_hash: [u8; 32],
) -> Result<(Frame, Block)> {
    let mut frame = Frame {
        receipt: Receipt {
            offset,
            ordinal,
            payload_length,
            hash: [0; 32],
        },
        parent,
        payload_hash,
        frame_length: crate::range::pack::frame_length(payload_length)?,
    };
    check_frame(&frame)?;
    let mut b = [0; BLOCK];
    base(&mut b, HEADER_MAGIC, file_id);
    fields(&mut b, &frame);
    frame.receipt.hash = checksum::seal(HEADER_DOMAIN, &mut b);
    Ok((frame, b))
}
pub(crate) fn decode_frame_header(bytes: &Block, file_id: [u8; 16]) -> Result<Frame> {
    checksum::verify(HEADER_DOMAIN, bytes)?;
    check_base(bytes, HEADER_MAGIC, Some(file_id))?;
    if bytes[152..CHECKSUM].iter().any(|v| *v != 0) {
        return Err(Error::Corrupt("packed frame header reserved"));
    }
    let parent = if bytes[64..120] == [0; 56] {
        None
    } else {
        Some(receipt_read(bytes, 64)?)
    };
    let frame = Frame {
        receipt: Receipt {
            offset: u64::from_le_bytes(number(bytes, 32)?),
            ordinal: u64::from_le_bytes(number(bytes, 40)?),
            payload_length: u64::from_le_bytes(number(bytes, 48)?),
            hash: number(bytes, CHECKSUM)?,
        },
        parent,
        payload_hash: number(bytes, 120)?,
        frame_length: u64::from_le_bytes(number(bytes, 56)?),
    };
    check_frame(&frame)?;
    Ok(frame)
}
pub(crate) fn encode_frame_footer(file_id: [u8; 16], frame: &Frame) -> Block {
    let mut b = [0; BLOCK];
    base(&mut b, FOOTER_MAGIC, file_id);
    fields(&mut b, frame);
    b[152..184].copy_from_slice(&frame.receipt.hash);
    checksum::seal(FOOTER_DOMAIN, &mut b);
    b
}
pub(crate) fn decode_frame_footer(bytes: &Block, file_id: [u8; 16], frame: &Frame) -> Result<()> {
    checksum::verify(FOOTER_DOMAIN, bytes)?;
    check_base(bytes, FOOTER_MAGIC, Some(file_id))?;
    let mut expected = [0; BLOCK];
    base(&mut expected, FOOTER_MAGIC, file_id);
    fields(&mut expected, frame);
    expected[152..184].copy_from_slice(&frame.receipt.hash);
    if bytes[..CHECKSUM] != expected[..CHECKSUM] {
        return Err(Error::Corrupt("packed footer/header mismatch"));
    }
    Ok(())
}
pub(crate) fn validate_frame_payload(file: &File, frame: &Frame) -> Result<()> {
    check_frame(frame)?;
    let mut buffer = [0; 131072];
    let mut hash = Sha256::new();
    let mut offset = frame.receipt.offset + BLOCK as u64;
    let mut remaining = frame.receipt.payload_length;
    while remaining != 0 {
        let n = remaining.min(buffer.len() as u64) as usize;
        read_exact_at(file, &mut buffer[..n], offset)?;
        hash.update(&buffer[..n]);
        offset += n as u64;
        remaining -= n as u64;
    }
    if <[u8; 32]>::from(hash.finalize()) != frame.payload_hash {
        return Err(Error::Corrupt("packed payload checksum"));
    }
    let footer = frame.receipt.offset + frame.frame_length - BLOCK as u64;
    while offset < footer {
        let n = (footer - offset).min(buffer.len() as u64) as usize;
        read_exact_at(file, &mut buffer[..n], offset)?;
        if buffer[..n].iter().any(|v| *v != 0) {
            return Err(Error::Corrupt("packed padding is not zero"));
        }
        offset += n as u64;
    }
    Ok(())
}
