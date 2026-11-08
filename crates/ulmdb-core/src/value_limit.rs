//! Authenticated bounded file-value profiles and their contiguous segment envelopes.
use crate::storage::{Error, Result};
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ValueLimit(u32);
impl ValueLimit {
    pub const DEFAULT: Self = Self(67_108_864);
    pub const EXTENDED: Self = Self(134_217_728);
    /// Explicit opt-in; the pinned LMDB4GiB maximum is not implied.
    pub const LARGE: Self = Self(536_870_912);
    /// Pinned LMDB MAXDATASIZE, supported only by64-bit address spaces.
    pub const MAXIMUM: Self = Self(u32::MAX);
    pub const fn is_supported(self) -> bool {
        self.0 != u32::MAX || usize::BITS > 32
    }
    pub const fn platform_maximum() -> Self {
        if usize::BITS > 32 {
            Self::MAXIMUM
        } else {
            Self::LARGE
        }
    }
    /// Persisted physical bound, independent of the decoding host's word size.
    pub const fn segment_limit(self) -> u64 {
        if self.0 == u32::MAX {
            8_589_934_592
        } else if self.0 > Self::EXTENDED.0 {
            1_073_741_824
        } else {
            268_435_456
        }
    }
    /// Zero means that this profile is unsupported on the current host.
    pub const fn segment_bytes(self) -> usize {
        if self.is_supported() {
            self.segment_limit() as usize
        } else {
            0
        }
    }
    pub const fn new(bytes: usize) -> Result<Self> {
        match bytes {
            67_108_864 => Ok(Self::DEFAULT),
            134_217_728 => Ok(Self::EXTENDED),
            536_870_912 => Ok(Self::LARGE),
            4_294_967_295 if usize::BITS > 32 => Ok(Self::MAXIMUM),
            _ => Err(Error::InvalidInput("unsupported file value limit")),
        }
    }
    pub const fn get(self) -> usize {
        self.0 as usize
    }
}
impl Default for ValueLimit {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod maximum_tests {
    use super::*;
    #[test]
    fn maximum_is_exact_and_old_profiles_keep_their_envelopes() {
        assert_eq!(ValueLimit::DEFAULT.segment_limit(), 256 << 20);
        assert_eq!(ValueLimit::EXTENDED.segment_limit(), 256 << 20);
        assert_eq!(ValueLimit::LARGE.segment_limit(), 1 << 30);
        assert_eq!(ValueLimit::MAXIMUM.get() as u64, 0xffff_ffff);
        assert_eq!(ValueLimit::MAXIMUM.segment_limit(), 8 << 30);
        assert_eq!(ValueLimit::new(u32::MAX as usize).is_ok(), usize::BITS > 32);
        #[cfg(target_pointer_width = "64")]
        assert!(ValueLimit::new((u32::MAX as usize) + 1).is_err());
        #[cfg(target_pointer_width = "32")]
        assert_eq!(ValueLimit::MAXIMUM.segment_bytes(), 0);
    }
}

// Compile-evaluated32-bit refusal: no linker/runtime is required to prove that
// the persisted maximum cannot become a slice profile on a narrow host.
#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(!ValueLimit::MAXIMUM.is_supported());
    assert!(ValueLimit::MAXIMUM.segment_bytes() == 0);
    let refusal = ValueLimit::new(u32::MAX as usize);
    assert!(matches!(&refusal, Err(_)));
    core::mem::forget(refusal);
};
