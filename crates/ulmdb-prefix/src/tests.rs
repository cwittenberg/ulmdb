use super::*;
use crate::node::Node;
use ulmdb_core::{
    Address, Prefix,
    storage::{Error, ReadStore, Reference, Result, WriteStore},
};
use ulmdb_reference::{ModelPrefix, PrefixStore};

#[derive(Clone)]
struct Arena {
    bytes: Vec<u8>,
    mutable_from: usize,
    allocations: usize,
    payload_allocations: usize,
    rewrites: usize,
}
impl Default for Arena {
    fn default() -> Self {
        Self {
            bytes: vec![0; 16],
            mutable_from: 16,
            allocations: 0,
            payload_allocations: 0,
            rewrites: 0,
        }
    }
}
impl ReadStore for Arena {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("test segment"));
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        let end = start.checked_add(length).ok_or(Error::LimitExceeded)?;
        self.bytes
            .get(start..end)
            .ok_or(Error::Corrupt("test bounds"))
    }
}
impl WriteStore for Arena {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.allocations += 1;
        if bytes.get(..2) == Some(&1u16.to_le_bytes()) {
            self.payload_allocations += 1;
        }
        let start = self.bytes.len().div_ceil(alignment) * alignment;
        self.bytes.resize(start, 0);
        self.bytes.extend_from_slice(bytes);
        Ok(Reference {
            segment: 1,
            offset: start as u64,
        })
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.rewrites += 1;
        if old.offset >= self.mutable_from as u64 {
            let start = old.offset as usize;
            self.bytes[start..start + bytes.len()].copy_from_slice(bytes);
            Ok(old)
        } else {
            self.allocate(bytes, alignment)
        }
    }
    fn replace_payload(&mut self, old: Reference, value: &[u8]) -> Result<Reference> {
        if !old.is_null()
            && old.offset >= self.mutable_from as u64
            && self.payload(old)?.len() == value.len()
        {
            let start = old.offset as usize + 16;
            self.bytes[start..start + value.len()].copy_from_slice(value);
            return Ok(old);
        }
        self.put_payload(value)
    }
}
fn p(address: Address, length: u8) -> Prefix {
    Prefix::normalize(address, length).unwrap()
}
fn model(prefix: Prefix) -> ModelPrefix {
    ModelPrefix::normalize(prefix.network(), prefix.length()).unwrap()
}
fn owned(value: Option<(Prefix, &[u8])>) -> Option<(Prefix, Vec<u8>)> {
    value.map(|(p, v)| (p, v.to_vec()))
}
fn expected(value: Option<(&ModelPrefix, &[u8])>) -> Option<(Prefix, Vec<u8>)> {
    value.map(|(p, v)| {
        (
            Prefix::new(p.network(), p.length() as u8).unwrap(),
            v.to_vec(),
        )
    })
}
fn keys<'a>(cursor: Cursor<'a, Arena>) -> Vec<Prefix> {
    cursor.map(|item| item.unwrap().0).collect()
}
fn model_keys(prefixes: Vec<&ModelPrefix>) -> Vec<Prefix> {
    prefixes
        .into_iter()
        .map(|p| Prefix::new(p.network(), p.length() as u8).unwrap())
        .collect()
}
fn verify(arena: &Arena, root: Reference, oracle: &PrefixStore, query: Prefix) {
    let m = model(query);
    let prepared = Reader::new(arena, root).unwrap();
    assert_eq!(
        owned(prepared.longest_match(query.network()).unwrap()),
        expected(oracle.longest_match(query.network()))
    );
    assert_eq!(
        owned(prepared.covering(query).unwrap()),
        expected(oracle.covering(&m))
    );
    assert_eq!(
        prepared.exact(query).unwrap().map(|(_, value)| value),
        oracle.exact(&m)
    );
    assert_eq!(
        owned(get(arena, root, query.network()).unwrap()),
        expected(oracle.longest_match(query.network()))
    );
    assert_eq!(
        owned(covering(arena, root, query).unwrap()),
        expected(oracle.covering(&m))
    );
    assert_eq!(
        exact(arena, root, query).unwrap().map(|(_, v)| v),
        oracle.exact(&m)
    );
    assert_eq!(
        keys(ancestors(arena, root, query).unwrap()),
        model_keys(oracle.ancestors(&m))
    );
    assert_eq!(
        keys(subnets(arena, root, query).unwrap()),
        model_keys(oracle.subnets(&m))
    );
    assert_eq!(
        keys(overlaps(arena, root, query).unwrap()),
        model_keys(oracle.overlaps(&m))
    );
}

