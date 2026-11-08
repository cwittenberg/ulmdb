use super::audit_tree;
use crate::{Cursor, PAGE_SIZE, page::encode_v1 as encode};
use std::{cell::RefCell, collections::BTreeSet, ops::Bound};
use ulmdb_core::storage::{Error, ReadStore, Reference, Result, WriteStore};

#[derive(Default)]
struct Store {
    bytes: Vec<u8>,
    payloads: BTreeSet<Reference>,
    headers_read: RefCell<BTreeSet<Reference>>,
}
impl ReadStore for Store {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("audit test missing segment"));
        }
        if length == 16 && self.payloads.contains(&at) {
            self.headers_read.borrow_mut().insert(at);
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes
            .get(start..start.checked_add(length).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("audit test unreadable extent"))
    }
}
impl WriteStore for Store {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        let start = self.bytes.len().div_ceil(alignment) * alignment;
        self.bytes.resize(start, 0);
        self.bytes.extend_from_slice(bytes);
        let at = Reference {
            segment: 1,
            offset: start as u64,
        };
        if bytes.get(..8) == Some(&[1, 0, 0, 0, 16, 0, 0, 0]) {
            self.payloads.insert(at);
        }
        Ok(at)
    }
    fn rewrite(&mut self, _old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.allocate(bytes, alignment)
    }
}
fn empty(store: &mut Store) -> Reference {
    store
        .allocate(&encode(0, Reference::NULL, &[]).unwrap(), PAGE_SIZE)
        .unwrap()
}
fn branch(
    store: &mut Store,
    level: u16,
    first: Reference,
    child: Reference,
    key: Reference,
) -> Reference {
    let mut cell = vec![0xff, 0xff, 1, 0];
    cell.extend_from_slice(&child.encode());
    cell.extend_from_slice(&key.encode());
    store
        .allocate(&encode(level, first, &[&cell]).unwrap(), PAGE_SIZE)
        .unwrap()
}
fn leaf(store: &mut Store, keys: &[u8]) -> Reference {
    let mut records = Vec::new();
    for key in keys {
        let key = store.put_payload(&vec![*key; 300]).unwrap();
        let value = store.put_payload(&vec![0x88; 600]).unwrap();
        let mut raw = vec![0xff, 0xff, 3, 0];
        raw.extend_from_slice(&600u32.to_le_bytes());
        raw.extend_from_slice(&key.encode());
        raw.extend_from_slice(&value.encode());
        records.push(raw);
    }
    let cells: Vec<_> = records.iter().map(Vec::as_slice).collect();
    store
        .allocate(&encode(0, Reference::NULL, &cells).unwrap(), PAGE_SIZE)
        .unwrap()
}

#[test]
fn unreadable_branch_overflow_separator_with_empty_children_is_not_skipped() {
    for corrupt_header in [false, true] {
        let mut store = Store::default();
        let left = empty(&mut store);
        let right = empty(&mut store);
        let separator = if corrupt_header {
            let at = store.put_payload(b"middle").unwrap();
            store.bytes[at.offset as usize] = 0;
            at
        } else {
            Reference {
                segment: 99,
                offset: 4096,
            }
        };
        let root = branch(&mut store, 1, left, right, separator);
        // This demonstrates why leaf enumeration alone cannot authorize pruning:
        // neither empty leaf makes the old cursor dereference its separator.
        assert!(
            Cursor::new(&store, root, Bound::Unbounded, Bound::Unbounded, false)
                .unwrap()
                .next()
                .is_none()
        );
        assert!(matches!(
            audit_tree(&store, root, 3),
            Err(Error::Corrupt(_))
        ));
    }
}

