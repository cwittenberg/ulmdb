//! Permanent protocol gate, never an alternative checkpoint/floor authority.
use crate::{Error, Result};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::{fs::OpenOptions, io::Read, path::Path};
use ulmdb_storage::hybrid::retirement::selection::Compound;
pub(super) const NAME: &str = "HYBRID.required";
const DOMAIN: &[u8] = b"ULMDB/hybrid/required-floor/v2\0";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::hybrid) struct Required {
    uuid: [u8; 16],
    epoch: u64,
    header: [u8; 32],
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(bytes);
    h.finalize().into()
}
pub(super) fn encode(c: Compound) -> [u8; 128] {
    let mut b = [0; 128];
    b[..8].copy_from_slice(b"ULMDBHR2");
    b[8..12].copy_from_slice(&2u32.to_le_bytes());
    b[12..16].copy_from_slice(&128u32.to_le_bytes());
    let s = c.selector();
    b[16..32].copy_from_slice(&s.uuid);
    b[32..40].copy_from_slice(&s.epoch.to_le_bytes());
    b[40..48].copy_from_slice(&2u64.to_le_bytes());
    b[48..80].copy_from_slice(&c.header_digest());
    let digest = hash(&b[..96]);
    b[96..].copy_from_slice(&digest);
    b
}
fn decode(b: &[u8; 128]) -> Result<Required> {
    let r = Required {
        uuid: b[16..32].try_into().unwrap(),
        epoch: u64::from_le_bytes(b[32..40].try_into().unwrap()),
        header: b[48..80].try_into().unwrap(),
    };
    if &b[..8] != b"ULMDBHR2"
        || b[8..12] != 2u32.to_le_bytes()
        || b[12..16] != 128u32.to_le_bytes()
        || b[40..48] != 2u64.to_le_bytes()
        || b[80..96] != [0; 16]
        || hash(&b[..96]) != b[96..]
        || r.uuid == [0; 16]
        || r.epoch == 0
        || r.header == [0; 32]
    {
        return Err(Error::Corrupt("required-floor marker header/hash"));
    }
    Ok(r)
}
impl Required {
    pub fn check(self, c: Compound) -> Result<()> {
        let s = c.selector();
        if self.uuid != s.uuid || self.epoch != s.epoch || self.header != c.header_digest() {
            return Err(Error::Corrupt("required-floor realm/header mismatch"));
        }
        Ok(())
    }
}
pub(in crate::hybrid) fn read(engine: &Path) -> Result<Option<Required>> {
    let mut f = match OpenOptions::new()
        .read(true)
        .custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS)
        .open(engine.join(NAME))
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !f.metadata()?.is_file() || f.metadata()?.len() != 128 {
        return Err(Error::Corrupt("required-floor marker type/length"));
    }
    let mut b = [0; 128];
    f.read_exact(&mut b)?;
    Ok(Some(decode(&b)?))
}
