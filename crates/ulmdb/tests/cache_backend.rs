#![cfg(target_os = "linux")]
use std::{fs, path::PathBuf};
use ulmdb::chunked::CacheOptions;
use ulmdb::{CoordinatedEnvironment, DatabaseKind, Environment, Error, PublicationMode};
use ulmdb_storage::chunk_cache::{ChunkCache, Limits};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "cache-backend-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn cache() -> CacheOptions {
    CacheOptions::new(
        ChunkCache::new(Limits {
            mapped_bytes: 256 << 10,
            windows: 16,
            chunk_bytes: 16384,
            max_window_bytes: 128 << 10,
        })
        .unwrap(),
    )
}
#[test]
fn normal_transaction_api_reads_writes_reopens_and_keeps_old_borrows_with_cached_base() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new();
        let env = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        for k in 0..5000u64 {
            w.put(b"kv", &k.to_be_bytes(), &[41; 80]).unwrap();
        }
        w.commit().unwrap();
        drop(env);
        let cache = cache();
        let (owner, read) = CoordinatedEnvironment::open_with_initial_read_and_cache_options(
            &d.0,
            Default::default(),
            Default::default(),
            None,
            Some(cache.clone()),
        )
        .unwrap();
        assert!(read.uses_chunk_cache());
        assert_eq!(read.stats().segments, 1);
        let old = read.get(b"kv", &0u64.to_be_bytes()).unwrap().unwrap();
        assert_eq!(old, [41; 80]);
        owner
            .update(|w| w.put(b"kv", &0u64.to_be_bytes(), b"new"))
            .unwrap();
        let next = owner.begin_read().unwrap();
        assert!(next.uses_chunk_cache());
        assert_eq!(next.sequence(), 2);
        assert_eq!(
            next.get(b"kv", &0u64.to_be_bytes()).unwrap(),
            Some(b"new".as_slice())
        );
        assert_eq!(old, [41; 80]);
        drop(next);
        let writer = Environment::open_with_cache_options(
            &d.0,
            Default::default(),
            Default::default(),
            None,
            Some(cache),
        )
        .unwrap();
        let mut w = writer.begin_write().unwrap();
        w.put(b"kv", &1u64.to_be_bytes(), b"again").unwrap();
        w.commit().unwrap();
        assert!(writer.begin_read().uses_chunk_cache());
        assert_eq!(writer.begin_read().stats().segments, 3);
        assert_eq!(old, [41; 80]);
    }
}
#[test]
fn cached_incremental_import_preserves_source_and_reopens() {
    let d = Dir::new();
    fs::create_dir(&d.0).unwrap();
    let source = d.0.join("source");
    let target = d.0.join("target");
    let env = Environment::create(&source).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"k", b"one").unwrap();
    w.commit().unwrap();
    ulmdb::backup::export(&env, &target).unwrap();
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"k", b"two").unwrap();
    w.commit().unwrap();
    let mut dump = Vec::new();
    env.dump_incremental(1, &mut dump).unwrap();
    drop(env);
    let e = Environment::open_with_cache_options(
        &target,
        Default::default(),
        Default::default(),
        None,
        Some(cache()),
    )
    .unwrap();
    e.load_incremental(&mut dump.as_slice()).unwrap();
    assert!(e.begin_read().uses_chunk_cache());
    assert_eq!(
        e.begin_read().get(b"kv", b"k").unwrap(),
        Some(b"two".as_slice())
    );
}

