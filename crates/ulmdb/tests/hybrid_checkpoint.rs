//! Actual H3 issuance/rotation/staging. No checkpoint activation or trim claim.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use ulmdb::{
    Address, DatabaseKind, Error, Prefix,
    hybrid::{Environment, ImageMode, Options},
};
use ulmdb_storage::hybrid::{
    checkpoint::{self, CheckpointLimits},
    recovery,
};
struct Dir(PathBuf);
impl Dir {
    fn new(base: &Path) -> Self {
        let p = base.join(format!(
            "ulmdb-h5-env-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn options(image: ImageMode) -> Options {
    let mut o = Options {
        image,
        ..Options::default()
    };
    o.chunk.capacity = 1024 * 1024;
    o.chunk.reserve_quantum = 65536;
    o.chunk.max_burned_bytes = 512 * 1024;
    o.max_image_bytes = 8 * 1024 * 1024;
    o
}
fn init(env: &Environment) {
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
    w.create_database(b"v6", DatabaseKind::Ipv6).unwrap();
    w.put(b"kv", b"key", b"old").unwrap();
    w.insert_prefix(
        b"v4",
        Prefix::new(Address::V4(0x0a000000), 8).unwrap(),
        b"four",
    )
    .unwrap();
    w.insert_prefix(
        b"v6",
        Prefix::new(Address::V6(0x20010db8000000000000000000000000), 32).unwrap(),
        b"six",
    )
    .unwrap();
    w.commit().unwrap();
}
fn cache_count(dir: &Path) -> usize {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("image-"))
        .count()
}

fn exact_origins_match_retained_wal(artifact: &Path, wal: &Path, selected_lsn: u64) {
    use std::collections::BTreeMap;
    use ulmdb_storage::hybrid::{
        provenance,
        redo::{Authority, SourceReceipt},
    };
    let plan = recovery::inspect(&fs::File::open(wal).unwrap()).unwrap();
    let mut expected = BTreeMap::new();
    for decoded in plan.records.iter().filter(|d| d.record.lsn <= selected_lsn) {
        let record = &decoded.record;
        for object in &record.objects {
            expected.insert(
                object.dest(),
                SourceReceipt {
                    uuid: record.uuid,
                    epoch: record.epoch,
                    creation_lsn: record.lsn,
                    creation_sequence: record.sequence,
                    creation_commit: record.commit,
                    provenance_digest: decoded.receipt.digest,
                    authority: Authority::Wal {
                        lsn: record.lsn,
                        sequence: record.sequence,
                        digest: decoded.receipt.digest,
                    },
                },
            );
        }
    }
    let descriptor = checkpoint::verify_artifact(artifact, CheckpointLimits::default()).unwrap();
    for chunk in descriptor.chunks {
        let bytes = fs::read(artifact.join(format!("chunk-{:016x}.origins", chunk.identity.chunk)))
            .unwrap();
        let table = provenance::decode(&bytes).unwrap();
        for origin in table.origins() {
            assert_eq!(expected.remove(&origin.extent), Some(origin.receipt));
        }
    }
    assert!(expected.is_empty());
}
#[test]
fn genesis_capture_uses_exact_journal_root_and_never_reserves_an_extra_logical_chunk() {
    for image in [ImageMode::File, ImageMode::Anonymous] {
        for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
            let d = Dir::new(base);
            let engine = d.0.join("engine");
            let env = Environment::create(&engine, options(image)).unwrap();
            let before = fs::read(engine.join("HYBRID.wal")).unwrap();
            for n in 0..3 {
                let capture = env.capture_checkpoint().unwrap();
                assert_eq!(capture.chunk_count(), 0);
                assert_eq!(capture.selection().root.next_chunk, 1);
                assert!(!capture.report().rotated);
                assert_eq!(capture.report().added_image_capacity, 0);
                let artifact = capture
                    .stage(
                        d.0.join(format!("empty{n}")),
                        1 + n,
                        CheckpointLimits::default(),
                    )
                    .unwrap();
                let descriptor =
                    checkpoint::verify_artifact(artifact.directory(), CheckpointLimits::default())
                        .unwrap();
                assert_eq!(descriptor.selected, capture.selection());
                assert_eq!(descriptor.required_wal_floor, 0);
            }
            assert_eq!(cache_count(&engine), usize::from(image == ImageMode::File));
            assert_eq!(before, fs::read(engine.join("HYBRID.wal")).unwrap());
            drop(env);
            let env = Environment::open(&engine, options(image)).unwrap();
            assert_eq!(
                env.capture_checkpoint()
                    .unwrap()
                    .selection()
                    .root
                    .next_chunk,
                1
            );
            init(&env);
            let plan =
                recovery::inspect(&fs::File::open(engine.join("HYBRID.wal")).unwrap()).unwrap();
            assert_eq!(plan.records[0].record.chunks[0].chunk, 1);
            assert_eq!(env.sequence(), 1);
        }
    }
}
#[test]
fn real_cut_seals_once_reuses_pending_chunk_and_survives_updates_env_drop_and_reopen() {
    for image in [ImageMode::File, ImageMode::Anonymous] {
        for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
            let d = Dir::new(base);
            let engine = d.0.join("engine");
            let env = Arc::new(Environment::create(&engine, options(image)).unwrap());
            init(&env);
            let old = env.begin_read();
            let held = old.get(b"kv", b"key").unwrap().unwrap();
            let wal = fs::read(engine.join("HYBRID.wal")).unwrap();
            let cut = env.capture_checkpoint().unwrap();
            assert!(cut.report().rotated);
            assert_eq!(cut.chunk_count(), 1);
            assert_eq!(cut.report().added_image_capacity, 1024 * 1024);
            assert!(cut.report().sealed_slack_bytes > 0);
            assert_eq!(
                cache_count(&engine),
                2 * usize::from(image == ImageMode::File)
            );
            let repeat = env.capture_checkpoint().unwrap();
            assert!(!repeat.report().rotated);
            assert_eq!(cut.selection(), repeat.selection());
            assert_eq!(fs::read(engine.join("HYBRID.wal")).unwrap(), wal);
            std::thread::scope(|s| {
                let writer = s.spawn(|| {
                    for n in 0..16u64 {
                        let mut w = env.begin_write().unwrap();
                        w.put(b"kv", b"key", &n.to_le_bytes()).unwrap();
                        w.commit().unwrap();
                    }
                });
                let artifact = cut
                    .stage(d.0.join("concurrent"), 77, CheckpointLimits::default())
                    .unwrap();
                let desc =
                    checkpoint::verify_artifact(artifact.directory(), CheckpointLimits::default())
                        .unwrap();
                assert_eq!(desc.selected, cut.selection());
                writer.join().unwrap();
            });
            assert_eq!(held, b"old");
            assert_eq!(
                old.longest_match(b"v4", Address::V4(0x0a010203))
                    .unwrap()
                    .unwrap()
                    .1,
                b"four"
            );
            let later = env.capture_checkpoint().unwrap();
            assert_eq!(later.chunk_count(), 2);
            drop(env);
            // Owning cuts/readers have no environment fence; a new writer can reopen.
            let env = Environment::open(&engine, options(image)).unwrap();
            assert_eq!(
                env.begin_read().get(b"kv", b"key").unwrap(),
                Some(15u64.to_le_bytes().as_slice())
            );
            assert_eq!(held, b"old");
            let artifact = later
                .stage(d.0.join("after-reopen"), 78, CheckpointLimits::default())
                .unwrap();
            let desc =
                checkpoint::verify_artifact(artifact.directory(), CheckpointLimits::default())
                    .unwrap();
            assert_eq!(desc.selected, later.selection());
            exact_origins_match_retained_wal(
                artifact.directory(),
                &engine.join("HYBRID.wal"),
                later.selection().receipt.lsn,
            );
            exact_origins_match_retained_wal(
                &d.0.join("concurrent"),
                &engine.join("HYBRID.wal"),
                cut.selection().receipt.lsn,
            );
            // Recovered active metadata can again be consumed and sources spanning
            // old sealed chunks still resolve under exact current receipts.
            let captured = env.capture_checkpoint().unwrap();
            assert_eq!(captured.chunk_count(), 2);
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"key", b"continued").unwrap();
            w.commit().unwrap();
            drop(env);
            let env = Environment::open(&engine, options(image)).unwrap();
            assert_eq!(
                env.begin_read().get(b"kv", b"key").unwrap(),
                Some(b"continued".as_slice())
            );
        }
    }
}
#[test]
fn unused_manual_rotation_and_known_refusal_preserve_selected_cut_and_source_bounds() {
    for image in [ImageMode::File, ImageMode::Anonymous] {
        for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
            let d = Dir::new(base);
            let engine = d.0.join("engine");
            let mut o = options(image);
            o.max_wal_bytes = 220000;
            let env = Environment::create(&engine, o).unwrap();
            init(&env);
            let selected = env.sequence();
            env.rotate_chunk().unwrap();
            let caches = cache_count(&engine);
            let cut = env.capture_checkpoint().unwrap();
            assert!(!cut.report().rotated);
            assert_eq!(cache_count(&engine), caches);
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"large", &vec![8; 65536]).unwrap();
            assert!(w.commit().is_err());
            assert_eq!(env.sequence(), selected);
            let cut2 = env.capture_checkpoint().unwrap();
            assert_eq!(cut.selection(), cut2.selection());
            assert!(!cut2.report().rotated);
            let artifact = cut2
                .stage(d.0.join("refused"), 88, CheckpointLimits::default())
                .unwrap();
            checkpoint::verify_artifact(artifact.directory(), CheckpointLimits::default()).unwrap();
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"key", b"retry").unwrap();
            w.commit().unwrap();
            drop(env);
            let env = Environment::open(&engine, o).unwrap();
            assert_eq!(
                env.begin_read().get(b"kv", b"key").unwrap(),
                Some(b"retry".as_slice())
            );
        }
    }
}
#[test]
fn poisoned_journal_refuses_capture_without_rotating_or_changing_old_readers() {
    for image in [ImageMode::File, ImageMode::Anonymous] {
        use ulmdb_platform::FileExt;
        let d = Dir::new(Path::new("/tmp"));
        let engine = d.0.join("engine");
        let env = Environment::create(&engine, options(image)).unwrap();
        init(&env);
        let read = env.begin_read();
        let n = cache_count(&engine);
        let f = fs::OpenOptions::new()
            .write(true)
            .open(engine.join("HYBRID.wal"))
            .unwrap();
        f.write_all_at(b"orphan", f.metadata().unwrap().len())
            .unwrap();
        drop(f);
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", b"failed").unwrap();
        assert!(w.commit().is_err());
        assert!(matches!(
            env.capture_checkpoint(),
            Err(Error::OutcomeUnknown)
        ));
        assert_eq!(cache_count(&engine), n);
        assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"old".as_slice()));
    }
}

#[test]
fn denied_cold_rotation_keeps_active_receipts_and_writer_ready() {
    for image in [ImageMode::File, ImageMode::Anonymous] {
        let d = Dir::new(Path::new("/tmp"));
        let engine = d.0.join("engine");
        let mut o = options(image);
        o.max_image_bytes = o.chunk.capacity as u64;
        let env = Environment::create(&engine, o).unwrap();
        init(&env);
        let wal = fs::read(engine.join("HYBRID.wal")).unwrap();
        assert!(matches!(
            env.capture_checkpoint(),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(cache_count(&engine), usize::from(image == ImageMode::File));
        assert_eq!(fs::read(engine.join("HYBRID.wal")).unwrap(), wal);
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", b"still-ready").unwrap();
        w.commit().unwrap();
        drop(env);
        let env = Environment::open(&engine, o).unwrap();
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"still-ready".as_slice())
        );
    }
}
