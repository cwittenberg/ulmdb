use super::*;

fn reserved_offsets(v6: bool) -> Vec<usize> {
    let mut offsets = (10..16).chain(80..96).collect::<Vec<_>>();
    if !v6 {
        offsets.extend(20..32);
    }
    offsets
}
fn family_queries(v6: bool) -> (Prefix, Prefix, Address, Address) {
    if v6 {
        let network = 0x2001_0db8u128 << 96;
        (
            p(Address::V6(network), 32),
            p(Address::V6(network | (1u128 << 64)), 64),
            Address::V6(network | (1u128 << 64) | 1),
            Address::V6(network | (1u128 << 95) | 1),
        )
    } else {
        (
            p(Address::V4(0x0a00_0000), 8),
            p(Address::V4(0x0a01_0000), 16),
            Address::V4(0x0a01_0001),
            Address::V4(0x0a80_0001),
        )
    }
}

#[test]
fn every_reserved_bit_rejects_ipv4_and_ipv6_roots_across_read_entry_points() {
    let mut cases = 0;
    for v6 in [false, true] {
        let (prefix, target, address, _) = family_queries(v6);
        let mut arena = Arena::default();
        let root = insert(&mut arena, Reference::NULL, prefix, b"root").unwrap();
        let mut oracle = PrefixStore::default();
        oracle.insert(model(prefix), b"root".to_vec());
        verify(&arena, root, &oracle, prefix);
        verify(&arena, root, &oracle, target);
        assert_eq!(
            owned(get(&arena, root, address).unwrap()),
            expected(oracle.longest_match(address))
        );
        for offset in reserved_offsets(v6) {
            for bit in 0..8 {
                let mut bad = arena.clone();
                bad.bytes[root.offset as usize + offset] ^= 1 << bit;
                assert!(
                    matches!(Node::read(&bad, root), Err(Error::Corrupt(_))),
                    "v6={v6} byte={offset} bit={bit}"
                );
                assert!(matches!(Reader::new(&bad, root), Err(Error::Corrupt(_))));
                assert!(matches!(get(&bad, root, address), Err(Error::Corrupt(_))));
                assert!(matches!(exact(&bad, root, prefix), Err(Error::Corrupt(_))));
                assert!(matches!(
                    covering(&bad, root, target),
                    Err(Error::Corrupt(_))
                ));
                assert!(matches!(
                    ancestors(&bad, root, target),
                    Err(Error::Corrupt(_))
                ));
                assert!(matches!(
                    subnets(&bad, root, prefix),
                    Err(Error::Corrupt(_))
                ));
                assert!(matches!(
                    overlaps(&bad, root, prefix),
                    Err(Error::Corrupt(_))
                ));
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 448); //272 IPv4 and176 IPv6 reserved-bit mutations.
}

#[test]
fn every_reserved_bit_in_descendants_is_checked_after_prepared_root_binding() {
    let mut cases = 0;
    for v6 in [false, true] {
        let (prefix, target, address, other_branch) = family_queries(v6);
        let mut arena = Arena::default();
        let mut root = insert(&mut arena, Reference::NULL, prefix, b"fallback").unwrap();
        root = insert(&mut arena, root, target, b"target").unwrap();
        let mut oracle = PrefixStore::default();
        oracle.insert(model(prefix), b"fallback".to_vec());
        oracle.insert(model(target), b"target".to_vec());
        verify(&arena, root, &oracle, prefix);
        verify(&arena, root, &oracle, target);
        let node = Node::read(&arena, root).unwrap();
        let child = node.children[crate::node::branch(target.network(), prefix.length())];
        assert!(!child.is_null());
        assert_eq!(Node::read(&arena, child).unwrap().prefix, target);
        for offset in reserved_offsets(v6) {
            for bit in 0..8 {
                let mut bad = arena.clone();
                bad.bytes[child.offset as usize + offset] ^= 1 << bit;
                let prepared = Reader::new(&bad, root).unwrap();
                assert!(
                    matches!(prepared.longest_match(address), Err(Error::Corrupt(_))),
                    "v6={v6} byte={offset} bit={bit}"
                );
                assert!(matches!(prepared.exact(target), Err(Error::Corrupt(_))));
                assert!(matches!(prepared.covering(target), Err(Error::Corrupt(_))));
                assert!(matches!(get(&bad, root, address), Err(Error::Corrupt(_))));
                assert!(matches!(exact(&bad, root, target), Err(Error::Corrupt(_))));
                assert!(matches!(
                    covering(&bad, root, target),
                    Err(Error::Corrupt(_))
                ));
                assert!(matches!(
                    ancestors(&bad, root, target),
                    Err(Error::Corrupt(_))
                ));
                for mut cursor in [
                    subnets(&bad, root, prefix).unwrap(),
                    overlaps(&bad, root, prefix).unwrap(),
                ] {
                    assert!(matches!(cursor.next(), Some(Err(Error::Corrupt(_)))));
                    assert!(cursor.next().is_none());
                }
                // Scalar reads validate the path they traverse; an independent
                // missing branch still returns its correct ancestor fallback.
                let expected = expected(oracle.longest_match(other_branch));
                assert_eq!(
                    owned(prepared.longest_match(other_branch).unwrap()),
                    expected
                );
                assert_eq!(owned(get(&bad, root, other_branch).unwrap()), expected);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 448);
}
