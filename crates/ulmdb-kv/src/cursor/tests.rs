use super::{Cursor, InOrderPosition};
use crate::page::{PAGE_SIZE, encode_v1};
use std::{cell::RefCell, collections::BTreeMap, ops::Bound};
use ulmdb_core::storage::{Error, ReadStore, Reference, Result};

#[derive(Default)]
struct Store {
    bytes: Vec<u8>,
    page_reads: RefCell<BTreeMap<Reference, usize>>,
}

impl ReadStore for Store {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("cursor test segment"));
        }
        if length == PAGE_SIZE {
            *self.page_reads.borrow_mut().entry(at).or_default() += 1;
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes
            .get(start..start.checked_add(length).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("cursor test extent"))
    }
}

impl Store {
    fn page(&mut self, level: u16, first: Reference, cells: &[Vec<u8>]) -> Reference {
        let cells: Vec<_> = cells.iter().map(Vec::as_slice).collect();
        let bytes = encode_v1(level, first, &cells).unwrap();
        let at = Reference {
            segment: 1,
            offset: self.bytes.len() as u64,
        };
        self.bytes.extend_from_slice(&bytes);
        at
    }

    fn leaf(&mut self, ids: &[u64]) -> Reference {
        let cells: Vec<_> = ids
            .iter()
            .map(|id| {
                let mut cell = vec![0; 24];
                cell[..2].copy_from_slice(&8u16.to_le_bytes());
                cell[4..8].copy_from_slice(&8u32.to_le_bytes());
                cell[8..16].copy_from_slice(&id.to_be_bytes());
                cell[16..24].copy_from_slice(&id.to_le_bytes());
                cell
            })
            .collect();
        self.page(0, Reference::NULL, &cells)
    }

    fn branch(&mut self, level: u16, first: Reference, edges: &[(u64, Reference)]) -> Reference {
        let cells: Vec<_> = edges
            .iter()
            .map(|(id, child)| {
                let mut cell = vec![0; 28];
                cell[..2].copy_from_slice(&8u16.to_le_bytes());
                cell[4..20].copy_from_slice(&child.encode());
                cell[20..28].copy_from_slice(&id.to_be_bytes());
                cell
            })
            .collect();
        self.page(level, first, &cells)
    }
}

// Empty children retain their separator ranges, as in a sparsely deleted tree.
fn sparse_tree() -> (Store, Reference, Vec<Reference>, Vec<u64>) {
    let mut store = Store::default();
    let leaves = [
        &[][..],
        &[10, 11, 12],
        &[],
        &[30, 31],
        &[],
        &[40, 41, 42, 43],
        &[],
    ]
    .map(|ids| store.leaf(ids));
    let left = store.branch(1, leaves[0], &[(10, leaves[1]), (20, leaves[2])]);
    let right = store.branch(
        1,
        leaves[3],
        &[(35, leaves[4]), (40, leaves[5]), (50, leaves[6])],
    );
    let root = store.branch(2, left, &[(30, right)]);
    (
        store,
        root,
        leaves.to_vec(),
        vec![10, 11, 12, 30, 31, 40, 41, 42, 43],
    )
}

fn id(pair: (&[u8], &[u8])) -> u64 {
    let key = u64::from_be_bytes(pair.0.try_into().unwrap());
    assert_eq!(pair.1, key.to_le_bytes());
    key
}

#[test]
fn each_direction_loads_each_leaf_once_across_empty_children_and_branch_boundaries() {
    let (store, root, leaves, expected) = sparse_tree();
    for reverse in [false, true] {
        store.page_reads.borrow_mut().clear();
        let mut cursor =
            Cursor::new(&store, root, Bound::Unbounded, Bound::Unbounded, reverse).unwrap();
        let first = cursor.next().unwrap().unwrap();
        let mut actual = vec![id(first)];
        actual.extend(cursor.by_ref().map(|pair| id(pair.unwrap())));
        assert!(cursor.next().is_none());
        drop(cursor);
        // Results borrow the store, independently of cursor lifetime and page changes.
        assert_eq!(id(first), if reverse { 43 } else { 10 });
        let mut want = expected.clone();
        if reverse {
            want.reverse();
        }
        assert_eq!(actual, want);
        let reads = store.page_reads.borrow();
        for leaf in &leaves {
            assert_eq!(reads.get(leaf), Some(&1), "leaf {leaf:?}");
        }
    }
}

