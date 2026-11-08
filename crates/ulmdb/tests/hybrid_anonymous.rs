//! Same durable WAL and logical bytes, independently selectable replay cache.
use ulmdb::{
    Address, DatabaseKind, Prefix,
    hybrid::{Environment, ImageMode, Options, RedoMode},
};
fn options(image: ImageMode, redo: RedoMode) -> Options {
    Options {
        image,
        redo,
        chunk: ulmdb_storage::hybrid::ChunkLimits {
            capacity: 2 * 1024 * 1024,
            reserve_quantum: 65536,
            max_burned_bytes: 1024 * 1024,
        },
        max_image_bytes: 8 * 1024 * 1024,
        ..Options::default()
    }
}

fn images(path: &std::path::Path) -> usize {
    std::fs::read_dir(path)
        .unwrap()
        .filter(|r| {
            r.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".cache")
        })
        .count()
}
fn set(env: &Environment, n: u64) {
    let mut w = env.begin_write().unwrap();
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
fn verify(env: &Environment, n: u64) {
    let r = env.begin_read();
    assert_eq!(r.get(b"kv", b"state").unwrap().unwrap(), n.to_le_bytes());
    assert_eq!(
        r.longest_match(b"v4", Address::V4(0x0a000001))
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
fn anonymous_file_mode_cross_reopen_atomicity_and_old_borrows() {
    for base in [std::env::temp_dir(), std::path::PathBuf::from("/dev/shm")] {
        for redo in [RedoMode::Compact, RedoMode::FullPostimage] {
            for initial in [ImageMode::File, ImageMode::Anonymous] {
                for replay in [ImageMode::File, ImageMode::Anonymous] {
                    let path = base.join(format!(
                        "hybrid-anon-env-{:032x}",
                        ulmdb_storage::new_identity().unwrap()
                    ));
                    let env = Environment::create(&path, options(initial, redo)).unwrap();
                    assert_eq!(env.options().image, initial);
                    let mut w = env.begin_write().unwrap();
                    for (name, kind) in [
                        (b"kv".as_slice(), DatabaseKind::Kv),
                        (b"v4", DatabaseKind::Ipv4),
                        (b"v6", DatabaseKind::Ipv6),
                    ] {
                        w.create_database(name, kind).unwrap();
                    }
                    w.commit().unwrap();
                    set(&env, 0);
                    let old = env.begin_read();
                    let held = old.get(b"kv", b"state").unwrap().unwrap();
                    let sequence = env.sequence();
                    let length = env.wal_bytes().unwrap();
                    let mut noop = env.begin_write().unwrap();
                    noop.put(b"kv", b"state", &0u64.to_le_bytes()).unwrap();
                    assert_eq!(noop.commit().unwrap(), sequence);
                    assert_eq!(env.wal_bytes().unwrap(), length);
                    let mut aborted = env.begin_write().unwrap();
                    aborted.put(b"kv", b"state", &999u64.to_le_bytes()).unwrap();
                    drop(aborted);
                    verify(&env, 0);
                    for n in 1..=3 {
                        set(&env, n);
                    }
                    env.rotate_chunk().unwrap();
                    set(&env, 4);
                    verify(&env, 4);
                    assert_eq!(held, 0u64.to_le_bytes());
                    assert_eq!(
                        images(&path),
                        if initial == ImageMode::File { 2 } else { 0 }
                    );
                    assert!(matches!(
                        Environment::open(&path, options(replay, redo)),
                        Err(ulmdb::Error::Busy)
                    ));
                    let sequence = env.sequence();
                    let wal = std::fs::read(path.join("HYBRID.wal")).unwrap();
                    drop(env);
                    assert_eq!(images(&path), 0);
                    let env = Environment::open(&path, options(replay, redo)).unwrap();
                    assert_eq!(env.sequence(), sequence);
                    assert_eq!(wal, std::fs::read(path.join("HYBRID.wal")).unwrap());
                    assert_eq!(env.options().image, replay);
                    assert_eq!(images(&path), if replay == ImageMode::File { 2 } else { 0 });
                    verify(&env, 4);
                    assert_eq!(held, 0u64.to_le_bytes());
                    set(&env, 5);
                    verify(&env, 5);
                    assert_eq!(held, 0u64.to_le_bytes());
                    drop(env);
                    drop(old);
                    std::fs::remove_dir_all(path).unwrap();
                }
            }
        }
    }
}
#[test]
fn file_cache_remains_default() {
    assert_eq!(Options::default().image, ImageMode::File);
}

#[test]
fn virtual_budget_is_not_a_residency_claim() {
    let base = std::env::temp_dir().join(format!(
        "anon-budget-{:032x}",
        ulmdb_storage::new_identity().unwrap()
    ));
    let opts = options(ImageMode::Anonymous, RedoMode::Compact);
    let env = Environment::create(&base, opts).unwrap();
    let status = env.image_usage().unwrap();
    assert_eq!(status.mode, ImageMode::Anonymous);
    assert_eq!(status.virtual_capacity_bytes, opts.chunk.capacity as u64);
    assert_eq!(status.owned_cache_files, 0);
    assert_eq!(status.active.file_reserved_bytes, 0);
    assert_eq!(status.active.anonymous_virtual_bytes, opts.chunk.capacity);
    let write = env.begin_write().unwrap();
    assert!(matches!(env.image_usage(), Err(ulmdb::Error::Busy)));
    drop(write);
    // Genesis cache SID1 is unpublished: an unused rotation is a no-op.
    env.rotate_chunk().unwrap();
    assert_eq!(
        env.image_usage().unwrap().virtual_capacity_bytes,
        opts.chunk.capacity as u64
    );
    let mut write = env.begin_write().unwrap();
    write
        .create_database(b"published", DatabaseKind::Kv)
        .unwrap();
    write.commit().unwrap();
    env.rotate_chunk().unwrap();
    assert_eq!(
        env.image_usage().unwrap().virtual_capacity_bytes,
        2 * opts.chunk.capacity as u64
    );
    drop(env);
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn anonymous_concurrent_snapshots_share_only_immutable_prefixes() {
    let path = std::env::temp_dir().join(format!(
        "anon-concurrent-{:032x}",
        ulmdb_storage::new_identity().unwrap()
    ));
    let env = std::sync::Arc::new(
        Environment::create(&path, options(ImageMode::Anonymous, RedoMode::Compact)).unwrap(),
    );
    let mut w = env.begin_write().unwrap();
    for (name, kind) in [
        (b"kv".as_slice(), DatabaseKind::Kv),
        (b"v4", DatabaseKind::Ipv4),
        (b"v6", DatabaseKind::Ipv6),
    ] {
        w.create_database(name, kind).unwrap();
    }
    w.commit().unwrap();
    set(&env, 0);
    std::thread::scope(|scope| {
        for _ in 0..3 {
            let env = std::sync::Arc::clone(&env);
            scope.spawn(move || {
                for _ in 0..1000 {
                    let r = env.begin_read();
                    let value = r.get(b"kv", b"state").unwrap().unwrap();
                    assert_eq!(
                        r.longest_match(b"v4", Address::V4(0x0a000001))
                            .unwrap()
                            .unwrap()
                            .1,
                        value
                    );
                    assert_eq!(
                        r.longest_match(b"v6", Address::V6(0x20010db8000000000000000000000001))
                            .unwrap()
                            .unwrap()
                            .1,
                        value
                    );
                }
            });
        }
        for n in 1..=64 {
            set(&env, n);
        }
    });
    verify(&env, 64);
    drop(env);
    std::fs::remove_dir_all(path).unwrap();
}