#[test]
fn cached_compact_copy_and_maintenance_stream_larger_than_the_cache() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new();
        fs::create_dir(&d.0).unwrap();
        let source = d.0.join("source");
        let target = d.0.join("compact");
        let e = Environment::create_with_publication(&source, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        for (name, kind) in [
            (b"kv".as_slice(), DatabaseKind::Kv),
            (b"dups", DatabaseKind::Duplicates { width: None }),
            (b"ip", DatabaseKind::Ipv4),
        ] {
            w.create_database(name, kind).unwrap();
        }
        for n in 0..5000u64 {
            w.put(b"kv", &n.to_be_bytes(), &[61; 128]).unwrap();
        }
        for n in 0..600u64 {
            w.insert_duplicate(b"dups", b"k", &n.to_be_bytes()).unwrap();
        }
        for n in 0..200u32 {
            w.insert_prefix(
                b"ip",
                ulmdb::Prefix::new(ulmdb::Address::V4(0x0a000000 + n), 32).unwrap(),
                &[73; 512],
            )
            .unwrap();
        }
        w.commit().unwrap();
        drop(e);
        let config = cache();
        let ctl = config.cache.clone();
        let capture = ulmdb::OwnedFileCapture::current_for_copy(
            &source,
            ulmdb_storage::coordination::Coordination::capture_directory(
                &source,
                Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
        let view = capture
            .admit_copy_view_with_cache_options(
                Default::default(),
                Default::default(),
                None,
                Some(config.clone()),
            )
            .unwrap();
        ulmdb::backup::compact_file_copy(&view, &target).unwrap();
        assert!(ctl.stats().unwrap().evictions > 0);
        drop(view);
        let e = Environment::open_with_cache_options(
            &source,
            Default::default(),
            Default::default(),
            None,
            Some(config),
        )
        .unwrap();
        let old = e.begin_read();
        let held = old.get(b"kv", &0u64.to_be_bytes()).unwrap().unwrap();
        e.prune_inventory().unwrap();
        e.compact().unwrap();
        assert!(e.begin_read().uses_chunk_cache());
        assert_eq!(held, [61; 128]);
        let copied = Environment::open(&target).unwrap();
        assert_eq!(
            copied
                .begin_read()
                .get(b"kv", &4999u64.to_be_bytes())
                .unwrap(),
            Some([61; 128].as_slice())
        );
        assert_eq!(
            copied.begin_read().duplicate_count(b"dups", b"k").unwrap(),
            600
        );
        assert_eq!(
            copied
                .begin_read()
                .longest_match(b"ip", ulmdb::Address::V4(0x0a0000c7))
                .unwrap()
                .unwrap()
                .1,
            [73; 512]
        );
    }
}

#[test]
fn cached_fixed_combination_rejects_before_layout_creation_or_mutation() {
    let d = Dir::new();
    drop(Environment::create(&d.0).unwrap());
    let mut e = Environment::open_with_cache_options(
        &d.0,
        Default::default(),
        Default::default(),
        None,
        Some(cache()),
    )
    .unwrap();
    assert!(matches!(
        e.enable_fixed_map(64 << 20, None),
        Err(Error::InvalidInput(_))
    ));
    assert!(!d.0.join("FIXEDMAP").exists());
    drop(e);
    let mut normal = Environment::open(&d.0).unwrap();
    normal.enable_fixed_map(64 << 20, None).unwrap();
    drop(normal);
    let fixed = ulmdb::FixedMap::open_namespace(&d.0).unwrap();
    let before = fs::read(d.0.join("FIXEDMAP")).unwrap();
    let e = Environment::open_with_cache_options(
        &d.0,
        Default::default(),
        Default::default(),
        None,
        Some(cache()),
    )
    .unwrap();
    assert!(matches!(
        e.bind_fixed_map_layout(fixed.clone()),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        e.begin_read().with_fixed_map(&d.0, fixed.clone()),
        Err(Error::InvalidInput(_))
    ));
    let mut w = e.begin_write().unwrap();
    assert!(matches!(
        w.bind_fixed_map(fixed),
        Err(Error::InvalidInput(_))
    ));
    w.abort();
    assert_eq!(fs::read(d.0.join("FIXEDMAP")).unwrap(), before);
}

#[test]
fn cached_ordinary_writer_keeps_declared_namespace_usable_by_fixed_reopen() {
    // An unrelated libtest thread can map into the released persisted address.
    // Execute this address-space assumption in its own process; occupied-address
    // rejection is exercised explicitly below rather than retried or weakened.
    if std::env::var_os("ULMDB_CACHE_FIXED_REOPEN_WORKER").is_none() {
        struct Worker(std::process::Child);
        impl Drop for Worker {
            fn drop(&mut self) {
                if self.0.try_wait().ok().flatten().is_none() {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
        }
        let mut child = Worker(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "cached_ordinary_writer_keeps_declared_namespace_usable_by_fixed_reopen",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("ULMDB_CACHE_FIXED_REOPEN_WORKER", "1")
                .spawn()
                .unwrap(),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success(), "fixed cached reopen worker: {status}");
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "fixed cached reopen deadline"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new();
        let e = Environment::create_fixed_map(&d.0, mode, 128 << 20, None).unwrap();
        let placement = e.fixed_map_info().unwrap().unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"k", b"old").unwrap();
        w.commit().unwrap();
        drop(e);
        let e = Environment::open_with_cache_options(
            &d.0,
            Default::default(),
            Default::default(),
            None,
            Some(cache()),
        )
        .unwrap();
        let r = e.begin_read();
        let m = r.file_manifest().unwrap();
        let declaration = ulmdb::FixedMapDeclaration::open(&d.0, m.uuid, m.epoch).unwrap();
        drop(r);
        e.track_fixed_map_declaration(declaration).unwrap();
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"k", b"new").unwrap();
        w.commit().unwrap();
        assert!(e.begin_read().uses_chunk_cache());
        drop(e);
        let occupied_directory = Dir::new();
        let occupied = Environment::create_fixed_map(
            &occupied_directory.0,
            PublicationMode::Legacy,
            placement.capacity,
            Some(placement.address),
        )
        .unwrap();
        let mut writer = occupied.begin_write().unwrap();
        writer.create_database(b"guard", DatabaseKind::Kv).unwrap();
        writer.put(b"guard", b"marker", b"occupied").unwrap();
        writer.commit().unwrap();
        let occupied_read = occupied.begin_read();
        let marker = occupied_read.get(b"guard", b"marker").unwrap().unwrap();
        let marker_address = marker.as_ptr() as usize;
        assert!(
            (placement.address..placement.address + placement.capacity).contains(&marker_address)
        );
        let current = fs::read(d.0.join("CURRENT")).unwrap();
        assert!(matches!(
            Environment::open_fixed_map(&d.0),
            Err(Error::Busy)
        ));
        assert_eq!(marker, b"occupied");
        assert_eq!(marker.as_ptr() as usize, marker_address);
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), current);
        drop(occupied_read);
        drop(occupied);
        let e = Environment::open_fixed_map(&d.0).unwrap();
        let r = e.begin_read();
        let value = r.get(b"kv", b"k").unwrap().unwrap();
        assert_eq!(value, b"new");
        let info = r.fixed_map_info().unwrap().unwrap();
        assert!((info.address..info.address + info.capacity).contains(&(value.as_ptr() as usize)));
    }
}
