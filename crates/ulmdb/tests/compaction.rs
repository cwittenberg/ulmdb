use std::{
    ops::Bound,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::Duration,
};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix, ReadTxn};
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-compaction-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn p(address: Address, length: u8) -> Prefix {
    Prefix::normalize(address, length).unwrap()
}
fn populate(env: &Environment) {
    let mut w = env.begin_write().unwrap();
    for (name, kind) in [
        (b"kv".as_slice(), DatabaseKind::Kv),
        (b"empty", DatabaseKind::Kv),
        (b"empty4", DatabaseKind::Ipv4),
        (b"empty6", DatabaseKind::Ipv6),
        (b"empty-dups", DatabaseKind::Duplicates { width: None }),
        (b"empty-fixed", DatabaseKind::Duplicates { width: Some(4) }),
        (b"v4", DatabaseKind::Ipv4),
        (b"v6", DatabaseKind::Ipv6),
        (b"dups", DatabaseKind::Duplicates { width: None }),
        (b"fixed", DatabaseKind::Duplicates { width: Some(4) }),
        (b"zero", DatabaseKind::Duplicates { width: Some(0) }),
        (b"dropped", DatabaseKind::Kv),
    ] {
        w.create_database(name, kind).unwrap();
    }
    for n in 0..600u64 {
        w.put(b"kv", &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
    }
    w.put(b"kv", b"", b"").unwrap();
    w.put(b"kv", &vec![0x44; 4096], &vec![0x88; 2_000_000])
        .unwrap();
    for n in 0..40u32 {
        w.insert_duplicate(b"dups", b"tree", &n.to_be_bytes())
            .unwrap();
        w.insert_duplicate(b"fixed", b"tree", &n.to_be_bytes())
            .unwrap();
    }
    for v in [b"".as_slice(), b"a", b"ab"] {
        w.insert_duplicate(b"dups", b"inline", v).unwrap();
    }
    w.insert_duplicate(b"dups", b"large", &vec![0xa4; 65536])
        .unwrap();
    w.insert_duplicate(b"zero", b"zero", b"").unwrap();
    for length in [0, 8, 16, 32] {
        w.insert_prefix(b"v4", p(Address::V4(0x0a01_0001), length), &[length])
            .unwrap();
    }
    for length in [0, 32, 64, 128] {
        w.insert_prefix(
            b"v6",
            p(Address::V6(0x20010db8u128 << 96 | 1), length),
            &[length],
        )
        .unwrap();
    }
    w.put(b"dropped", b"gone", &vec![0x99; 100_000]).unwrap();
    w.commit().unwrap();
    for n in 0..4 {
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"state", &[n; 100_000]).unwrap();
        w.commit().unwrap();
    }
    let mut w = env.begin_write().unwrap();
    w.drop_database(b"dropped").unwrap();
    w.delete(b"kv", b"state").unwrap();
    for n in 0..200u64 {
        w.delete(b"kv", &n.to_be_bytes()).unwrap();
    }
    w.commit().unwrap();
}
#[derive(Debug, Eq, PartialEq)]
struct Logical {
    kv: Vec<(Vec<u8>, Vec<u8>)>,
    dups: Vec<(Vec<u8>, Vec<u8>)>,
    fixed: Vec<(Vec<u8>, Vec<u8>)>,
    zero: Vec<(Vec<u8>, Vec<u8>)>,
    v4: Vec<(Prefix, Vec<u8>)>,
    v6: Vec<(Prefix, Vec<u8>)>,
}
fn pairs(read: &ReadTxn, name: &[u8], duplicate: bool) -> Vec<(Vec<u8>, Vec<u8>)> {
    if duplicate {
        read.duplicate_pairs(name, Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .map(|r| {
                let (k, v) = r.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect()
    } else {
        read.range(name, Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .map(|r| {
                let (k, v) = r.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect()
    }
}
fn logical(read: &ReadTxn) -> Logical {
    Logical {
        kv: pairs(read, b"kv", false),
        dups: pairs(read, b"dups", true),
        fixed: pairs(read, b"fixed", true),
        zero: pairs(read, b"zero", true),
        v4: read
            .subnets(b"v4", p(Address::V4(0), 0))
            .unwrap()
            .map(|r| {
                let (p, v) = r.unwrap();
                (p, v.to_vec())
            })
            .collect(),
        v6: read
            .subnets(b"v6", p(Address::V6(0), 0))
            .unwrap()
            .map(|r| {
                let (p, v) = r.unwrap();
                (p, v.to_vec())
            })
            .collect(),
    }
}
fn exercise(parent: &Path) {
    let directory = Directory::new(parent);
    let env = Environment::create(&directory.0).unwrap();
    populate(&env);
    let old = env.begin_read();
    let expected = logical(&old);
    let borrowed = old.get(b"kv", &vec![0x44; 4096]).unwrap().unwrap();
    let handle = old.open_database(b"kv").unwrap();
    let before = ulmdb_storage::read_current(&directory.0).unwrap();
    let source_files = std::fs::read_dir(directory.0.join("segments"))
        .unwrap()
        .count();
    let report = env.compact().unwrap();
    let after = ulmdb_storage::read_current(&directory.0).unwrap();
    assert_eq!(report.sequence, before.sequence);
    assert_eq!(report.commit, before.commit);
    assert_ne!(report.generation, before.generation);
    assert_eq!(report.previous_generation, before.generation);
    assert_eq!(after.previous, before.generation);
    assert_eq!(after.uuid, before.uuid);
    assert_eq!(after.epoch, before.epoch);
    assert_eq!(after.parent, before.parent);
    assert_eq!(after.sequence, before.sequence);
    assert_eq!(after.commit, before.commit);
    assert_eq!(after.next_database, before.next_database);
    assert!(after.next_segment > before.next_segment);
    assert_eq!(report.after_segments, 1);
    assert!(report.before_segments > 1);
    assert!(report.after_segment_bytes < report.before_segment_bytes);
    assert_eq!(
        std::fs::read_dir(directory.0.join("segments"))
            .unwrap()
            .count(),
        source_files + 1,
        "compaction does not delete retained files"
    );
    assert!(
        after
            .segments
            .iter()
            .all(|entry| entry.id >= before.next_segment)
    );
    let now = env.begin_read();
    assert_eq!(logical(&now), expected);
    assert_eq!(
        now.kv_handle(&handle).unwrap().get(b"").unwrap(),
        Some(&b""[..])
    );
    assert!(matches!(
        now.open_database(b"dropped"),
        Err(Error::MissingDatabase)
    ));
    assert_eq!(pairs(&now, b"empty", false), Vec::new());
    assert!(
        now.longest_match(b"empty4", Address::V4(0))
            .unwrap()
            .is_none()
    );
    assert!(
        now.longest_match(b"empty6", Address::V6(0))
            .unwrap()
            .is_none()
    );
    assert_eq!(now.duplicate_count(b"empty-dups", b"missing").unwrap(), 0);
    assert_eq!(now.duplicate_count(b"empty-fixed", b"missing").unwrap(), 0);
    assert_eq!(borrowed, &vec![0x88; 2_000_000]);
    assert_eq!(logical(&old), expected);
    ulmdb::backup::verify(&env).unwrap();
    // A following logical transaction starts from the relocated roots normally.
    let mut w = env.begin_write().unwrap();
    w.put_handle(&handle, b"next", b"commit").unwrap();
    assert_eq!(w.commit().unwrap(), before.sequence + 1);
    assert_eq!(now.get(b"kv", b"next").unwrap(), None);
    assert_eq!(borrowed[1_999_999], 0x88);
    drop(env);
    assert_eq!(logical(&old), expected);
    assert_eq!(borrowed[0], 0x88);
    let reopened = Environment::open(&directory.0).unwrap();
    assert_eq!(
        reopened.begin_read().get(b"kv", b"next").unwrap(),
        Some(&b"commit"[..])
    );
    ulmdb::backup::verify(&reopened).unwrap();
}
#[test]
fn all_database_kinds_old_borrows_handles_and_reopen_disk() {
    exercise(&std::env::temp_dir());
}
#[test]
fn all_database_kinds_old_borrows_handles_and_reopen_tmpfs() {
    if Path::new("/dev/shm").is_dir() {
        exercise(Path::new("/dev/shm"));
    }
}

#[test]
fn empty_compaction_preserves_logical_identity_and_burned_database_ids() {
    let directory = Directory::new(&std::env::temp_dir());
    let env = Environment::create(&directory.0).unwrap();
    let mut aborted = env.begin_write().unwrap();
    aborted
        .create_database(b"aborted", DatabaseKind::Kv)
        .unwrap();
    let burned = aborted.open_database(b"aborted").unwrap().id();
    aborted.abort();
    let before = env.begin_read();
    let report = env.compact().unwrap();
    assert_eq!(report.before_segments, 0);
    assert_eq!(report.after_segments, 0);
    assert_eq!(report.sequence, 0);
    assert_eq!(report.commit, before.commit_identity());
    let m = ulmdb_storage::read_current(&directory.0).unwrap();
    assert!(m.next_database > burned);
    assert_eq!(m.next_segment, 1);
    let mut w = env.begin_write().unwrap();
    w.create_database(b"new", DatabaseKind::Kv).unwrap();
    assert!(w.open_database(b"new").unwrap().id() > burned);
    w.commit().unwrap();
}

#[test]
fn active_writer_and_savepoint_serialize_compaction_without_blocking_readers() {
    let directory = Directory::new(&std::env::temp_dir());
    let env = Arc::new(Environment::create(&directory.0).unwrap());
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"a", b"old").unwrap();
    w.commit().unwrap();
    let old = env.begin_read();
    let mut writer = env.begin_write().unwrap();
    let mut nested = writer.savepoint().unwrap();
    nested.put(b"kv", b"a", b"new").unwrap();
    let (send, receive) = mpsc::channel();
    let worker = env.clone();
    let thread = std::thread::spawn(move || send.send(worker.compact()).unwrap());
    assert!(receive.recv_timeout(Duration::from_millis(20)).is_err());
    assert_eq!(
        env.begin_read().get(b"kv", b"a").unwrap(),
        Some(&b"old"[..])
    );
    nested.release().unwrap();
    writer.commit().unwrap();
    let report = receive
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    thread.join().unwrap();
    assert_eq!(report.sequence, 2);
    assert_eq!(
        env.begin_read().get(b"kv", b"a").unwrap(),
        Some(&b"new"[..])
    );
    assert_eq!(old.get(b"kv", b"a").unwrap(), Some(&b"old"[..]));
}

#[test]
fn segment_identity_resource_limit_is_known_nonpublication_and_writer_remains_usable() {
    let directory = Directory::new(&std::env::temp_dir());
    ulmdb_storage::create_layout(&directory.0).unwrap();
    let mut manifest = ulmdb_storage::Manifest::empty().unwrap();
    manifest.next_segment = u64::MAX;
    ulmdb_storage::install_snapshot(&directory.0, &manifest).unwrap();
    let env = Environment::open(&directory.0).unwrap();
    let before = std::fs::read(directory.0.join("CURRENT")).unwrap();
    assert!(matches!(env.compact(), Err(Error::LimitExceeded)));
    assert_eq!(std::fs::read(directory.0.join("CURRENT")).unwrap(), before);
    assert_eq!(env.begin_read().generation(), manifest.generation);
    // The exhausted ID range also prevents ordinary writer allocation; this
    // remains a known resource failure rather than a poisoned outcome.
    assert!(matches!(env.begin_write(), Err(Error::LimitExceeded)));
}

#[test]
fn seal_io_error_does_not_publish_or_poison_and_retry_succeeds() {
    let directory = Directory::new(&std::env::temp_dir());
    let env = Environment::create(&directory.0).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"a", b"old").unwrap();
    w.commit().unwrap();
    let before = std::fs::read(directory.0.join("CURRENT")).unwrap();
    let generation = env.begin_read().generation();
    std::fs::rename(
        directory.0.join("segments"),
        directory.0.join("segments.saved"),
    )
    .unwrap();
    // Leave the seal directory missing to cause real I/O failure. A regular
    // replacement is rejected earlier as structural corruption by access checks.
    let result = env.compact();
    assert!(
        matches!(&result, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound),
        "unexpected seal result: {result:?}"
    );
    assert_eq!(std::fs::read(directory.0.join("CURRENT")).unwrap(), before);
    assert_eq!(env.begin_read().generation(), generation);
    std::fs::rename(
        directory.0.join("segments.saved"),
        directory.0.join("segments"),
    )
    .unwrap();
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"a", b"after failure").unwrap();
    w.commit().unwrap();
    assert_eq!(
        env.begin_read().get(b"kv", b"a").unwrap(),
        Some(&b"after failure"[..])
    );
    env.compact().unwrap();
}

#[test]
fn legacy_minor1_subtree_compacts_to_current_inline_without_changing_logical_identity() {
    use sha2::{Digest, Sha256};
    use ulmdb_core::storage::Reference;
    let directory = Directory::new(&std::env::temp_dir());
    ulmdb_storage::create_layout(&directory.0).unwrap();
    let mut arena = ulmdb_storage::Arena::new(ulmdb_storage::SegmentTable::default(), 1).unwrap();
    let mut child = Reference::NULL;
    for value in [b"one".as_slice(), b"two"] {
        child = ulmdb_kv::put(&mut arena, child, value, b"").unwrap();
    }
    let mut bucket = [0; 24];
    bucket[..16].copy_from_slice(&child.encode());
    bucket[16..].copy_from_slice(&2u64.to_le_bytes());
    let root = ulmdb_kv::put(&mut arena, Reference::NULL, b"key", &bucket).unwrap();
    let mut descriptor = [0; 80];
    descriptor[..8].copy_from_slice(&1u64.to_le_bytes());
    descriptor[8..10].copy_from_slice(&2u16.to_le_bytes());
    descriptor[10..12].copy_from_slice(&1u16.to_le_bytes());
    descriptor[12..14].copy_from_slice(&3u16.to_le_bytes());
    descriptor[16..32].copy_from_slice(&root.encode());
    descriptor[32..40].copy_from_slice(&4096u64.to_le_bytes());
    descriptor[40..48].copy_from_slice(&67_108_864u64.to_le_bytes());
    descriptor[48..56].copy_from_slice(&65536u64.to_le_bytes());
    descriptor[56..64].copy_from_slice(&u64::MAX.to_le_bytes());
    let catalog = ulmdb_kv::put(&mut arena, Reference::NULL, b"dups", &descriptor).unwrap();
    let (table, entry) = arena.seal(&directory.0).unwrap();
    let mut entry = entry.unwrap();
    drop(table);
    let filename = |h: &[u8; 32]| h.iter().map(|b| format!("{b:02x}")).collect::<String>() + ".seg";
    let old_file = directory.0.join("segments").join(filename(&entry.hash));
    let mut bytes = std::fs::read(&old_file).unwrap();
    bytes[10..12].copy_from_slice(&1u16.to_le_bytes());
    bytes[56..64].copy_from_slice(&1u64.to_le_bytes());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&old_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    std::fs::write(&old_file, &bytes).unwrap();
    entry.hash = Sha256::digest(&bytes).into();
    let final_file = directory.0.join("segments").join(filename(&entry.hash));
    std::fs::rename(old_file, &final_file).unwrap();
    let mut permissions = std::fs::metadata(&final_file).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&final_file, permissions).unwrap();
    let mut manifest = ulmdb_storage::Manifest::empty().unwrap();
    manifest.format_minor = 1;
    manifest.sequence = 7;
    manifest.next_database = 2;
    manifest.next_segment = 2;
    manifest.catalog = catalog;
    manifest.segments = (vec![entry]).into();
    ulmdb_storage::install_snapshot(&directory.0, &manifest).unwrap();
    let env = Environment::open(&directory.0).unwrap();
    let old = env.begin_read();
    let old_values = pairs(&old, b"dups", true);
    let report = env.compact().unwrap();
    assert_eq!(report.sequence, 7);
    assert_eq!(report.duplicate_pairs, 2);
    assert_eq!(report.commit, manifest.commit);
    assert_eq!(env.begin_read().stats().format_minor, 2);
    assert_eq!(pairs(&env.begin_read(), b"dups", true), old_values);
    assert_eq!(pairs(&old, b"dups", true), old_values);
    let next = ulmdb_storage::read_current(&directory.0).unwrap();
    let table = ulmdb_storage::SegmentTable::load(&directory.0, &next.segments).unwrap();
    let descriptor = ulmdb_kv::get(&table, next.catalog, b"dups")
        .unwrap()
        .unwrap();
    let root = Reference::decode(&descriptor[16..32]).unwrap();
    let compacted = ulmdb_kv::get(&table, root, b"key").unwrap().unwrap();
    assert_eq!(
        Reference::decode(&compacted[..16]).unwrap(),
        Reference::NULL,
        "small legacy subtree should demote to inline"
    );
}

#[cfg(feature = "fault-injection")]
#[test]
fn compaction_fault_worker() {
    let Some(path) = std::env::var_os("ULMDB_COMPACTION_WORKER") else {
        return;
    };
    let env = Environment::open(path).unwrap();
    let before = env.begin_read().generation();
    let outcome = env.compact();
    if let Ok(point) = std::env::var("ULMDB_FAIL_AT") {
        if point.starts_with("segment.") {
            assert!(matches!(outcome, Err(Error::Io(_))));
            env.begin_write().unwrap().abort();
        } else {
            assert!(matches!(outcome, Err(Error::OutcomeUnknown)));
            assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
        }
        assert_eq!(env.begin_read().generation(), before);
    } else {
        outcome.unwrap();
        if std::env::var("ULMDB_CRASH_AT").is_ok_and(|v| v == "compaction.after_activation") {
            std::process::exit(86);
        }
    }
}

#[cfg(feature = "fault-injection")]
#[test]
fn injected_io_and_process_interruption_preserve_complete_compaction_snapshots() {
    use std::process::Command;
    for crash in [false, true] {
        for point in [
            "segment.before_write",
            "segment.after_rename",
            "descriptor.after_write",
            "pointer.PREVIOUS.after_rename",
            "pointer.CURRENT.before_rename",
            "pointer.CURRENT.after_rename",
            "pointer.CURRENT.after_directory_sync",
        ] {
            let directory = Directory::new(&std::env::temp_dir());
            let env = Environment::create(&directory.0).unwrap();
            let mut w = env.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            w.create_database(b"dups", DatabaseKind::Duplicates { width: None })
                .unwrap();
            w.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
            w.put(b"kv", b"state", b"same").unwrap();
            w.insert_duplicate(b"dups", b"key", b"value").unwrap();
            w.insert_prefix(b"v4", p(Address::V4(0), 0), b"profile")
                .unwrap();
            w.commit().unwrap();
            let before = ulmdb_storage::read_current(&directory.0).unwrap();
            drop(env);
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "compaction_fault_worker", "--nocapture"])
                .env("ULMDB_COMPACTION_WORKER", &directory.0)
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
            let env = Environment::open(&directory.0).unwrap();
            ulmdb::backup::verify(&env).unwrap();
            let after = ulmdb_storage::read_current(&directory.0).unwrap();
            assert_eq!(after.sequence, before.sequence);
            assert_eq!(after.commit, before.commit);
            assert_eq!(after.parent, before.parent);
            let published = matches!(
                point,
                "pointer.CURRENT.after_rename" | "pointer.CURRENT.after_directory_sync"
            );
            assert_eq!(after.generation != before.generation, published, "{point}");
            let read = env.begin_read();
            assert_eq!(read.get(b"kv", b"state").unwrap(), Some(&b"same"[..]));
            assert_eq!(
                read.duplicate_first(b"dups", b"key").unwrap(),
                Some(&b"value"[..])
            );
            assert_eq!(
                read.longest_match(b"v4", Address::V4(u32::MAX))
                    .unwrap()
                    .unwrap()
                    .1,
                b"profile"
            );
        }
    }
}

