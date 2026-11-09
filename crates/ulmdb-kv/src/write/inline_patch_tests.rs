//! Compare the narrow inline path with the retained pre-optimization plan.
use super::*;
use std::cell::{Cell, RefCell};
use ulmdb_core::storage::ReadStore;

struct Store {
    bytes: Vec<u8>,
    size: usize,
    budget: Cell<usize>,
    reads: RefCell<Vec<(Reference, usize)>>,
}
impl Store {
    fn new(size: usize) -> Self {
        Self {
            bytes: Vec::new(),
            size,
            budget: Cell::new(usize::MAX),
            reads: RefCell::default(),
        }
    }
    fn root(&mut self, cells: &[Vec<u8>]) -> Reference {
        let cells = cells.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let bytes = page::encode_v1_for(self.size, 0, Reference::NULL, &cells).unwrap();
        self.allocate(&bytes, self.size).unwrap()
    }
}
impl ReadStore for Store {
    fn page_size(&self) -> usize {
        self.size
    }
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        let mut reads = self.reads.borrow_mut();
        reads.push((at, len));
        if reads.len() > self.budget.get() {
            return Err(Error::LimitExceeded);
        }
        if at.segment != 1 {
            return Err(Error::Corrupt("inline patch oracle segment"));
        }
        let p = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes
            .get(p..p.checked_add(len).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("inline patch oracle extent"))
    }
}
impl WriteStore for Store {
    fn allocate(&mut self, bytes: &[u8], align: usize) -> Result<Reference> {
        let start = self.bytes.len().next_multiple_of(align);
        self.bytes.resize(start, 0);
        self.bytes.extend_from_slice(bytes);
        Ok(Reference {
            segment: 1,
            offset: start as u64,
        })
    }
    fn rewrite(&mut self, _: Reference, bytes: &[u8], align: usize) -> Result<Reference> {
        self.allocate(bytes, align)
    }
}
fn outcome(plan: Result<Plan>) -> String {
    match plan {
        Err(e) => format!("{e:?}"),
        Ok(Plan::Noop) => "noop".into(),
        Ok(Plan::PatchValue { offset }) => format!("patch:{offset}"),
        Ok(Plan::Leaf { index, exists }) => format!("leaf:{index}:{exists}"),
        Ok(Plan::Branch {
            level,
            index,
            child,
            offset,
            last_child,
            migrate,
        }) => format!("branch:{level}:{index}:{child:?}:{offset}:{last_child}:{migrate}"),
    }
}
fn compare(store: &Store, root: Reference, key: &[u8], value: Option<&[u8]>) {
    store.reads.borrow_mut().clear();
    // Existing oracle is frozen before the V1 inline optimization. Its V1
    // branch contains the complete original raw_cell -> cell sequence.
    let expected = outcome(super::plan_tests::original_plan(
        store, root, key, value, None,
    ));
    let reads = store.reads.borrow().clone();
    store.reads.borrow_mut().clear();
    let actual = outcome(super::plan(store, root, key, value, None));
    assert_eq!(
        actual,
        expected,
        "key={key:?}, valueLen={:?}",
        value.map(<[u8]>::len)
    );
    assert_eq!(*store.reads.borrow(), reads);
}
fn inline_cell(key: &[u8], value: &[u8]) -> Vec<u8> {
    let end = 8 + key.len() + value.len();
    let mut raw = vec![0; end + end % 2];
    raw[..2].copy_from_slice(&(key.len() as u16).to_le_bytes());
    raw[4..8].copy_from_slice(&(value.len() as u32).to_le_bytes());
    raw[8..8 + key.len()].copy_from_slice(key);
    raw[8 + key.len()..end].copy_from_slice(value);
    raw
}

