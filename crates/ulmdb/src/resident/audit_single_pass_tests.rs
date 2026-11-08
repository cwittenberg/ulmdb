use super::*;
use ulmdb_core::storage::WriteStore;

#[derive(Default)]
struct Bytes {
    bytes: Vec<u8>,
}
impl ReadStore for Bytes {
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("single-pass missing segment"));
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes
            .get(start..start.checked_add(len).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("single-pass missing bytes"))
    }
}
impl WriteStore for Bytes {
    fn rewrite(&mut self, _old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.allocate(bytes, alignment)
    }
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        let offset = self.bytes.len().div_ceil(alignment) * alignment;
        self.bytes.resize(offset, 0);
        self.bytes.extend_from_slice(bytes);
        Ok(Reference {
            segment: 1,
            offset: offset as u64,
        })
    }
}
struct Count<'a> {
    source: &'a Bytes,
    reads: Cell<u64>,
    budget: u64,
}
impl ReadStore for Count<'_> {
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        let n = self.reads.get();
        if n >= self.budget {
            return Err(Error::LimitExceeded);
        }
        self.reads.set(n + 1);
        self.source.read(at, len)
    }
}
fn count(source: &Bytes, budget: u64) -> Count<'_> {
    Count {
        source,
        reads: Cell::new(0),
        budget,
    }
}
fn legacy_kv<S: ReadStore>(store: &S, root: Reference, budget: u64, pages: &mut u64) -> Result<()> {
    add(pages, ulmdb_kv::audit_tree(store, root, budget)?.pages)?;
    bounded_values(store, root)
}
fn fixture() -> (Bytes, Reference) {
    let mut store = Bytes::default();
    let mut root = Reference::NULL;
    for n in 0..512u32 {
        root = ulmdb_kv::put(&mut store, root, &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
    }
    root = ulmdb_kv::put(&mut store, root, &vec![0xfd; 300], &vec![3; 3500]).unwrap();
    (store, root)
}
#[test]
fn actual_single_pass_helper_removes_only_redundant_cursor_reads_and_shares_budget() {
    let (store, root) = fixture();
    let graph = count(&store, u64::MAX);
    let single = count(&store, u64::MAX);
    let legacy = count(&store, u64::MAX);
    let cursor = count(&store, u64::MAX);
    let expected = ulmdb_kv::audit_tree(&graph, root, 10_000).unwrap();
    let mut pages = 0;
    single_pass_kv(&single, root, 10_000, &mut pages).unwrap();
    let mut old_pages = 0;
    legacy_kv(&legacy, root, 10_000, &mut old_pages).unwrap();
    bounded_values(&cursor,root).unwrap();
    assert_eq!((pages, old_pages), (expected.pages, expected.pages));
    assert_eq!(single.reads.get(), graph.reads.get());
    println!(
        "single_pass_reference_reads graph={} single={} legacy={} records=513",
        graph.reads.get(),
        single.reads.get(),
        legacy.reads.get()
    );
    // The ordinary cursor now reuses a checked leaf across its records, so a
    // second cursor pass is charged by page transitions rather than by513rows.
    // The single-pass audit must still remove that entire redundant pass.
    assert!(cursor.reads.get()>0 && cursor.reads.get()<513);
    assert_eq!(legacy.reads.get(),single.reads.get()+cursor.reads.get());
    let enough = count(&store, single.reads.get());
    single_pass_kv(&enough, root, 10_000, &mut 0).unwrap();
    let short = count(&store, single.reads.get() - 1);
    assert!(matches!(
        single_pass_kv(&short, root, 10_000, &mut 0),
        Err(Error::LimitExceeded)
    ));
    let old_short = count(&store, single.reads.get());
    assert!(matches!(
        legacy_kv(&old_short, root, 10_000, &mut 0),
        Err(Error::LimitExceeded)
    ));
}
#[test]
fn oversize_key_or_value_is_reported_only_after_graph_success_and_checked_counts() {
    for key_large in [false, true] {
        let mut store = Bytes::default();
        let key = if key_large {
            vec![0x61; 4081]
        } else {
            b"a".to_vec()
        };
        let value = if key_large {
            b"v".to_vec()
        } else {
            vec![7; 4081]
        };
        let root = ulmdb_kv::put(&mut store, Reference::NULL, &key, &value).unwrap();
        assert!(matches!(
            legacy_kv(&store, root, 100, &mut 0),
            Err(Error::Corrupt(
                "resident indexed object exceeds declared limit"
            ))
        ));
        let got = single_pass_kv(&store, root, 100, &mut 0);
        assert!(matches!(
            got,
            Err(Error::Corrupt(
                "resident indexed object exceeds declared limit"
            ))
        ));
        assert!(matches!(
            single_pass_kv(&store, root, 0, &mut 0),
            Err(Error::LimitExceeded)
        ));
        let mut overflowing = u64::MAX;
        assert!(matches!(
            single_pass_kv(&store, root, 100, &mut overflowing),
            Err(Error::LimitExceeded)
        ));
    }
}
#[test]
fn deferred_oversize_flag_cannot_hide_a_later_malformed_graph_or_read_budget_error() {
    let mut store = Bytes::default();
    let mut root = ulmdb_kv::put(&mut store, Reference::NULL, &[0], &vec![7; 4081]).unwrap();
    for n in 0..512u32 {
        // Overflow separators force the V1 branch layout whose slot offset is
        // deliberately corrupted below, including with compact pages enabled.
        let mut key = vec![1; 300];
        key[1..5].copy_from_slice(&n.to_be_bytes());
        root = ulmdb_kv::put(&mut store, root, &key, b"v").unwrap();
    }
    let p = root.offset as usize;
    assert_eq!(
        u16::from_le_bytes(store.bytes[p + 2..p + 4].try_into().unwrap()),
        1
    );
    assert_eq!(
        u16::from_le_bytes(store.bytes[p + 4..p + 6].try_into().unwrap()),
        1
    );
    let count = u16::from_le_bytes(store.bytes[p + 6..p + 8].try_into().unwrap()) as usize;
    assert!(count > 0);
    let cell = u16::from_le_bytes(
        store.bytes[p + 32 + 2 * (count - 1)..p + 34 + 2 * (count - 1)]
            .try_into()
            .unwrap(),
    ) as usize;
    store.bytes[p + cell + 4..p + cell + 20].copy_from_slice(
        &Reference {
            segment: 2,
            offset: 4096,
        }
        .encode(),
    );
    assert!(matches!(
        single_pass_kv(&store, root, 100, &mut 0),
        Err(Error::Corrupt("single-pass missing segment"))
    ));
    assert!(matches!(
        legacy_kv(&store, root, 100, &mut 0),
        Err(Error::Corrupt("single-pass missing segment"))
    ));
    assert!(matches!(
        single_pass_kv(&store, root, 2, &mut 0),
        Err(Error::LimitExceeded)
    ));
}

#[test]
fn mapped_shared_trace_retains_identical_live_versions_and_exact_budget_boundary() {
    for capacity in [
        crate::CatalogCapacity::DEFAULT,
        crate::CatalogCapacity::EXTENDED,
    ] {
        mapped_shared_trace(capacity);
    }
}
fn mapped_shared_trace(capacity: crate::CatalogCapacity) {
    use ulmdb_storage::resident::{Config, Pool};
    let pool = Pool::create(Config {
        kv_pages: 64,
        tiny_pages: 64,
        max_reserved_bytes: 64 << 20,
        max_metadata_bytes: 64 << 20,
        max_views: 32,
    })
    .unwrap();
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let mut root = Reference::NULL;
    for n in 0..512u32 {
        root = ulmdb_kv::put(&mut arena, root, &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
    }
    root = ulmdb_kv::put(&mut arena, root, &vec![0xfe; 300], &vec![5; 3500]).unwrap();
    let database = crate::catalog::Database {
        ordering: crate::ordering::DatabaseOrdering::default(),
        id: 1,
        kind: DatabaseKind::Kv,
        root,
    };
    let mut catalog = ulmdb_kv::put(
        &mut arena,
        Reference::NULL,
        b"kv",
        &catalog_codec::encode(&database),
    )
    .unwrap();
    if let Some(marker) = capacity.marker() {
        catalog = ulmdb_kv::put(&mut arena, catalog, b"", &marker).unwrap();
    }
    let view = arena.freeze().unwrap().activate();
    let old = Trace {
        view: &view,
        budget: 100_000,
        reads: Cell::new(0),
        live: RefCell::new(BTreeSet::new()),
    };
    ulmdb_kv::audit_tree(&old, catalog, 100_000).unwrap();
    // The full audit also probes the reserved empty catalog key to discover
    // capacity. Count that real lookup in this independent legacy trace: even
    // an absent DEFAULT marker requires a charged read of the catalog leaf.
    let before_profile = old.reads.get();
    let found_capacity = ulmdb_kv::get(&old, catalog, b"")
        .unwrap()
        .map(crate::CatalogCapacity::decode_marker)
        .transpose()
        .unwrap()
        .unwrap_or_default();
    assert_eq!(found_capacity, capacity);
    assert_eq!(old.reads.get() - before_profile, 1);
    for row in
        ulmdb_kv::Cursor::new(&old, catalog, Bound::Unbounded, Bound::Unbounded, false).unwrap()
    {
        row.unwrap();
    }
    legacy_kv(&old, root, 100_000, &mut 0).unwrap();
    let result = audit(&view, catalog, 2, 100_000).unwrap();
    assert_eq!(result.catalog_capacity, capacity);
    assert_eq!(result.live, *old.live.borrow());
    println!(
        "single_pass_mapped_reads enabled={} capacity={} selected={} legacy={} live_versions={}",
        cfg!(feature = "resident-single-pass-audit"),
        capacity.get(),
        result.reads,
        old.reads.get(),
        result.live.len()
    );
    #[cfg(feature = "resident-single-pass-audit")]
    assert!(old.reads.get() >= result.reads + 513);
    #[cfg(not(feature = "resident-single-pass-audit"))]
    assert_eq!(old.reads.get(), result.reads);
    assert!(matches!(
        audit(&view, catalog, 2, result.reads - 1),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(
        audit(&view, catalog, 2, result.reads).unwrap().live,
        result.live
    );
}
