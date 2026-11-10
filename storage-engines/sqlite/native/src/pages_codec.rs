//! Private fixed-page codec. Raw 4096-byte values retain their original format.
#![forbid(unsafe_code)]

use std::borrow::Cow;

pub const PAGE_BYTES: usize = 4096;
const MAGIC: &[u8; 8] = b"USQLZ4\x01\0";
const MIN_SAVING: usize = 64;
const MAX_ENCODED: usize = lz4_flex::block::get_maximum_output_size(PAGE_BYTES);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    InvalidLength,
    InvalidHeader,
    MalformedBlock,
    WrongDecodedLength,
}

/// Only use compression when the entire versioned record saves at least 64 B.
/// Incompressible input is borrowed; it incurs no raw fallback allocation.
pub fn encode(page: &[u8; PAGE_BYTES]) -> Cow<'_, [u8]> {
    let mut compressed = [0; MAX_ENCODED];
    let Ok(length) = lz4_flex::block::compress_into(page, &mut compressed) else {
        return Cow::Borrowed(page);
    };
    if MAGIC.len() + length + MIN_SAVING > PAGE_BYTES {
        return Cow::Borrowed(page);
    }
    let mut stored = Vec::with_capacity(MAGIC.len() + length);
    stored.extend_from_slice(MAGIC);
    stored.extend_from_slice(&compressed[..length]);
    Cow::Owned(stored)
}

/// Decode into a caller-owned fixed page without trusting a stored size field.
/// On error, the caller must discard the possibly partially written output.
pub fn decode_into(stored: &[u8], page: &mut [u8; PAGE_BYTES]) -> Result<(), CodecError> {
    // Length disambiguates legacy pages even when their bytes start with MAGIC.
    if stored.len() == PAGE_BYTES {
        page.copy_from_slice(stored);
        return Ok(());
    }
    if stored.len() <= MAGIC.len() || stored.len() >= PAGE_BYTES {
        return Err(CodecError::InvalidLength);
    }
    if !stored.starts_with(MAGIC) {
        return Err(CodecError::InvalidHeader);
    }
    let length = lz4_flex::block::decompress_into(&stored[MAGIC.len()..], page)
        .map_err(|_| CodecError::MalformedBlock)?;
    if length != PAGE_BYTES {
        return Err(CodecError::WrongDecodedLength);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(stored: &[u8]) -> Result<[u8; PAGE_BYTES], CodecError> {
        let mut page = [0; PAGE_BYTES];
        decode_into(stored, &mut page)?;
        Ok(page)
    }

    fn entropy(seed: u64) -> [u8; PAGE_BYTES] {
        let mut state = seed;
        let mut bytes = [0; PAGE_BYTES];
        for chunk in bytes.chunks_exact_mut(8) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            chunk.copy_from_slice(&state.to_le_bytes());
        }
        bytes
    }

    fn record(page: &[u8]) -> Vec<u8> {
        let mut stored = MAGIC.to_vec();
        stored.extend(lz4_flex::block::compress(page));
        stored
    }

    #[test]
    fn compressible_pages_roundtrip_and_save_space() {
        let mut sqlite_like = [0; PAGE_BYTES];
        sqlite_like[..16].copy_from_slice(b"SQLite format 3\0");
        for (index, byte) in sqlite_like[100..1100].iter_mut().enumerate() {
            *byte = (index % 251) as u8;
        }
        for original in [[0; PAGE_BYTES], [0xff; PAGE_BYTES], sqlite_like] {
            let stored = encode(&original);
            assert!(matches!(stored, Cow::Owned(_)));
            assert!(stored.len() + MIN_SAVING <= PAGE_BYTES);
            assert_eq!(decode(&stored).unwrap(), original);
        }
    }

    #[test]
    fn entropy_falls_back_without_a_raw_copy() {
        for seed in [1, 20261109, 0x123456789abcdef] {
            let original = entropy(seed);
            let stored = encode(&original);
            assert!(matches!(stored, Cow::Borrowed(_)));
            assert_eq!(stored.as_ptr(), original.as_ptr());
            assert_eq!(decode(&stored).unwrap(), original);
        }
    }

    #[test]
    fn legacy_raw_magic_is_unambiguous() {
        let mut original = entropy(7);
        original[..MAGIC.len()].copy_from_slice(MAGIC);
        assert_eq!(decode(&original).unwrap(), original);
        original[6] = 99; // A raw legacy page does not have a codec version.
        assert_eq!(decode(&original).unwrap(), original);
    }

    #[test]
    fn malformed_and_wrong_sized_records_are_rejected() {
        for length in [0, 1, MAGIC.len(), PAGE_BYTES + 1, PAGE_BYTES * 2] {
            assert_eq!(decode(&vec![0; length]), Err(CodecError::InvalidLength));
        }
        let mut stored = encode(&[0; PAGE_BYTES]).into_owned();
        stored[6] ^= 1;
        assert_eq!(decode(&stored), Err(CodecError::InvalidHeader));
        for length in MAGIC.len() + 1..stored.len() {
            let original = encode(&[0; PAGE_BYTES]).into_owned();
            assert!(decode(&original[..length]).is_err());
        }
        let mut invalid_offset = MAGIC.to_vec();
        invalid_offset.extend_from_slice(&[0, 0, 0]);
        assert_eq!(decode(&invalid_offset), Err(CodecError::MalformedBlock));
        assert_eq!(
            decode(&record(&[3; PAGE_BYTES - 1])),
            Err(CodecError::WrongDecodedLength)
        );
        assert_eq!(
            decode(&record(&[3; PAGE_BYTES + 1])),
            Err(CodecError::MalformedBlock)
        );
        assert_eq!(
            decode(&record(&[3; PAGE_BYTES * 16])),
            Err(CodecError::MalformedBlock)
        );
    }

    #[test]
    fn mixed_entropy_roundtrips_at_compression_threshold() {
        let original = entropy(91);
        let mut compressed = 0;
        let mut raw = 0;
        for zeroes in (0..=PAGE_BYTES).step_by(17).chain([PAGE_BYTES]) {
            let mut page = original;
            page[..zeroes].fill(0);
            let stored = encode(&page);
            if matches!(stored, Cow::Owned(_)) {
                compressed += 1;
                assert!(stored.len() + MIN_SAVING <= PAGE_BYTES);
            } else {
                raw += 1;
            }
            assert_eq!(decode(&stored).unwrap(), page);
        }
        assert!(compressed > 0 && raw > 0);
    }

    #[test]
    fn malformed_entropy_never_panics_or_allocates_from_a_stored_length() {
        let noise = entropy(37);
        for length in (MAGIC.len() + 1..PAGE_BYTES).step_by(7) {
            let mut stored = noise[..length].to_vec();
            stored[..MAGIC.len()].copy_from_slice(MAGIC);
            assert!(std::panic::catch_unwind(|| decode(&stored)).is_ok());
        }
    }
}
