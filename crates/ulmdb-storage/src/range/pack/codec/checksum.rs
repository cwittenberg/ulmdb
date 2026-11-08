use crate::range::pack::{BLOCK, Error, Result};
use sha2::{Digest, Sha256};
pub(super) const CHECKSUM: usize = BLOCK - 32;
pub(super) fn digest(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(bytes);
    hash.finalize().into()
}
pub(super) fn seal(domain: &[u8], bytes: &mut [u8; BLOCK]) -> [u8; 32] {
    let hash = digest(domain, &bytes[..CHECKSUM]);
    bytes[CHECKSUM..].copy_from_slice(&hash);
    hash
}
pub(super) fn verify(domain: &[u8], bytes: &[u8; BLOCK]) -> Result<()> {
    if digest(domain, &bytes[..CHECKSUM]) != bytes[CHECKSUM..] {
        return Err(Error::Corrupt("packed block checksum"));
    }
    Ok(())
}
