use super::*;
use crate::{Cursor, audit_tree, get, height};
use std::{cell::Cell, collections::BTreeMap, ops::Bound};
use ulmdb_core::storage::ReadStore;

struct Store {
    bytes: Vec<u8>,
    size: usize,
    private: bool,
    writes: usize,
    reads: Cell<usize>,
}

impl Store {
    fn new(size: usize, private: bool) -> Self {
        Self {
            bytes: Vec::new(),
            size,
            private,
            writes: 0,
            reads: Cell::new(0),
        }
    }
}

impl ReadStore for Store {
    fn page_size(&self) -> usize {
        self.size
    }

    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        self.reads.set(self.reads.get() + 1);
        if at.segment != 1 {
            return Err(Error::Corrupt("prune test segment"));
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes
            .get(start..start.checked_add(length).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("prune test extent"))
    }
}

impl WriteStore for Store {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        let start = self.bytes.len().div_ceil(alignment) * alignment;
        self.bytes.resize(start, 0);
        self.bytes.extend_from_slice(bytes);
        self.writes += 1;
        Ok(Reference {
            segment: 1,
            offset: start as u64,
        })
    }

    fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        if self.private {
            self.writes += 1;
            let start = old.offset as usize;
            self.bytes[start..start + bytes.len()].copy_from_slice(bytes);
            Ok(old)
        } else {
            self.allocate(bytes, alignment)
        }
    }
}

type Model = BTreeMap<Vec<u8>, Vec<u8>>;

fn check(store: &Store, root: Reference, expected: &Model) {
    let audit = audit_tree(store, root, 10_000).unwrap();
    assert_eq!(audit.leaf_records, expected.len() as u64);
    if expected.is_empty() {
        assert!(root.is_null());
        return;
    }
    for (key, value) in expected {
        assert_eq!(get(store, root, key).unwrap(), Some(value.as_slice()));
    }
    for reverse in [false, true] {
        let actual: Vec<_> = Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, reverse)
            .unwrap()
            .map(|row| {
                let (key, value) = row.unwrap();
                (key.to_vec(), value.to_vec())
            })
            .collect();
        let mut want: Vec<_> = expected
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if reverse {
            want.reverse();
        }
        assert_eq!(actual, want);
        if expected.len() >= 4 {
            let lower = expected.keys().nth(expected.len() / 4).unwrap();
            let upper = expected.keys().nth(expected.len() * 3 / 4).unwrap();
            let actual: Vec<_> = Cursor::new(
                store,
                root,
                Bound::Excluded(lower.as_slice()),
                Bound::Included(upper.as_slice()),
                reverse,
            )
            .unwrap()
            .map(|row| {
                let (key, value) = row.unwrap();
                (key.to_vec(), value.to_vec())
            })
            .collect();
            let mut want: Vec<_> = expected
                .iter()
                .filter(|(key, _)| *key > lower && *key <= upper)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if reverse {
                want.reverse();
            }
            assert_eq!(actual, want);
        }
    }
    let mut pending = vec![root];
    while let Some(at) = pending.pop() {
        let page = Page::load(store, at).unwrap();
        if page.level == 0 {
            assert_ne!(page.count, 0, "reachable empty leaf");
        } else {
            for index in 0..=page.count {
                pending.push(page.child(store, index).unwrap());
            }
        }
    }
}

fn key(n: u64) -> Vec<u8> {
    let mut key = n.to_be_bytes().to_vec();
    if n.is_multiple_of(11) {
        key.resize(1025, n as u8);
    }
    key
}

