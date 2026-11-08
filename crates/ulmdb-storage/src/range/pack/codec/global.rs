use super::*;
const MAGIC: &[u8; 8] = b"ULMDBRG5";
const DOMAIN: &[u8] = b"ULMDB/range-env/global/v5\0";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Global {
    pub file_id: [u8; 16],
    #[cfg(feature = "range-lifecycle")]
    pub anchor: Option<crate::range::rotation::Anchor>,
}
impl Global {
    pub fn initial(file_id: [u8; 16]) -> Self {
        Self {
            file_id,
            #[cfg(feature = "range-lifecycle")]
            anchor: None,
        }
    }
}
pub(crate) fn encode_global(global: &Global) -> Block {
    let mut b = [0; BLOCK];
    base(&mut b, MAGIC, global.file_id);
    for (offset, value) in [
        (32, crate::range::pack::DATA_START),
        (40, crate::range::pack::ALIGNMENT),
        (48, crate::range::pack::MAX_PAYLOAD),
        (56, crate::range::pack::HEAD_OFFSETS[0]),
        (64, crate::range::pack::HEAD_OFFSETS[1]),
    ] {
        b[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    #[cfg(feature = "range-lifecycle")]
    if let Some(anchor) = global.anchor {
        b[72..456].copy_from_slice(&anchor.encode());
    }
    checksum::seal(DOMAIN, &mut b);
    b
}
pub(crate) fn decode_global(bytes: &Block) -> Result<Global> {
    checksum::verify(DOMAIN, bytes)?;
    let file_id = check_base(bytes, MAGIC, None)?;
    for (offset, value) in [
        (32, crate::range::pack::DATA_START),
        (40, crate::range::pack::ALIGNMENT),
        (48, crate::range::pack::MAX_PAYLOAD),
        (56, crate::range::pack::HEAD_OFFSETS[0]),
        (64, crate::range::pack::HEAD_OFFSETS[1]),
    ] {
        if bytes[offset..offset + 8] != value.to_le_bytes() {
            return Err(Error::Corrupt("unsupported packed geometry"));
        }
    }
    #[cfg(feature = "range-lifecycle")]
    let anchor = if bytes[72..456].iter().any(|b| *b != 0) {
        Some(crate::range::rotation::Anchor::decode(&bytes[72..456])?)
    } else {
        None
    };
    #[cfg(feature = "range-lifecycle")]
    let reserved = 456;
    #[cfg(not(feature = "range-lifecycle"))]
    let reserved = 72;
    if bytes[reserved..CHECKSUM].iter().any(|v| *v != 0) {
        return Err(Error::Corrupt("packed global reserved"));
    }
    Ok(Global {
        file_id,
        #[cfg(feature = "range-lifecycle")]
        anchor,
    })
}
