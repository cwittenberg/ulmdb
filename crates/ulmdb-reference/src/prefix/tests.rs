use super::*;
use ulmdb_core::{Prefix, kernel};

#[test]
fn cidr_covering_is_not_first_address_lpm() {
    let mut store = PrefixStore::default();
    for (n, l, v) in [
        (0x0a000000, 8, b'A'),
        (0x0a010000, 16, b'B'),
        (0x0a010200, 24, b'C'),
        (0x0a010200, 32, b'D'),
    ] {
        store.insert(ModelPrefix::normalize(Address::V4(n), l).unwrap(), vec![v]);
    }
    let q = ModelPrefix::normalize(Address::V4(0x0a010200), 24).unwrap();
    assert_eq!(store.longest_match(q.network()).unwrap().1, b"D");
    assert_eq!(store.covering(&q).unwrap().1, b"C");
    assert_eq!(
        store
            .ancestors(&q)
            .iter()
            .map(|p| p.length())
            .collect::<Vec<_>>(),
        vec![24, 16, 8]
    );
    assert_eq!(store.subnets(&q).len(), 2);
    assert_eq!(store.overlaps(&q).len(), 4);
    store.delete(&q);
    assert_eq!(
        store.longest_match(Address::V4(0x0a010201)).unwrap().1,
        b"B"
    );
    assert_eq!(store.longest_match(q.network()).unwrap().1, b"D");
}

#[test]
fn all_lengths_and_random_addresses_agree_with_independent_bit_oracle() {
    let mut seed = 0x1d0d_u128;
    for is_v6 in [false, true] {
        for depth in 0..=if is_v6 { 128 } else { 32 } {
            for _ in 0..32 {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let address = if is_v6 {
                    Address::V6(seed)
                } else {
                    Address::V4(seed as u32)
                };
                let oracle = ModelPrefix::normalize(address, depth).unwrap();
                let production = Prefix::normalize(address, depth).unwrap();
                assert_eq!(production.network(), oracle.network());
                let other = if is_v6 {
                    Address::V6(!seed)
                } else {
                    Address::V4(!seed as u32)
                };
                assert_eq!(
                    production.contains_address(other),
                    oracle.contains_address(other)
                );
            }
        }
    }
    for depth in 0..=u8::MAX {
        assert_eq!(kernel::prefix_equal32(0, 0, depth), depth <= 32);
        assert_eq!(kernel::prefix_equal128(0, 0, depth), depth <= 128);
    }
}
