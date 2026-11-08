//! Per-mapping read-ahead request. Advice never changes bytes or ownership.
use crate::{Error, Result};

/// Immutable advice for file-backed mappings owned by an environment.
/// `Random` requests Unix `MADV_RANDOM`; it is a best-effort OS hint, matching
/// LMDB NORDAHEAD. It makes no performance guarantee and is not persisted.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReadAhead {
    #[default]
    Normal,
    Random,
}
impl ReadAhead {
    /// Reject unsupported explicit requests before opening or creating files.
    pub fn validate(self) -> Result<()> {
        if self == Self::Random && !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
            return Err(Error::InvalidInput("random mapping advice is unsupported"));
        }
        Ok(())
    }
    pub(crate) fn apply(self, bytes: &memmap2::Mmap) {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if self == Self::Random {
            // This is per-VMA advice, not a file/inode-wide fadvise policy.
            // Pinned LMDB ignores advisory errors; retain that best-effort
            // contract. Real kernel acceptance is tested separately.
            let _ = bytes.advise(memmap2::Advice::Random);
        }
        // Pinned LMDB's Windows provider accepts NORDAHEAD without applying
        // Unix mapping advice. Keep that flag contract for Windows as well.
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let _ = bytes;
    }
}
