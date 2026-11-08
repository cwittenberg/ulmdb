use super::Page;
#[test]
fn byte_decoder_matches_original_header_guard_for_all_counts_and_boundary_fields() {
    let mut bytes = [0u8; 4096];
    bytes[..2].copy_from_slice(&2u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&1u16.to_le_bytes());
    for count in 0..=u16::MAX {
        bytes[6..8].copy_from_slice(&count.to_le_bytes());
        let exact = 32usize + 2 * usize::from(count);
        for lower in [0, 32, 34, 4096, exact as u16] {
            bytes[8..10].copy_from_slice(&lower.to_le_bytes());
            for upper in [0, lower, 4095, 4096, u16::MAX] {
                bytes[10..12].copy_from_slice(&upper.to_le_bytes());
                let expected = usize::from(lower) == exact
                    && lower <= upper
                    && upper <= 4096
                    && upper.is_multiple_of(2);
                assert_eq!(
                    Page::decode(&bytes).is_ok(),
                    expected,
                    "count={count} lower={lower} upper={upper}"
                );
            }
        }
    }
}
