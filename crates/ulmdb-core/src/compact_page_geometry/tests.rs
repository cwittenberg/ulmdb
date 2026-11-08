use super::valid_geometry;

// Separate u64 endpoint oracle: no implementation's u32/cast/short-circuit math.
fn oracle(leaf: bool, count: u16, lower: u16, upper: u16, key: u16, value: u16) -> bool {
    let width = u64::from(key) + if leaf { u64::from(value) } else { 16 };
    let end = 32 + u64::from(count) * width;
    lower == 32
        && key <= 256
        && value <= 512
        && (leaf || value == 0)
        && end <= 4096
        && end == u64::from(upper)
        && (width != 0 || count <= 1)
}

#[test]
fn compact_page_geometry_all_counts_and_raw_width_boundaries() {
    let widths = [
        (0, 0),
        (0, 1),
        (1, 0),
        (16, 8),
        (256, 512),
        (256, 0),
        (257, 0),
        (0, 513),
        (u16::MAX, u16::MAX),
    ];
    for leaf in [false, true] {
        for (key, value) in widths {
            for count in 0..=u16::MAX {
                let width = u64::from(key) + if leaf { u64::from(value) } else { 16 };
                let end = 32 + u64::from(count) * width;
                let exact = u16::try_from(end).unwrap_or(u16::MAX);
                for upper in [0, 32, 4096, u16::MAX, exact] {
                    assert_eq!(
                        valid_geometry(leaf, count, 32, upper, key, value),
                        oracle(leaf, count, 32, upper, key, value),
                        "{leaf}/{count}/{upper}/{key}/{value}"
                    );
                }
            }
        }
    }
}

#[test]
fn compact_page_geometry_extent_disjointness_and_empty_records() {
    let counts = [0, 1, 2, 3, 5, 15, 16, 127, 254, 255, 256, 4064, u16::MAX];
    for leaf in [false, true] {
        for key in [0, 1, 15, 16, 255, 256] {
            for value in [0, 1, 7, 511, 512] {
                let width = usize::from(key) + if leaf { usize::from(value) } else { 16 };
                for count in counts {
                    let end = 32u64 + u64::from(count) * width as u64;
                    let Ok(upper) = u16::try_from(end) else {
                        continue;
                    };
                    if !valid_geometry(leaf, count, 32, upper, key, value) {
                        continue;
                    }
                    let mut previous_end = 32usize;
                    for i in 0..usize::from(count) {
                        let start = 32 + i * width;
                        let end = start + width;
                        assert_eq!(start, previous_end);
                        assert!(end <= usize::from(upper) && end <= 4096);
                        previous_end = end;
                    }
                    if width == 0 {
                        assert!(count <= 1 && upper == 32);
                    }
                }
            }
        }
    }
}

#[test]
fn compact_page_geometry_rejects_malformed_headers_without_overflow() {
    assert!(valid_geometry(true, 1, 32, 32, 0, 0));
    assert!(!valid_geometry(true, 2, 32, 32, 0, 0));
    assert!(valid_geometry(true, 4064, 32, 4096, 1, 0));
    assert!(valid_geometry(false, 254, 32, 4096, 0, 0));
    for (leaf, count, lower, upper, key, value) in [
        (true, u16::MAX, 32, 0, u16::MAX, u16::MAX),
        (true, u16::MAX, 32, u16::MAX, 256, 512),
        (true, 1, 31, 40, 4, 4),
        (true, 1, 33, 40, 4, 4),
        (false, 1, 32, 48, 0, 1),
        (true, 1, 32, 40, 257, 0),
        (true, 1, 32, 40, 0, 513),
        (true, 1, 32, 4097, 256, 512),
        (false, 1, 32, 49, 0, 0),
    ] {
        assert!(!valid_geometry(leaf, count, lower, upper, key, value));
    }
}
