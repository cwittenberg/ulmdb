use super::*;
use crate::{Cursor, PAGE_SIZE, audit_tree, delete, get, put};
use std::{collections::BTreeMap, ops::Bound};
use ulmdb_core::storage::ReadStore;
#[derive(Default)]
struct Arena {
    bytes: Vec<u8>,
    pages: Vec<Reference>,
    rewrites: usize,
    allocations: usize,
    fail: Option<usize>,
}
impl ReadStore for Arena {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("test segment"));
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes
            .get(start..start.checked_add(length).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("test extent"))
    }
}
impl WriteStore for Arena {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        if self.fail == Some(self.allocations) {
            return Err(Error::Io(std::io::Error::other("injected allocation")));
        }
        if self.bytes.is_empty() {
            self.bytes.resize(4096, 0);
        }
        let at = self.bytes.len().div_ceil(alignment) * alignment;
        self.bytes.resize(at, 0);
        self.bytes.extend_from_slice(bytes);
        self.allocations += 1;
        let reference = Reference {
            segment: 1,
            offset: at as u64,
        };
        if alignment == PAGE_SIZE && bytes.len() == PAGE_SIZE {
            self.pages.push(reference);
        }
        Ok(reference)
    }
    fn rewrite(&mut self, _old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.rewrites += 1;
        self.allocate(bytes, alignment)
    }
}
fn check(arena: &Arena, built: BulkBuild, expected: &BTreeMap<Vec<u8>, Vec<u8>>) {
    assert_eq!(built.records, expected.len() as u64);
    let actual: Vec<_> = Cursor::new(arena, built.root, Bound::Unbounded, Bound::Unbounded, false)
        .unwrap()
        .map(|pair| pair.map(|(k, v)| (k.to_vec(), v.to_vec())).unwrap())
        .collect();
    assert_eq!(
        actual,
        expected
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>()
    );
    let reverse: Vec<_> = Cursor::new(arena, built.root, Bound::Unbounded, Bound::Unbounded, true)
        .unwrap()
        .map(|pair| pair.map(|(k, v)| (k.to_vec(), v.to_vec())).unwrap())
        .collect();
    assert_eq!(reverse, actual.iter().rev().cloned().collect::<Vec<_>>());
    for (k, v) in expected {
        assert_eq!(get(arena, built.root, k).unwrap(), Some(v.as_slice()));
    }
    let audit = audit_tree(arena, built.root, u64::MAX).unwrap();
    assert_eq!(
        (
            audit.pages,
            audit.leaf_pages,
            audit.branch_pages,
            audit.leaf_records,
            audit.height
        ),
        (
            built.pages,
            built.leaf_pages,
            built.branch_pages,
            built.records,
            built.height
        )
    );
    assert!(built.frontier_levels <= 64);
    for at in &arena.pages {
        let page = crate::page::Page::load(arena, *at).unwrap();
        assert!(
            page.count > 0,
            "no empty leaves or unary internal output pages"
        );
    }
}
#[test]
fn sorted_builder_matches_ordered_oracle_for_empty_overflow_and_variable_records() {
    let mut expected = BTreeMap::new();
    expected.insert(Vec::new(), Vec::new());
    for n in 0..4000u64 {
        let value = match n % 7 {
            0 => Vec::new(),
            1 => vec![n as u8; 511],
            2 => vec![n as u8; 512],
            3 => vec![n as u8; 513],
            4 => vec![n as u8; 4097],
            _ => n.to_le_bytes().to_vec(),
        };
        expected.insert(n.to_be_bytes().to_vec(), value);
    }
    for length in [255, 256, 257, 4096] {
        expected.insert(vec![0xfe; length], vec![0x33; length]);
    }
    let mut arena = Arena::default();
    let built = build_sorted(
        &mut arena,
        expected
            .iter()
            .map(|(k, v)| Ok((k.as_slice(), v.as_slice()))),
    )
    .unwrap();
    assert_eq!(arena.rewrites, 0);
    check(&arena, built, &expected);
    fn borrowed(bound: &Bound<Vec<u8>>) -> Bound<&[u8]> {
        match bound {
            Bound::Unbounded => Bound::Unbounded,
            Bound::Included(key) => Bound::Included(key),
            Bound::Excluded(key) => Bound::Excluded(key),
        }
    }
    for (lower, upper) in [
        (
            Bound::Included(Vec::new()),
            Bound::Excluded(1000u64.to_be_bytes().to_vec()),
        ),
        (
            Bound::Excluded(1000u64.to_be_bytes().to_vec()),
            Bound::Included(vec![0xfe; 256]),
        ),
        (
            Bound::Included(vec![0xfe; 256]),
            Bound::Excluded(vec![0xfe; 257]),
        ),
        (Bound::Excluded(vec![0xfe; 257]), Bound::Unbounded),
    ] {
        let want: Vec<_> = expected
            .range((lower.clone(), upper.clone()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for reverse in [false, true] {
            let got: Vec<_> = Cursor::new(
                &arena,
                built.root,
                borrowed(&lower),
                borrowed(&upper),
                reverse,
            )
            .unwrap()
            .map(|row| row.map(|(k, v)| (k.to_vec(), v.to_vec())).unwrap())
            .collect();
            let want = if reverse {
                want.iter().rev().cloned().collect::<Vec<_>>()
            } else {
                want.clone()
            };
            assert_eq!(got, want);
        }
    }
    let previous = built.root;
    let root = put(&mut arena, previous, b"changed", b"new").unwrap();
    let root = delete(&mut arena, root, &0u64.to_be_bytes()).unwrap();
    assert_eq!(get(&arena, previous, b"changed").unwrap(), None);
    assert_eq!(get(&arena, root, b"changed").unwrap(), Some(&b"new"[..]));
    assert_eq!(
        get(&arena, previous, &0u64.to_be_bytes()).unwrap(),
        Some(&b""[..])
    );
    assert_eq!(get(&arena, root, &0u64.to_be_bytes()).unwrap(), None);
}
#[test]
fn suffix_boundaries_collapse_roots_and_avoid_single_child_internal_pages() {
    for count in [
        0,
        1,
        156,
        157,
        312,
        313,
        156 * 136,
        156 * 137,
        156 * 272 + 1,
    ] {
        let expected: BTreeMap<Vec<u8>, Vec<u8>> = (0..count as u64)
            .map(|n| (n.to_be_bytes().to_vec(), n.to_le_bytes().to_vec()))
            .collect();
        let mut arena = Arena::default();
        let built = build_sorted(&mut arena, expected.iter().map(|(k, v)| Ok((k, v)))).unwrap();
        assert_eq!(arena.rewrites, 0);
        check(&arena, built, &expected);
        if count == 0 {
            assert!(built.root.is_null());
            assert_eq!(
                (built.pages, built.height, built.frontier_levels),
                (0, 0, 0)
            );
        }
        if count <= 156 && count > 0 {
            assert_eq!((built.height, built.pages), (1, 1));
        }
    }
}
#[test]
fn duplicate_unsorted_iterator_and_store_errors_return_no_root_preserving_old_objects() {
    for input in [
        vec![(b"a".to_vec(), Vec::new()), (b"a".to_vec(), Vec::new())],
        vec![(b"b".to_vec(), Vec::new()), (b"a".to_vec(), Vec::new())],
    ] {
        let mut arena = Arena::default();
        assert!(matches!(
            build_sorted(&mut arena, input.into_iter().map(Ok)),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(arena.allocations, 0);
    }
    let mut arena = Arena::default();
    let old = put(&mut arena, Reference::NULL, b"old", b"immutable").unwrap();
    let original = arena.bytes.clone();
    let records = (0..2000u64)
        .map(|n| Ok((n.to_be_bytes(), n.to_le_bytes())))
        .chain(std::iter::once(Err(Error::Io(std::io::Error::other(
            "input failure",
        )))));
    assert!(matches!(
        build_sorted(&mut arena, records),
        Err(Error::Io(_))
    ));
    assert_eq!(&arena.bytes[..original.len()], original.as_slice());
    assert_eq!(get(&arena, old, b"old").unwrap(), Some(&b"immutable"[..]));
    arena.fail = Some(arena.allocations + 2);
    assert!(matches!(
        build_sorted(
            &mut arena,
            (0..3000u64).map(|n| Ok((n.to_be_bytes(), n.to_le_bytes())))
        ),
        Err(Error::Io(_))
    ));
    assert_eq!(&arena.bytes[..original.len()], original.as_slice());
    let mut empty = Arena::default();
    let key = vec![0; 4097];
    assert!(matches!(
        build_sorted(&mut empty, [Ok((key, Vec::<u8>::new()))]),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(empty.allocations, 0);
}

#[test]
fn maximum_inline_cells_balance_leaf_and_branch_suffixes_without_unary_pages() {
    for count in [6usize, 76, 151] {
        let expected: BTreeMap<Vec<u8>, Vec<u8>> = (0..count as u64)
            .map(|n| {
                let mut key = vec![0xa5; 256];
                key[..8].copy_from_slice(&n.to_be_bytes());
                (key, vec![0x5a; 512])
            })
            .collect();
        let mut arena = Arena::default();
        let built = build_sorted(&mut arena, expected.iter().map(|(k, v)| Ok((k, v)))).unwrap();
        check(&arena, built, &expected);
        assert_eq!(built.leaf_pages, count.div_ceil(5) as u64);
        assert_eq!(
            arena.allocations as u64, built.pages,
            "maximum-inline cells require no payload allocations"
        );
        assert_eq!(arena.rewrites, 0);
    }
}

#[test]
fn overflow_minimum_keys_route_through_real_branch_separators() {
    let expected: BTreeMap<Vec<u8>, Vec<u8>> = (0..2000u64)
        .map(|n| {
            let mut key = vec![0xa5; if n % 2 == 0 { 257 } else { 4096 }];
            key[..8].copy_from_slice(&n.to_be_bytes());
            (key, n.to_le_bytes().to_vec())
        })
        .collect();
    let mut arena = Arena::default();
    let built = build_sorted(&mut arena, expected.iter().map(|(k, v)| Ok((k, v)))).unwrap();
    check(&arena, built, &expected);
    assert_eq!(arena.rewrites, 0);
    assert!(built.height >= 2);
    let root = crate::page::Page::load(&arena, built.root).unwrap();
    for index in 0..root.count {
        let raw = root.raw_cell(index).unwrap();
        assert_eq!(crate::page::u16_at(raw, 0).unwrap(), u16::MAX);
        assert_eq!(crate::page::u16_at(raw, 2).unwrap() & 1, 1);
    }
}

#[test]
fn incremental_errors_are_terminal_even_when_caller_ignores_them() {
    for invalid in [b"a".as_slice(), b"".as_slice(), &vec![0x62; 4097]] {
        let mut arena = Arena::default();
        let mut builder = SortedBuilder::new();
        builder.push(&mut arena, b"a", b"accepted").unwrap();
        assert!(matches!(
            builder.push(&mut arena, invalid, b"bad"),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            builder.push(&mut arena, b"z", b"later"),
            Err(Error::InvalidInput("sorted builder is poisoned"))
        ));
        assert!(matches!(
            builder.finish(&mut arena),
            Err(Error::InvalidInput("sorted builder is poisoned"))
        ));
    }
    let mut arena = Arena::default();
    let old = put(&mut arena, Reference::NULL, b"old", b"immutable").unwrap();
    let before = arena.bytes.clone();
    let mut builder = SortedBuilder::new();
    for n in 0..1000u64 {
        builder.push(&mut arena, &n.to_be_bytes(), b"new").unwrap();
    }
    arena.fail = Some(arena.allocations);
    assert!(matches!(
        builder.push(&mut arena, b"z", &vec![0x55; 513]),
        Err(Error::Io(_))
    ));
    arena.fail = None;
    assert!(matches!(
        builder.finish(&mut arena),
        Err(Error::InvalidInput("sorted builder is poisoned"))
    ));
    assert_eq!(&arena.bytes[..before.len()], &before);
    assert_eq!(get(&arena, old, b"old").unwrap(), Some(&b"immutable"[..]));
    let mut builder = SortedBuilder::new();
    builder.push(&mut arena, b"a", b"").unwrap();
    arena.fail = Some(arena.allocations);
    assert!(matches!(builder.finish(&mut arena), Err(Error::Io(_))));
}

#[test]
fn duplicate_child_mode_preserves_65536_byte_keys_and_rejects_invalid_limits() {
    let mut expected = BTreeMap::new();
    expected.insert(Vec::new(), Vec::new());
    for n in 0..400u64 {
        let mut key = vec![0x42; 65_536];
        key[..8].copy_from_slice(&n.to_be_bytes());
        expected.insert(key, Vec::new());
    }
    expected.insert(vec![0xfe; 4096], Vec::new());
    expected.insert(vec![0xff; 65_535], Vec::new());
    expected.insert(vec![0xff; 65_536], Vec::new());
    let mut arena = Arena::default();
    let mut builder = SortedBuilder::duplicate_values();
    for (key, value) in &expected {
        builder.push(&mut arena, key, value).unwrap();
    }
    let built = builder.finish(&mut arena).unwrap();
    assert!(built.height >= 2);
    check(&arena, built, &expected);
    assert_eq!(arena.rewrites, 0);
    for (key, value) in [
        (&vec![0x33; 65_537][..], b"".as_slice()),
        (b"a".as_slice(), b"nonempty".as_slice()),
    ] {
        let mut invalid = SortedBuilder::duplicate_values();
        assert!(matches!(
            invalid.push(&mut arena, key, value),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            invalid.finish(&mut arena),
            Err(Error::InvalidInput("sorted builder is poisoned"))
        ));
    }
    let mut invalid = SortedBuilder::duplicate_values();
    invalid.push(&mut arena, &vec![0x55; 65_536], b"").unwrap();
    assert!(matches!(
        invalid.push(&mut arena, &vec![0x55; 65_536], b""),
        Err(Error::InvalidInput("duplicate bulk key"))
    ));
    assert!(invalid.finish(&mut arena).is_err());
    let mut normal = SortedBuilder::new();
    assert!(matches!(
        normal.push(&mut arena, &vec![0x22; 4097], b""),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn nested_incremental_builders_share_store_without_aliasing_or_rewriting() {
    let mut arena = Arena::default();
    let mut outer = SortedBuilder::new();
    let mut expected = BTreeMap::new();
    for n in 0..40u64 {
        let mut inner = SortedBuilder::duplicate_values();
        let values: BTreeMap<_, _> = (0..200u64)
            .map(|v| (v.to_be_bytes().to_vec(), Vec::new()))
            .collect();
        for (value, empty) in &values {
            inner.push(&mut arena, value, empty).unwrap();
        }
        let built = inner.finish(&mut arena).unwrap();
        check(&arena, built, &values);
        let bytes = built.root.encode();
        outer.push(&mut arena, &n.to_be_bytes(), &bytes).unwrap();
        expected.insert(n.to_be_bytes().to_vec(), bytes.to_vec());
    }
    let built = outer.finish(&mut arena).unwrap();
    check(&arena, built, &expected);
    assert_eq!(arena.rewrites, 0);
}