#[test]
fn complete_multilevel_audit_touches_every_overflow_header_and_counts_cells() {
    let mut store = Store::default();
    let a = leaf(&mut store, b"ab");
    let b = leaf(&mut store, b"fg");
    let c = leaf(&mut store, b"mn");
    let d = leaf(&mut store, b"xz");
    let f = store.put_payload(&vec![b'f'; 300]).unwrap();
    let x = store.put_payload(&vec![b'x'; 300]).unwrap();
    let m = store.put_payload(&vec![b'm'; 300]).unwrap();
    let left = branch(&mut store, 1, a, b, f);
    let right = branch(&mut store, 1, c, d, x);
    let root = branch(&mut store, 2, left, right, m);
    let audit = audit_tree(&store, root, 7).unwrap();
    assert_eq!(audit.pages, 7);
    assert_eq!(audit.height, 3);
    assert_eq!(
        (
            audit.branch_pages,
            audit.leaf_pages,
            audit.leaf_records,
            audit.separators
        ),
        (3, 4, 8, 3)
    );
    assert_eq!((audit.overflow_keys, audit.overflow_values), (11, 8));
    assert_eq!(
        *store.headers_read.borrow(),
        store.payloads,
        "all nineteen distinct payload headers must be visited"
    );
    assert!(matches!(
        audit_tree(&store, root, 6),
        Err(Error::LimitExceeded)
    ));
}

#[test]
fn null_roots_depth_cycles_skipped_levels_and_explicit_alias_visit_budget() {
    let mut store = Store::default();
    assert_eq!(audit_tree(&store, Reference::NULL, 0).unwrap().pages, 0);
    let leaf = empty(&mut store);
    assert!(matches!(
        audit_tree(&store, leaf, 0),
        Err(Error::LimitExceeded)
    ));
    let separator = store.put_payload(b"middle").unwrap();
    let alias = branch(&mut store, 1, leaf, leaf, separator);
    assert!(matches!(
        audit_tree(&store, alias, 2),
        Err(Error::LimitExceeded)
    ));
    let audit = audit_tree(&store, alias, 3).unwrap();
    assert_eq!((audit.pages, audit.leaf_pages), (3, 2));
    let skipped = branch(&mut store, 2, leaf, leaf, separator);
    assert!(matches!(
        audit_tree(&store, skipped, 3),
        Err(Error::Corrupt(_))
    ));
    let cycle = empty(&mut store);
    let bytes = encode(1, cycle, &[]).unwrap();
    let start = cycle.offset as usize;
    store.bytes[start..start + PAGE_SIZE].copy_from_slice(&bytes);
    assert!(matches!(
        audit_tree(&store, cycle, 100),
        Err(Error::Corrupt(_))
    ));
    let mut deep = empty(&mut store);
    for level in 1..64 {
        deep = store
            .allocate(&encode(level, deep, &[]).unwrap(), PAGE_SIZE)
            .unwrap();
    }
    assert_eq!(audit_tree(&store, deep, 64).unwrap().height, 64);
    assert!(matches!(
        audit_tree(&store, deep, 63),
        Err(Error::LimitExceeded)
    ));
    store.bytes[deep.offset as usize + 4..deep.offset as usize + 6]
        .copy_from_slice(&64u16.to_le_bytes());
    assert!(matches!(
        audit_tree(&store, deep, 100),
        Err(Error::Corrupt(_))
    ));
}

#[test]
fn overlapping_cells_and_cross_page_separator_violations_are_rejected() {
    let mut store = Store::default();
    // A valid embedded second record lies wholly within the first record's
    // value. Both keys sort correctly, but physical cells must not overlap.
    let second = [2, 0, 0, 0, 0, 0, 0, 0, b'b', b'b'];
    let mut first = vec![2, 0, 0, 0, 10, 0, 0, 0, b'a', b'a'];
    first.extend_from_slice(&second);
    let mut bytes = encode(0, Reference::NULL, &[&first]).unwrap();
    let start = u16::from_le_bytes(bytes[32..34].try_into().unwrap());
    bytes[6..8].copy_from_slice(&2u16.to_le_bytes());
    bytes[8..10].copy_from_slice(&36u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&(start + 10).to_le_bytes());
    let root = store.allocate(&bytes, PAGE_SIZE).unwrap();
    assert!(matches!(
        audit_tree(&store, root, 1),
        Err(Error::Corrupt("overlapping tree cells"))
    ));
    let bad_left = leaf(&mut store, b"z");
    let empty_right = empty(&mut store);
    let separator = store.put_payload(&vec![b'm'; 300]).unwrap();
    let root = branch(&mut store, 1, bad_left, empty_right, separator);
    assert!(matches!(
        audit_tree(&store, root, 3),
        Err(Error::Corrupt(
            "audited key outside ancestor separator bounds"
        ))
    ));
}

