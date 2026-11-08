use std::{
    collections::BTreeSet,
    ops::Bound,
    path::{Path, PathBuf},
};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix};
use ulmdb_core::storage::{ReadStore, Reference, WriteStore};
use ulmdb_storage::{Arena, SegmentTable};
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-prune-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn p(a: Address, length: u8) -> Prefix {
    Prefix::normalize(a, length).unwrap()
}
fn files(path: &Path, name: &str) -> usize {
    std::fs::read_dir(path.join(name)).unwrap().count()
}
fn exercise(parent: &Path) {
    let dir = Directory::new(parent);
    let env = Environment::create(&dir.0).unwrap();
    let mut w = env.begin_write().unwrap();
    for (name, kind) in [
        (b"kv".as_slice(), DatabaseKind::Kv),
        (b"dups", DatabaseKind::Duplicates { width: None }),
        (b"fixed", DatabaseKind::Duplicates { width: Some(4) }),
        (b"v4", DatabaseKind::Ipv4),
        (b"v6", DatabaseKind::Ipv6),
        (b"empty", DatabaseKind::Kv),
    ] {
        w.create_database(name, kind).unwrap();
    }
    w.put(b"kv", b"", b"").unwrap();
    w.put(b"kv", b"large", &vec![0x8a; 2_000_000]).unwrap();
    for n in 0..40u32 {
        w.insert_duplicate(b"dups", b"tree", &n.to_be_bytes())
            .unwrap();
        w.insert_duplicate(b"fixed", b"tree", &n.to_be_bytes())
            .unwrap();
    }
    w.insert_duplicate(b"dups", b"inline", b"").unwrap();
    w.insert_duplicate(b"dups", b"inline", b"a").unwrap();
    w.insert_duplicate(b"dups", b"large", &vec![0x62; 65536])
        .unwrap();
    w.insert_prefix(b"v4", p(Address::V4(0), 0), b"fallback")
        .unwrap();
    w.insert_prefix(b"v4", p(Address::V4(0x0a00_0000), 8), b"network")
        .unwrap();
    w.insert_prefix(b"v6", p(Address::V6(0x20010db8u128 << 96), 32), b"six")
        .unwrap();
    w.commit().unwrap();
    for n in 0..6u8 {
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"version", &[n; 1000]).unwrap();
        w.commit().unwrap();
    }
    let old = env.begin_read();
    let old_large = old.get(b"kv", b"large").unwrap().unwrap();
    let handle = old.open_database(b"kv").unwrap();
    let before = ulmdb_storage::read_current(&dir.0).unwrap();
    let data_files = files(&dir.0, "segments");
    let descriptors = files(&dir.0, "commits");
    let report = env.prune_inventory().unwrap();
    assert!(report.changed);
    assert!(report.after_segments < report.before_segments);
    assert!(report.after_segment_bytes < report.before_segment_bytes);
    let after = ulmdb_storage::read_current(&dir.0).unwrap();
    assert_ne!(after.generation, before.generation);
    assert_eq!(after.previous, before.generation);
    assert_eq!(
        (
            after.sequence,
            after.commit,
            after.parent,
            after.epoch,
            after.uuid,
            after.catalog,
            after.next_segment
        ),
        (
            before.sequence,
            before.commit,
            before.parent,
            before.epoch,
            before.uuid,
            before.catalog,
            before.next_segment
        )
    );
    assert_eq!(
        files(&dir.0, "segments"),
        data_files,
        "no data files created or deleted"
    );
    assert_eq!(files(&dir.0, "commits"), descriptors + 1);
    let now = env.begin_read();
    let large = now.get(b"kv", b"large").unwrap().unwrap();
    assert!(
        std::ptr::eq(old_large.as_ptr(), large.as_ptr()),
        "selected mappings are shared without remapping"
    );
    assert_eq!(large.len(), 2_000_000);
    assert_eq!(large[1_999_999], 0x8a);
    assert_eq!(
        now.kv_handle(&handle).unwrap().get(b"version").unwrap(),
        Some(&[5u8; 1000][..])
    );
    assert_eq!(now.duplicate_count(b"dups", b"tree").unwrap(), 40);
    assert_eq!(now.duplicate_count(b"fixed", b"tree").unwrap(), 40);
    let expected: Vec<Vec<u8>> = (0..40u32)
        .map(|value| value.to_be_bytes().to_vec())
        .collect();
    for reader in [&old, &now] {
        for name in [b"dups".as_slice(), b"fixed"] {
            let values: Vec<Vec<u8>> = reader
                .duplicate_values(name, b"tree", Bound::Unbounded, Bound::Unbounded, false)
                .unwrap()
                .map(|value| value.unwrap().to_vec())
                .collect();
            assert_eq!(
                values, expected,
                "every duplicate value survives projection"
            );
        }
    }
    assert_eq!(
        now.duplicate_first(b"dups", b"inline").unwrap(),
        Some(&b""[..])
    );
    assert_eq!(
        now.duplicate_first(b"dups", b"large")
            .unwrap()
            .unwrap()
            .len(),
        65536
    );
    assert_eq!(
        now.longest_match(b"v4", Address::V4(1)).unwrap().unwrap().1,
        b"fallback"
    );
    assert_eq!(
        now.longest_match(b"v4", Address::V4(0x0a01_0001))
            .unwrap()
            .unwrap()
            .1,
        b"network"
    );
    assert_eq!(
        now.longest_match(b"v6", Address::V6(0x20010db8u128 << 96 | 1))
            .unwrap()
            .unwrap()
            .1,
        b"six"
    );
    assert_eq!(
        now.range(b"empty", Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .count(),
        0
    );
    let pointer = std::fs::read(dir.0.join("CURRENT")).unwrap();
    let no_op = env.prune_inventory().unwrap();
    assert!(!no_op.changed);
    assert_eq!(no_op.generation, after.generation);
    assert_eq!(std::fs::read(dir.0.join("CURRENT")).unwrap(), pointer);
    assert_eq!(files(&dir.0, "commits"), descriptors + 1);
    let mut w = env.begin_write().unwrap();
    w.put_handle(&handle, b"next", b"commit").unwrap();
    w.commit().unwrap();
    env.reclaim().unwrap();
    assert_eq!(old_large[0], 0x8a);
    assert_eq!(old.get(b"kv", b"next").unwrap(), None);
    drop(env);
    let reopened = Environment::open(&dir.0).unwrap();
    ulmdb::backup::verify(&reopened).unwrap();
    assert_eq!(
        reopened.begin_read().get(b"kv", b"next").unwrap(),
        Some(&b"commit"[..])
    );
    assert_eq!(old_large[1], 0x8a);
}
#[test]
fn trace_all_index_kinds_share_mappings_and_preserve_old_readers_disk() {
    exercise(&std::env::temp_dir());
}
#[test]
fn trace_all_index_kinds_share_mappings_and_preserve_old_readers_tmpfs() {
    if Path::new("/dev/shm").is_dir() {
        exercise(Path::new("/dev/shm"));
    }
}

