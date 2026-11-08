//! Dense v1 hybrid record family; distinct from every existing storage format.
mod decode;
mod encode;
mod fields;
use crate::hybrid::redo::{Record, WalReceipt};
pub use decode::decode;
pub use encode::encode;
pub(crate) use encode::{encode_checked, encode_checked_into};
use sha2::{Digest, Sha256};
pub const HEADER_BYTES: usize = 256;
pub const FOOTER_BYTES: usize = 64;
pub const MAX_RECORD_BYTES: usize = 134_217_728;
pub(crate) const MAGIC: &[u8; 8] = b"ULMDBHR1";
pub(crate) const FOOTER_MAGIC: &[u8; 8] = b"ULMDBHF1";
pub(crate) const DOMAIN: &[u8] = b"ULMDB/hybrid/physical-redo/v1\0";
pub struct Encoded {
    pub bytes: Vec<u8>,
    pub digest: [u8; 32],
}
pub struct Decoded {
    pub record: Record,
    pub receipt: WalReceipt,
}
pub(crate) fn digest(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(bytes);
    h.finalize().into()
}