#[test]
fn inline_patch_plans_match_frozen_offsets_noops_and_length_change_fallbacks() {
    for size in [512, 1024, 4096, 65536] {
        for (key_len, value_len) in [
            (0, 0),
            (1, 0),
            (1, 1),
            (3, 5),
            (8, 8),
            (255, 17),
            (257, 0),
            (1000, 600),
        ] {
            if 32 + 2 + 8 + key_len + value_len + 1 > size {
                continue;
            }
            let key = vec![0x41; key_len];
            let old = vec![0x52; value_len];
            let mut store = Store::new(size);
            let root = store.root(&[inline_cell(&key, &old)]);
            for value in [
                old.clone(),
                vec![0x63; value_len],
                Vec::new(),
                vec![0x74; value_len + 1],
            ] {
                compare(&store, root, &key, Some(&value));
                compare(&store, root, b"missing", Some(&value));
            }
            compare(&store, root, &key, None);
            compare(&store, root, b"missing", None);
        }
    }
}

#[test]
fn malformed_padding_is_checked_before_equal_value_noop_and_all_error_order_matches() {
    for value in [b"".as_slice(), b"old8byte"] {
        let mut raw = inline_cell(b"odd", value);
        *raw.last_mut().unwrap() = 1;
        let mut store = Store::new(PAGE_SIZE);
        let root = store.root(&[raw]);
        compare(&store, root, b"odd", Some(value));
        assert!(matches!(
            super::plan(&store, root, b"odd", Some(value), None),
            Err(Error::Corrupt("nonzero cell padding"))
        ));
    }
    let valid = page::encode_v1(0, Reference::NULL, &[&inline_cell(b"odd", b"old8byte")]).unwrap();
    for offset in 0..PAGE_SIZE {
        for xor in [1, 255] {
            let mut bytes = valid;
            bytes[offset] ^= xor;
            let mut store = Store::new(PAGE_SIZE);
            let root = store.allocate(&bytes, PAGE_SIZE).unwrap();
            for value in [b"old8byte".as_slice(), b"new8byte", b"short"] {
                compare(&store, root, b"odd", Some(value));
            }
        }
    }
}

#[test]
fn overflow_keys_values_and_every_read_budget_preserve_frozen_fallbacks() {
    let mut store = Store::new(PAGE_SIZE);
    let key = vec![0x55; 1025];
    let other_key = vec![0x77; 1025];
    let large_value = vec![0x66; 1025];
    let pairs = [
        (b"inline".as_slice(), b"old8byte".as_slice()),
        (key.as_slice(), b"old8byte"),
        (b"value", large_value.as_slice()),
        (other_key.as_slice(), large_value.as_slice()),
    ];
    let mut roots = Vec::new();
    for (key, value) in pairs {
        let raw = leaf_cell(&mut store, key, value, None).unwrap();
        let root = store.root(&[raw.as_bytes().to_vec()]);
        roots.push((root, key.to_vec(), value.to_vec()));
    }
    for (root, key, old) in roots {
        for budget in (0..=24).chain(std::iter::once(usize::MAX)) {
            store.budget.set(budget);
            compare(&store, root, &key, Some(&old));
            compare(&store, root, &key, Some(&vec![0x33; old.len()]));
            compare(&store, root, &key, Some(b"different length"));
            compare(&store, root, &key, None);
        }
    }
}

