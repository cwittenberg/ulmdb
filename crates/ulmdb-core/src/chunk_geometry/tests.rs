use super::*;

#[test]
fn logical_extremes_have_small_physical_offsets() {
    for offset in [0, DATA_BYTES - 1, DATA_BYTES, DATA_BYTES + 1, u64::MAX] {
        let (index, physical) = locate(offset);
        assert_eq!(
            index as u128 * DATA_BYTES as u128 + (physical - HEADER_BYTES) as u128,
            offset as u128
        );
        assert!(physical <= i32::MAX as u64);
    }
}

#[test]
fn continuation_preserves_boundaries_and_rejects_overflow() {
    assert_eq!(frame_end(0, ALIGN_BYTES), Some(ALIGN_BYTES));
    assert_eq!(next_frame(0, ALIGN_BYTES, ALIGN_BYTES), Some(ALIGN_BYTES));
    assert_eq!(next_frame(0, DATA_BYTES, ALIGN_BYTES), Some(DATA_BYTES));
    assert_eq!(
        next_frame(DATA_BYTES - ALIGN_BYTES, DATA_BYTES, ALIGN_BYTES),
        Some(DATA_BYTES)
    );
    assert_eq!(
        next_frame(
            DATA_BYTES - 2 * ALIGN_BYTES,
            DATA_BYTES - ALIGN_BYTES,
            2 * ALIGN_BYTES
        ),
        Some(DATA_BYTES)
    );
    assert_eq!(next_frame(0, 0, ALIGN_BYTES), None);
    assert_eq!(next_frame(1, ALIGN_BYTES, ALIGN_BYTES), None);
    assert_eq!(next_frame(0, ALIGN_BYTES, 0), None);
    assert_eq!(frame_end(u64::MAX - ALIGN_BYTES + 1, ALIGN_BYTES), None);
    assert_eq!(
        next_frame(
            u64::MAX - 2 * ALIGN_BYTES + 1,
            u64::MAX - ALIGN_BYTES + 1,
            2 * ALIGN_BYTES
        ),
        None
    );
}

#[test]
fn payload_rounding_is_exact_at_each_alignment_boundary() {
    for payload in [
        0,
        1,
        ALIGN_BYTES - 8193,
        ALIGN_BYTES - 8192,
        ALIGN_BYTES - 8191,
        MAX_PAYLOAD_BYTES,
    ] {
        let length = frame_length(payload).unwrap();
        assert!(length >= payload + 8192);
        assert!(length < payload + 8192 + ALIGN_BYTES);
        assert_eq!(length % ALIGN_BYTES, 0);
    }
    assert_eq!(frame_length(MAX_PAYLOAD_BYTES + 1), None);
    assert_eq!(frame_length(u64::MAX), None);
}
