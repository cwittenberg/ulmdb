use super::*;
use std::collections::BTreeMap;

fn sorted(rows: &BTreeMap<(Address, u8), Vec<u8>>) -> Vec<(Prefix, Vec<u8>)> {
    rows.iter()
        .map(|(&(address, length), value)| (Prefix::new(address, length).unwrap(), value.clone()))
        .collect()
}

#[test]
fn streaming_builder_matches_independent_oracle_for_all_lengths_and_random_ranges() {
    for ipv6 in [false, true] {
        let width = if ipv6 { 128 } else { 32 };
        let address = |n: u128| {
            if ipv6 {
                Address::V6(n)
            } else {
                Address::V4(n as u32)
            }
        };
        let mut rows = BTreeMap::new();
        let mut seed = 0x5af19e1789abdef123456789abcdef01u128;
        for length in 0..=width {
            for raw in [0, u128::MAX, seed] {
                let prefix = p(address(raw), length);
                rows.insert((prefix.network(), length), vec![length]);
            }
            seed = seed.wrapping_mul(0xda942042e4dd58b5).wrapping_add(17);
        }
        for _ in 0..500 {
            seed = seed.wrapping_mul(0xda942042e4dd58b5).wrapping_add(17);
            let length = (seed % u128::from(width + 1)) as u8;
            let prefix = p(address(seed), length);
            rows.insert((prefix.network(), length), seed.to_le_bytes().to_vec());
        }
        rows.insert((address(0), 0), Vec::new());
        let entries = sorted(&rows);
        let mut oracle = PrefixStore::default();
        for (prefix, value) in &entries {
            oracle.insert(model(*prefix), value.clone());
        }
        let mut arena = Arena::default();
        let built = build_sorted(
            &mut arena,
            entries.iter().map(|(p, v)| Ok((*p, v.as_slice()))),
        )
        .unwrap();
        assert_eq!(built.records as usize, rows.len());
        assert!(built.nodes < 2 * built.records);
        assert!(built.max_frontier <= usize::from(width) + 1);
        assert_eq!(arena.rewrites, 0);
        assert_eq!(arena.payload_allocations, entries.len());
        assert_eq!(arena.allocations as u64, built.nodes + built.records);
        assert_eq!(
            subnets(&arena, built.root, p(address(0), 0))
                .unwrap()
                .map(|r| {
                    let (p, v) = r.unwrap();
                    (p, v.to_vec())
                })
                .collect::<Vec<_>>(),
            entries
        );
        for (prefix, _) in &entries {
            verify(&arena, built.root, &oracle, *prefix);
        }
        for length in 0..=width {
            seed = seed.wrapping_mul(0xda942042e4dd58b5).wrapping_add(17);
            verify(&arena, built.root, &oracle, p(address(seed), length));
        }
        // Every stored node has strictly earlier children: no node rewrite or forward link.
        let mut pending = vec![built.root];
        while let Some(reference) = pending.pop() {
            let node = Node::read(&arena, reference).unwrap();
            for child in node.children.into_iter().filter(|r| !r.is_null()) {
                assert!(child.offset < reference.offset);
                pending.push(child);
            }
        }
    }
}

#[test]
fn builder_empty_singleton_and_full_ancestor_frontier() {
    let mut arena = Arena::default();
    let empty = build_sorted(&mut arena, std::iter::empty::<Result<(Prefix, &[u8])>>()).unwrap();
    assert_eq!(empty.root, Reference::NULL);
    assert_eq!(empty.records, 0);
    assert_eq!(empty.nodes, 0);
    assert_eq!(arena.allocations, 0);
    for address in [Address::V4(0), Address::V6(0)] {
        let width = crate::node::width(address);
        let rows = (0..=width).map(|length| Ok((p(address, length), vec![length])));
        let built = build_sorted(&mut arena, rows).unwrap();
        assert_eq!(built.nodes, u64::from(width) + 1);
        assert_eq!(built.max_frontier, usize::from(width) + 1);
        let mut root = built.root;
        for length in (0..=width).rev() {
            assert_eq!(
                get(&arena, root, address).unwrap().unwrap().0.length(),
                length
            );
            root = delete(&mut arena, root, p(address, length)).unwrap();
        }
        assert!(root.is_null());
        let built = build_sorted(&mut arena, [Ok((p(address, width), &[][..]))]).unwrap();
        assert_eq!(built.nodes, 1);
        assert_eq!(
            exact(&arena, built.root, p(address, width))
                .unwrap()
                .unwrap()
                .1,
            b""
        );
    }
}

#[test]
fn rejects_duplicate_descending_family_and_iterator_failure_without_rewriting_old_objects() {
    let mut arena = Arena::default();
    let old = insert(&mut arena, Reference::NULL, p(Address::V4(0), 0), b"old").unwrap();
    arena.mutable_from = arena.bytes.len();
    let before = arena.bytes.clone();
    for rows in [
        vec![
            (p(Address::V4(0), 0), b"a".as_slice()),
            (p(Address::V4(0), 0), b"b"),
        ],
        vec![
            (p(Address::V4(2), 32), b"a".as_slice()),
            (p(Address::V4(1), 32), b"b"),
        ],
        vec![
            (p(Address::V4(0), 32), b"a".as_slice()),
            (p(Address::V4(0), 0), b"b"),
        ],
        vec![
            (p(Address::V4(0), 0), b"a".as_slice()),
            (p(Address::V6(0), 0), b"b"),
        ],
    ] {
        assert!(matches!(
            build_sorted(&mut arena, rows.into_iter().map(Ok)),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(&arena.bytes[..before.len()], &before);
        assert_eq!(
            exact(&arena, old, p(Address::V4(0), 0)).unwrap().unwrap().1,
            b"old"
        );
    }
    let rows = [
        Ok((p(Address::V6(0), 0), b"new".as_slice())),
        Err(Error::Corrupt("source failure")),
    ];
    assert!(matches!(
        build_sorted(&mut arena, rows),
        Err(Error::Corrupt("source failure"))
    ));
    assert_eq!(&arena.bytes[..before.len()], &before);
}