/// The fixture exceeds one arena and must compact successfully. A later
/// artificial segment-ID boundary then verifies known nonpublication on roll.
#[test]
fn live_data_exceeding_one_arena_compacts_and_rollover_id_failure_is_atomic() {
    let directory = Directory::new(&std::env::temp_dir());
    // Keep this fixture's existing mappings in the same live window as the
    // final usable ID. Otherwise an ordinary writer correctly fails window
    // admission before reaching the distinct 64-bit rollover boundary.
    ulmdb_storage::create_layout(&directory.0).unwrap();
    let mut initial = ulmdb_storage::Manifest::empty().unwrap();
    initial.next_segment = u64::MAX - 16;
    ulmdb_storage::install_snapshot(&directory.0, &initial).unwrap();
    let env = Environment::open(&directory.0).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.commit().unwrap();
    let value = vec![0x6b; 67_108_864];
    for n in 0..4u64 {
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", &n.to_be_bytes(), &value).unwrap();
        w.commit().unwrap();
    }
    let old = env.begin_read();
    let borrowed = old.get(b"kv", &0u64.to_be_bytes()).unwrap().unwrap();
    let before = ulmdb_storage::read_current(&directory.0).unwrap();
    let report = env.compact().unwrap();
    assert!(report.after_segments >= 2);
    assert!(report.after_segment_bytes > 268_435_456);
    assert_eq!(
        (report.sequence, report.commit),
        (before.sequence, before.commit)
    );
    assert_ne!(report.generation, before.generation);
    let compacted = ulmdb_storage::read_current(&directory.0).unwrap();
    assert!(
        compacted
            .segments
            .iter()
            .all(|entry| entry.id >= before.next_segment)
    );
    assert_eq!(
        compacted.next_segment,
        before.next_segment + report.after_segments as u64
    );
    let read = env.begin_read();
    for n in 0..4u64 {
        assert_eq!(
            read.get(b"kv", &n.to_be_bytes()).unwrap(),
            Some(value.as_slice())
        );
    }
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"after", b"usable").unwrap();
    w.commit().unwrap();
    env.reclaim().unwrap();
    assert_eq!(borrowed[67_108_863], 0x6b);
    assert_eq!(
        old.get(b"kv", &3u64.to_be_bytes()).unwrap(),
        Some(value.as_slice())
    );
    drop(env);
    let reopened = Environment::open(&directory.0).unwrap();
    assert_eq!(
        reopened.begin_read().get(b"kv", b"after").unwrap(),
        Some(&b"usable"[..])
    );
    assert_eq!(borrowed[0], 0x6b);
    drop(reopened);
    // Reserving the final usable segment ID still permits a valid source
    // descriptor, but a multi-segment rebuild cannot roll beyond that ID.
    let mut exhausted = ulmdb_storage::read_current(&directory.0).unwrap();
    exhausted.previous = exhausted.generation;
    exhausted.generation = ulmdb_storage::new_identity().unwrap();
    exhausted.next_segment = u64::MAX - 1;
    ulmdb_storage::publish(&directory.0, &exhausted).unwrap();
    let env = Environment::open(&directory.0).unwrap();
    let pointer = std::fs::read(directory.0.join("CURRENT")).unwrap();
    let files = std::fs::read_dir(directory.0.join("segments"))
        .unwrap()
        .count();
    assert!(matches!(env.compact(), Err(Error::LimitExceeded)));
    assert_eq!(std::fs::read(directory.0.join("CURRENT")).unwrap(), pointer);
    assert_eq!(env.begin_read().generation(), exhausted.generation);
    assert_eq!(
        std::fs::read_dir(directory.0.join("segments"))
            .unwrap()
            .count(),
        files,
        "ID exhaustion is checked before sealing the first roll"
    );
    let read = env.begin_read();
    for n in 0..4u64 {
        assert_eq!(
            read.get(b"kv", &n.to_be_bytes()).unwrap(),
            Some(value.as_slice())
        );
    }
    env.begin_write().unwrap().abort();
}
