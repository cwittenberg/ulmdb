//! Actual public APIs compared to the captured old implementation and bit oracle.
#[allow(dead_code)]
#[path = "support/original_prefix.rs"]
mod old;
use ulmdb_core::{Address, Prefix, PrefixError};

fn bits(value: u128, length: u8, width: u8) -> Option<u128> {
    if length > width {
        return None;
    }
    let mut result = 0;
    for index in 0..u32::from(width) {
        if index + u32::from(length) >= u32::from(width) && value & (1u128 << index) != 0 {
            result |= 1u128 << index;
        }
    }
    Some(result)
}
fn number(a: Address) -> u128 {
    match a {
        Address::V4(a) => u128::from(a),
        Address::V6(a) => a,
    }
}
fn original(a: Address) -> old::Address {
    match a {
        Address::V4(a) => old::Address::V4(a),
        Address::V6(a) => old::Address::V6(a),
    }
}
fn old_number(a: old::Address) -> u128 {
    match a {
        old::Address::V4(a) => u128::from(a),
        old::Address::V6(a) => a,
    }
}
fn error(e: old::PrefixError) -> PrefixError {
    match e {
        old::PrefixError::InvalidLength => PrefixError::InvalidLength,
        old::PrefixError::HostBitsSet => PrefixError::HostBitsSet,
    }
}
const fn inspect(prefix: Prefix) -> (Address, u8) {
    (prefix.network(), prefix.length())
}

#[test]
fn every_typed_length_matches_bit_oracle_and_original_public_api() {
    let mut state = 0x839a_d521_4356_7a12_92ff_031a_9851_b734u128;
    let mut values = vec![0, 1, u128::MAX];
    for bit in 0..128 {
        values.push(1u128 << bit);
        values.push(!(1u128 << bit));
    }
    for _ in 0..257 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 43;
        values.push(state);
    }
    let mut checked = 0;
    for width in [32u8, 128] {
        for value in &values {
            let a = if width == 32 {
                Address::V4(*value as u32)
            } else {
                Address::V6(*value)
            };
            for length in 0..=u8::MAX {
                let expected = bits(number(a), length, width);
                let actual = Prefix::normalize(a, length);
                let prior = old::Prefix::normalize(original(a), length);
                match expected {
                    None => {
                        assert_eq!(actual, Err(PrefixError::InvalidLength));
                        assert_eq!(
                            prior
                                .map_err(error)
                                .map(|p| (old_number(p.network()), p.length())),
                            Err(PrefixError::InvalidLength)
                        );
                        assert_eq!(Prefix::new(a, length), Err(PrefixError::InvalidLength));
                        assert_eq!(
                            old::Prefix::new(original(a), length)
                                .map_err(error)
                                .map(|p| old_number(p.network())),
                            Err(PrefixError::InvalidLength)
                        );
                    }
                    Some(n) => {
                        let p = actual.unwrap();
                        let q = prior.unwrap();
                        assert_eq!((number(p.network()), p.length()), (n, length));
                        assert_eq!((old_number(q.network()), q.length()), (n, length));
                        assert_eq!(inspect(p), (p.network(), length));
                        assert_eq!(Prefix::normalize(p.network(), length), Ok(p));
                        let strict =
                            Prefix::new(a, length).map(|p| (number(p.network()), p.length()));
                        let before = old::Prefix::new(original(a), length)
                            .map(|p| (old_number(p.network()), p.length()))
                            .map_err(error);
                        let want = if n == number(a) {
                            Ok((n, length))
                        } else {
                            Err(PrefixError::HostBitsSet)
                        };
                        assert_eq!(strict, want);
                        assert_eq!(before, want);
                        assert!(p.contains_address(a));
                        let (first, last) = p.address_bounds();
                        let (old_first, old_last) = q.address_bounds();
                        assert_eq!(
                            (number(first), number(last)),
                            (old_number(old_first), old_number(old_last))
                        );
                        assert!(number(first) <= number(a) && number(a) <= number(last));
                    }
                }
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 264_192);
}

#[test]
fn errors_preserve_existing_prefixes_and_boundary_family_semantics() {
    assert_eq!(std::any::type_name::<Address>(), "ulmdb_core::Address");
    assert_eq!(std::any::type_name::<Prefix>(), "ulmdb_core::Prefix");
    assert_eq!(
        std::any::type_name::<PrefixError>(),
        "ulmdb_core::PrefixError"
    );
    assert_eq!(
        std::mem::size_of::<Prefix>(),
        std::mem::size_of::<old::Prefix>()
    );
    assert_eq!(
        std::mem::align_of::<Prefix>(),
        std::mem::align_of::<old::Prefix>()
    );
    let prefixes = [
        Prefix::new(Address::V4(0), 0).unwrap(),
        Prefix::new(Address::V4(u32::MAX), 32).unwrap(),
        Prefix::new(Address::V6(0), 0).unwrap(),
        Prefix::new(Address::V6(u128::MAX), 128).unwrap(),
    ];
    let before = prefixes;
    for length in 0..=u8::MAX {
        for address in [Address::V4(u32::MAX), Address::V6(u128::MAX)] {
            let _ = Prefix::new(address, length);
            let _ = Prefix::normalize(address, length);
            assert_eq!(prefixes, before);
        }
    }
    for p in prefixes {
        for q in prefixes {
            let a = old::Prefix::new(original(p.network()), p.length()).unwrap();
            let b = old::Prefix::new(original(q.network()), q.length()).unwrap();
            assert_eq!(p.contains_prefix(q), a.contains_prefix(b));
            assert_eq!(p.overlaps(q), a.overlaps(b));
        }
    }
}

#[test]
fn canonical_error_leaf_witnesses_execute_the_actual_public_api() {
    assert_eq!(
        Prefix::new(Address::V4(0x0a000001), 33),
        Err(PrefixError::InvalidLength)
    );
    assert_eq!(
        Prefix::new(Address::V6(1), 129),
        Err(PrefixError::InvalidLength)
    );
    assert_eq!(
        Prefix::new(Address::V4(0x0a000001), 24),
        Err(PrefixError::HostBitsSet)
    );
    assert_eq!(
        Prefix::new(Address::V6(1), 127),
        Err(PrefixError::HostBitsSet)
    );
    for (address, length, network) in [
        (Address::V4(0x0a000001), 24, Address::V4(0x0a000000)),
        (Address::V6(1), 127, Address::V6(0)),
    ] {
        let first = Prefix::normalize(address, length).unwrap();
        let second = Prefix::normalize(address, length).unwrap();
        assert_eq!(first, second);
        assert_eq!((first.network(), first.length()), (network, length));
    }
    assert_eq!(
        Prefix::new(Address::V4(u32::MAX), 32).unwrap().network(),
        Address::V4(u32::MAX)
    );
    assert_eq!(
        Prefix::new(Address::V6(u128::MAX), 128).unwrap().network(),
        Address::V6(u128::MAX)
    );
}
