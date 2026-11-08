//! Full-WAL recovery-base activation. This is not WAL retirement qualification.
use std::{
    fs,
    path::{Path, PathBuf},
};
use ulmdb::{
    Address, DatabaseKind, Error, Prefix,
    hybrid::{Environment, ImageMode, Options, RedoMode},
};
use ulmdb_storage::hybrid::checkpoint::{CheckpointLimits, verify_artifact};
struct Dir(PathBuf);
impl Dir {
    fn new(base: &Path) -> Self {
        let p = base.join(format!(
            "ulmdb-h5-activate-{}",
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
fn options(image: ImageMode, redo: RedoMode) -> Options {
    Options {
        image,
        redo,
        chunk: ulmdb_storage::hybrid::ChunkLimits {
            capacity: 1024 * 1024,
            reserve_quantum: 65536,
            max_burned_bytes: 512 * 1024,
        },
        max_image_bytes: 8 * 1024 * 1024,
        ..Options::default()
    }
}
fn set(env: &Environment, n: u64, create: bool) {
    let mut w = env.begin_write().unwrap();
    if create {
        for (name, kind) in [
            (b"kv".as_slice(), DatabaseKind::Kv),
            (b"v4", DatabaseKind::Ipv4),
            (b"v6", DatabaseKind::Ipv6),
        ] {
            w.create_database(name, kind).unwrap();
        }
    }
    w.put(b"kv", b"state", &n.to_le_bytes()).unwrap();
    w.insert_prefix(
        b"v4",
        Prefix::new(Address::V4(0x0a000000), 8).unwrap(),
        &n.to_le_bytes(),
    )
    .unwrap();
    w.insert_prefix(
        b"v6",
        Prefix::new(Address::V6(0x20010db8000000000000000000000000), 32).unwrap(),
        &n.to_le_bytes(),
    )
    .unwrap();
    w.commit().unwrap();
}
fn check(env: &Environment, n: u64) {
    let r = env.begin_read();
    assert_eq!(
        r.get(b"kv", b"state").unwrap(),
        Some(n.to_le_bytes().as_slice())
    );
    assert_eq!(
        r.longest_match(b"v4", Address::V4(0x0a010203))
            .unwrap()
            .unwrap()
            .1,
        n.to_le_bytes()
    );
    assert_eq!(
        r.longest_match(b"v6", Address::V6(0x20010db8000000000000000000000001))
            .unwrap()
            .unwrap()
            .1,
        n.to_le_bytes()
    );
}
#[test]
fn installing_cut_c_preserves_later_head_h_and_replays_suffix_into_fresh_backings() {
    for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
        for image in [ImageMode::File, ImageMode::Anonymous] {
            for redo in [RedoMode::Compact, RedoMode::FullPostimage] {
                let d = Dir::new(base);
                let path = d.0.join("engine");
                let o = options(image, redo);
                let env = Environment::create(&path, o).unwrap();
                set(&env, 0, true);
                let old = env.begin_read();
                let held = old.get(b"kv", b"state").unwrap().unwrap();
                let mut prepared = env.prepare_checkpoint(CheckpointLimits::default()).unwrap();
                assert_eq!(prepared.selection().receipt.sequence, 1);
                for n in 1..=3 {
                    set(&env, n, false);
                }
                let h = env.sequence();
                let wal = fs::read(path.join("HYBRID.wal")).unwrap();
                let report = env
                    .activate_checkpoint(&mut prepared, CheckpointLimits::default())
                    .unwrap();
                assert_eq!(report.current_sequence, h);
                assert_eq!(report.checkpoint_sequence, 1);
                assert_eq!(env.sequence(), h);
                check(&env, 3);
                assert_eq!(held, 0u64.to_le_bytes());
                assert_eq!(wal, fs::read(path.join("HYBRID.wal")).unwrap());
                assert!(
                    env.activate_checkpoint(&mut prepared, CheckpointLimits::default())
                        .unwrap()
                        .already_selected
                );
                drop(env);
                let replay = if image == ImageMode::File {
                    ImageMode::Anonymous
                } else {
                    ImageMode::File
                };
                let env = Environment::open(&path, options(replay, redo)).unwrap();
                assert_eq!(env.sequence(), h);
                check(&env, 3);
                assert_eq!(held, 0u64.to_le_bytes());
                assert_eq!(wal, fs::read(path.join("HYBRID.wal")).unwrap());
                assert!(matches!(
                    env.activate_checkpoint(&mut prepared, CheckpointLimits::default()),
                    Err(Error::InvalidInput(_))
                ));
                set(&env, 4, false);
                drop(env);
                let env = Environment::open(&path, o).unwrap();
                check(&env, 4);
            }
        }
    }
}
#[test]
fn genesis_activation_and_zero_suffix_keep_reserved_chunk_unpublished() {
    for image in [ImageMode::File, ImageMode::Anonymous] {
        let d = Dir::new(Path::new("/tmp"));
        let path = d.0.join("engine");
        let o = options(image, RedoMode::Compact);
        let env = Environment::create(&path, o).unwrap();
        let mut p = env.prepare_checkpoint(CheckpointLimits::default()).unwrap();
        assert_eq!(p.selection().root.next_chunk, 1);
        env.activate_checkpoint(&mut p, CheckpointLimits::default())
            .unwrap();
        drop(env);
        let env = Environment::open(&path, o).unwrap();
        assert_eq!(env.sequence(), 0);
        set(&env, 7, true);
        drop(env);
        let env = Environment::open(&path, o).unwrap();
        check(&env, 7);
    }
}
#[test]
fn monotone_coverage_rejects_older_same_env_prepared_cut_without_head_change() {
    let d = Dir::new(Path::new("/tmp"));
    let path = d.0.join("engine");
    let env = Environment::create(&path, options(ImageMode::Anonymous, RedoMode::Compact)).unwrap();
    set(&env, 0, true);
    let mut old = env.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    set(&env, 1, false);
    let mut new = env.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    set(&env, 2, false);
    env.activate_checkpoint(&mut new, CheckpointLimits::default())
        .unwrap();
    let before = fs::read(path.join("HYBRID.base")).unwrap();
    assert!(matches!(
        env.activate_checkpoint(&mut old, CheckpointLimits::default()),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(before, fs::read(path.join("HYBRID.base")).unwrap());
    check(&env, 2);
    drop(env);
    let env = Environment::open(&path, options(ImageMode::File, RedoMode::Compact)).unwrap();
    check(&env, 2);
}
#[test]
fn abort_burned_gaps_are_not_object_authority_and_recovery_zeroes_them() {
    for image in [ImageMode::File, ImageMode::Anonymous] {
        let d = Dir::new(Path::new("/tmp"));
        let path = d.0.join("engine");
        let o = options(image, RedoMode::Compact);
        let env = Environment::create(&path, o).unwrap();
        set(&env, 0, true);
        let mut aborted = env.begin_write().unwrap();
        aborted.put(b"kv", b"aborted", &vec![91; 65536]).unwrap();
        drop(aborted);
        set(&env, 1, false);
        let mut p = env.prepare_checkpoint(CheckpointLimits::default()).unwrap();
        let desc = verify_artifact(p.directory(), CheckpointLimits::default()).unwrap();
        assert!(desc.chunks[0].end > 65536);
        env.activate_checkpoint(&mut p, CheckpointLimits::default())
            .unwrap();
        drop(env);
        let env = Environment::open(&path, o).unwrap();
        check(&env, 1);
        assert!(env.begin_read().get(b"kv", b"aborted").unwrap().is_none());
        set(&env, 2, false);
        drop(env);
        let env = Environment::open(&path, o).unwrap();
        check(&env, 2);
    }
}
#[test]
fn selected_artifact_corruption_fails_closed_and_never_falls_back_to_full_wal() {
    let d = Dir::new(Path::new("/tmp"));
    let path = d.0.join("engine");
    let o = options(ImageMode::File, RedoMode::Compact);
    let env = Environment::create(&path, o).unwrap();
    set(&env, 0, true);
    let mut p = env.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    env.activate_checkpoint(&mut p, CheckpointLimits::default())
        .unwrap();
    set(&env, 1, false);
    let old = env.begin_read();
    drop(env);
    let image = p.directory().join("chunk-0000000000000001.image");
    let mut bytes = fs::read(&image).unwrap();
    bytes[4096] ^= 1;
    ulmdb_platform::set_private_permissions(&std::fs::File::open(&image).unwrap(), true).unwrap();
    fs::write(&image, bytes).unwrap();
    let wal = fs::read(path.join("HYBRID.wal")).unwrap();
    assert!(Environment::open(&path, o).is_err());
    assert_eq!(wal, fs::read(path.join("HYBRID.wal")).unwrap());
    assert_eq!(
        old.get(b"kv", b"state").unwrap(),
        Some(1u64.to_le_bytes().as_slice())
    );
}

fn forge_structurally_valid_staged_value(directory: &Path) -> ulmdb_storage::Reference {
    use sha2::{Digest, Sha256};

    use ulmdb_storage::hybrid::checkpoint::CheckpointArchive;
    let archive = CheckpointArchive::open(directory, CheckpointLimits::default()).unwrap();
    let descriptor = archive.descriptor();
    let database = ulmdb_kv::get(&archive, descriptor.selected.root.catalog, b"kv")
        .unwrap()
        .unwrap();
    let root = ulmdb_storage::Reference::decode(&database[16..32]).unwrap();
    let value = ulmdb_kv::get(&archive, root, b"state").unwrap().unwrap();
    let whole = archive
        .chunks()
        .find(|(id, _)| **id == root.segment)
        .unwrap()
        .1
        .read(0, descriptor.chunks[0].end as usize)
        .unwrap();
    let offset = (value.as_ptr() as usize)
        .checked_sub(whole.as_ptr() as usize)
        .unwrap();
    assert_eq!(value.len(), 8);
    // End every immutable archive mapping before adversarial backing-file writes.
    drop(archive);
    let filename = directory.join(format!("chunk-{:016x}.image", root.segment));
    let mut bytes = fs::read(&filename).unwrap();
    bytes[offset..offset + 8].copy_from_slice(&99u64.to_le_bytes());
    ulmdb_platform::set_private_permissions(&std::fs::File::open(&filename).unwrap(), true)
        .unwrap();
    fs::write(&filename, &bytes).unwrap();
    ulmdb_platform::set_private_permissions(&std::fs::File::open(&filename).unwrap(), false)
        .unwrap();
    let mut h = Sha256::new();
    h.update(b"ULMDB/hybrid/checkpoint-image/v1\0");
    h.update(&bytes);
    let image_digest = h.finalize();
    let path = directory.join("CHECKPOINT");
    let mut d = fs::read(&path).unwrap();
    d[256 + 40..256 + 72].copy_from_slice(&image_digest);
    let length = d.len();
    let mut h = Sha256::new();
    h.update(b"ULMDB/hybrid/checkpoint-artifact/v1\0");
    h.update(&d[..length - 32]);
    d[length - 32..].copy_from_slice(&h.finalize());
    ulmdb_platform::set_private_permissions(&std::fs::File::open(&path).unwrap(), true).unwrap();
    fs::write(&path, d).unwrap();
    ulmdb_platform::set_private_permissions(&std::fs::File::open(&path).unwrap(), false).unwrap();
    let forged = CheckpointArchive::open(directory, CheckpointLimits::default()).unwrap();
    ulmdb_kv::audit_tree(&forged, root, 100).unwrap();
    assert_eq!(
        ulmdb_kv::get(&forged, root, b"state").unwrap(),
        Some(99u64.to_le_bytes().as_slice())
    );
    root
}
#[test]
fn rehashed_valid_artifact_cannot_mint_private_facade_authority() {
    let d = Dir::new(Path::new("/tmp"));
    let path = d.0.join("engine");
    let env = Environment::create(&path, options(ImageMode::Anonymous, RedoMode::Compact)).unwrap();
    set(&env, 0, true);
    let mut p = env.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    forge_structurally_valid_staged_value(p.directory());
    assert!(verify_artifact(p.directory(), CheckpointLimits::default()).is_ok());
    assert!(matches!(
        env.activate_checkpoint(&mut p, CheckpointLimits::default()),
        Err(Error::Corrupt("checkpoint cut image bytes differ"))
    ));
    assert!(!path.join("HYBRID.base").exists());
    check(&env, 0);
    set(&env, 1, false);
}
#[test]
fn rehashed_selected_artifact_only_fails_persisted_selector_binding() {
    let d = Dir::new(Path::new("/tmp"));
    let path = d.0.join("engine");
    let o = options(ImageMode::File, RedoMode::Compact);
    let env = Environment::create(&path, o).unwrap();
    set(&env, 0, true);
    let mut p = env.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    env.activate_checkpoint(&mut p, CheckpointLimits::default())
        .unwrap();
    let selected = fs::read(path.join("HYBRID.base")).unwrap();
    drop(env);
    forge_structurally_valid_staged_value(p.directory());
    assert!(verify_artifact(p.directory(), CheckpointLimits::default()).is_ok());
    let wal = fs::read(path.join("HYBRID.wal")).unwrap();
    assert!(Environment::open(&path, o).is_err());
    assert_eq!(selected, fs::read(path.join("HYBRID.base")).unwrap());
    assert_eq!(wal, fs::read(path.join("HYBRID.wal")).unwrap());
}
#[test]
fn presized_genesis_and_nonempty_base_activation_keep_capacity_and_cross_backings() {
    for image in [ImageMode::File, ImageMode::Anonymous] {
        for empty in [false, true] {
            let d = Dir::new(Path::new("/tmp"));
            let path = d.0.join("engine");
            let mut o = options(image, RedoMode::Compact);
            o.wal_layout = ulmdb::hybrid::WalLayout::PreSized {
                capacity: 2 * 1024 * 1024,
            };
            let env = Environment::create(&path, o).unwrap();
            if !empty {
                set(&env, 0, true);
            }
            let mut p = env.prepare_checkpoint(CheckpointLimits::default()).unwrap();
            if empty {
                set(&env, 1, true);
            } else {
                set(&env, 1, false);
            }
            let h = env.sequence();
            let wal = fs::read(path.join("HYBRID.wal")).unwrap();
            assert_eq!(wal.len(), 2 * 1024 * 1024);
            env.activate_checkpoint(&mut p, CheckpointLimits::default())
                .unwrap();
            assert_eq!(env.sequence(), h);
            assert_eq!(wal, fs::read(path.join("HYBRID.wal")).unwrap());
            drop(env);
            o.image = if image == ImageMode::File {
                ImageMode::Anonymous
            } else {
                ImageMode::File
            };
            let env = Environment::open(&path, o).unwrap();
            check(&env, 1);
            assert_eq!(env.sequence(), h);
            assert_eq!(
                fs::metadata(path.join("HYBRID.wal")).unwrap().len(),
                2 * 1024 * 1024
            );
            set(&env, 2, false);
            drop(env);
            let env = Environment::open(&path, o).unwrap();
            check(&env, 2);
        }
    }
}

#[test]
fn metadata_only_suffix_after_selected_base_preserves_counters_and_original_origins() {
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
        ) -> ulmdb::Result<&'a [u8]> {
            Err(Error::Corrupt("no metadata source"))
        }
    }
    for image in [ImageMode::File, ImageMode::Anonymous] {
        let d = Dir::new(Path::new("/tmp"));
        let path = d.0.join("engine");
        let mut o = options(image, RedoMode::Compact);
        o.wal_layout = ulmdb::hybrid::WalLayout::PreSized {
            capacity: 2 * 1024 * 1024,
        };
        let env = Environment::create(&path, o).unwrap();
        set(&env, 0, true);
        let old = env.begin_read();
        let mut p = env.prepare_checkpoint(CheckpointLimits::default()).unwrap();
        env.activate_checkpoint(&mut p, CheckpointLimits::default())
            .unwrap();
        drop(env);
        let (mut j, mut root) =
            Journal::open_checked(&path.join("HYBRID.wal"), |plan| Ok(plan.root())).unwrap();
        let header = j.bootstrap();
        let parent = j.selected();
        root.catalog = ulmdb_storage::Reference::NULL;
        root.next_database += 1;
        j.commit_checked(
            &Record {
                uuid: header.uuid,
                epoch: header.epoch,
                lsn: parent.lsn + 1,
                sequence: parent.sequence + 1,
                commit: header.commit + u128::from(parent.lsn + 1),
                parent_commit: j.commit_id(),
                parent,
                root,
                chunks: Vec::new(),
                objects: Vec::new(),
            },
            &NoSources,
            |_| Ok(()),
        )
        .unwrap();
        drop(j);
        let env = Environment::open(&path, o).unwrap();
        assert_eq!(env.sequence(), 2);
        assert_eq!(env.begin_read().database_count(), 0);
        assert_eq!(
            old.get(b"kv", b"state").unwrap(),
            Some(0u64.to_le_bytes().as_slice())
        );
        let mut next = env.prepare_checkpoint(CheckpointLimits::default()).unwrap();
        assert_eq!(next.selection().root.next_chunk, 2);
        assert_eq!(next.selection().root.next_database, 5);
        env.activate_checkpoint(&mut next, CheckpointLimits::default())
            .unwrap();
        let archive = ulmdb_storage::hybrid::checkpoint::CheckpointArchive::open(
            next.directory(),
            CheckpointLimits::default(),
        )
        .unwrap();
        assert!(
            archive
                .chunks()
                .flat_map(|(_, c)| c.table().origins())
                .all(|o| o.receipt.creation_lsn == 1)
        );
        drop(archive);
        set(&env, 9, true);
        drop(env);
        let env = Environment::open(&path, o).unwrap();
        check(&env, 9);
    }
}
