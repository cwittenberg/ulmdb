use super::*;
const GLOBAL_MAGIC: &[u8; 8] = b"ULMDBC07";
const HEAD_MAGIC: &[u8; 8] = b"ULMDBH07";
const GLOBAL_DOMAIN: &[u8] = b"ULMDB/chunks/control/v7\0";
const HEAD_DOMAIN: &[u8] = b"ULMDB/chunks/head/v7\0";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Global {
    pub stream: StreamId,
}
fn geometry_fields() -> [(usize, u64); 8] {
    [
        (32, CONTROL_LEN),
        (40, geometry::ALIGN_BYTES),
        (48, geometry::DATA_BYTES),
        (56, geometry::HEADER_BYTES),
        (64, geometry::MAX_PAYLOAD),
        (72, HEAD_OFFSETS[0]),
        (80, HEAD_OFFSETS[1]),
        (88, 8192),
    ]
}
pub(crate) fn encode_global(global: Global) -> Block {
    let mut bytes = [0; BLOCK];
    base(&mut bytes, GLOBAL_MAGIC, global.stream);
    for (at, value) in geometry_fields() {
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    seal(GLOBAL_DOMAIN, &mut bytes);
    bytes
}
pub(crate) fn decode_global(bytes: &[u8]) -> Result<Global> {
    let bytes = block(bytes, GLOBAL_DOMAIN)?;
    let stream = check_base(bytes, GLOBAL_MAGIC, None)?;
    for (at, value) in geometry_fields() {
        if bytes[at..at + 8] != value.to_le_bytes() {
            return Err(Error::Corrupt("v7 control geometry"));
        }
    }
    if bytes[96..CHECKSUM].iter().any(|b| *b != 0) {
        return Err(Error::Corrupt("v7 control reserved"));
    }
    Ok(Global { stream })
}
pub(crate) fn encode_head(stream: StreamId, receipt: Receipt) -> Block {
    let mut bytes = [0; BLOCK];
    base(&mut bytes, HEAD_MAGIC, stream);
    receipt_write(&mut bytes, 32, receipt);
    seal(HEAD_DOMAIN, &mut bytes);
    bytes
}
pub(crate) fn decode_head(bytes: &[u8], stream: StreamId) -> Result<Receipt> {
    let bytes = block(bytes, HEAD_DOMAIN)?;
    check_base(bytes, HEAD_MAGIC, Some(stream))?;
    if bytes[32 + RECEIPT_BYTES..CHECKSUM].iter().any(|b| *b != 0) {
        return Err(Error::Corrupt("v7 head reserved"));
    }
    receipt_read(bytes, 32, stream)
}
