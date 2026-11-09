use std::{cell::Cell, collections::BTreeMap};
use ulmdb_core::{
    Address, Prefix,
    storage::{Error, ReadStore, Reference, Result, WriteStore},
};
use ulmdb_prefix::{build_sorted, exact, longest_match, subnets, upsert_sorted};
use ulmdb_storage::{Arena, SegmentTable};

fn random(state: &mut u128) -> u128 {
    *state ^= *state << 23;
    *state ^= *state >> 17;
    *state ^= *state << 26;
    *state
}

#[test]
fn arbitrary_ancestor_splits_and_exact_replacements_match_an_independent_map() {
    for v6 in [false, true] {
        let address = |n| {
            if v6 {
                Address::V6(n)
            } else {
                Address::V4(n as u32)
            }
        };
        let width = if v6 { 128 } else { 32 };
        let mut state = 0x1234_5678_90ab_cdef_1234_5678_90ab_cdefu128;
        let mut initial = BTreeMap::new();
        for _ in 0..1000 {
            let prefix = Prefix::normalize(
                address(random(&mut state)),
                (random(&mut state) % (width + 1)) as u8,
            )
            .unwrap();
            initial.insert(prefix, random(&mut state).to_be_bytes().to_vec());
        }
        // Every possible ancestor depth at the same network exercises the
        // network/length ordering and the full-width branch boundary.
        for length in 0..=width as u8 {
            initial.insert(Prefix::new(address(0), length).unwrap(), vec![length]);
        }
        let mut arena = Arena::new(SegmentTable::default(), 11).unwrap();
        let mut root = build_sorted(&mut arena, initial.iter().map(|(p, v)| Ok((*p, v))))
            .unwrap()
            .root;
        let mut expected = initial;
        for pass in 0..3 {
            let mut input = BTreeMap::new();
            for _ in 0..1000 {
                let prefix = Prefix::normalize(
                    address(random(&mut state)),
                    (random(&mut state) % (width + 1)) as u8,
                )
                .unwrap();
                input.insert(prefix, vec![pass; (random(&mut state) % 1024) as usize]);
            }
            for length in 0..=width as u8 {
                input.insert(Prefix::new(address(0), length).unwrap(), vec![pass, length]);
            }
            root = upsert_sorted(&mut arena, root, input.iter().map(|(p, v)| Ok((*p, v))))
                .unwrap()
                .root;
            expected.extend(input);
            let actual = subnets(&arena, root, Prefix::new(address(0), 0).unwrap())
                .unwrap()
                .map(|record| record.map(|(p, v)| (p, v.to_vec())).unwrap())
                .collect::<BTreeMap<_, _>>();
            assert_eq!(actual, expected);
            for _ in 0..100 {
                let query = address(random(&mut state));
                let expected_hit = expected
                    .iter()
                    .filter(|(prefix, _)| prefix.contains_address(query))
                    .max_by_key(|(p, _)| p.length());
                assert_eq!(
                    longest_match(&arena, root, query).unwrap(),
                    expected_hit.map(|(p, v)| (*p, v.as_slice()))
                );
            }
        }
        let revision = arena.revision();
        assert_eq!(
            upsert_sorted(&mut arena, root, expected.iter().map(|(p, v)| Ok((*p, v))))
                .unwrap()
                .root,
            root
        );
        assert_eq!(arena.revision(), revision);
    }
}

struct Count<'a> {
    arena: &'a mut Arena,
    reads: Cell<usize>,
    writes: usize,
    remaining: usize,
}
impl ReadStore for Count<'_> {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        self.reads.set(self.reads.get() + 1);
        self.arena.read(at, length)
    }
}
impl WriteStore for Count<'_> {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        if self.remaining == 0 {
            return Err(Error::LimitExceeded);
        }
        self.remaining -= 1;
        self.writes += 1;
        self.arena.allocate(bytes, alignment)
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        if self.remaining == 0 {
            return Err(Error::LimitExceeded);
        }
        self.remaining -= 1;
        self.writes += 1;
        self.arena.rewrite(old, bytes, alignment)
    }
}

#[test]
fn sparse_updates_stay_on_one_path_and_failed_allocations_rollback_existing_values() {
    let mut arena = Arena::new(SegmentTable::default(), 12).unwrap();
    let prefix = |n| Prefix::new(Address::V4(n), 32).unwrap();
    let root = build_sorted(
        &mut arena,
        (0..10_000u32).map(|n| Ok((prefix(n), n.to_be_bytes()))),
    )
    .unwrap()
    .root;
    let checkpoint = arena.checkpoint().unwrap();
    let revision = arena.revision();
    let mut store = Count {
        arena: &mut arena,
        reads: Cell::new(0),
        writes: 0,
        remaining: usize::MAX,
    };
    upsert_sorted(&mut store, root, [Ok((prefix(5000), b"new"))]).unwrap();
    assert!(
        store.reads.get() < 100,
        "a sparse update must avoid walking untouched subtrees"
    );
    assert!(store.writes < 34);
    store.arena.rollback_checkpoint(&checkpoint).unwrap();
    assert_eq!(store.arena.revision(), revision);
    let checkpoint = store.arena.checkpoint().unwrap();
    store.remaining = 10;
    assert!(matches!(
        upsert_sorted(
            &mut store,
            root,
            (0..1000u32).map(|n| Ok((prefix(n), b"bad")))
        ),
        Err(Error::LimitExceeded)
    ));
    store.arena.rollback_checkpoint(&checkpoint).unwrap();
    assert_eq!(store.arena.revision(), revision);
    assert_eq!(
        exact(store.arena, root, prefix(7)).unwrap().unwrap().1,
        7u32.to_be_bytes()
    );
}

#[test]
fn byte_bounded_chunks_keep_order_validation_and_can_be_rolled_back() {
    let mut arena = Arena::new(SegmentTable::default(), 13).unwrap();
    let prefix = |n| Prefix::new(Address::V4(n), 32).unwrap();
    let root = build_sorted(&mut arena, [Ok((prefix(0), b"old"))])
        .unwrap()
        .root;
    let checkpoint = arena.checkpoint().unwrap();
    let value = vec![0x55; 100_000];
    let input = (0..20)
        .map(|n| Ok((prefix(n), value.as_slice())))
        .chain(std::iter::once(Ok((prefix(19), value.as_slice()))));
    assert!(upsert_sorted(&mut arena, root, input).is_err());
    arena.rollback_checkpoint(&checkpoint).unwrap();
    assert_eq!(exact(&arena, root, prefix(0)).unwrap().unwrap().1, b"old");
}
