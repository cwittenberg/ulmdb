use super::wire::{HEADER, Node};
use super::*;
use std::collections::BTreeMap;
use ulmdb_core::{
    Address, Prefix,
    storage::{Error, ReadStore, Reference, Result, WriteStore},
};
#[derive(Clone)]
struct Memory {
    bytes: Vec<u8>,
    barrier: u64,
    allocations: usize,
    rewrites: usize,
}
impl Default for Memory {
    fn default() -> Self {
        Self {
            bytes: vec![0; 16],
            barrier: 16,
            allocations: 0,
            rewrites: 0,
        }
    }
}
impl ReadStore for Memory {
    fn read(&self, r: Reference, n: usize) -> Result<&[u8]> {
        if r.segment != 1 {
            return Err(Error::Corrupt("test segment"));
        }
        let start = usize::try_from(r.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes
            .get(start..start.checked_add(n).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("test extent"))
    }
}
impl WriteStore for Memory {
    fn allocate(&mut self, b: &[u8], a: usize) -> Result<Reference> {
        let start = self.bytes.len().next_multiple_of(a);
        self.bytes.resize(start, 0);
        self.bytes.extend_from_slice(b);
        self.allocations += 1;
        Ok(Reference {
            segment: 1,
            offset: start as u64,
        })
    }
    fn rewrite(&mut self, r: Reference, b: &[u8], a: usize) -> Result<Reference> {
        self.rewrites += 1;
        if r.offset >= self.barrier {
            let start = r.offset as usize;
            self.bytes[start..start + b.len()].copy_from_slice(b);
            Ok(r)
        } else {
            self.allocate(b, a)
        }
    }
}
fn p(a: Address, n: u8) -> Prefix {
    Prefix::normalize(a, n).unwrap()
}
fn rand(s: &mut u64) -> u64 {
    *s = s
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *s
}
fn bits(a: Address) -> Vec<bool> {
    let (n, w) = match a {
        Address::V4(n) => (u128::from(n), 32),
        Address::V6(n) => (n, 128),
    };
    (0..w).map(|i| n & (1u128 << (w - i - 1)) != 0).collect()
}
fn contains(a: Prefix, b: Prefix) -> bool {
    let x = bits(a.network());
    let y = bits(b.network());
    x.len() == y.len()
        && a.length() <= b.length()
        && x[..a.length() as usize] == y[..a.length() as usize]
}
fn owned(x: Option<(Prefix, &[u8])>) -> Option<(Prefix, Vec<u8>)> {
    x.map(|(p, v)| (p, v.to_vec()))
}
fn verify<S: ReadStore + ?Sized>(
    s: &S,
    root: Reference,
    map: &BTreeMap<Prefix, Vec<u8>>,
    query: Prefix,
) {
    let best = |q: Prefix| {
        map.iter()
            .filter(|(k, _)| contains(**k, q))
            .max_by_key(|(k, _)| k.length())
            .map(|(k, v)| (*k, v.clone()))
    };
    assert_eq!(
        owned(exact(s, root, query).unwrap()),
        map.get(&query).map(|v| (query, v.clone()))
    );
    assert_eq!(owned(covering(s, root, query).unwrap()), best(query));
    let full = p(
        query.network(),
        if matches!(query.network(), Address::V4(_)) {
            32
        } else {
            128
        },
    );
    assert_eq!(
        owned(longest_match(s, root, query.network()).unwrap()),
        best(full)
    );
    let mut want: Vec<_> = map
        .iter()
        .filter(|(k, _)| contains(**k, query))
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    want.sort_by_key(|(k, _)| std::cmp::Reverse(k.length()));
    assert_eq!(
        ancestors(s, root, query)
            .unwrap()
            .map(|x| {
                let (k, v) = x.unwrap();
                (k, v.to_vec())
            })
            .collect::<Vec<_>>(),
        want
    );
    for selection in [Selection::Subnets, Selection::Overlaps] {
        let mut want: Vec<_> = map
            .iter()
            .filter(|(k, _)| {
                contains(query, **k)
                    || matches!(selection, Selection::Overlaps) && contains(**k, query)
            })
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        want.sort_by_key(|(k, _)| (bits(k.network()), k.length()));
        let got = Cursor::new(s, root, query, selection, Budget::default())
            .unwrap()
            .map(|x| {
                let (k, v) = x.unwrap();
                (k, v.to_vec())
            })
            .collect::<Vec<_>>();
        assert_eq!(got, want);
    }
}
#[test]
fn every_prefix_length_branch_bit_and_query_operation_matches_scanning_bitvectors() {
    for family in [4, 6] {
        let mut s = Memory::default();
        let mut root = Reference::NULL;
        let mut map = BTreeMap::new();
        let mut seed = 42;
        let width = if family == 4 { 32 } else { 128 };
        for i in 0..700 {
            let n = rand(&mut seed);
            let address = if family == 4 {
                Address::V4(n as u32)
            } else {
                Address::V6((u128::from(n) << 64) | u128::from(rand(&mut seed)))
            };
            let key = p(address, (i % (width + 1)) as u8);
            let value = if i % 29 == 0 {
                Vec::new()
            } else {
                (i as u64).to_le_bytes().to_vec()
            };
            root = insert(&mut s, root, key, &value).unwrap();
            map.insert(key, value);
            if i % 31 == 0 {
                verify(&s, root, &map, key);
            }
        }
        for i in 0..150 {
            let n = rand(&mut seed);
            let q = p(
                if family == 4 {
                    Address::V4(n as u32)
                } else {
                    Address::V6((u128::from(n) << 64) | u128::from(rand(&mut seed)))
                },
                (i % (width + 1)) as u8,
            );
            verify(&s, root, &map, q);
        }
        let snapshot = s.clone();
        let old = root;
        let oldmap = map.clone();
        s.barrier = s.bytes.len() as u64;
        let keys: Vec<_> = map.keys().copied().collect();
        for (k, key) in keys.into_iter().enumerate() {
            if k % 2 == 0 {
                root = delete(&mut s, root, key).unwrap();
                map.remove(&key);
            } else {
                root = insert(&mut s, root, key, &[9, 8, 7]).unwrap();
                map.insert(key, vec![9, 8, 7]);
            }
            if k % 47 == 0 {
                verify(&s, root, &map, key);
                verify(&snapshot, old, &oldmap, key);
            }
        }
        for key in map.keys().copied().collect::<Vec<_>>() {
            root = delete(&mut s, root, key).unwrap();
        }
        assert!(root.is_null());
        assert!(s.rewrites > 0);
    }
}
#[test]
fn stride_slot_boundaries_compression_idempotence_and_old_root_are_real() {
    let mut s = Memory::default();
    let a = p(Address::V4(0x12345678), 32);
    let b = p(Address::V4(0x92345678), 32);
    let mut root = insert(&mut s, Reference::NULL, a, &[]).unwrap();
    let before = s.allocations;
    assert_eq!(insert(&mut s, root, a, &[]).unwrap(), root);
    assert_eq!(s.allocations, before);
    let leaf = root;
    root = insert(&mut s, root, b, &[1]).unwrap();
    let n = Node::read(&s, root, &mut Visits::new(Budget::default())).unwrap();
    assert_eq!(n.base.length(), 0);
    assert_eq!(n.masks(), (0, 0x0202));
    root = delete(&mut s, root, b).unwrap();
    assert_eq!(root, leaf);
    for n in 0..=32 {
        let key = p(Address::V4(0x12345678), n);
        root = insert(&mut s, root, key, &[n]).unwrap();
    }
    let ancestors = ancestors(&s, root, a)
        .unwrap()
        .map(|x| x.unwrap().0.length())
        .collect::<Vec<_>>();
    assert_eq!(ancestors, (0..=32).rev().collect::<Vec<_>>());
    assert_eq!(
        Reader::with_budget(&s, root, Budget { max_nodes: 0 })
            .err()
            .map(|e| matches!(e, Error::LimitExceeded)),
        Some(true)
    );
    assert!(matches!(
        Reader::with_budget(&s, root, Budget { max_nodes: 8 })
            .unwrap()
            .longest_match(a.network()),
        Err(Error::LimitExceeded)
    ));
    assert!(matches!(
        exact(&s, root, p(Address::V6(0), 0)),
        Err(Error::InvalidInput(_))
    ));
    let mut cursor = Cursor::new(
        &s,
        root,
        p(Address::V4(0), 0),
        Selection::Overlaps,
        Budget { max_nodes: 1 },
    )
    .unwrap();
    assert!(cursor.any(|r| r.is_err()));
    assert!(cursor.next().is_none());
}
#[test]
fn wire_header_packed_refs_child_depth_and_budget_corruption_reject() {
    let mut s = Memory::default();
    let mut root = Reference::NULL;
    for n in 0..32 {
        root = insert(&mut s, root, p(Address::V4(n << 24), 8), &[n as u8]).unwrap();
    }
    let original = s.clone();
    let at = root.offset as usize;
    let size = Node::read(&s, root, &mut Visits::new(Budget::default()))
        .unwrap()
        .len();
    assert!(size >= HEADER);
    for index in 0..HEADER {
        let mut bad = original.clone();
        bad.bytes[at + index] ^= 0x80;
        if Reader::new(&bad, root).is_ok() {
            assert!(
                overlaps(&bad, root, p(Address::V4(0), 0))
                    .unwrap()
                    .any(|v| v.is_err())
                    || (16..32).contains(&index)
            );
        }
    }
    // Explicit malformed null, alignment, counts and length, independent of arbitrary valid alternative headers.
    for (index, bytes) in [
        (0, 0u16.to_le_bytes().to_vec()),
        (4, 64u32.to_le_bytes().to_vec()),
        (9, vec![1]),
        (12, 0x8000u16.to_le_bytes().to_vec()),
        (32, 99u16.to_le_bytes().to_vec()),
        (36, vec![1]),
        (HEADER, 0u64.to_le_bytes().to_vec()),
    ] {
        let mut bad = original.clone();
        bad.bytes[at + index..at + index + bytes.len()].copy_from_slice(&bytes);
        assert!(Reader::new(&bad, root).is_err());
    }
    let mut bad = original.clone();
    let node = Node::read(&bad, root, &mut Visits::new(Budget::default())).unwrap();
    let child = node.children.into_iter().find(|r| !r.is_null()).unwrap();
    let childat = child.offset as usize;
    bad.bytes[childat + 9] = 0;
    bad.bytes[childat + 16..childat + 32].fill(0);
    assert!(
        overlaps(&bad, root, p(Address::V4(0), 0))
            .unwrap()
            .any(|x| x.is_err())
    );
}
#[test]
fn mapped_segments_old_borrow_private_rewrite_abort_and_reopen_on_disk_and_ram() {
    use ulmdb_storage::{Arena, SegmentTable};
    for path in [std::env::temp_dir(), std::path::PathBuf::from("/dev/shm")] {
        let dir = path.join(format!(
            "multibit-prototype-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        ulmdb_storage::create_layout(&dir).unwrap();
        let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
        let mut root = Reference::NULL;
        let mut map = BTreeMap::new();
        for i in 0..2048u32 {
            let key = p(Address::V4(i.wrapping_mul(2654435761)), 32);
            let value = i.to_le_bytes().to_vec();
            root = insert(&mut arena, root, key, &value).unwrap();
            map.insert(key, value);
        }
        let oldroot = root;
        let (table, _) = arena.seal(&dir).unwrap();
        let key = *map.keys().nth(600).unwrap();
        let oldvalue = exact(&table, oldroot, key).unwrap().unwrap().1;
        let oldbytes = oldvalue.to_vec();
        let mut writer = Arena::new(table.clone(), 2).unwrap();
        let first = writer.dirty_bytes();
        root = insert(&mut writer, root, key, &[7, 6, 5, 4]).unwrap();
        let changed = writer.dirty_bytes() - first;
        assert!(
            changed < 33 * wire::MAX_SIZE + 128,
            "one-path allocation bound {changed}"
        );
        let fixed = writer.dirty_bytes();
        for _ in 0..100 {
            root = insert(&mut writer, root, key, &[8, 7, 6, 5]).unwrap();
        }
        assert_eq!(
            writer.dirty_bytes(),
            fixed,
            "equal-size private payload replacement"
        );
        map.insert(key, vec![8, 7, 6, 5]);
        assert_eq!(oldvalue, oldbytes);
        verify(
            &table,
            oldroot,
            &{
                let mut old = map.clone();
                old.insert(key, oldbytes.clone());
                old
            },
            key,
        );
        let checkpoint = writer.checkpoint().unwrap();
        let saved = root;
        root = delete(&mut writer, root, key).unwrap();
        assert!(exact(&writer, root, key).unwrap().is_none());
        writer.rollback_checkpoint(&checkpoint).unwrap();
        root = saved;
        assert_eq!(exact(&writer, root, key).unwrap().unwrap().1, &[8, 7, 6, 5]);
        let (table, _) = writer.seal(&dir).unwrap();
        let entries = table.entries();
        let reopened = SegmentTable::load(&dir, &entries).unwrap();
        verify(&reopened, root, &map, key);
        assert_eq!(oldvalue, oldbytes);
        let escaped = table.clone();
        let worker = std::thread::spawn(move || {
            let count = subnets(&escaped, root, p(Address::V4(0), 0))
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .len();
            assert_eq!(count, 2048);
        });
        worker.join().unwrap();
        let mut aborted = Arena::new(table.clone(), 3).unwrap();
        let dropped = delete(&mut aborted, root, key).unwrap();
        assert!(exact(&aborted, dropped, key).unwrap().is_none());
        drop(aborted);
        assert_eq!(exact(&table, root, key).unwrap().unwrap().1, &[8, 7, 6, 5]);
        let longkey = p(Address::V6(u128::MAX), 128);
        let mut big = Arena::new(SegmentTable::default(), 4).unwrap();
        let huge = vec![0xa5; 65536];
        let bigroot = insert(&mut big, Reference::NULL, longkey, &huge).unwrap();
        assert_eq!(exact(&big, bigroot, longkey).unwrap().unwrap().1, huge);
        let bigroot = insert(&mut big, bigroot, longkey, &[]).unwrap();
        assert_eq!(exact(&big, bigroot, longkey).unwrap().unwrap().1, &[]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
#[test]
fn every_within_stride_bitmap_and_terminal_slot_matches_existing_reference_model() {
    use ulmdb_reference::{ModelPrefix, PrefixStore};
    for family in [4, 6] {
        let width = if family == 4 { 32 } else { 128 };
        for depth in (0..width).step_by(4) {
            let mut memory = Memory::default();
            let mut root = Reference::NULL;
            let mut oracle = PrefixStore::default();
            let mut map = BTreeMap::new();
            let base = p(
                if family == 4 {
                    Address::V4(0xa56b7c9d)
                } else {
                    Address::V6(0xa56b7c9da56b7c9da56b7c9da56b7c9d)
                },
                depth,
            );
            let node = Node::empty(base);
            for i in 0..15 {
                let key = node.prefix(i);
                let value = vec![i as u8];
                root = insert(&mut memory, root, key, &value).unwrap();
                oracle.insert(
                    ModelPrefix::normalize(key.network(), key.length()).unwrap(),
                    value.clone(),
                );
                map.insert(key, value);
            }
            for side in 0..16 {
                let key = node.region(side);
                let value = vec![0xf0, side as u8];
                root = insert(&mut memory, root, key, &value).unwrap();
                oracle.insert(
                    ModelPrefix::normalize(key.network(), key.length()).unwrap(),
                    value.clone(),
                );
                map.insert(key, value);
            }
            let decoded = Node::read(&memory, root, &mut Visits::new(Budget::default())).unwrap();
            assert_eq!(decoded.masks(), (0x7fff, 0xffff));
            assert_eq!(decoded.len(), 560);
            for side in 0..16 {
                let query = node.region(side);
                let m = ModelPrefix::normalize(query.network(), query.length()).unwrap();
                assert_eq!(
                    exact(&memory, root, query).unwrap().map(|(_, v)| v),
                    oracle.exact(&m)
                );
                let expected = oracle.longest_match(query.network()).map(|(p, v)| {
                    (
                        Prefix::new(p.network(), p.length() as u8).unwrap(),
                        v.to_vec(),
                    )
                });
                assert_eq!(
                    owned(longest_match(&memory, root, query.network()).unwrap()),
                    expected
                );
            }
            verify(&memory, root, &map, base);
        }
    }
}
#[test]
fn new_wire_is_explicit_and_never_silently_reinterprets_binary_roots() {
    let mut memory = Memory::default();
    let key = p(Address::V6(0x80000000000000000000000000000000), 1);
    let binary = crate::insert(&mut memory, Reference::NULL, key, b"binary").unwrap();
    assert!(Reader::new(&memory, binary).is_err());
    let multibit = insert(&mut memory, Reference::NULL, key, b"multibit").unwrap();
    assert!(crate::Reader::new(&memory, multibit).is_err());
    assert_eq!(
        crate::exact(&memory, binary, key).unwrap().unwrap().1,
        b"binary"
    );
    assert_eq!(
        exact(&memory, multibit, key).unwrap().unwrap().1,
        b"multibit"
    );
}
#[test]
fn maximum_depth_dense_frontiers_fit_bounded_dfs_without_dropping_results() {
    let mut s = Memory::default();
    let mut root = Reference::NULL;
    let mut map = BTreeMap::new();
    for depth in (0..128).step_by(4) {
        let node = Node::empty(p(Address::V6(0), depth));
        for i in 0..15 {
            let key = node.prefix(i);
            root = insert(&mut s, root, key, &[i as u8]).unwrap();
            map.insert(key, vec![i as u8]);
        }
        for i in 0..16 {
            let key = node.region(i);
            root = insert(&mut s, root, key, &[i as u8]).unwrap();
            map.insert(key, vec![i as u8]);
        }
    }
    verify(&s, root, &map, p(Address::V6(0), 0));
    verify(&s, root, &map, p(Address::V6(0), 128));
}

mod borrowed;
