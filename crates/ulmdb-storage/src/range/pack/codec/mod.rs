//! Experimental packed protocol1. All checksums are domain-separated SHA256.
mod checksum;
mod frame;
mod global;
mod head;
use crate::range::pack::{BLOCK, Error, Receipt, Result};
use checksum::CHECKSUM;
pub(crate) use frame::{
    Frame, decode_frame_footer, decode_frame_header, encode_frame_footer, encode_frame_header,
    validate_frame_payload,
};
pub(crate) use global::{Global, decode_global, encode_global};
pub(crate) use head::{decode_head, encode_head};
pub(crate) type Block = [u8; BLOCK];
fn number<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N]> {
    bytes
        .get(
            offset
                ..offset
                    .checked_add(N)
                    .ok_or(Error::Corrupt("packed field overflow"))?,
        )
        .ok_or(Error::Corrupt("short packed field"))?
        .try_into()
        .map_err(|_| Error::Corrupt("packed field width"))
}
fn base(bytes: &mut Block, magic: &[u8; 8], file_id: [u8; 16]) {
    bytes[..8].copy_from_slice(magic);
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&(BLOCK as u16).to_le_bytes());
    bytes[16..32].copy_from_slice(&file_id);
}
fn check_base(bytes: &Block, magic: &[u8; 8], file_id: Option<[u8; 16]>) -> Result<[u8; 16]> {
    let id = number(bytes, 16)?;
    if &bytes[..8] != magic
        || bytes[8..10] != 1u16.to_le_bytes()
        || bytes[10..12] != (BLOCK as u16).to_le_bytes()
        || bytes[12..16] != [0; 4]
        || id == [0; 16]
        || file_id.is_some_and(|expected| expected != id)
    {
        return Err(Error::Corrupt("packed block type/version/identity"));
    }
    Ok(id)
}
fn receipt_write(bytes: &mut [u8], offset: usize, receipt: Receipt) {
    bytes[offset..offset + 8].copy_from_slice(&receipt.offset.to_le_bytes());
    bytes[offset + 8..offset + 16].copy_from_slice(&receipt.ordinal.to_le_bytes());
    bytes[offset + 16..offset + 24].copy_from_slice(&receipt.payload_length.to_le_bytes());
    bytes[offset + 24..offset + 56].copy_from_slice(&receipt.hash);
}
fn receipt_read(bytes: &[u8], offset: usize) -> Result<Receipt> {
    let receipt = Receipt {
        offset: u64::from_le_bytes(number(bytes, offset)?),
        ordinal: u64::from_le_bytes(number(bytes, offset + 8)?),
        payload_length: u64::from_le_bytes(number(bytes, offset + 16)?),
        hash: number(bytes, offset + 24)?,
    };
    receipt_end(receipt)?;
    Ok(receipt)
}
fn receipt_end(receipt: Receipt) -> Result<u64> {
    receipt.validate()?;
    receipt.frame_end()
}

#[cfg(test)]
mod tests;