#[test]
fn every_prefix_length_and_fallback_after_delete() {
    for address in [Address::V4(u32::MAX), Address::V6(u128::MAX)] {
        let width = if matches!(address, Address::V4(_)) {
            32
        } else {
            128
        };
        let mut arena = Arena::default();
        let mut root = Reference::NULL;
        let mut oracle = PrefixStore::default();
        for length in 0..=width {
            let prefix = p(address, length);
            root = insert(&mut arena, root, prefix, &[length]).unwrap();
            oracle.insert(model(prefix), vec![length]);
        }
        for length in 0..=width {
            verify(&arena, root, &oracle, p(address, length));
        }
        for length in (0..=width).rev() {
            let prefix = p(address, length);
            root = delete(&mut arena, root, prefix).unwrap();
            oracle.delete(&model(prefix));
            verify(&arena, root, &oracle, prefix);
        }
        assert!(root.is_null());
    }
}

fn random(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}
fn address(seed: &mut u64, v6: bool) -> Address {
    if v6 {
        Address::V6((u128::from(random(seed)) << 64) | u128::from(random(seed)))
    } else {
        Address::V4(random(seed) as u32)
    }
}
#[test]
fn differential_mixed_updates_enumeration_and_immutable_snapshots() {
    for v6 in [false, true] {
        let mut seed = 0x1b0f_7a21_872c_01d5;
        let mut arena = Arena::default();
        let mut root = Reference::NULL;
        let mut oracle = PrefixStore::default();
        let mut inserted = Vec::new();
        let width = if v6 { 128 } else { 32 };
        for step in 0..900u32 {
            let prefix = if step % 4 == 3 && !inserted.is_empty() {
                inserted[(random(&mut seed) as usize) % inserted.len()]
            } else {
                let address = address(&mut seed, v6);
                p(address, (random(&mut seed) % (width + 1)) as u8)
            };
            let snapshot = arena.clone();
            let prior_root = root;
            let prior = oracle.clone();
            // Freeze previously allocated bytes, then exercise both the initial
            // immutable copy and repeated in-place writer-private rewrites.
            arena.mutable_from = arena.bytes.len();
            for iteration in 0..2u32 {
                if step % 4 == 3 {
                    root = delete(&mut arena, root, prefix).unwrap();
                    oracle.delete(&model(prefix));
                } else {
                    let value = (step + iteration).to_le_bytes();
                    root = insert(&mut arena, root, prefix, &value).unwrap();
                    oracle.insert(model(prefix), value.to_vec());
                    inserted.push(prefix);
                }
                assert_eq!(
                    owned(get(&arena, root, prefix.network()).unwrap()),
                    expected(oracle.longest_match(prefix.network()))
                );
            }
            verify(&snapshot, prior_root, &prior, prefix);
            verify(&arena, root, &oracle, prefix);
            for _ in 0..4 {
                let query_address = address(&mut seed, v6);
                assert_eq!(
                    owned(get(&arena, root, query_address).unwrap()),
                    expected(oracle.longest_match(query_address))
                );
            }
            if step % 29 == 0 {
                let query_address = address(&mut seed, v6);
                let query = p(query_address, (random(&mut seed) % (width + 1)) as u8);
                verify(&arena, root, &oracle, query);
                verify(&arena, root, &oracle, p(query_address, 0));
            }
        }
    }
}

#[test]
fn compressed_paths_family_isolation_empty_values_and_subtree_deletion() {
    let mut arena = Arena::default();
    let mut root = Reference::NULL;
    let zero = p(Address::V4(0), 0);
    let nested = p(Address::V4(0x0a00_0000), 8);
    let leaf = p(Address::V4(0x0a01_0000), 16);
    root = insert(&mut arena, root, leaf, b"leaf").unwrap();
    assert!(
        get(&arena, root, Address::V4(0x0b01_0000))
            .unwrap()
            .is_none()
    );
    root = insert(&mut arena, root, zero, b"default").unwrap();
    root = insert(&mut arena, root, nested, b"").unwrap();
    assert_eq!(
        get(&arena, root, Address::V4(0x0a02_0000)).unwrap(),
        Some((nested, &b""[..]))
    );
    root = delete(&mut arena, root, nested).unwrap();
    assert_eq!(
        get(&arena, root, Address::V4(0x0a02_0000)).unwrap(),
        Some((zero, &b"default"[..]))
    );
    assert_eq!(
        get(&arena, root, leaf.network()).unwrap(),
        Some((leaf, &b"leaf"[..]))
    );
    assert!(get(&arena, root, Address::V6(0)).unwrap().is_none());
    assert!(insert(&mut arena, root, p(Address::V6(0), 0), b"bad").is_err());
    assert_eq!(
        delete(&mut arena, root, p(Address::V6(0), 0)).unwrap(),
        root
    );
    assert!(keys(overlaps(&arena, root, p(Address::V6(0), 0)).unwrap()).is_empty());
}

