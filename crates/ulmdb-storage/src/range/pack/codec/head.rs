use super::*;
const MAGIC: &[u8; 8] = b"ULMDBPH1";
const DOMAIN: &[u8] = b"ULMDB/packed/head/v1\0";
pub(crate) fn encode_head(file_id: [u8; 16], receipt: Receipt) -> Block {
    let mut b = [0; BLOCK];
    base(&mut b, MAGIC, file_id);
    receipt_write(&mut b, 32, receipt);
    checksum::seal(DOMAIN, &mut b);
    b
}
pub(crate) fn decode_head(bytes: &Block, file_id: [u8; 16]) -> Result<Receipt> {
    checksum::verify(DOMAIN, bytes)?;
    check_base(bytes, MAGIC, Some(file_id))?;
    if bytes[88..CHECKSUM].iter().any(|v| *v != 0) {
        return Err(Error::Corrupt("packed head reserved"));
    }
    receipt_read(bytes, 32)
}
