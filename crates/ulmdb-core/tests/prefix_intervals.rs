//! Independent per-bit construction plus numeric inclusive interval oracle.
use ulmdb_core::{Address, Prefix};

fn value(a: Address) -> u128 {
    match a {
        Address::V4(v) => u128::from(v),
        Address::V6(v) => v,
    }
}
fn address(v: u128, width: u8) -> Address {
    if width == 32 {
        Address::V4(v as u32)
    } else {
        Address::V6(v)
    }
}
fn oracle(v: u128, length: u8, width: u8) -> (u128, u128) {
    let mut first = if width == 32 {
        v & u128::from(u32::MAX)
    } else {
        v
    };
    let mut last = first;
    for bit in 0..u32::from(width - length) {
        first &= !(1u128 << bit);
        last |= 1u128 << bit;
    }
    (first, last)
}

#[test]
fn inclusive_intervals_match_actual_public_operations_for_both_families() {
    let mut rows = Vec::new();
    let mut random = 0x812a_1773_ecca_4491_bdaa_1390_dab1_906fu128;
    for width in [32u8, 128] {
        for length in 0..=width {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 43;
            for raw in [
                0,
                1,
                u128::MAX,
                0xaaaa_aaaa_aaaa_aaaa_aaaa_aaaa_aaaa_aaaa,
                random,
                !random,
            ] {
                let (first, last) = oracle(raw, length, width);
                let p = Prefix::new(address(first, width), length).unwrap();
                assert_eq!(
                    p.address_bounds(),
                    (address(first, width), address(last, width))
                );
                assert_eq!(Prefix::normalize(address(raw, width), length), Ok(p));
                for point in [
                    0,
                    u128::MAX,
                    raw,
                    first,
                    last,
                    first.saturating_sub(1),
                    last.saturating_add(1),
                ] {
                    let point = address(point, width);
                    assert_eq!(
                        p.contains_address(point),
                        first <= value(point) && value(point) <= last
                    );
                    assert!(!p.contains_address(address(
                        value(point),
                        if width == 32 { 128 } else { 32 }
                    )));
                }
                rows.push((p, width, first, last));
            }
        }
    }
    let mut pairs = 0usize;
    for &(a, aw, af, az) in &rows {
        for &(b, bw, bf, bz) in &rows {
            let subset = aw == bw && af <= bf && bz <= az;
            let overlap = aw == bw && af <= bz && bf <= az;
            assert_eq!(a.contains_prefix(b), subset, "{a:?} {b:?}");
            assert_eq!(a.overlaps(b), overlap, "{a:?} {b:?}");
            assert_eq!(a.overlaps(b), b.overlaps(a));
            if subset {
                assert!(overlap);
            }
            pairs += 1;
        }
    }
    assert_eq!(rows.len(), 972);
    assert_eq!(pairs, 944_784);
    println!(
        "checked {} prefixes and {pairs} ordered interval pairs",
        rows.len()
    );
}