#[test]
fn corrupt_headers_references_paths_and_payloads_are_rejected() {
    let mut arena = Arena::default();
    let root = insert(&mut arena, Reference::NULL, p(Address::V4(0), 0), b"value").unwrap();
    for offset in [0, 2, 4, 8, 9, 10, 20, 80] {
        let mut bad = arena.clone();
        bad.bytes[root.offset as usize + offset] = 255;
        assert!(get(&bad, root, Address::V4(0)).is_err(), "offset {offset}");
    }
    let mut node = Node::read(&arena, root).unwrap();
    node.children[0] = root; // A cycle fails strict-depth validation.
    let at = arena.allocate(&node.encode(), 16).unwrap();
    assert!(get(&arena, at, Address::V4(0)).is_err());
    assert!(
        subnets(&arena, at, p(Address::V4(0), 0))
            .unwrap()
            .next()
            .unwrap()
            .is_err()
    );
    node.children[0] = Reference::NULL;
    node.value = Reference {
        segment: 1,
        offset: 17,
    };
    let at = arena.allocate(&node.encode(), 16).unwrap();
    assert!(get(&arena, at, Address::V4(0)).is_err());
    let value = Node::read(&arena, root).unwrap().value;
    arena.bytes[value.offset as usize] = 0;
    assert!(exact(&arena, root, p(Address::V4(0), 0)).is_err());
    let truncated = Reference {
        segment: 1,
        offset: arena.bytes.len().div_ceil(16) as u64 * 16,
    };
    assert!(get(&arena, truncated, Address::V4(0)).is_err());
}

#[test]
fn exhaustive_small_universe_splits_replacements_and_compression() {
    for v6 in [false, true] {
        let make = |n: u8| {
            if v6 {
                Address::V6(u128::from(n) << 124)
            } else {
                Address::V4(u32::from(n) << 28)
            }
        };
        let prefixes: Vec<_> = (0..=4)
            .flat_map(|depth| (0..1u8 << depth).map(move |n| p(make(n << (4 - depth)), depth)))
            .collect();
        let mut arena = Arena::default();
        let mut root = Reference::NULL;
        let mut oracle = PrefixStore::default();
        // Odd multiplication permutes all 31 entries, repeatedly introducing
        // ancestors above existing roots and branches within skipped paths.
        for step in 0..62 {
            let prefix = prefixes[(step * 13) % prefixes.len()];
            let value = [step as u8];
            root = insert(&mut arena, root, prefix, &value).unwrap();
            oracle.insert(model(prefix), value.to_vec());
            for &query in &prefixes {
                verify(&arena, root, &oracle, query);
            }
        }
        for step in 0..31 {
            let prefix = prefixes[(step * 7) % prefixes.len()];
            root = delete(&mut arena, root, prefix).unwrap();
            oracle.delete(&model(prefix));
            for &query in &prefixes {
                verify(&arena, root, &oracle, query);
            }
        }
        assert!(root.is_null());
        for query in prefixes {
            verify(&arena, root, &oracle, query);
        }
    }
}

#[test]
fn malformed_branch_family_and_skipped_ancestor_are_rejected() {
    let mut arena = Arena::default();
    let root_prefix = p(Address::V4(0x0a00_0000), 8);
    let value = arena.put_payload(b"x").unwrap();
    for bad_prefix in [
        p(Address::V4(0x0b00_0000), 16), // Skipped path disagrees.
        p(Address::V4(0x0a80_0000), 16), // Wrong branch at bit eight.
        p(Address::V6(0), 16),           // Wrong address family.
    ] {
        let bad = arena
            .allocate(&Node::leaf(bad_prefix, value).encode(), 16)
            .unwrap();
        let mut parent = Node::leaf(root_prefix, Reference::NULL);
        parent.children[0] = bad;
        let root = arena.allocate(&parent.encode(), 16).unwrap();
        assert!(get(&arena, root, Address::V4(0x0a00_0001)).is_err());
        assert!(
            Reader::new(&arena, root)
                .unwrap()
                .longest_match(Address::V4(0x0a00_0001))
                .is_err()
        );
        let mut cursor = subnets(&arena, root, root_prefix).unwrap();
        assert!(cursor.next().unwrap().is_err());
        assert!(cursor.next().is_none());
        assert!(insert(&mut arena, root, p(Address::V4(0x0a00_0000), 24), b"new").is_err());
        assert!(delete(&mut arena, root, p(Address::V4(0x0a00_0000), 24)).is_err());
    }
}

