//! The persisted checkpoint count guard, shared by native decoding and Verus.
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

verus! {
/// The format's maximum descriptor chunk count fits both supported host widths.
pub const MAX_DESCRIPTOR_CHUNKS: usize = 1_048_575;

/// Reject malformed persisted counts before converting to the host word width.
#[inline]
pub fn descriptor_count(raw: u64, max_chunks: usize) -> (count: Option<usize>)
    ensures match count {
        Some(n) => n as int == raw as int && n <= max_chunks
            && n <= MAX_DESCRIPTOR_CHUNKS,
        None => raw as int > max_chunks as int
            || raw as int > MAX_DESCRIPTOR_CHUNKS as int,
    },
{
    if raw > MAX_DESCRIPTOR_CHUNKS as u64 || raw > max_chunks as u64 {
        None
    } else {
        Some(raw as usize)
    }
}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_counts_match_checked_host_conversion_and_independent_caps() {
        let limit = MAX_DESCRIPTOR_CHUNKS as u64;
        let raw_counts = [
            0,
            1,
            limit - 1,
            limit,
            limit + 1,
            u32::MAX as u64 - 1,
            u32::MAX as u64,
            u32::MAX as u64 + 1,
            u64::MAX,
        ];
        for maximum in [
            0,
            1,
            MAX_DESCRIPTOR_CHUNKS - 1,
            MAX_DESCRIPTOR_CHUNKS,
            MAX_DESCRIPTOR_CHUNKS + 1,
            usize::MAX,
        ] {
            for raw in raw_counts {
                let expected = usize::try_from(raw)
                    .ok()
                    .filter(|n| *n <= maximum && *n <= 1_048_575);
                assert_eq!(
                    descriptor_count(raw, maximum),
                    expected,
                    "raw={raw}, maximum={maximum}, host_bits={}",
                    usize::BITS
                );
            }
        }
        for raw in 0..=MAX_DESCRIPTOR_CHUNKS as u64 + 1 {
            assert_eq!(
                descriptor_count(raw, usize::MAX),
                (raw <= limit).then_some(raw as usize)
            );
        }
    }
}