#[test]
fn table_selection_handles_sparse_ids_cow_chunks_missing_ids_and_empty_sets() {
    let dir = Directory::new(&std::env::temp_dir());
    ulmdb_storage::create_layout(&dir.0).unwrap();
    let mut arena = Arena::new(SegmentTable::default(), 17).unwrap();
    let first = arena.put_payload(b"first").unwrap();
    let (table, _) = arena.seal(&dir.0).unwrap();
    let mut arena = Arena::new(table, 18).unwrap();
    let middle = arena.put_payload(b"middle").unwrap();
    let (table, _) = arena.seal(&dir.0).unwrap();
    let mut arena = Arena::new(table, 1_000_000).unwrap();
    let last = arena.put_payload(b"last").unwrap();
    let (table, _) = arena.seal(&dir.0).unwrap();
    let selected = table.select(&BTreeSet::from([17, 1_000_000])).unwrap();
    assert_eq!(selected.entries().len(), 2);
    assert_eq!(table.entries().len(), 3);
    assert!(std::ptr::eq(
        selected.payload(first).unwrap().as_ptr(),
        table.payload(first).unwrap().as_ptr()
    ));
    assert_eq!(selected.payload(last).unwrap(), b"last");
    assert!(selected.payload(middle).is_err());
    assert_eq!(table.payload(middle).unwrap(), b"middle");
    assert!(table.select(&BTreeSet::from([19])).is_err());
    assert!(table.select(&BTreeSet::from([0])).is_err());
    assert!(table.select(&BTreeSet::new()).unwrap().entries().is_empty());
    assert_eq!(table.payload(last).unwrap(), b"last");
}