#[test]
fn model_deletes_to_null_and_reinserts_across_geometries_private_and_cow() {
    for size in [256, 512, 1024, 4096, 16384, 65536] {
        for private in [false, true] {
            let mut store = Store::new(size, private);
            let mut root = Reference::NULL;
            let mut model = Model::new();
            for n in 0..260 {
                let value = vec![n as u8; if n % 7 == 0 { 4097 } else { 512.min(size / 8) }];
                root = put(&mut store, root, &key(n), &value).unwrap();
                model.insert(key(n), value);
            }
            assert!(height(&store, root).unwrap() >= 2);
            if size == 256 {
                assert!(height(&store, root).unwrap() >= 3);
            }
            check(&store, root, &model);
            let held = (root, model.clone());
            // Delete both extremes and then an independent deterministic mixed
            // order, so every separator/child position eventually disappears.
            let mut ids: Vec<_> = (1..259u64).collect();
            let mut seed = 0x9204_715b_ac31_d86fu64;
            for i in (1..ids.len()).rev() {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                ids.swap(i, seed as usize % (i + 1));
            }
            ids.splice(..0, [0, 259]);
            for (step, n) in ids.into_iter().enumerate() {
                let depth = height(&store, root).unwrap() as usize;
                let writes = store.writes;
                store.reads.set(0);
                root = delete(&mut store, root, &key(n)).unwrap();
                assert!(store.writes - writes <= depth, "unbounded ancestor rewrite");
                assert!(store.reads.get() <= 8 * depth + 4, "sibling subtree walk");
                if !root.is_null() {
                    let page = Page::load(&store, root).unwrap();
                    assert!(page.level == 0 || page.count != 0, "root was not collapsed");
                }
                model.remove(&key(n));
                if step % 23 == 0 || model.len() <= 3 {
                    check(&store, root, &model);
                }
            }
            assert!(root.is_null());
            if !private {
                check(&store, held.0, &held.1);
            }
            for n in [5, 3, 9, 0, 259] {
                root = put(&mut store, root, &key(n), b"reinserted").unwrap();
                model.insert(key(n), b"reinserted".to_vec());
            }
            check(&store, root, &model);
            let writes = store.writes;
            let same = delete(&mut store, root, b"absent").unwrap();
            assert_eq!(same, root);
            assert_eq!(store.writes, writes);
        }
    }
}

fn leaf(store: &mut Store, key: &[u8], value: &[u8]) -> Reference {
    let cell = leaf_cell(store, key, value, None).unwrap();
    let bytes = page::encode_v1_for(store.size, 0, Reference::NULL, &[cell.as_bytes()]).unwrap();
    store.allocate(&bytes, store.size).unwrap()
}

fn branch(
    store: &mut Store,
    level: u16,
    first: Reference,
    rest: &[(&[u8], Reference)],
) -> Reference {
    let cells: Vec<_> = rest
        .iter()
        .map(|(key, child)| {
            let cell = leaf_cell(store, key, b"", None).unwrap();
            branch_cell(
                store,
                &Separator::from_cell(cell.as_bytes(), 0).unwrap(),
                *child,
            )
            .unwrap()
        })
        .collect();
    let cells: Vec<_> = cells.iter().map(CellBuffer::as_bytes).collect();
    let bytes = page::encode_v1_for(store.size, level, first, &cells).unwrap();
    store.allocate(&bytes, store.size).unwrap()
}

#[test]
fn sparse_unary_nonroots_keep_levels_and_lower_bounds_then_root_collapses() {
    let mut store = Store::new(256, false);
    let a = leaf(&mut store, b"10", b"a");
    let b = leaf(&mut store, b"30", b"b");
    let c = leaf(&mut store, b"50", b"c");
    let a = branch(&mut store, 1, a, &[]);
    let b = branch(&mut store, 1, b, &[]);
    let c = branch(&mut store, 1, c, &[]);
    let base = branch(&mut store, 2, a, &[(b"20", b), (b"40", c)]);
    let original = branch(&mut store, 3, base, &[]);
    let initial: Model = [
        (b"10".to_vec(), b"a".to_vec()),
        (b"30".to_vec(), b"b".to_vec()),
        (b"50".to_vec(), b"c".to_vec()),
    ]
    .into();
    audit_tree(&store, original, 100).unwrap();
    let mut root = delete(&mut store, original, b"30").unwrap();
    let mut model = initial.clone();
    model.remove(b"30".as_slice());
    check(&store, root, &model);
    assert_eq!(height(&store, root).unwrap(), 3);
    let parent = Page::load(&store, root).unwrap();
    assert_eq!(parent.key_at(&store, 0).unwrap(), b"40");
    root = put(&mut store, root, b"25", b"between").unwrap();
    model.insert(b"25".to_vec(), b"between".to_vec());
    check(&store, root, &model);
    for key in [b"10", b"25"] {
        root = delete(&mut store, root, key).unwrap();
        model.remove(key.as_slice());
        check(&store, root, &model);
    }
    assert_eq!(height(&store, root).unwrap(), 1);
    check(&store, original, &initial);
    assert!(delete(&mut store, root, b"50").unwrap().is_null());
}

