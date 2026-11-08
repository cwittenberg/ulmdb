#![cfg(target_os = "linux")]
use std::{fs, ops::Bound, path::PathBuf, sync::Arc};
use ulmdb::{
    Address, ChunkedReadTxn, DatabaseKind, Environment, Error, PageSize, Prefix, PublicationMode,
};
use ulmdb_storage::{
    chunk_cache::{ChunkCache, Limits},
    chunk_table::PinLimits,
};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "chunked-native-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn modes() -> [PublicationMode; 3] {
    [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ]
}
fn cache() -> Arc<ChunkCache> {
    ChunkCache::new(Limits {
        mapped_bytes: 128 << 10,
        windows: 8,
        chunk_bytes: 16384,
        max_window_bytes: 64 << 10,
    })
    .unwrap()
}

#[test]
fn admission_checks_large_value_extents_without_pinning_their_bodies() {
    for mode in modes() {
        let dir = Dir::new();
        let env = Environment::create_with_publication(&dir.0, mode).unwrap();
        let large = vec![0x37; (2 << 20) + 33];
        let mut write = env.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        write.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
        write.put(b"kv", b"large", &large).unwrap();
        write.put(b"kv", b"small", b"small").unwrap();
        write
            .insert_prefix(
                b"ip",
                Prefix::new(Address::V4(0x0a000000), 8).unwrap(),
                &large,
            )
            .unwrap();
        write.commit().unwrap();
        drop(env);
        let cache = cache();
        let read = ChunkedReadTxn::open_coordinated_snapshot(&dir.0, cache.clone()).unwrap();
        assert_eq!(read.pinned_requests().unwrap(), 0);
        let small = read.get(b"kv", b"small").unwrap().unwrap();
        assert_eq!(small, b"small");
        assert!(matches!(
            read.get(b"kv", b"large"),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(small, b"small");
        assert!(cache.stats().unwrap().mapped_bytes <= 128 << 10);
    }
}
fn seed(env: &Environment) {
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"dup", DatabaseKind::Duplicates { width: Some(8) })
        .unwrap();
    w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
    for k in 0..3000u64 {
        w.put(b"kv", &k.to_be_bytes(), &vec![(k % 251) as u8; 80])
            .unwrap();
    }
    for v in 0..40u64 {
        w.insert_duplicate(b"dup", b"k", &v.to_be_bytes()).unwrap();
    }
    w.insert_prefix(
        b"ip",
        Prefix::new(Address::V4(0x0a000000), 8).unwrap(),
        b"network",
    )
    .unwrap();
    w.commit().unwrap();
}
#[test]
fn bounded_admission_exceeds_cache_size_and_old_borrows_survive_writer_compaction() {
    for mode in modes() {
        for size in [4096, 16384] {
            let d = Dir::new();
            let env = Environment::create_with_page_size(&d.0, mode, PageSize::new(size).unwrap())
                .unwrap();
            seed(&env);
            let cache = cache();
            let read = ChunkedReadTxn::open_coordinated_snapshot(&d.0, cache.clone()).unwrap();
            assert_eq!(read.sequence(), 1);
            assert_eq!(read.manifest().page_size.get(), size);
            assert_eq!(
                read.pinned_requests().unwrap(),
                0,
                "admission scopes must not pin the graph into transaction"
            );
            assert!(cache.stats().unwrap().mapped_bytes <= 128 << 10);
            assert!(
                cache.stats().unwrap().evictions > 0,
                "graph larger than cache must recycle admission windows"
            );
            let old = read.get(b"kv", &0u64.to_be_bytes()).unwrap().unwrap();
            assert_eq!(old, vec![0; 80]);
            let initial = read.pinned_requests().unwrap();
            for _ in 0..1000 {
                assert_eq!(read.get(b"kv", &0u64.to_be_bytes()).unwrap(), Some(old));
            }
            assert_eq!(read.pinned_requests().unwrap(), initial);
            assert_eq!(read.duplicates(b"dup").unwrap().count(b"k").unwrap(), 40);
            assert_eq!(
                read.longest_match(b"ip", Address::V4(0x0a010203))
                    .unwrap()
                    .unwrap()
                    .1,
                b"network"
            );
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", &0u64.to_be_bytes(), b"new").unwrap();
            w.commit().unwrap();
            env.compact().unwrap();
            env.prune_inventory().unwrap();
            env.reclaim().unwrap();
            drop(env);
            assert_eq!(
                read.get(b"kv", &2999u64.to_be_bytes()).unwrap(),
                Some(vec![(2999 % 251) as u8; 80].as_slice()),
                "an unvisited old chunk remains readable after source reclamation"
            );
            assert_eq!(old, vec![0; 80]);
            assert_eq!(read.commit_identity(), read.manifest().commit);
        }
    }
}
#[test]
fn fully_pinned_cap_is_explicit_and_prior_values_survive() {
    let d = Dir::new();
    let env = Environment::create(&d.0).unwrap();
    seed(&env);
    let cache = cache();
    let read = ChunkedReadTxn::open_coordinated_snapshot(&d.0, cache.clone()).unwrap();
    let first = read.get(b"kv", &0u64.to_be_bytes()).unwrap().unwrap();
    let mut limited = false;
    for k in 1..3000u64 {
        match read.get(b"kv", &k.to_be_bytes()) {
            Ok(Some(v)) => assert_eq!(v, vec![(k % 251) as u8; 80]),
            Err(Error::LimitExceeded) => {
                limited = true;
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(limited);
    assert_eq!(first, vec![0; 80]);
    assert!(cache.stats().unwrap().mapped_bytes <= 128 << 10);
    drop(read);
    cache.trim_unpinned().unwrap();
    assert_eq!(cache.stats().unwrap().mapped_bytes, 0);
}
#[test]
fn admission_caps_and_late_corruption_are_not_skipped() {
    let d = Dir::new();
    let env = Environment::create(&d.0).unwrap();
    seed(&env);
    assert!(matches!(
        ChunkedReadTxn::open_with_options(
            &d.0,
            cache(),
            PinLimits::default(),
            ulmdb::chunked::AdmissionLimits {
                reads: 1,
                nodes: 1,
                metadata_bytes: 1 << 20
            },
            None
        ),
        Err(Error::LimitExceeded)
    ));
    let normal = env.begin_read();
    let entry = normal.file_manifest().unwrap().segments.last().unwrap();
    let file = ulmdb_storage::segment_path(&d.0, entry);
    use std::os::unix::fs::{FileExt, PermissionsExt};
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    let f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(file)
        .unwrap();
    let mut byte = [0];
    f.read_exact_at(&mut byte, entry.length - 1).unwrap();
    byte[0] ^= 1;
    f.write_all_at(&byte, entry.length - 1).unwrap();
    assert!(matches!(
        ChunkedReadTxn::open_coordinated_snapshot(&d.0, cache()),
        Err(Error::Corrupt(_))
    ));
}
struct Cipher;
impl ulmdb_storage::transform::Encryption for Cipher {
    fn transform(
        &self,
        src: &[u8],
        dst: &mut [u8],
        key: &[u8],
        iv: &[u8],
        auth: &mut [u8],
        direction: ulmdb_storage::transform::Direction,
    ) -> ulmdb::Result<()> {
        use sha2::{Digest, Sha256};
        let tag = |bytes: &[u8]| {
            let mut h = Sha256::new();
            h.update(key);
            h.update(iv);
            h.update(bytes);
            h.finalize()
        };
        if direction == ulmdb_storage::transform::Direction::Decrypt && auth != &tag(src)[..16] {
            return Err(Error::InvalidInput("test MAC"));
        }
        for (i, (a, b)) in src.iter().zip(dst.iter_mut()).enumerate() {
            *b = *a ^ key[i % key.len()] ^ iv[i % iv.len()]
        }
        if direction == ulmdb_storage::transform::Direction::Encrypt {
            auth.copy_from_slice(&tag(dst)[..16])
        }
        Ok(())
    }
}
fn context(limit: u64) -> Arc<ulmdb_storage::transform::TransformContext> {
    use ulmdb_storage::transform::*;
    Arc::new(
        TransformContext::new(
            None,
            Some(EncryptionConfig {
                callback: Arc::new(Cipher),
                key: vec![17; 32],
                authentication_bytes: 16,
            }),
            limit,
        )
        .unwrap(),
    )
}
#[test]
fn encrypted_admission_fits_quota_smaller_than_complete_decoded_segment() {
    for mode in modes() {
        let d = Dir::new();
        let env = Environment::create_with_transform(&d.0, mode, context(8 << 20)).unwrap();
        seed(&env);
        let bytes = env
            .begin_read()
            .file_manifest()
            .unwrap()
            .segments
            .iter()
            .map(|e| e.length)
            .sum::<u64>();
        assert!(bytes > 256 << 10);
        drop(env);
        let read = ChunkedReadTxn::open_with_options(
            &d.0,
            cache(),
            PinLimits::default(),
            Default::default(),
            Some(context(256 << 10)),
        )
        .unwrap();
        assert_eq!(
            read.get(b"kv", &7u64.to_be_bytes()).unwrap(),
            Some([7; 80].as_slice())
        );
        assert_eq!(read.duplicates(b"dup").unwrap().count(b"k").unwrap(), 40);
    }
}
#[test]
fn builtin_and_custom_ordering_are_preserved_without_inventing_callbacks() {
    use ulmdb::ordering::{DatabaseComparators, DatabaseOrdering as O, OrderingSpec as S};
    for mode in modes() {
        let d = Dir::new();
        let env = Environment::create_with_publication(&d.0, mode).unwrap();
        let callbacks = DatabaseComparators {
            keys: Some(Arc::new(|a, b| {
                a.iter()
                    .map(u8::to_ascii_lowercase)
                    .cmp(b.iter().map(u8::to_ascii_lowercase))
            })),
            duplicates: None,
        };
        let mut w = env.begin_write().unwrap();
        w.create_database_ordered(
            b"reverse",
            DatabaseKind::Kv,
            O {
                keys: S::ReverseBytes,
                duplicates: S::Bytewise,
            },
        )
        .unwrap();
        w.create_database_ordered(
            b"custom",
            DatabaseKind::Kv,
            O {
                keys: S::Custom,
                duplicates: S::Bytewise,
            },
        )
        .unwrap();
        for key in [b"az", b"za", b"bx"] {
            w.put(b"reverse", key, key).unwrap();
        }
        w.put_with_ordering(b"custom", b"Alpha", b"first", &callbacks)
            .unwrap();
        w.put_with_ordering(b"custom", b"ALPHA", b"second", &callbacks)
            .unwrap();
        w.commit().unwrap();
        let read = ChunkedReadTxn::open_coordinated_snapshot(&d.0, cache()).unwrap();
        let rows = read
            .range(b"reverse", Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .map(|p| p.unwrap().0.to_vec())
            .collect::<Vec<_>>();
        assert_eq!(rows, vec![b"za".to_vec(), b"bx".to_vec(), b"az".to_vec()]);
        assert!(matches!(
            read.get(b"custom", b"alpha"),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(
            read.get_with_ordering(b"custom", b"alpha", &callbacks)
                .unwrap(),
            Some(b"second".as_slice())
        );
    }
}
#[test]
fn hash_valid_malformed_graph_is_rejected_before_reader_publication() {
    use sha2::{Digest, Sha256};
    let d = Dir::new();
    let env = Environment::create(&d.0).unwrap();
    seed(&env);
    let mut manifest = env.begin_read().file_manifest().unwrap().clone();
    drop(env);
    let e = manifest.segments.last().unwrap().clone();
    let path = ulmdb_storage::segment_path(&d.0, &e);
    let mut bytes = fs::read(path).unwrap();
    bytes[manifest.catalog.offset as usize] = 255;
    let hash: [u8; 32] = Sha256::digest(&bytes).into();
    let mut changed = e;
    changed.hash = hash;
    let dest = ulmdb_storage::segment_path(&d.0, &changed);
    fs::write(dest, &bytes).unwrap();
    let mut entries = manifest.segments.to_vec();
    *entries.last_mut().unwrap() = changed;
    manifest.segments = entries.into();
    manifest.previous = manifest.generation;
    manifest.parent = manifest.commit;
    manifest.commit = ulmdb_storage::new_identity().unwrap();
    manifest.sequence += 1;
    manifest.generation = ulmdb_storage::new_identity().unwrap();
    ulmdb_storage::publish(&d.0, &manifest).unwrap();
    assert!(matches!(
        ChunkedReadTxn::open_coordinated_snapshot(&d.0, cache()),
        Err(Error::Corrupt(_))
    ));
}
#[test]
fn geometry_extremes_genesis_previous_and_thread_contracts() {
    fn transferable<T: Send + Sync>() {}
    transferable::<ChunkedReadTxn>();
    for mode in modes() {
        for size in [256, 65536] {
            let d = Dir::new();
            let env = Environment::create_with_page_size(&d.0, mode, PageSize::new(size).unwrap())
                .unwrap();
            let cache = ChunkCache::new(Limits {
                mapped_bytes: 512 << 10,
                windows: 32,
                chunk_bytes: 16384,
                max_window_bytes: 128 << 10,
            })
            .unwrap();
            let empty = ChunkedReadTxn::open_coordinated_snapshot(&d.0, cache.clone()).unwrap();
            assert_eq!(empty.sequence(), 0);
            drop(empty);
            let mut w = env.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            for k in 0..200u64 {
                w.put(b"kv", &k.to_be_bytes(), b"old").unwrap();
            }
            w.commit().unwrap();
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", &0u64.to_be_bytes(), b"new").unwrap();
            w.commit().unwrap();
            let current = ChunkedReadTxn::open_coordinated_snapshot(&d.0, cache.clone()).unwrap();
            assert_eq!(
                current.get(b"kv", &0u64.to_be_bytes()).unwrap(),
                Some(b"new".as_slice())
            );
            let capture = ulmdb_storage::coordination::Coordination::capture_directory(
                &d.0,
                Default::default(),
            )
            .unwrap();
            let previous = ulmdb::OwnedFileCapture::previous(&d.0, capture)
                .unwrap()
                .admit_chunked(cache, PinLimits::default(), Default::default(), None)
                .unwrap();
            assert_eq!(previous.sequence(), 1);
            assert_eq!(
                previous.get(b"kv", &0u64.to_be_bytes()).unwrap(),
                Some(b"old".as_slice())
            );
        }
    }
}