// Build an empty KV tree whose overflow separator is the only reference to one
// non-dense inventory segment. Leaf-only enumeration would drop it incorrectly.
fn separator_fixture(path: &Path, corrupt: bool, legacy: bool) {
    use sha2::{Digest, Sha256};
    ulmdb_storage::create_layout(path).unwrap();
    let mut arena = Arena::new(SegmentTable::default(), 17).unwrap();
    let separator = arena.put_payload(b"middle").unwrap();
    let (table, _) = arena.seal(path).unwrap();
    let mut arena = Arena::new(table, 256).unwrap();
    let mut empty = [0u8; 4096];
    empty[..2].copy_from_slice(&2u16.to_le_bytes());
    empty[2..4].copy_from_slice(&1u16.to_le_bytes());
    empty[8..10].copy_from_slice(&32u16.to_le_bytes());
    empty[10..12].copy_from_slice(&4096u16.to_le_bytes());
    let leaf = arena.allocate(&empty, 4096).unwrap();
    let (table, _) = arena.seal(path).unwrap();
    let mut arena = Arena::new(table, 4096).unwrap();
    let mut branch = [0u8; 4096];
    branch[..2].copy_from_slice(&3u16.to_le_bytes());
    branch[2..4].copy_from_slice(&1u16.to_le_bytes());
    branch[4..6].copy_from_slice(&1u16.to_le_bytes());
    branch[6..8].copy_from_slice(&1u16.to_le_bytes());
    branch[8..10].copy_from_slice(&34u16.to_le_bytes());
    branch[10..12].copy_from_slice(&4060u16.to_le_bytes());
    branch[16..32].copy_from_slice(&leaf.encode());
    branch[32..34].copy_from_slice(&4060u16.to_le_bytes());
    branch[4060..4064].copy_from_slice(&[255, 255, 1, 0]);
    branch[4064..4080].copy_from_slice(&leaf.encode());
    branch[4080..4096].copy_from_slice(
        &if corrupt {
            Reference {
                segment: 999,
                offset: 4096,
            }
        } else {
            separator
        }
        .encode(),
    );
    let root = arena.allocate(&branch, 4096).unwrap();
    let (table, _) = arena.seal(path).unwrap();
    let mut arena = Arena::new(table, 1_000_000).unwrap();
    let mut db = [0u8; 80];
    db[..8].copy_from_slice(&1u64.to_le_bytes());
    db[8..10].copy_from_slice(&1u16.to_le_bytes());
    db[10..12].copy_from_slice(&1u16.to_le_bytes());
    db[16..32].copy_from_slice(&root.encode());
    db[32..40].copy_from_slice(&4096u64.to_le_bytes());
    db[40..48].copy_from_slice(&67_108_864u64.to_le_bytes());
    db[48..56].copy_from_slice(&65536u64.to_le_bytes());
    db[56..64].copy_from_slice(&u64::MAX.to_le_bytes());
    let catalog = ulmdb_kv::put(&mut arena, Reference::NULL, b"kv", &db).unwrap();
    let (table, _) = arena.seal(path).unwrap();
    let mut arena = Arena::new(table, 1_000_001).unwrap();
    arena.put_payload(b"unreachable").unwrap();
    let (table, _) = arena.seal(path).unwrap();
    let mut entries = table.entries();
    drop(table);
    if legacy {
        for entry in &mut entries {
            let name = |hash: &[u8; 32]| {
                hash.iter().map(|b| format!("{b:02x}")).collect::<String>() + ".seg"
            };
            let old = path.join("segments").join(name(&entry.hash));
            let mut bytes = std::fs::read(&old).unwrap();
            bytes[10..12].copy_from_slice(&1u16.to_le_bytes());
            bytes[56..64].copy_from_slice(&1u64.to_le_bytes());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            std::fs::write(&old, &bytes).unwrap();
            entry.hash = Sha256::digest(&bytes).into();
            std::fs::rename(&old, path.join("segments").join(name(&entry.hash))).unwrap();
        }
    }
    let mut manifest = ulmdb_storage::Manifest::empty().unwrap();
    manifest.format_minor = if legacy { 1 } else { 2 };
    manifest.catalog = catalog;
    manifest.next_database = 2;
    manifest.next_segment = 1_000_002;
    manifest.segments = (entries).into();
    ulmdb_storage::install_snapshot(path, &manifest).unwrap();
}
#[test]
fn unused_separator_segment_is_retained_for_empty_children_including_legacy() {
    for legacy in [false, true] {
        let dir = Directory::new(&std::env::temp_dir());
        separator_fixture(&dir.0, false, legacy);
        let env = Environment::open(&dir.0).unwrap();
        assert_eq!(
            env.begin_read()
                .range(b"kv", Bound::Unbounded, Bound::Unbounded, false)
                .unwrap()
                .count(),
            0
        );
        let before = ulmdb_storage::read_current(&dir.0).unwrap();
        let report = env.prune_inventory().unwrap();
        assert_eq!((report.before_segments, report.after_segments), (5, 4));
        let after = ulmdb_storage::read_current(&dir.0).unwrap();
        assert_eq!(after.format_minor, before.format_minor);
        assert!(after.segments.iter().any(|e| e.id == 17));
        assert!(!after.segments.iter().any(|e| e.id == 1_000_001));
        assert_eq!(env.begin_read().get(b"kv", b"query").unwrap(), None);
        ulmdb::backup::verify(&env).unwrap();
    }
}
#[test]
fn corrupt_unused_separator_prevents_any_publication_and_empty_engine_is_noop() {
    let dir = Directory::new(&std::env::temp_dir());
    separator_fixture(&dir.0, true, false);
    let env = Environment::open(&dir.0).unwrap();
    let pointer = std::fs::read(dir.0.join("CURRENT")).unwrap();
    let count = files(&dir.0, "commits");
    assert!(matches!(env.prune_inventory(), Err(Error::Corrupt(_))));
    assert_eq!(std::fs::read(dir.0.join("CURRENT")).unwrap(), pointer);
    assert_eq!(files(&dir.0, "commits"), count);
    env.begin_write().unwrap().abort();
    let empty = Directory::new(&std::env::temp_dir());
    let env = Environment::create(&empty.0).unwrap();
    let before = env.begin_read().generation();
    let report = env.prune_inventory().unwrap();
    assert!(!report.changed);
    assert_eq!(report.generation, before);
}