#[test]
fn deleted_last_cell_still_checks_overflow_reference_and_padding() {
    for overflow in [false, true] {
        let mut store = Store::new(4096, false);
        let value = vec![7; if overflow { 513 } else { 0 }];
        let root = leaf(&mut store, b"x", &value);
        let page = Page::load(&store, root).unwrap();
        let start = page::u16_at(page.bytes, HEADER).unwrap() as usize;
        let absolute = root.offset as usize + start;
        if overflow {
            store.bytes[absolute + 9..absolute + 25].fill(0);
        } else {
            store.bytes[absolute + 9] = 1;
        }
        let writes = store.writes;
        assert!(matches!(
            delete(&mut store, root, b"x"),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(store.writes, writes);
    }
}

#[test]
fn pruning_checks_removed_separator_and_collapse_checks_surviving_child_level() {
    let mut store = Store::new(256, false);
    let a = leaf(&mut store, b"10", b"a");
    let b = leaf(&mut store, b"30", b"b");
    let root = branch(&mut store, 1, a, &[(b"20", b)]);
    let page = Page::load(&store, root).unwrap();
    let start = page::u16_at(page.bytes, HEADER).unwrap() as usize;
    store.bytes[root.offset as usize + start + 4..root.offset as usize + start + 20].fill(0);
    assert!(matches!(
        delete(&mut store, root, b"10"),
        Err(Error::Corrupt(_))
    ));

    let a = branch(&mut store, 1, a, &[]);
    let malformed = branch(&mut store, 2, a, &[(b"20", b)]);
    assert!(matches!(
        delete(&mut store, malformed, b"10"),
        Err(Error::Corrupt("collapsed root level mismatch"))
    ));
}

#[test]
fn compact_last_cell_keeps_full_tail_validation_before_empty_propagation() {
    let mut store = Store::new(4096, false);
    let cell = leaf_cell(&mut store, b"x", b"v", None).unwrap();
    let layout = crate::compact::Layout { key: 1, value: 1 };
    let bytes = crate::compact::encode(0, Reference::NULL, &[cell.as_bytes()], layout).unwrap();
    let root = store.allocate(&bytes, 4096).unwrap();
    let mut bad = bytes;
    bad[4095] = 1;
    let bad = store.allocate(&bad, 4096).unwrap();
    assert!(matches!(
        delete(&mut store, bad, b"x"),
        Err(Error::Corrupt(_))
    ));
    assert!(delete(&mut store, root, b"x").unwrap().is_null());
}

#[test]
fn missing_delete_preserves_imported_unary_root_but_private_deletion_collapses() {
    for private in [false, true] {
        let mut store = Store::new(256, private);
        let a = leaf(&mut store, b"10", b"a");
        let b = leaf(&mut store, b"30", b"b");
        let child = branch(&mut store, 1, a, &[(b"20", b)]);
        let root = branch(&mut store, 2, child, &[]);
        let writes = store.writes;
        assert_eq!(delete(&mut store, root, b"missing").unwrap(), root);
        assert_eq!(store.writes, writes);
        let collapsed = delete(&mut store, root, b"10").unwrap();
        assert_eq!(height(&store, collapsed).unwrap(), 1);
        assert_eq!(get(&store, collapsed, b"30").unwrap(), Some(&b"b"[..]));
    }
}

#[test]
fn empty_compact_root_and_promoted_empty_child_validate_tail_before_null() {
    let layout = crate::compact::Layout { key: 1, value: 1 };
    for corrupt in [false, true] {
        let mut store = Store::new(4096, false);
        let mut empty = crate::compact::encode(0, Reference::NULL, &[], layout).unwrap();
        if corrupt {
            empty[4095] = 1;
        }
        let empty = store.allocate(&empty, 4096).unwrap();
        let result = delete(&mut store, empty, b"missing");
        if corrupt {
            assert!(matches!(result, Err(Error::Corrupt(_))));
        } else {
            assert!(result.unwrap().is_null());
        }
        let a = leaf(&mut store, b"10", b"a");
        let root = branch(&mut store, 1, a, &[(b"20", empty)]);
        let result = delete(&mut store, root, b"10");
        if corrupt {
            assert!(matches!(result, Err(Error::Corrupt(_))));
        } else {
            assert!(result.unwrap().is_null());
        }
    }
}
