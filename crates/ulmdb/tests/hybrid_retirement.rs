//! H5-RETIRE primitives only: all WAL bytes remain and no floor is selected.
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};
use ulmdb::{
    DatabaseKind, Error,
    hybrid::{Environment, ImageMode, Options},
};
use ulmdb_platform::FileExt;
use ulmdb_storage::hybrid::{
    checkpoint::{CheckpointArchive, CheckpointLimits, selection},
    journal::WalLayout,
    recovery,
    retirement::{self, CheckpointAliases},
};
const WITNESS_OFFSETS: [u64; 2] = [65536, 131072];
struct Dir(PathBuf);
impl Dir {
    fn new(base: &Path) -> Self {
        let p = base.join(format!(
            "ulmdb-retire-{}",
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
fn options(image: ImageMode, wal_layout: WalLayout) -> Options {
    Options {
        image,
        wal_layout,
        chunk: ulmdb_storage::hybrid::ChunkLimits {
            capacity: 1024 * 1024,
            reserve_quantum: 65536,
            max_burned_bytes: 512 * 1024,
        },
        max_image_bytes: 16 * 1024 * 1024,
        max_wal_bytes: 8 * 1024 * 1024,
        ..Options::default()
    }
}
fn set(e: &Environment, n: u64) {
    let mut w = e.begin_write().unwrap();
    if n == 0 {
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    }
    let mut value = vec![42u8; 8192];
    value[100..108].copy_from_slice(&n.to_le_bytes());
    w.put(b"kv", b"state", &value).unwrap();
    w.commit().unwrap();
}
fn aliases(path: &Path) -> CheckpointAliases {
    let s = selection::read(path).unwrap().unwrap();
    let a = CheckpointArchive::open(&s.directory(path), CheckpointLimits::default()).unwrap();
    CheckpointAliases::from_archive(&a).unwrap()
}
fn install(e: &Environment) {
    let mut p = e.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    e.activate_checkpoint(&mut p, CheckpointLimits::default())
        .unwrap();
}
#[test]
fn real_selected_c_suffix_recovery_and_exact_alias_identity_negatives() {
    for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
        for image in [ImageMode::File, ImageMode::Anonymous] {
            for layout in [
                WalLayout::Dense,
                WalLayout::PreSized {
                    capacity: 8 * 1024 * 1024,
                },
            ] {
                let d = Dir::new(base);
                let path = d.0.join("db");
                let e = Environment::create(&path, options(image, layout)).unwrap();
                set(&e, 0);
                set(&e, 1);
                set(&e, 2);
                install(&e);
                let a = aliases(&path);
                for n in 3..7 {
                    set(&e, n);
                }
                let file = File::open(path.join("HYBRID.wal")).unwrap();
                let f = recovery::inspect(&file).unwrap();
                let s = retirement::inspect(&file, &a).unwrap();
                assert_eq!(s.selected, f.selected);
                assert_eq!(s.root(), f.root());
                assert_eq!(s.first_lsn(), 4);
                assert_eq!(s.boundary.predecessor.receipt.lsn, 2);
                for (x, y) in s.records().iter().zip(&f.records[3..]) {
                    assert_eq!(x.receipt, y.receipt);
                    assert_eq!(x.record, y.record);
                }
                let object = f.records[0].record.objects.first().unwrap();
                let at = object.dest();
                let origin = a.origin(at.at).unwrap();
                let receipt = origin.receipt;
                assert!(a.authorizes(&at, &receipt));
                let mut bad = at;
                bad.at.offset += 8;
                assert!(!a.authorizes(&bad, &receipt));
                bad = at;
                bad.at.segment += 1;
                assert!(!a.authorizes(&bad, &receipt));
                bad = at;
                bad.length += 1;
                assert!(!a.authorizes(&bad, &receipt));
                bad = at;
                bad.logical_generation ^= 1;
                assert!(!a.authorizes(&bad, &receipt));
                for field in 0..10 {
                    let mut r = receipt;
                    match field {
                        0 => r.uuid[0] ^= 1,
                        1 => r.epoch += 1,
                        2 => r.creation_lsn += 1,
                        3 => r.creation_sequence += 1,
                        4 => r.creation_commit ^= 1,
                        5 => r.provenance_digest[0] ^= 1,
                        6 => {
                            if let ulmdb_storage::hybrid::redo::Authority::Wal {
                                ref mut lsn, ..
                            } = r.authority
                            {
                                *lsn += 1
                            }
                        }
                        7 => {
                            if let ulmdb_storage::hybrid::redo::Authority::Wal {
                                ref mut sequence,
                                ..
                            } = r.authority
                            {
                                *sequence += 1
                            }
                        }
                        8 => {
                            if let ulmdb_storage::hybrid::redo::Authority::Wal {
                                ref mut digest,
                                ..
                            } = r.authority
                            {
                                digest[0] ^= 1
                            }
                        }
                        _ => {
                            r.authority = ulmdb_storage::hybrid::redo::Authority::Checkpoint {
                                generation: 1,
                                sequence: r.creation_sequence,
                                digest: r.provenance_digest,
                            }
                        }
                    };
                    assert!(!a.authorizes(&at, &r), "identity field {field}");
                }
                let before = fs::read(path.join("HYBRID.wal")).unwrap();
                let permit = e.prepare_retirement(CheckpointLimits::default()).unwrap();
                assert_eq!(permit.report().validated_through, 7);
                assert_eq!(permit.report().checkpoint_sequence, 3);
                assert_eq!(before, fs::read(path.join("HYBRID.wal")).unwrap());
                drop(permit);
            }
        }
    }
}
#[test]
fn captures_block_retirement_without_poisoning_and_materialized_readers_do_not() {
    let d = Dir::new(Path::new("/tmp"));
    let path = d.0.join("db");
    let o = options(ImageMode::Anonymous, WalLayout::Dense);
    let e = Environment::create(&path, o).unwrap();
    set(&e, 0);
    let old = e.begin_read();
    let bytes = old.get(b"kv", b"state").unwrap().unwrap();
    let mut p = e.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    e.activate_checkpoint(&mut p, CheckpointLimits::default())
        .unwrap();
    assert!(matches!(
        e.prepare_retirement(CheckpointLimits::default()),
        Err(Error::Busy)
    ));
    set(&e, 1);
    drop(p);
    let permit = e.prepare_retirement(CheckpointLimits::default()).unwrap();
    assert!(matches!(
        e.prepare_checkpoint(CheckpointLimits::default()),
        Err(Error::Busy)
    ));
    set(&e, 2);
    assert_eq!(bytes[100..108], 0u64.to_le_bytes());
    drop(e);
    assert!(matches!(Environment::open(&path, o), Err(Error::Busy)));
    drop(permit);
    let e = Environment::open(&path, o).unwrap();
    let p = e.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    drop(e);
    let e = Environment::open(&path, o).unwrap();
    assert!(matches!(
        e.prepare_retirement(CheckpointLimits::default()),
        Err(Error::Busy)
    ));
    set(&e, 3);
    drop(p);
    e.prepare_retirement(CheckpointLimits::default()).unwrap();
    assert_eq!(bytes[100..108], 0u64.to_le_bytes());
}
#[test]
fn retained_boundary_survives_obsolete_prefix_zeroing_without_whole_prefix_fallback() {
    for layout in [
        WalLayout::Dense,
        WalLayout::PreSized {
            capacity: 8 * 1024 * 1024,
        },
    ] {
        let d = Dir::new(Path::new("/tmp"));
        let path = d.0.join("db");
        let e = Environment::create(&path, options(ImageMode::File, layout)).unwrap();
        for n in 0..5 {
            set(&e, n);
        }
        install(&e);
        for n in 5..8 {
            set(&e, n);
        }
        let a = aliases(&path);
        let source = path.join("HYBRID.wal");
        let copy = d.0.join("copy");
        fs::copy(&source, &copy).unwrap();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&copy)
            .unwrap();
        let s = retirement::inspect(&file, &a).unwrap();
        let start = 196608;
        let end = s.boundary.predecessor.receipt.offset;
        file.write_all_at(&vec![0; (end - start) as usize], start)
            .unwrap();
        assert!(recovery::inspect(&file).is_err());
        let after = retirement::inspect(&file, &a).unwrap();
        assert_eq!(after.selected, s.selected);
        assert_eq!(after.root(), s.root());
        // Both boundary bodies are authenticated by the selected checkpoint chain.
        for offset in [
            s.boundary.predecessor.receipt.offset,
            s.boundary.base.receipt.offset,
        ] {
            let original = fs::read(&copy).unwrap();
            file.write_all_at(&[255], offset).unwrap();
            assert!(retirement::inspect(&file, &a).is_err());
            file.write_all_at(&original, 0).unwrap();
        }
    }
}
#[test]
fn one_bad_head_requires_exact_successor_and_never_falls_back_to_cut() {
    for layout in [
        WalLayout::Dense,
        WalLayout::PreSized {
            capacity: 8 * 1024 * 1024,
        },
    ] {
        let d = Dir::new(Path::new("/tmp"));
        let path = d.0.join("db");
        let e = Environment::create(&path, options(ImageMode::Anonymous, layout)).unwrap();
        for n in 0..3 {
            set(&e, n);
        }
        install(&e);
        let a = aliases(&path);
        for suffix in [false, true] {
            if suffix {
                set(&e, 3);
                set(&e, 4);
            }
            let original = fs::read(path.join("HYBRID.wal")).unwrap();
            let copy = d.0.join(if suffix { "later" } else { "at-cut" });
            fs::write(&copy, &original).unwrap();
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&copy)
                .unwrap();
            let selected = retirement::inspect(&f, &a).unwrap();
            // Losing newer head is recoverable through exactly one retained successor.
            f.write_all_at(&[255], WITNESS_OFFSETS[selected.slot])
                .unwrap();
            let p = retirement::inspect(&f, &a).unwrap();
            assert!(p.heal);
            assert_eq!(p.selected, selected.selected);
            // A complete successor plus any extra nonzero tail is ambiguous.
            let extra = selected.selected.receipt.end().unwrap();
            f.write_all_at(&[1], extra).unwrap();
            assert!(retirement::inspect(&f, &a).is_err());
            f.set_len(original.len() as u64).unwrap();
            f.write_all_at(&original, 0).unwrap();
            f.write_all_at(&[255], WITNESS_OFFSETS[1 - selected.slot])
                .unwrap();
            assert!(retirement::inspect(&f, &a).is_err());
            f.write_all_at(&[255], WITNESS_OFFSETS[selected.slot])
                .unwrap();
            assert!(retirement::inspect(&f, &a).is_err());
        }
    }
}
#[test]
fn genesis_and_no_selected_checkpoint_are_explicit() {
    let d = Dir::new(Path::new("/tmp"));
    let path = d.0.join("db");
    let e = Environment::create(&path, options(ImageMode::Anonymous, WalLayout::Dense)).unwrap();
    assert!(matches!(
        e.prepare_retirement(CheckpointLimits::default()),
        Err(Error::InvalidInput(_))
    ));
    install(&e);
    assert!(matches!(
        e.prepare_retirement(CheckpointLimits::default()),
        Err(Error::InvalidInput(_))
    ));
    assert!(!path.join("HYBRID.required").exists());
    set(&e, 0);
}
#[test]
fn dropped_database_suffix_and_authoritative_validation_failure_keep_error_boundaries() {
    let d = Dir::new(Path::new("/tmp"));
    let path = d.0.join("db");
    let o = options(ImageMode::Anonymous, WalLayout::Dense);
    let e = Environment::create(&path, o).unwrap();
    set(&e, 0);
    install(&e);
    let a = aliases(&path);
    let mut w = e.begin_write().unwrap();
    w.drop_database(b"kv").unwrap();
    w.commit().unwrap();
    let wal = path.join("HYBRID.wal");
    let file = File::open(&wal).unwrap();
    let full = recovery::inspect(&file).unwrap();
    let seeded = retirement::inspect(&file, &a).unwrap();
    assert_eq!(seeded.root(), full.root());
    let p = e.prepare_retirement(CheckpointLimits::default()).unwrap();
    drop(p);
    // A corrupt authoritative body fences subsequent writes, whereas all Busy
    // gate failures above leave them available. The head bytes remain valid.
    let offset = full.records[0].receipt.offset;
    drop(file);
    let file = OpenOptions::new().write(true).open(wal).unwrap();
    file.write_all_at(&[255], offset).unwrap();
    assert!(matches!(
        e.prepare_retirement(CheckpointLimits::default()),
        Err(Error::Corrupt(_))
    ));
    assert!(matches!(e.begin_write(), Err(Error::OutcomeUnknown)));
}

#[test]
fn zero_object_metadata_record_uses_absolute_seeded_lsn_without_chunk_jump() {
    use ulmdb_storage::hybrid::{
        journal::Journal,
        redo::{Extent, Record, SourceReceipt, SourceResolver},
    };
    struct NoSources;
    impl SourceResolver for NoSources {
        fn resolve_committed<'a>(
            &'a self,
            _: &Extent,
            _: &SourceReceipt,
        ) -> ulmdb_storage::Result<&'a [u8]> {
            panic!("zero-object metadata record must never resolve a source")
        }
    }
    let d = Dir::new(Path::new("/tmp"));
    let path = d.0.join("db");
    let e = Environment::create(&path, options(ImageMode::Anonymous, WalLayout::Dense)).unwrap();
    install(&e);
    let a = aliases(&path);
    drop(e);
    let wal = path.join("HYBRID.wal");
    let (mut journal, ()) = Journal::open_checked(&wal, |_| Ok(())).unwrap();
    let header = journal.bootstrap();
    for n in 1..=3 {
        let record = Record {
            uuid: header.uuid,
            epoch: header.epoch,
            lsn: n,
            sequence: n,
            commit: ulmdb_storage::new_identity().unwrap(),
            parent_commit: journal.commit_id(),
            parent: journal.selected(),
            root: journal.root(),
            chunks: Vec::new(),
            objects: Vec::new(),
        };
        journal
            .commit_checked(&record, &NoSources, |_| Ok(()))
            .unwrap();
    }
    let f = File::open(&wal).unwrap();
    let full = recovery::inspect(&f).unwrap();
    let seeded = retirement::inspect(&f, &a).unwrap();
    assert_eq!(seeded.first_lsn(), 1);
    assert_eq!(seeded.records().len(), 3);
    assert_eq!(seeded.selected, full.selected);
    assert_eq!(seeded.root().next_chunk, 1);
    assert!(
        seeded
            .records()
            .iter()
            .all(|d| d.record.objects.is_empty() && d.record.chunks.is_empty())
    );
}
#[test]
fn selected_checkpoint_authority_failures_fence_retirement_and_preserve_old_reads() {
    for kind in 0..5 {
        let d = Dir::new(Path::new("/tmp"));
        let path = d.0.join("db");
        let o = options(ImageMode::Anonymous, WalLayout::Dense);
        let e = Environment::create(&path, o).unwrap();
        set(&e, 0);
        install(&e);
        let selected = selection::read(&path).unwrap().unwrap();
        let old = e.begin_read();
        let held = old.get(b"kv", b"state").unwrap().unwrap();
        let asset = match kind {
            0 => selected
                .directory(&path)
                .join("chunk-0000000000000001.image"),
            1 => selected
                .directory(&path)
                .join("chunk-0000000000000001.origins"),
            2 | 4 => path.join("HYBRID.base"),
            _ => selected.directory(&path).join("CHECKPOINT"),
        };
        let original = fs::read(&asset).unwrap();
        let mut bad = original.clone();
        if kind == 4 {
            fs::remove_file(&asset).unwrap();
        } else {
            let file = File::open(&asset).unwrap();
            ulmdb_platform::set_private_permissions(&file, true).unwrap();
            bad[0] ^= 1;
            fs::write(&asset, &bad).unwrap();
        }
        let error = e
            .prepare_retirement(CheckpointLimits::default())
            .err()
            .unwrap();
        assert!(matches!(error, Error::Corrupt(_) | Error::Io(_)));
        assert!(matches!(e.begin_write(), Err(Error::OutcomeUnknown)));
        assert_eq!(held[100..108], 0u64.to_le_bytes());
        fs::write(&asset, &original).unwrap();
        drop(e);
        let e = Environment::open(&path, o).unwrap();
        set(&e, 1);
    }
}
#[test]
fn retirement_caller_limits_do_not_fence_selected_authority() {
    let d = Dir::new(Path::new("/tmp"));
    let path = d.0.join("db");
    let e = Environment::create(&path, options(ImageMode::Anonymous, WalLayout::Dense)).unwrap();
    set(&e, 0);
    install(&e);
    for kind in 0..4 {
        let mut limits = CheckpointLimits::default();
        match kind {
            0 => limits.max_chunks = 0,
            1 => limits.max_provenance_bytes = 0,
            2 => limits.max_apparent_bytes = 0,
            _ => limits.max_allocated_bytes = 0,
        };
        assert!(matches!(
            e.prepare_retirement(limits),
            Err(Error::LimitExceeded)
        ));
        set(&e, kind + 1);
    }
    e.prepare_retirement(CheckpointLimits::default()).unwrap();
}
#[test]
fn coherent_pre_checkpoint_heads_cannot_regress_the_selected_seed() {
    for layout in [
        WalLayout::Dense,
        WalLayout::PreSized {
            capacity: 8 * 1024 * 1024,
        },
    ] {
        let d = Dir::new(Path::new("/tmp"));
        let path = d.0.join("db");
        let e = Environment::create(&path, options(ImageMode::Anonymous, layout)).unwrap();
        set(&e, 0);
        let earlier = fs::read(path.join("HYBRID.wal")).unwrap();
        set(&e, 1);
        set(&e, 2);
        install(&e);
        let a = aliases(&path);
        let copy = d.0.join("old-head-pair");
        fs::copy(path.join("HYBRID.wal"), &copy).unwrap();
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&copy)
            .unwrap();
        for at in WITNESS_OFFSETS {
            let at = at as usize;
            f.write_all_at(&earlier[at..at + 4096], at as u64).unwrap();
        }
        assert!(retirement::inspect(&f, &a).is_err());
    }
}