struct RecordReads<'a> {
    store: &'a Store,
    calls: RefCell<Vec<(Reference, usize)>>,
}
impl ReadStore for RecordReads<'_> {
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        self.calls.borrow_mut().push((at, len));
        self.store.read(at, len)
    }
}

#[test]
fn leaf_visitor_keeps_exact_graph_reads_counts_and_overflow_records() {
    let mut store = Store::default();
    let mut root = Reference::NULL;
    let mut expected = std::collections::BTreeMap::new();
    for n in 0..512u32 {
        let key = n.to_be_bytes();
        let value = n.to_le_bytes();
        root = crate::put(&mut store, root, &key, &value).unwrap();
        expected.insert(key.to_vec(), value.to_vec());
    }
    let long_key = vec![0xfe; 300];
    let long_value = vec![0x5a; 5000];
    root = crate::put(&mut store, root, &long_key, &long_value).unwrap();
    expected.insert(long_key, long_value);
    let plain = RecordReads {
        store: &store,
        calls: RefCell::new(Vec::new()),
    };
    let observed = RecordReads {
        store: &store,
        calls: RefCell::new(Vec::new()),
    };
    let before = audit_tree(&plain, root, 10_000).unwrap();
    let mut rows = Vec::new();
    let after = super::audit_tree_with_leaf_visitor(&observed, root, 10_000, |k, v| {
        rows.push((k.to_vec(), v.to_vec()));
    })
    .unwrap();
    assert_eq!(before, after);
    assert_eq!(*plain.calls.borrow(), *observed.calls.borrow());
    assert_eq!(rows, expected.into_iter().collect::<Vec<_>>());
    assert!(after.overflow_keys > 0 && after.overflow_values > 0);
    assert_eq!(after.leaf_records, 513);
}

#[test]
fn observer_flags_do_not_short_circuit_later_graph_and_budget_errors() {
    let mut store = Store::default();
    let left = crate::put(&mut store, Reference::NULL, b"a", &vec![7; 4081]).unwrap();
    let right = Reference {
        segment: 2,
        offset: PAGE_SIZE as u64,
    };
    let separator = store.put_payload(&vec![b'm'; 300]).unwrap();
    let root = branch(&mut store, 1, left, right, separator);
    let mut oversized = false;
    let result = super::audit_tree_with_leaf_visitor(&store, root, 100, |_, value| {
        oversized |= value.len() > 4080;
    });
    assert!(oversized);
    assert!(matches!(
        result,
        Err(Error::Corrupt("audit test missing segment"))
    ));
    oversized = false;
    let result = super::audit_tree_with_leaf_visitor(&store, root, 2, |_, value| {
        oversized |= value.len() > 4080;
    });
    assert!(oversized);
    assert!(matches!(result, Err(Error::LimitExceeded)));
    let mut called = false;
    assert_eq!(
        super::audit_tree_with_leaf_visitor(&store, Reference::NULL, 0, |_, _| called = true)
            .unwrap(),
        super::TreeAudit::default()
    );
    assert!(!called);
}

fn inline_leaf_page(store: &mut Store, keys: &[Vec<u8>], compact: bool) -> Reference {
    let cells: Vec<_> = keys
        .iter()
        .map(|key| {
            let mut raw = vec![0; (8 + key.len()).next_multiple_of(2)];
            raw[..2].copy_from_slice(&(key.len() as u16).to_le_bytes());
            raw[8..8 + key.len()].copy_from_slice(key);
            raw
        })
        .collect();
    let refs: Vec<_> = cells.iter().map(Vec::as_slice).collect();
    let bytes = if compact {
        crate::compact::encode(
            0,
            Reference::NULL,
            &refs,
            crate::compact::Layout {
                key: keys.first().map_or(0, Vec::len),
                value: 0,
            },
        )
        .unwrap()
    } else {
        encode(0, Reference::NULL, &refs).unwrap()
    };
    store.allocate(&bytes, PAGE_SIZE).unwrap()
}
fn inline_branch_page(
    store: &mut Store,
    left: Reference,
    right: Reference,
    separator: &[u8],
    compact: bool,
) -> Reference {
    let mut cell = vec![0; (20 + separator.len()).next_multiple_of(2)];
    cell[..2].copy_from_slice(&(separator.len() as u16).to_le_bytes());
    cell[4..20].copy_from_slice(&right.encode());
    cell[20..20 + separator.len()].copy_from_slice(separator);
    let bytes = if compact {
        crate::compact::encode(
            1,
            left,
            &[&cell],
            crate::compact::Layout {
                key: separator.len(),
                value: 0,
            },
        )
        .unwrap()
    } else {
        encode(1, left, &[&cell]).unwrap()
    };
    store.allocate(&bytes, PAGE_SIZE).unwrap()
}