#[test]
fn prepared_root_decodes_once_and_outside_misses_read_no_storage() {
    use std::cell::Cell;
    struct Counted<'a> {
        arena: &'a Arena,
        reads: Cell<usize>,
    }
    impl ReadStore for Counted<'_> {
        fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
            self.reads.set(self.reads.get() + 1);
            self.arena.read(at, length)
        }
    }
    for (prefix, outside) in [
        (p(Address::V4(0x0a00_0000), 8), Address::V4(0xc000_0201)),
        (
            p(Address::V6(0x2001_0db8u128 << 96), 32),
            Address::V6(0x2001_0db9u128 << 96),
        ),
    ] {
        let mut arena = Arena::default();
        let root = insert(&mut arena, Reference::NULL, prefix, b"payload").unwrap();
        let counted = Counted {
            arena: &arena,
            reads: Cell::new(0),
        };
        let prepared = Reader::new(&counted, root).unwrap();
        assert_eq!(counted.reads.get(), 1);
        counted.reads.set(0);
        for _ in 0..100 {
            assert_eq!(prepared.longest_match(outside).unwrap(), None);
            assert_eq!(
                prepared
                    .covering(Prefix::normalize(outside, prefix.length()).unwrap())
                    .unwrap(),
                None
            );
        }
        assert_eq!(counted.reads.get(), 0);
        assert_eq!(
            prepared.longest_match(prefix.network()).unwrap(),
            Some((prefix, b"payload".as_slice()))
        );
        assert_eq!(
            counted.reads.get(),
            2,
            "only payload header and bytes are loaded"
        );
        assert_eq!(prepared.root_prefix(), Some(prefix));
        let mut bad = arena.clone();
        bad.bytes[root.offset as usize] = 0;
        assert!(Reader::new(&bad, root).is_err());
    }
}

#[test]
fn repeated_private_replacement_reuses_payload_and_all_ancestor_nodes() {
    for v6 in [false, true] {
        let address = if v6 {
            Address::V6(0x0a01u128 << 112)
        } else {
            Address::V4(0x0a01_0000)
        };
        let target = p(address, 16);
        let mut arena = Arena::default();
        let mut root = Reference::NULL;
        // Exact values at each ancestor ensure the replacement descends through
        // a nontrivial path rather than exercising a single root-only update.
        for length in [0, 4, 8, 16] {
            root = insert(&mut arena, root, p(address, length), &0u64.to_le_bytes()).unwrap();
        }
        let snapshot = arena.clone();
        let old_root = root;
        arena.mutable_from = arena.bytes.len();
        let payloads_before = arena.payload_allocations;
        root = insert(&mut arena, root, target, &1u64.to_le_bytes()).unwrap();
        assert_ne!(root, old_root, "immutable path must be copied");
        assert_eq!(arena.payload_allocations, payloads_before + 1);
        let private_root = root;
        let bytes_after_copy = arena.bytes.len();
        let allocations_after_copy = arena.allocations;
        let rewrites_after_copy = arena.rewrites;
        for value in 2..=1024u64 {
            root = insert(&mut arena, root, target, &value.to_le_bytes()).unwrap();
            assert_eq!(root, private_root);
            assert_eq!(arena.bytes.len(), bytes_after_copy);
            assert_eq!(arena.allocations, allocations_after_copy);
            assert_eq!(
                arena.rewrites, rewrites_after_copy,
                "unchanged references must not rewrite ancestors"
            );
            assert_eq!(
                exact(&arena, root, target).unwrap().unwrap().1,
                &value.to_le_bytes()
            );
            assert_eq!(
                exact(&arena, old_root, target).unwrap().unwrap().1,
                &0u64.to_le_bytes()
            );
            assert_eq!(
                exact(&snapshot, old_root, target).unwrap().unwrap().1,
                &0u64.to_le_bytes()
            );
        }
        // Size changes must allocate without altering the immutable snapshot.
        root = insert(&mut arena, root, target, b"larger than eight bytes").unwrap();
        assert_eq!(arena.payload_allocations, payloads_before + 2);
        assert_eq!(
            exact(&arena, root, target).unwrap().unwrap().1,
            b"larger than eight bytes"
        );
        assert_eq!(
            exact(&arena, old_root, target).unwrap().unwrap().1,
            &0u64.to_le_bytes()
        );
    }
}

mod node_validation;

mod bulk;

#[path = "tests/radix_decoder/mod.rs"]
mod radix_decoder;
