//! Compound recovery bases retain all physical WAL bytes. No reclamation claim.
use std::{
    fs,
    path::{Path, PathBuf},
};
use ulmdb::{
    Address, DatabaseKind, Error, Prefix,
    hybrid::{Environment, ImageMode, LiveRedoMode, Options, WalLayout},
};
use ulmdb_storage::hybrid::{
    checkpoint::CheckpointLimits,
    retirement::selection::{RecoveryBase, read_typed},
};
struct Dir(PathBuf);
impl Dir {
    fn new(base: &Path) -> Self {
        let path = base.join(format!(
            "ulmdb-floor-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
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
        w.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
        w.create_database(b"v6", DatabaseKind::Ipv6).unwrap();
    }
    let mut value = vec![33u8; 8192];
    value[300..308].copy_from_slice(&n.to_le_bytes());
    w.put(b"kv", b"state", &value).unwrap();
    w.insert_prefix(
        b"v4",
        Prefix::new(Address::V4(0xc0000200), 24).unwrap(),
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
fn check(e: &Environment, n: u64) {
    let r = e.begin_read();
    assert_eq!(
        &r.get(b"kv", b"state").unwrap().unwrap()[300..308],
        &n.to_le_bytes()
    );
    assert_eq!(
        r.longest_match(b"v4", Address::V4(0xc0000201))
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
fn checkpoint(e: &Environment) {
    let mut p = e.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    e.activate_checkpoint(&mut p, CheckpointLimits::default())
        .unwrap();
}
#[test]
fn compound_activation_keeps_h_and_replays_exact_sources_into_fresh_owners() {
    for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
        for image in [ImageMode::File, ImageMode::Anonymous] {
            for layout in [
                WalLayout::Dense,
                WalLayout::PreSized {
                    capacity: 8 * 1024 * 1024,
                },
            ] {
                for mode in [LiveRedoMode::Owned, LiveRedoMode::Borrowed] {
                    let d = Dir::new(base);
                    let path = d.0.join("engine");
                    let o = Options {
                        live_redo: mode,
                        ..options(image, layout)
                    };
                    let e = Environment::create(&path, o).unwrap();
                    set(&e, 0);
                    set(&e, 1);
                    checkpoint(&e);
                    let old = e.begin_read();
                    let borrowed = old.get(b"kv", b"state").unwrap().unwrap();
                    set(&e, 2);
                    let sequence = e.sequence();
                    let wal = fs::read(path.join("HYBRID.wal")).unwrap();
                    let mut p = e.prepare_retirement(CheckpointLimits::default()).unwrap();
                    let report = e.activate_retirement(&mut p).unwrap();
                    assert_eq!(report.current_sequence, sequence);
                    assert_eq!(report.checkpoint_sequence, 2);
                    assert_eq!(e.sequence(), sequence);
                    check(&e, 2);
                    assert_eq!(&borrowed[300..308], &1u64.to_le_bytes());
                    assert_eq!(wal, fs::read(path.join("HYBRID.wal")).unwrap());
                    assert_eq!(fs::metadata(path.join("HYBRID.base")).unwrap().len(), 512);
                    assert_eq!(
                        fs::metadata(path.join("HYBRID.required")).unwrap().len(),
                        128
                    );
                    let Some(RecoveryBase::Compound(first)) = read_typed(&path).unwrap() else {
                        panic!("compound required")
                    };
                    assert_eq!(first.serial(), 1);
                    assert!(e.activate_retirement(&mut p).unwrap().already_selected);
                    set(&e, 3);
                    checkpoint(&e);
                    let Some(RecoveryBase::Compound(second)) = read_typed(&path).unwrap() else {
                        panic!("C2 must retain V2")
                    };
                    assert!(second.selector().cut.receipt.lsn > first.selector().cut.receipt.lsn);
                    assert_eq!(second.prefix_end(), first.prefix_end());
                    assert_eq!(second.first_required(), first.first_required());
                    assert_eq!(second.header_digest(), first.header_digest());
                    assert!(second.serial() > first.serial());
                    check(&e, 3);
                    drop(e);
                    let e = Environment::open(
                        &path,
                        Options {
                            live_redo: mode,
                            ..options(
                                if image == ImageMode::File {
                                    ImageMode::Anonymous
                                } else {
                                    ImageMode::File
                                },
                                layout,
                            )
                        },
                    )
                    .unwrap();
                    check(&e, 3);
                    assert_eq!(&borrowed[300..308], &1u64.to_le_bytes());
                    assert!(matches!(
                        e.activate_retirement(&mut p),
                        Err(Error::InvalidInput(_))
                    ));
                    set(&e, 4);
                    drop(e);
                    let e = Environment::open(&path, o).unwrap();
                    check(&e, 4);
                }
            }
        }
    }
}
fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir(target).unwrap();
    for item in fs::read_dir(source).unwrap() {
        let item = item.unwrap();
        let destination = target.join(item.file_name());
        if item.file_type().unwrap().is_dir() {
            copy_tree(&item.path(), &destination);
        } else {
            fs::copy(item.path(), destination).unwrap();
        }
    }
}
fn selected(
    base: &Path,
    image: ImageMode,
    layout: WalLayout,
) -> (Dir, PathBuf, Options, Environment) {
    let d = Dir::new(base);
    let path = d.0.join("engine");
    let o = options(image, layout);
    let e = Environment::create(&path, o).unwrap();
    for n in 0..3 {
        set(&e, n);
    }
    checkpoint(&e);
    set(&e, 3);
    let mut p = e.prepare_retirement(CheckpointLimits::default()).unwrap();
    e.activate_retirement(&mut p).unwrap();
    (d, path, o, e)
}
#[test]
fn genesis_refuses_without_damage_and_c1_uses_only_the_canonical_genesis_witness() {
    let d = Dir::new(Path::new("/tmp"));
    let path = d.0.join("engine");
    let o = options(ImageMode::Anonymous, WalLayout::Dense);
    let e = Environment::create(&path, o).unwrap();
    checkpoint(&e);
    let before = fs::read(path.join("HYBRID.base")).unwrap();
    assert!(matches!(
        e.prepare_retirement(CheckpointLimits::default()),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(before, fs::read(path.join("HYBRID.base")).unwrap());
    assert!(!path.join("HYBRID.required").exists());
    set(&e, 0);
    checkpoint(&e);
    let mut p = e.prepare_retirement(CheckpointLimits::default()).unwrap();
    let r = e.activate_retirement(&mut p).unwrap();
    assert_eq!(r.first_required_lsn, 0);
    assert_eq!(r.proposed_prefix_end, 196608);
    drop(e);
    let e = Environment::open(&path, o).unwrap();
    check(&e, 0);
    set(&e, 1);
}
#[test]
fn burned_image_gaps_and_zeroed_copied_obsolete_wal_prefix_recover_without_full_history_fallback() {
    use std::fs::OpenOptions;
    use ulmdb_platform::FileExt;
    for layout in [
        WalLayout::Dense,
        WalLayout::PreSized {
            capacity: 8 * 1024 * 1024,
        },
    ] {
        let d = Dir::new(Path::new("/tmp"));
        let path = d.0.join("engine");
        let o = options(ImageMode::File, layout);
        let e = Environment::create(&path, o).unwrap();
        set(&e, 0);
        set(&e, 1);
        {
            let mut w = e.begin_write().unwrap();
            w.put(b"kv", b"abandoned", &vec![81; 65536]).unwrap();
            drop(w);
        }
        set(&e, 2);
        checkpoint(&e);
        set(&e, 3);
        let mut p = e.prepare_retirement(CheckpointLimits::default()).unwrap();
        e.activate_retirement(&mut p).unwrap();
        let Some(RecoveryBase::Compound(c)) = read_typed(&path).unwrap() else {
            panic!()
        };
        let clone = d.0.join("copied");
        copy_tree(&path, &clone);
        let end = c.first_required().receipt.offset;
        assert!(end > 196608);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(clone.join("HYBRID.wal"))
            .unwrap();
        file.write_all_at(&vec![0; (end - 196608) as usize], 196608)
            .unwrap();
        assert!(ulmdb_storage::hybrid::recovery::inspect(&file).is_err());
        drop(file);
        let rebuilt = Environment::open(&clone, options(ImageMode::Anonymous, layout)).unwrap();
        check(&rebuilt, 3);
        assert_eq!(rebuilt.begin_read().get(b"kv", b"abandoned").unwrap(), None);
        set(&rebuilt, 4);
        drop(rebuilt);
        let rebuilt = Environment::open(&clone, o).unwrap();
        check(&rebuilt, 4);
        check(&e, 3);
    }
}
#[test]
fn required_marker_cuts_fail_closed_or_repair_only_after_valid_selected_graph() {
    for kind in 0..5 {
        let (_d, path, o, e) = selected(Path::new("/tmp"), ImageMode::Anonymous, WalLayout::Dense);
        let wal = fs::read(path.join("HYBRID.wal")).unwrap();
        let marker = path.join("HYBRID.required");
        let selector = path.join("HYBRID.base");
        match kind {
            0 => fs::remove_file(&marker).unwrap(),
            1 => fs::remove_file(&selector).unwrap(),
            2 => {
                ulmdb_platform::set_private_permissions(
                    &std::fs::File::open(&selector).unwrap(),
                    true,
                )
                .unwrap();
                fs::write(&selector, [0; 256]).unwrap();
            }
            _ => {
                ulmdb_platform::set_private_permissions(
                    &std::fs::File::open(&marker).unwrap(),
                    true,
                )
                .unwrap();
                let mut bytes = fs::read(&marker).unwrap();
                bytes[if kind == 3 { 0 } else { 32 }] ^= 1;
                if kind == 4 {
                    use sha2::{Digest, Sha256};
                    let mut h = Sha256::new();
                    h.update(b"ULMDB/hybrid/required-floor/v2\0");
                    h.update(&bytes[..96]);
                    bytes[96..].copy_from_slice(&h.finalize());
                }
                fs::write(&marker, bytes).unwrap();
            }
        }
        drop(e);
        let opened = Environment::open(&path, o);
        if kind == 0 {
            let e = opened.unwrap();
            check(&e, 3);
            assert_eq!(fs::metadata(marker).unwrap().len(), 128);
            assert_eq!(wal, fs::read(path.join("HYBRID.wal")).unwrap());
            set(&e, 4);
        } else {
            assert!(matches!(opened, Err(Error::Corrupt(_))));
        }
        if kind != 0 {
            assert_eq!(wal, fs::read(path.join("HYBRID.wal")).unwrap());
        }
    }
}
#[test]
fn rehashed_immutable_bootstrap_drift_is_rejected_before_any_repair() {
    use sha2::{Digest, Sha256};
    use std::fs::OpenOptions;
    use ulmdb_platform::FileExt;
    for layout in [
        WalLayout::Dense,
        WalLayout::PreSized {
            capacity: 8 * 1024 * 1024,
        },
    ] {
        let (_d, path, o, e) = selected(Path::new("/tmp"), ImageMode::Anonymous, layout);
        drop(e);
        let wal = path.join("HYBRID.wal");
        let mut bytes = fs::read(&wal).unwrap();
        bytes[40] ^= 1;
        let mut h = Sha256::new();
        h.update(if layout == WalLayout::Dense {
            b"ULMDB/hybrid/header/v1\0".as_slice()
        } else {
            b"ULMDB/hybrid/header/v2\0".as_slice()
        });
        h.update(&bytes[..224]);
        bytes[224..256].copy_from_slice(&h.finalize());
        OpenOptions::new()
            .write(true)
            .open(&wal)
            .unwrap()
            .write_all_at(&bytes[..256], 0)
            .unwrap();
        assert!(matches!(
            Environment::open(&path, o),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(bytes, fs::read(wal).unwrap());
    }
}
fn overwrite(path: &Path, bytes: &[u8]) {
    if path.exists() {
        ulmdb_platform::set_private_permissions(&std::fs::File::open(path).unwrap(), true).unwrap();
    }
    fs::write(path, bytes).unwrap();
}
fn rehash(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(domain);
    h.update(bytes);
    h.finalize().into()
}
fn bad_catalog_with_consistent_file_hashes(engine: &Path) {
    let Some(RecoveryBase::Compound(c)) = read_typed(engine).unwrap() else {
        panic!()
    };
    let directory = c.selector().directory(engine);
    let descriptor =
        ulmdb_storage::hybrid::checkpoint::verify_artifact(&directory, CheckpointLimits::default())
            .unwrap();
    assert_eq!(descriptor.chunks.len(), 1);
    assert_eq!(descriptor.selected.root.catalog.segment, 1);
    let image = directory.join("chunk-0000000000000001.image");
    let mut bytes = fs::read(&image).unwrap();
    bytes[descriptor.selected.root.catalog.offset as usize] = 255;
    let digest = rehash(b"ULMDB/hybrid/checkpoint-image/v1\0", &bytes);
    overwrite(&image, &bytes);
    let path = directory.join("CHECKPOINT");
    let mut d = fs::read(&path).unwrap();
    d[296..328].copy_from_slice(&digest);
    let n = d.len();
    let digest = rehash(b"ULMDB/hybrid/checkpoint-artifact/v1\0", &d[..n - 32]);
    d[n - 32..].copy_from_slice(&digest);
    overwrite(&path, &d);
    assert!(
        ulmdb_storage::hybrid::checkpoint::verify_artifact(&directory, CheckpointLimits::default())
            .is_ok()
    );
    let path = engine.join("HYBRID.base");
    let mut pointer = fs::read(&path).unwrap();
    pointer[168..200].copy_from_slice(&digest);
    let digest = rehash(b"ULMDB/hybrid/compound-floor/v2\0", &pointer[..480]);
    pointer[480..].copy_from_slice(&digest);
    overwrite(&path, &pointer);
}
#[test]
fn selected_graph_and_source_validation_precede_heal_and_missing_marker_repair() {
    use std::fs::OpenOptions;
    use ulmdb_platform::FileExt;
    for layout in [
        WalLayout::Dense,
        WalLayout::PreSized {
            capacity: 8 * 1024 * 1024,
        },
    ] {
        let (d, path, o, e) = selected(Path::new("/tmp"), ImageMode::Anonymous, layout);
        drop(e);
        let full = ulmdb_storage::hybrid::recovery::inspect(
            &fs::File::open(path.join("HYBRID.wal")).unwrap(),
        )
        .unwrap();
        let offsets = [65536u64, 131072];
        for mode in 0..4 {
            let target = d.0.join(format!("head-{mode}"));
            copy_tree(&path, &target);
            let wal = target.join("HYBRID.wal");
            let f = OpenOptions::new().write(true).open(&wal).unwrap();
            let bad = if mode == 0 || mode == 3 {
                full.slot
            } else {
                1 - full.slot
            };
            f.write_all_at(&[255], offsets[bad]).unwrap();
            if mode == 2 {
                f.write_all_at(&[255], offsets[1 - bad]).unwrap();
            }
            drop(f);
            if mode == 3 {
                bad_catalog_with_consistent_file_hashes(&target);
                fs::remove_file(target.join("HYBRID.required")).unwrap();
            }
            let before = fs::read(&wal).unwrap();
            let opened = Environment::open(&target, o);
            if mode == 0 {
                let e = opened.unwrap();
                check(&e, 3);
                assert_ne!(before, fs::read(&wal).unwrap());
            } else {
                assert!(matches!(opened, Err(Error::Corrupt(_))));
                assert_eq!(before, fs::read(&wal).unwrap());
                if mode == 3 {
                    assert!(!target.join("HYBRID.required").exists());
                }
            }
        }
    }
}
#[test]
fn full_old_capabilities_block_floor_and_source_authority_survives_late_metadata_errors() {
    let d = Dir::new(Path::new("/tmp"));
    let path = d.0.join("engine");
    let o = options(ImageMode::Anonymous, WalLayout::Dense);
    let e = Environment::create(&path, o).unwrap();
    set(&e, 0);
    set(&e, 1);
    let mut cp = e.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    e.activate_checkpoint(&mut cp, CheckpointLimits::default())
        .unwrap();
    assert!(matches!(
        e.prepare_retirement(CheckpointLimits::default()),
        Err(Error::Busy)
    ));
    set(&e, 2);
    drop(cp);
    let mut p = e.prepare_retirement(CheckpointLimits::default()).unwrap();
    assert!(matches!(e.capture_checkpoint(), Err(Error::Busy)));
    set(&e, 3);
    e.activate_retirement(&mut p).unwrap();
    check(&e, 3);
    let cp = e.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    drop(cp);
    let marker = path.join("HYBRID.required");
    let original = fs::read(&marker).unwrap();
    overwrite(&marker, b"bad");
    assert!(matches!(
        e.prepare_retirement(CheckpointLimits::default()),
        Err(Error::Corrupt(_))
    ));
    assert!(matches!(e.begin_write(), Err(Error::OutcomeUnknown)));
    check(&e, 3);
    overwrite(&marker, &original);
    drop(e);
    let e = Environment::open(&path, o).unwrap();
    check(&e, 3);
    set(&e, 4);
}