#[test]
fn word_ancestor_bounds_keep_inclusive_lower_and_exclusive_upper() {
    for compact in [false, true] {
        for separator in [
            1u64,
            0xff,
            0x100,
            0xffff,
            0x10000,
            0x7fff_ffff_ffff_ffff,
            0x8000_0000_0000_0000,
            u64::MAX,
        ] {
            for (left_word, right_word, valid) in [
                (separator - 1, separator, true),
                (separator, separator, false),
                (separator - 1, separator - 1, false),
            ] {
                let mut store = Store::default();
                let left =
                    inline_leaf_page(&mut store, &[left_word.to_be_bytes().to_vec()], compact);
                let right =
                    inline_leaf_page(&mut store, &[right_word.to_be_bytes().to_vec()], compact);
                let root =
                    inline_branch_page(&mut store, left, right, &separator.to_be_bytes(), compact);
                let trace = RecordReads {
                    store: &store,
                    calls: RefCell::new(Vec::new()),
                };
                let result = audit_tree(&trace, root, 3);
                if valid {
                    assert_eq!(result.unwrap().pages, 3);
                    assert_eq!(trace.calls.borrow().len(), 6, "all page loads stay charged");
                } else {
                    assert!(matches!(
                        result,
                        Err(Error::Corrupt(
                            "audited key outside ancestor separator bounds"
                        ))
                    ));
                }
                assert!(matches!(
                    audit_tree(&store, root, 1),
                    Err(Error::LimitExceeded)
                ));
            }
        }
    }
}

#[test]
fn mixed_width_ancestor_bounds_match_slice_order_and_local_errors_precede_bounds() {
    for compact in [false, true] {
        for (leaf_width, separator_width) in [(8, 8), (9, 9), (8, 9), (9, 8)] {
            let separator = vec![0x80; separator_width];
            for left_byte in [0x7f, 0x80, 0x81] {
                for right_byte in [0x7f, 0x80, 0x81] {
                    let left_key = vec![left_byte; leaf_width];
                    let right_key = vec![right_byte; leaf_width];
                    let expected = left_key < separator && right_key >= separator;
                    let mut store = Store::default();
                    let left = inline_leaf_page(&mut store, &[left_key], compact);
                    let right = inline_leaf_page(&mut store, &[right_key], compact);
                    let root = inline_branch_page(&mut store, left, right, &separator, compact);
                    let result = audit_tree(&store, root, 3);
                    if expected {
                        assert_eq!(result.unwrap().leaf_records, 2);
                    } else {
                        assert!(matches!(
                            result,
                            Err(Error::Corrupt(
                                "audited key outside ancestor separator bounds"
                            ))
                        ));
                    }
                }
            }
        }
        let mut store = Store::default();
        // Both keys violate the parent's upper bound, but this whole leaf's
        // descending local order is checked first, before ancestor comparisons.
        let left = inline_leaf_page(
            &mut store,
            &[2u64.to_be_bytes().to_vec(), 1u64.to_be_bytes().to_vec()],
            compact,
        );
        let right = inline_leaf_page(&mut store, &[1u64.to_be_bytes().to_vec()], compact);
        let root = inline_branch_page(&mut store, left, right, &1u64.to_be_bytes(), compact);
        assert!(matches!(
            audit_tree(&store, root, 3),
            Err(Error::Corrupt("unordered tree keys"))
        ));
    }
}
