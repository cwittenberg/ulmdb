//! Explicit cross-pack anchor. It is not an ordinary business publication.
use super::*;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Anchor {
    pub source: [u8; 256],
    pub source_global: [u8; 32],
    pub commit: u128,
    pub parent: u128,
    pub next_database: u64,
}
impl Anchor {
    pub fn sequence(self) -> u64 {
        u64::from_le_bytes(self.source[176..184].try_into().unwrap())
    }
    pub fn file(self) -> [u8; 16] {
        self.source[16..32].try_into().unwrap()
    }
    pub fn validate(self) -> Result<()> {
        if &self.source[..8] != b"ULMDBRP1"
            || self.source[8..10] != 1u16.to_le_bytes()
            || self.source[10..12] != [0; 2]
            || self.source[12..16] != 256u32.to_le_bytes()
            || self.file() == [0; 16]
            || self.source[32..48] == [0; 16]
            || self.source_global == [0; 32]
            || self.commit == 0
            || self.next_database == 0
            || <[u8; 32]>::from(Sha256::digest(&self.source[..224])) != self.source[224..]
        {
            return Err(Error::Corrupt("range rotation anchor"));
        }
        Ok(())
    }
    pub fn encode(self) -> [u8; 384] {
        let mut b = [0; 384];
        b[..8].copy_from_slice(b"ULMDBRA1");
        b[8..264].copy_from_slice(&self.source);
        b[264..296].copy_from_slice(&self.source_global);
        b[296..312].copy_from_slice(&self.commit.to_le_bytes());
        b[312..328].copy_from_slice(&self.parent.to_le_bytes());
        b[328..336].copy_from_slice(&self.next_database.to_le_bytes());
        b
    }
    pub fn decode(b: &[u8]) -> Result<Self> {
        if b.len() != 384 || &b[..8] != b"ULMDBRA1" || b[336..].iter().any(|b| *b != 0) {
            return Err(Error::Corrupt("range anchor encoding"));
        }
        let a = Self {
            source: b[8..264].try_into().unwrap(),
            source_global: b[264..296].try_into().unwrap(),
            commit: u128::from_le_bytes(b[296..312].try_into().unwrap()),
            parent: u128::from_le_bytes(b[312..328].try_into().unwrap()),
            next_database: u64::from_le_bytes(b[328..336].try_into().unwrap()),
        };
        a.validate()?;
        Ok(a)
    }
    pub fn matches(self, m: &Manifest) -> bool {
        self.source[32..48] == m.uuid
            && self.source[184..192] == m.epoch.to_le_bytes()
            && self.sequence() == m.sequence
            && self.commit == m.commit
            && self.parent == m.parent
            && self.next_database == m.next_database
    }
}
