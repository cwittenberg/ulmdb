use super::valid_geometry;
#[test]
fn every_raw_count_and_boundary_geometry_matches_independent_wide_arithmetic() {
    for count in 0..=u16::MAX {
        let exact = 32u64 + 2 * u64::from(count);
        let mut lowers = vec![0, 31, 32, 34, 4095, 4096, u16::MAX];
        if exact <= u16::MAX as u64 {
            lowers.push(exact as u16);
            if exact > 0 {
                lowers.push((exact - 1) as u16);
            }
            if exact < u16::MAX as u64 {
                lowers.push((exact + 1) as u16);
            }
        }
        for lower in lowers {
            for upper in [
                0,
                31,
                32,
                33,
                lower,
                lower.saturating_add(1),
                4095,
                4096,
                u16::MAX,
            ] {
                let expected = u64::from(lower) == exact
                    && lower <= upper
                    && upper <= 4096
                    && upper.is_multiple_of(2);
                assert_eq!(
                    valid_geometry(count, lower, upper),
                    expected,
                    "count={count} lower={lower} upper={upper}"
                );
                if expected {
                    assert!(count <= 2032);
                    for index in 0..usize::from(count) {
                        let start = 32 + 2 * index;
                        assert!(
                            start >= 32 && start + 2 <= usize::from(lower) && start + 2 <= 4096
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn packed_edges_and_malformed_layouts_have_explicit_outcomes() {
    for (count, lower, upper) in [(0, 32, 32), (0, 32, 4096), (1, 34, 34), (2032, 4096, 4096)] {
        assert!(valid_geometry(count, lower, upper));
    }
    for (count, lower, upper) in [
        (0, 0, 4096),
        (1, 32, 4096),
        (1, 34, 32),
        (0, 32, 4095),
        (0, 32, 4098),
        (2033, 4098, 4098),
        (u16::MAX, u16::MAX, u16::MAX),
    ] {
        assert!(!valid_geometry(count, lower, upper));
    }
}
