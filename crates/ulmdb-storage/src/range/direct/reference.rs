//! Format6 references are absolute physical identities, never format4 origins.
use super::super::pack;
use crate::{Error, Result};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PhysicalRef {
    pub file: [u8; 16],
    pub offset: u64,
    pub length: u64,
    pub hash: [u8; 32],
}
impl PhysicalRef {
    pub fn encode(self) -> [u8; 64] {
        let mut b = [0; 64];
        b[..16].copy_from_slice(&self.file);
        b[16..24].copy_from_slice(&self.offset.to_le_bytes());
        b[24..32].copy_from_slice(&self.length.to_le_bytes());
        b[32..].copy_from_slice(&self.hash);
        b
    }
    pub fn decode(b: &[u8]) -> Result<Option<Self>> {
        if b.len() != 64 {
            return Err(Error::Corrupt("physical locator reference length"));
        }
        if b == [0; 64] {
            return Ok(None);
        }
        let r = Self {
            file: b[..16].try_into().unwrap(),
            offset: u64::from_le_bytes(b[16..24].try_into().unwrap()),
            length: u64::from_le_bytes(b[24..32].try_into().unwrap()),
            hash: b[32..64].try_into().unwrap(),
        };
        if r.file == [0; 16] || r.length == 0 {
            return Err(Error::Corrupt("partial null physical locator"));
        }
        Ok(Some(r))
    }
    pub fn end(self) -> Result<u64> {
        self.offset
            .checked_add(self.length)
            .ok_or(Error::Corrupt("physical locator extent overflow"))
    }
    pub fn checked(self, file: [u8; 16], limit: u64, node: bool) -> Result<()> {
        if self.file != file
            || self.offset < pack::DATA_START
            || self.end()? > limit
            || limit > pack::MAX_FILE_BYTES
            || !limit.is_multiple_of(pack::ALIGNMENT)
        {
            return Err(Error::Corrupt("physical locator identity/sealed extent"));
        }
        if node {
            if !self.offset.is_multiple_of(32) || ![96, 160].contains(&self.length) {
                return Err(Error::Corrupt("physical node shape"));
            }
        } else if !self.offset.is_multiple_of(4096)
            || !(4096..=pack::MAX_PAYLOAD).contains(&self.length)
        {
            return Err(Error::Corrupt("physical segment shape"));
        }
        Ok(())
    }
}