#[cfg(feature = "fault-injection")]
#[test]
fn prune_fault_worker() {
    let Some(path) = std::env::var_os("ULMDB_PRUNE_WORKER") else {
        return;
    };
    let env = Environment::open(path).unwrap();
    let old = env.begin_read().generation();
    let outcome = env.prune_inventory();
    if std::env::var_os("ULMDB_FAIL_AT").is_some() {
        assert!(matches!(outcome, Err(Error::OutcomeUnknown)));
        assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
        assert_eq!(env.begin_read().generation(), old);
    } else {
        outcome.unwrap();
    }
}
#[cfg(feature = "fault-injection")]
#[test]
fn publication_io_and_crash_errors_never_activate_a_partial_inventory() {
    use std::process::Command;
    for crash in [false, true] {
        for point in [
            "descriptor.after_write",
            "pointer.CURRENT.before_rename",
            "pointer.CURRENT.after_rename",
        ] {
            let dir = Directory::new(&std::env::temp_dir());
            let env = Environment::create(&dir.0).unwrap();
            let mut w = env.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            w.commit().unwrap();
            for n in 0..3u8 {
                let mut w = env.begin_write().unwrap();
                w.put(b"kv", b"value", &[n]).unwrap();
                w.commit().unwrap();
            }
            let before = ulmdb_storage::read_current(&dir.0).unwrap();
            let data_files = files(&dir.0, "segments");
            drop(env);
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "prune_fault_worker", "--nocapture"])
                .env("ULMDB_PRUNE_WORKER", &dir.0)
                .env(
                    if crash {
                        "ULMDB_CRASH_AT"
                    } else {
                        "ULMDB_FAIL_AT"
                    },
                    point,
                )
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(if crash { 86 } else { 0 }),
                "{point} crash={crash}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let env = Environment::open(&dir.0).unwrap();
            ulmdb::backup::verify(&env).unwrap();
            let after = ulmdb_storage::read_current(&dir.0).unwrap();
            assert_eq!(
                (after.sequence, after.commit, after.parent, after.catalog),
                (
                    before.sequence,
                    before.commit,
                    before.parent,
                    before.catalog
                )
            );
            assert_eq!(
                after.generation != before.generation,
                point == "pointer.CURRENT.after_rename"
            );
            assert_eq!(files(&dir.0, "segments"), data_files);
            assert_eq!(
                env.begin_read().get(b"kv", b"value").unwrap(),
                Some(&b"\x02"[..])
            );
        }
    }
}

