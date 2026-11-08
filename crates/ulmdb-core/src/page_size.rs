//! Persisted logical tree-page geometry. Physical mapping alignment is separate.
use crate::storage::{Error, Result};
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PageSize(u32);
impl PageSize {
    pub const DEFAULT: Self = Self(4096);
    pub const fn new(bytes: usize) -> Result<Self> {
        if bytes < 256 || bytes > 65536 || !bytes.is_power_of_two() {
            Err(Error::InvalidInput(
                "page size must be a power of two from 256 through 65536",
            ))
        } else {
            Ok(Self(bytes as u32))
        }
    }
    pub const fn get(self) -> usize {
        self.0 as usize
    }
    /// Native byte-key limit; preserve the default profile and admit the
    /// larger LMDB keys required by larger logical pages.
    pub const fn native_key_limit(self) -> usize {
        let n = self.get() / 2 - 70;
        if n > 4096 { n } else { 4096 }
    }
    pub const fn lmdb_key_limit(self) -> usize {
        self.get() / 2 - 70
    }
}
impl Default for PageSize {
    fn default() -> Self {
        Self::DEFAULT
    }
}