#[test]
fn odd_key_noops_keep_arena_revision_and_cow_keeps_a_sealed_reader() {
    use ulmdb_storage::{Arena, SegmentTable};
    struct Dir(std::path::PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = Dir(std::env::temp_dir().join(format!(
        "inline-patch-prototype-{}",
        ulmdb_storage::new_identity().unwrap()
    )));
    ulmdb_storage::create_layout(&directory.0).unwrap();
    let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
    let root = super::put(&mut arena, Reference::NULL, b"odd", b"old8byte").unwrap();
    let (table, _) = arena.seal(&directory.0).unwrap();
    let held = crate::get(&table, root, b"odd").unwrap().unwrap();
    let mut arena = Arena::new(table.clone(), 2).unwrap();
    let before = (arena.dirty_bytes(), arena.revision());
    assert_eq!(
        super::put(&mut arena, root, b"odd", b"old8byte").unwrap(),
        root
    );
    assert_eq!((arena.dirty_bytes(), arena.revision()), before);
    let changed = super::put(&mut arena, root, b"odd", b"new8byte").unwrap();
    assert_eq!(held, b"old8byte");
    assert_eq!(
        crate::get(&table, root, b"odd").unwrap(),
        Some(b"old8byte".as_slice())
    );
    assert_eq!(
        crate::get(&arena, changed, b"odd").unwrap(),
        Some(b"new8byte".as_slice())
    );
    let before = (arena.dirty_bytes(), arena.revision());
    assert_eq!(
        super::put(&mut arena, changed, b"odd", b"new8byte").unwrap(),
        changed
    );
    assert_eq!((arena.dirty_bytes(), arena.revision()), before);
    let checkpoint = arena.checkpoint().unwrap();
    super::put(&mut arena, changed, b"odd", b"nextbyte").unwrap();
    arena.rollback_checkpoint(&checkpoint).unwrap();
    assert_eq!((arena.dirty_bytes(), arena.revision()), before);
    assert_eq!(
        crate::get(&arena, changed, b"odd").unwrap(),
        Some(b"new8byte".as_slice())
    );
    assert_eq!(held, b"old8byte");
}

// Append this test to the existing write::inline_patch_tests module. It uses
// that module's Store, inline_cell and outcome helpers; no archived source is
// changed by keeping this standalone integration snippet here.
#[test]
fn multi_cell_nonzero_slot_inline_plans_keep_frozen_offsets_noops_and_reads() {
    let rows: [(&[u8], &[u8], &[u8]); 4] = [
        (b"a", b"xx", b"YY"),
        (b"bb", b"old", b"NEW"),
        (b"ccc", b"old!", b"NEW!"),
        (b"dddd", b"tail5", b"NEXT5"),
    ];
    for size in [512, 1024, 4096, 65536] {
        // Distinct key/value lengths make the V1 physical slots nonuniform.
        // Every encoded record has a real trailing padding byte, including
        // earlier rows with odd key lengths and earlier rows with odd values.
        let cells: Vec<Vec<u8>> = rows
            .iter()
            .map(|(key, old, _)| {
                let raw = inline_cell(key, old);
                assert_eq!((8 + key.len() + old.len()) % 2, 1);
                assert_eq!(raw.last(), Some(&0));
                raw
            })
            .collect();
        let mut store = Store::new(size);
        let root = store.root(&cells);
        let original_bytes = store.bytes.clone();
        let page_at = usize::try_from(root.offset).unwrap();
        let wire = &store.bytes[page_at..page_at + size];
        let slot_start = |index: usize| {
            let slot = page::HEADER + index * 2;
            usize::from(u16::from_le_bytes(wire[slot..slot + 2].try_into().unwrap()))
        };
        for (index, &(key, old, changed)) in rows.iter().enumerate().skip(1) {
            assert_ne!(slot_start(index), slot_start(0));
            // Derive this independently from the encoded selected slot rather
            // than either the new accessor or the frozen plan's Cell.start.
            let expected_offset = slot_start(index) + 8 + key.len();
            assert_eq!(&wire[expected_offset..expected_offset + old.len()], old);
            assert_eq!(wire[expected_offset + old.len()], 0);
            for value in [old, changed] {
                store.reads.borrow_mut().clear();
                let expected = outcome(super::plan_tests::original_plan(
                    &store, root, key, Some(value), None,
                ));
                let expected_reads = store.reads.borrow().clone();
                store.reads.borrow_mut().clear();
                let actual = super::plan(&store, root, key, Some(value), None);
                if value == old {
                    assert!(matches!(&actual, Ok(Plan::Noop)));
                } else {
                    assert!(matches!(
                        &actual,
                        Ok(Plan::PatchValue { offset }) if *offset == expected_offset
                    ));
                }
                assert_eq!(outcome(actual), expected, "size={size}, slot={index}");
                assert_eq!(*store.reads.borrow(), expected_reads);
                assert_eq!(store.bytes.as_slice(), original_bytes.as_slice());
            }
        }
    }
}