// Valid sparse trees can revisit shared empty chains while keeping every
// separator's ancestor bounds. Their work must remain bounded after pruning.
fn projection_fixture_database(root: Reference, duplicates: bool) -> [u8; 80] {
    let mut db = [0u8; 80];
    db[..8].copy_from_slice(&1u64.to_le_bytes());
    db[8..10].copy_from_slice(&if duplicates { 2u16 } else { 1u16 }.to_le_bytes());
    db[10..12].copy_from_slice(&1u16.to_le_bytes());
    if duplicates {
        db[12..14].copy_from_slice(&3u16.to_le_bytes());
    }
    db[16..32].copy_from_slice(&root.encode());
    db[32..40].copy_from_slice(&4096u64.to_le_bytes());
    db[40..48].copy_from_slice(&67_108_864u64.to_le_bytes());
    db[48..56].copy_from_slice(&65536u64.to_le_bytes());
    db[56..64].copy_from_slice(&u64::MAX.to_le_bytes());
    db
}
fn reduced_budget_fixture(path: &Path, duplicates: bool, expensive_catalog: bool) {
    ulmdb_storage::create_layout(path).unwrap();
    let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
    let mut empty = [0u8; 4096];
    empty[..2].copy_from_slice(&2u16.to_le_bytes());
    empty[2..4].copy_from_slice(&1u16.to_le_bytes());
    empty[8..10].copy_from_slice(&32u16.to_le_bytes());
    empty[10..12].copy_from_slice(&4096u16.to_le_bytes());
    let empty = arena.allocate(&empty, 4096).unwrap();
    let catalog_record = projection_fixture_database(Reference::NULL, false);
    let full = ulmdb_kv::put(
        &mut arena,
        Reference::NULL,
        if expensive_catalog { b"db" } else { b"\xff" },
        if expensive_catalog {
            &catalog_record
        } else {
            b""
        },
    )
    .unwrap();
    let chain = |arena: &mut Arena, mut root: Reference| {
        for level in 1..=10u16 {
            let mut page = [0u8; 4096];
            page[..2].copy_from_slice(&3u16.to_le_bytes());
            page[2..4].copy_from_slice(&1u16.to_le_bytes());
            page[4..6].copy_from_slice(&level.to_le_bytes());
            page[8..10].copy_from_slice(&32u16.to_le_bytes());
            page[10..12].copy_from_slice(&4096u16.to_le_bytes());
            page[16..32].copy_from_slice(&root.encode());
            root = arena.allocate(&page, 4096).unwrap();
        }
        root
    };
    let empty_chain = chain(&mut arena, empty);
    let full_chain = chain(&mut arena, full);
    let mut root = [0u8; 4096];
    root[..2].copy_from_slice(&3u16.to_le_bytes());
    root[2..4].copy_from_slice(&1u16.to_le_bytes());
    root[4..6].copy_from_slice(&11u16.to_le_bytes());
    root[6..8].copy_from_slice(&169u16.to_le_bytes());
    root[8..10].copy_from_slice(&370u16.to_le_bytes());
    root[10..12].copy_from_slice(&378u16.to_le_bytes());
    root[16..32].copy_from_slice(&empty_chain.encode());
    for n in 0..169usize {
        let start = 4096 - (n + 1) * 22;
        root[32 + n * 2..34 + n * 2].copy_from_slice(&(start as u16).to_le_bytes());
        root[start..start + 2].copy_from_slice(&2u16.to_le_bytes());
        root[start + 4..start + 20]
            .copy_from_slice(&if n == 168 { full_chain } else { empty_chain }.encode());
        root[start + 20..start + 22].copy_from_slice(&((n + 1) as u16).to_be_bytes());
    }
    let root = arena.allocate(&root, 4096).unwrap();
    let root = if duplicates {
        let mut bucket = [0u8; 24];
        bucket[..16].copy_from_slice(&root.encode());
        bucket[16..].copy_from_slice(&1u64.to_le_bytes());
        ulmdb_kv::put(&mut arena, Reference::NULL, b"bucket", &bucket).unwrap()
    } else {
        root
    };
    let catalog = if expensive_catalog {
        root
    } else {
        let db = projection_fixture_database(root, duplicates);
        ulmdb_kv::put(&mut arena, Reference::NULL, b"db", &db).unwrap()
    };
    let (table, _) = arena.seal(path).unwrap();
    let mut arena = Arena::new(table, 2).unwrap();
    arena.put_payload(&vec![0x42; 131_072]).unwrap();
    let (table, _) = arena.seal(path).unwrap();
    let mut manifest = ulmdb_storage::Manifest::empty().unwrap();
    manifest.catalog = catalog;
    manifest.next_database = 2;
    manifest.next_segment = 3;
    manifest.segments = table.entries().into();
    ulmdb_storage::install_snapshot(path, &manifest).unwrap();
}
#[test]
fn reduced_inventory_page_budget_rejects_sparse_alias_work_in_catalog_kv_and_duplicate_subtree() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for (duplicates, expensive_catalog) in [(false, false), (true, false), (false, true)] {
            let directory = Directory::new(&parent);
            reduced_budget_fixture(&directory.0, duplicates, expensive_catalog);
            let env = Environment::open(&directory.0).unwrap();
            // The source really is valid under its original inventory budgets.
            ulmdb::backup::verify(&env).unwrap();
            let old = env.begin_read();
            if duplicates {
                assert_eq!(old.duplicate_count(b"db", b"bucket").unwrap(), 1);
                assert_eq!(
                    old.duplicate_first(b"db", b"bucket").unwrap(),
                    Some(&b"\xff"[..])
                );
            } else if expensive_catalog {
                assert_eq!(old.get(b"db", b"anything").unwrap(), None);
            } else {
                assert_eq!(old.get(b"db", b"\xff").unwrap(), Some(&b""[..]));
            }
            let current = std::fs::read(directory.0.join("CURRENT")).unwrap();
            let descriptors = files(&directory.0, "commits");
            assert!(
                matches!(env.prune_inventory(), Err(Error::LimitExceeded)),
                "duplicates={duplicates} catalog={expensive_catalog}"
            );
            assert_eq!(std::fs::read(directory.0.join("CURRENT")).unwrap(), current);
            assert_eq!(files(&directory.0, "commits"), descriptors);
            env.begin_write().unwrap().abort();
            ulmdb::backup::verify(&env).unwrap();
            assert_eq!(old.generation(), env.begin_read().generation());
        }
    }
}
