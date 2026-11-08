//! Derived immutable acceleration only; canonical ranges remain the authority.
use super::pool::{PAGE, check};
use crate::{Error, Result};
const GRANULE: usize = 16;
const ENTRIES: usize = PAGE / GRANULE;
pub(super) enum ReadIndex {
    FullPage,
    Canonical,
    Tiny(Box<[u16; ENTRIES]>),
}
impl ReadIndex {
    pub(super) fn new(tiny: bool, ranges: &[(u16, u16)]) -> Self {
        if !tiny {
            // Raw low-level replay still permits canonical partial KV ranges;
            // do not broaden them merely because a coordinate is in KV class.
            return if ranges == [(0, PAGE as u16)] {
                Self::FullPage
            } else {
                Self::Canonical
            };
        }
        let mut table = Box::new([0; ENTRIES]);
        let mut seen = [false; ENTRIES];
        for &(start, len) in ranges {
            let start = start as usize;
            let end = start + len as usize;
            // Private callers have already validated nonempty, sorted, disjoint
            // ranges. Invalid geometry stays canonical rather than indexing it.
            if len == 0 || end > PAGE {
                return Self::Canonical;
            }
            for g in start / GRANULE..=(end - 1) / GRANULE {
                if seen[g] {
                    table[g] = 0;
                    continue;
                }
                seen[g] = true;
                // A partial unaligned first granule and every multi-object
                // granule must use canonical search. Zero is a fallback marker.
                if start <= g * GRANULE {
                    table[g] = end as u16;
                }
            }
        }
        Self::Tiny(table)
    }
    pub(super) fn check(&self, ranges: &[(u16, u16)], offset: usize, len: usize) -> Result<()> {
        // Empty one-past/adjacent-page queries retain exact canonical semantics.
        if len == 0 {
            return check(ranges, offset, len);
        }
        let end = offset
            .checked_add(len)
            .filter(|n| *n <= PAGE)
            .ok_or(Error::Corrupt("resident cross-page extent"))?;
        match self {
            Self::FullPage => Ok(()),
            Self::Canonical => check(ranges, offset, len),
            Self::Tiny(table) => {
                let bound = table[offset / GRANULE] as usize;
                if bound == 0 {
                    check(ranges, offset, len)
                } else if end <= bound {
                    Ok(())
                } else {
                    Err(Error::Corrupt("resident cross-object extent"))
                }
            }
        }
    }
    #[cfg(test)]
    fn fast_bound(&self, offset: usize) -> u16 {
        match self {
            Self::Tiny(t) => t[offset / GRANULE],
            _ => 0,
        }
    }
}
pub(super) const TINY_INDEX_BYTES: usize = ENTRIES * std::mem::size_of::<u16>();
#[cfg(test)]
mod tests {
    use super::*;
    fn equivalent(ranges: &[(u16, u16)]) {
        let index = ReadIndex::new(true, ranges);
        for offset in 0..=PAGE {
            for len in 0..=PAGE - offset {
                assert_eq!(
                    index.check(ranges, offset, len).is_ok(),
                    check(ranges, offset, len).is_ok(),
                    "ranges{ranges:?},offset{offset},len{len}"
                );
            }
            for len in [PAGE + 1, usize::MAX] {
                assert_eq!(
                    index.check(ranges, offset, len).is_ok(),
                    check(ranges, offset, len).is_ok()
                );
            }
        }
        for offset in [PAGE + 1, usize::MAX] {
            for len in [0, 1, usize::MAX] {
                assert_eq!(
                    index.check(ranges, offset, len).is_ok(),
                    check(ranges, offset, len).is_ok()
                );
            }
        }
    }
    #[test]
    fn exhaustive_all_offsets_lengths_preserve_aligned_unaligned_gap_and_boundary_ranges() {
        for ranges in [
            vec![(0, 96), (96, 24), (128, 96), (4080, 16)],
            vec![(1, 1), (2, 1), (8, 8), (17, 31), (49, 4000)],
            vec![(0, 4096)],
            vec![(16, 1), (18, 1), (20, 1), (4095, 1)],
            (0..256u16).map(|i| (i * 16, 1)).collect(),
        ] {
            equivalent(&ranges);
        }
    }
    #[test]
    fn fallback_is_mandatory_for_partial_first_and_multi_object_granules() {
        let index = ReadIndex::new(true, &[(8, 24), (32, 8), (40, 8), (64, 24)]);
        assert_eq!(index.fast_bound(8), 0);
        assert_eq!(index.fast_bound(16), 32);
        assert_eq!(index.fast_bound(32), 0);
        assert_eq!(index.fast_bound(64), 88);
        assert_eq!(index.fast_bound(80), 88);
    }
    #[test]
    fn kv_fullpage_flag_never_broadens_partial_raw_replay_membership() {
        for ranges in [vec![(0, 4096)], vec![(0, 3000)], vec![(16, 4000)]] {
            let index = ReadIndex::new(false, &ranges);
            for offset in 0..=PAGE {
                for len in [0, 1, 96, 4096, usize::MAX] {
                    assert_eq!(
                        index.check(&ranges, offset, len).is_ok(),
                        check(&ranges, offset, len).is_ok()
                    );
                }
            }
        }
    }
}
