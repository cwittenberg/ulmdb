//! Explicit native catalog metadata profile, independent of tree page geometry.
use crate::storage::{Error, Result};
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct CatalogCapacity(bool);
impl CatalogCapacity {
    pub const DEFAULT: Self = Self(false);
    pub const EXTENDED: Self = Self(true);
    pub const fn get(self) -> usize {
        if self.0 { 65536 } else { 4096 }
    }
    pub fn new(capacity: usize) -> Result<Self> {
        match capacity {
            4096 => Ok(Self::DEFAULT),
            65536 => Ok(Self::EXTENDED),
            _ => Err(Error::InvalidInput("unsupported catalog capacity profile")),
        }
    }
    /// A reserved empty catalog key contains this exact versioned marker.
    /// Absence represents the old default; explicitly encoding default is
    /// noncanonical. Older catalog decoders reject the forbidden empty name.
    pub fn marker(self) -> Option<[u8; 24]> {
        if self == Self::DEFAULT {
            return None;
        }
        let mut b = [0; 24];
        b[..8].copy_from_slice(b"ULMDBCPL");
        b[8..10].copy_from_slice(&1u16.to_le_bytes());
        b[10..12].copy_from_slice(&24u16.to_le_bytes());
        b[12..16].copy_from_slice(&65536u32.to_le_bytes());
        Some(b)
    }
    pub fn decode_marker(bytes: &[u8]) -> Result<Self> {
        if Some(bytes) != Self::EXTENDED.marker().as_ref().map(|b| b.as_slice()) {
            return Err(Error::Corrupt("invalid catalog capacity marker"));
        }
        Ok(Self::EXTENDED)
    }
    pub const fn view_limit(self) -> usize {
        self.get() * 2 + 2
    }
}
