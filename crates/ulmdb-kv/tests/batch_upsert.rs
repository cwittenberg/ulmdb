use std::{cell::Cell, collections::BTreeMap, ops::Bound};
use ulmdb_core::storage::{ReadStore, Reference, Result, WriteStore};
use ulmdb_kv::{Cursor, audit_tree, build_sorted, get, height, upsert_sorted, upsert_sorted_with};
use ulmdb_storage::{Arena, SegmentTable};

struct Count<'a> {
    arena: &'a mut Arena,
    reads: Cell<usize>,
    writes: usize,
}
impl ReadStore for Count<'_> {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        self.reads.set(self.reads.get() + 1);
        self.arena.read(at, length)
    }
}
impl WriteStore for Count<'_> {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.writes += 1;
        self.arena.allocate(bytes, alignment)
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.writes += 1;
        self.arena.rewrite(old, bytes, alignment)
    }
}

#[test]
fn sparse_updates_do_not_walk_the_existing_index_and_dense_updates_share_pages() {
    let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
    let root = build_sorted(
        &mut arena,
        (0..100_000u64).map(|n| Ok((n.to_be_bytes(), n.to_le_bytes()))),
    )
    .unwrap()
    .root;
    let levels = height(&arena, root).unwrap();
    let mut store = Count {
        arena: &mut arena,
        reads: Cell::new(0),
        writes: 0,
    };
    let updated = upsert_sorted(
        &mut store,
        root,
        [Ok((50_000u64.to_be_bytes(), 7u64.to_be_bytes()))],
    )
    .unwrap();
    assert!(
        store.reads.get() < 100,
        "a point update must stay bounded by its search path"
    );
    assert!(store.writes <= usize::from(levels));
    store.writes = 0;
    let updated = upsert_sorted(
        &mut store,
        updated.root,
        (50_001..50_257u64).map(|n| Ok((n.to_be_bytes(), 9u64.to_be_bytes()))),
    )
    .unwrap();
    assert!(
        store.writes < 16,
        "dense batch must share leaf and ancestor rewrites"
    );
    assert_eq!(updated.records, 256);
    for n in 50_001..50_257u64 {
        assert_eq!(
            get(store.arena, updated.root, &n.to_be_bytes()).unwrap(),
            Some(9u64.to_be_bytes().as_slice())
        );
    }
    assert_eq!(
        get(store.arena, updated.root, &99_999u64.to_be_bytes()).unwrap(),
        Some(99_999u64.to_le_bytes().as_slice())
    );
    audit_tree(store.arena, updated.root, u64::MAX).unwrap();
}

#[test]
fn merged_pages_splits_overflow_records_and_unchanged_rows_match_an_ordered_model() {
    let mut arena = Arena::new(SegmentTable::default(), 2).unwrap();
    let mut model = BTreeMap::new();
    for n in 0..3000u32 {
        model.insert((n * 2).to_be_bytes().to_vec(), n.to_be_bytes().to_vec());
    }
    model.insert(vec![], vec![]);
    model.insert(vec![255; 4096], vec![5; 65_536]);
    let mut root = build_sorted(&mut arena, model.iter().map(|(k, v)| Ok((k, v))))
        .unwrap()
        .root;
    for pass in 0..3 {
        let input = (0..6500u32)
            .filter(|n| n % 3 != pass)
            .map(|n| {
                let key = n.to_be_bytes().to_vec();
                let value = if n % 101 == 0 {
                    vec![n as u8; 600]
                } else {
                    (n ^ pass).to_be_bytes().to_vec()
                };
                (key, value)
            })
            .collect::<Vec<_>>();
        root = upsert_sorted(&mut arena, root, input.iter().map(|(k, v)| Ok((k, v))))
            .unwrap()
            .root;
        model.extend(input);
        let actual = Cursor::new(&arena, root, Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .map(|pair| pair.map(|(k, v)| (k.to_vec(), v.to_vec())).unwrap())
            .collect::<BTreeMap<_, _>>();
        assert_eq!(actual, model);
        audit_tree(&arena, root, u64::MAX).unwrap();
    }
    let before = arena.revision();
    let same = upsert_sorted(&mut arena, root, model.iter().map(|(k, v)| Ok((k, v)))).unwrap();
    assert_eq!(same.root, root);
    assert_eq!(arena.revision(), before);
}

#[test]
fn comparator_equivalent_updates_keep_stored_keys_and_reject_duplicate_input_across_chunks() {
    let fold = |a: &[u8], b: &[u8]| {
        a.iter()
            .map(u8::to_ascii_lowercase)
            .cmp(b.iter().map(u8::to_ascii_lowercase))
    };
    let mut arena = Arena::new(SegmentTable::default(), 3).unwrap();
    let root = upsert_sorted_with(
        &mut arena,
        Reference::NULL,
        (0..600).map(|n| Ok((format!("K{n:04}"), b"old"))),
        &fold,
    )
    .unwrap()
    .root;
    let updated = upsert_sorted_with(
        &mut arena,
        root,
        (0..900).map(|n| Ok((format!("k{n:04}"), b"new"))),
        &fold,
    )
    .unwrap();
    let (key, value) = ulmdb_kv::get_pair_with(&arena, updated.root, b"k0007", &fold)
        .unwrap()
        .unwrap();
    assert_eq!((key, value), (b"K0007".as_slice(), b"new".as_slice()));
    assert_eq!(
        ulmdb_kv::get_pair_with(&arena, updated.root, b"K0800", &fold)
            .unwrap()
            .unwrap()
            .0,
        b"k0800"
    );
    let checkpoint = arena.checkpoint().unwrap();
    let before = arena.revision();
    let invalid = (0..256)
        .map(|n| Ok((format!("k{n:04}"), b"bad")))
        .chain(std::iter::once(Ok(("K0255".to_owned(), b"bad"))));
    assert!(upsert_sorted_with(&mut arena, updated.root, invalid, &fold).is_err());
    arena.rollback_checkpoint(&checkpoint).unwrap();
    assert_eq!(arena.revision(), before);
    assert_eq!(
        ulmdb_kv::get_with(&arena, updated.root, b"k0007", &fold).unwrap(),
        Some(b"new".as_slice())
    );
}