fn bound(value: Option<(u64, bool)>) -> Bound<Vec<u8>> {
    match value {
        Some((id, true)) => Bound::Included(id.to_be_bytes().to_vec()),
        Some((id, false)) => Bound::Excluded(id.to_be_bytes().to_vec()),
        None => Bound::Unbounded,
    }
}

fn borrowed(bound: &Bound<Vec<u8>>) -> Bound<&[u8]> {
    match bound {
        Bound::Included(value) => Bound::Included(value),
        Bound::Excluded(value) => Bound::Excluded(value),
        Bound::Unbounded => Bound::Unbounded,
    }
}

#[test]
fn bounded_seeks_match_an_independent_filter_in_both_directions() {
    let (store, root, _, expected) = sparse_tree();
    for (lower, upper) in [
        (Some((10, true)), Some((43, true))),
        (Some((10, false)), Some((43, false))),
        (Some((12, false)), Some((40, true))),
        (Some((20, true)), Some((35, false))),
        (Some((35, false)), Some((50, false))),
        (Some((0, true)), Some((9, true))),
        (Some((44, true)), None),
        (None, Some((9, true))),
        (Some((40, true)), Some((30, true))),
        (Some((40, true)), Some((40, true))),
        (Some((40, false)), Some((40, true))),
    ] {
        let expected: Vec<_> = expected
            .iter()
            .copied()
            .filter(|id| {
                lower.is_none_or(|(value, included)| *id > value || included && *id == value)
                    && upper.is_none_or(|(value, included)| *id < value || included && *id == value)
            })
            .collect();
        let (lower, upper) = (bound(lower), bound(upper));
        for reverse in [false, true] {
            let actual = Cursor::new(&store, root, borrowed(&lower), borrowed(&upper), reverse)
                .unwrap()
                .map(|pair| id(pair.unwrap()))
                .collect::<Vec<_>>();
            let mut want = expected.clone();
            if reverse {
                want.reverse();
            }
            assert_eq!(
                actual, want,
                "bounds {lower:?}, {upper:?}; reverse={reverse}"
            );
        }
    }
}

#[test]
fn reference_only_position_reacquires_leaf_from_each_supplied_scope() {
    struct Scope<'a>(&'a Store);
    impl ReadStore for Scope<'_> {
        fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
            self.0.read(at, length)
        }
    }
    let mut store = Store::default();
    let root = store.leaf(&[10, 11, 12]);
    let mut position = InOrderPosition::new(root);
    for expected in [10, 11, 12] {
        let scope = Scope(&store);
        assert_eq!(id(position.next(&scope).unwrap().unwrap()), expected);
        assert_eq!(
            store.page_reads.borrow().get(&root),
            Some(&((expected - 9) as usize))
        );
    }
    let scope = Scope(&store);
    assert!(position.next(&scope).unwrap().is_none());
    assert_eq!(store.page_reads.borrow().get(&root), Some(&4));
    assert!(position.next(&scope).unwrap().is_none());
    assert_eq!(store.page_reads.borrow().get(&root), Some(&4));
}

#[test]
fn cached_leaf_still_checks_each_cell_and_fuses_after_corruption() {
    let mut store = Store::default();
    let root = store.leaf(&[10, 11]);
    // Header and first cell are valid; only the second slot is malformed.
    store.bytes[root.offset as usize + 34..root.offset as usize + 36]
        .copy_from_slice(&3u16.to_le_bytes());
    let mut cursor = Cursor::new(&store, root, Bound::Unbounded, Bound::Unbounded, false).unwrap();
    assert_eq!(id(cursor.next().unwrap().unwrap()), 10);
    assert!(matches!(cursor.next(), Some(Err(Error::Corrupt(_)))));
    assert!(cursor.next().is_none());
    assert_eq!(store.page_reads.borrow().get(&root), Some(&1));
}

#[test]
fn crossing_a_page_boundary_checks_the_new_page_before_returning_rows() {
    let (mut store, root, leaves, _) = sparse_tree();
    let damaged = leaves[3].offset as usize;
    store.bytes[damaged + 2..damaged + 4].copy_from_slice(&255u16.to_le_bytes());
    let mut cursor = Cursor::new(&store, root, Bound::Unbounded, Bound::Unbounded, false).unwrap();
    for expected in [10, 11, 12] {
        assert_eq!(id(cursor.next().unwrap().unwrap()), expected);
    }
    assert!(matches!(cursor.next(), Some(Err(Error::Corrupt(_)))));
    assert!(cursor.next().is_none());
}
