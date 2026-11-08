use super::*;
use crate::page::{PAGE_SIZE, encode_v1};
use std::{cell::Cell, ops::Bound};

#[derive(Default)]
struct Store {
    bytes: Vec<u8>,
    page_reads: Cell<usize>,
}
impl ReadStore for Store {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("scoped position test segment"));
        }
        if length == PAGE_SIZE {
            self.page_reads.set(self.page_reads.get() + 1);
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes
            .get(start..start.checked_add(length).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("scoped position test extent"))
    }
}
impl Store {
    fn page(&mut self, level: u16, first: Reference, cells: &[Vec<u8>]) -> Reference {
        let rows: Vec<_> = cells.iter().map(Vec::as_slice).collect();
        let encoded = encode_v1(level, first, &rows).unwrap();
        let at = Reference { segment: 1, offset: self.bytes.len() as u64 };
        self.bytes.extend_from_slice(&encoded);
        at
    }
    fn leaf(&mut self, ids: &[u64]) -> Reference {
        let cells: Vec<_> = ids.iter().map(|id| {
            let mut cell = vec![0; 24];
            cell[..2].copy_from_slice(&8u16.to_le_bytes());
            cell[4..8].copy_from_slice(&8u32.to_le_bytes());
            cell[8..16].copy_from_slice(&id.to_be_bytes());
            cell[16..24].copy_from_slice(&id.to_le_bytes());
            cell
        }).collect();
        self.page(0, Reference::NULL, &cells)
    }
    fn branch(&mut self, left: Reference, separator: u64, right: Reference) -> Reference {
        let mut cell = vec![0; 28];
        cell[..2].copy_from_slice(&8u16.to_le_bytes());
        cell[4..20].copy_from_slice(&right.encode());
        cell[20..28].copy_from_slice(&separator.to_be_bytes());
        self.page(1, left, &[cell])
    }
}
fn key(pair: (&[u8], &[u8])) -> u64 {
    let key = u64::from_be_bytes(pair.0.try_into().unwrap());
    assert_eq!(pair.1, key.to_le_bytes());
    key
}

#[test]
fn scoped_step_loads_once_keeps_ahead_candidate_and_retains_row_borrows() {
    let mut store = Store::default();
    let root = store.leaf(&[10, 20, 30]);
    let mut position = CursorPosition::new(root);
    let retained = position.current_or_step(&store, Bound::Unbounded, false).unwrap().unwrap();
    assert_eq!(key(retained), 10);
    store.page_reads.set(0);
    assert_eq!(key(position.current_or_step(&store, Bound::Excluded(&10u64.to_be_bytes()), false).unwrap().unwrap()), 20);
    assert_eq!(store.page_reads.get(), 1);
    // A catalog-only bookmark can lag the already-found native candidate.
    store.page_reads.set(0);
    assert_eq!(key(position.current_or_step(&store, Bound::Excluded(&15u64.to_be_bytes()), false).unwrap().unwrap()), 20);
    assert_eq!(store.page_reads.get(), 1);
    assert_eq!(key(position.current_or_step(&store, Bound::Included(&20u64.to_be_bytes()), true).unwrap().unwrap()), 20);
    assert_eq!(key(position.current_or_step(&store, Bound::Excluded(&20u64.to_be_bytes()), true).unwrap().unwrap()), 10);
    assert!(position.current_or_step(&store, Bound::Excluded(&10u64.to_be_bytes()), true).unwrap().is_none());
    assert_eq!(key(position.current(&store).unwrap().unwrap()), 10);
    assert_eq!(key(position.current_or_step(&store, Bound::Excluded(&10u64.to_be_bytes()), false).unwrap().unwrap()), 20);
    assert_eq!(key(position.current_or_step(&store, Bound::Excluded(&20u64.to_be_bytes()), false).unwrap().unwrap()), 30);
    assert!(position.current_or_step(&store, Bound::Excluded(&30u64.to_be_bytes()), false).unwrap().is_none());
    assert_eq!(key(position.current(&store).unwrap().unwrap()), 30);
    assert_eq!(key(position.current_or_step(&store, Bound::Excluded(&30u64.to_be_bytes()), true).unwrap().unwrap()), 20);
    drop(position);
    assert_eq!(key(retained), 10);
}

#[test]
fn scoped_unpositioned_seek_handles_absent_and_inclusive_bounds_in_both_directions() {
    let mut store = Store::default();
    let root = store.leaf(&[10, 20, 30]);
    for (id, inclusive, reverse, expected) in [
        (15u64, true, false, Some(20)),
        (20, true, false, Some(20)),
        (20, false, false, Some(30)),
        (15, true, true, Some(10)),
        (20, true, true, Some(20)),
        (20, false, true, Some(10)),
        (30, false, false, None),
    ] {
        let bytes = id.to_be_bytes();
        let bound = if inclusive { Bound::Included(bytes.as_slice()) } else { Bound::Excluded(bytes.as_slice()) };
        let mut position = CursorPosition::new(root);
        assert_eq!(position.current_or_step(&store, bound, reverse).unwrap().map(key), expected);
    }
    let mut position = CursorPosition::new(root);
    assert_eq!(key(position.current_or_step(&store, Bound::Unbounded, false).unwrap().unwrap()), 10);
    store.page_reads.set(0);
    // Positioned continuations try one neighbor, rather than search for 99.
    assert!(position.current_or_step(&store, Bound::Included(&99u64.to_be_bytes()), false).unwrap().is_none());
    assert_eq!(store.page_reads.get(), 1);
    assert_eq!(key(position.current(&store).unwrap().unwrap()), 20);
}

#[test]
fn scoped_step_checks_corrupt_current_value_before_advancing_and_preserves_position() {
    let mut store = Store::default();
    let root = store.leaf(&[10, 20]);
    let mut position = CursorPosition::new(root);
    position.seek(&store, Bound::Unbounded, false).unwrap();
    let cell = u16::from_le_bytes(store.bytes[32..34].try_into().unwrap()) as usize;
    store.bytes[cell + 4..cell + 8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(position.current_or_step(&store, Bound::Excluded(&10u64.to_be_bytes()), false), Err(Error::Corrupt(_))));
    assert_eq!(position.leaf, Some((root, 0)));
}

#[test]
fn scoped_step_checks_corrupt_next_cell_and_crossing_page_without_losing_old_position() {
    let mut store = Store::default();
    let root = store.leaf(&[10, 20]);
    store.bytes[34..36].copy_from_slice(&3u16.to_le_bytes());
    let mut position = CursorPosition::new(root);
    position.seek(&store, Bound::Unbounded, false).unwrap();
    assert!(matches!(position.current_or_step(&store, Bound::Excluded(&10u64.to_be_bytes()), false), Err(Error::Corrupt(_))));
    assert_eq!(position.leaf, Some((root, 0)));

    let mut store = Store::default();
    let left = store.leaf(&[10]);
    let right = store.leaf(&[20]);
    let root = store.branch(left, 20, right);
    let mut position = CursorPosition::new(root);
    position.seek(&store, Bound::Unbounded, false).unwrap();
    let old_stack = position.stack.clone();
    let start = right.offset as usize;
    store.bytes[start + 2..start + 4].copy_from_slice(&255u16.to_le_bytes());
    assert!(matches!(position.current_or_step(&store, Bound::Excluded(&10u64.to_be_bytes()), false), Err(Error::Corrupt(_))));
    assert_eq!(position.leaf, Some((left, 0)));
    assert_eq!(position.stack, old_stack);
}
