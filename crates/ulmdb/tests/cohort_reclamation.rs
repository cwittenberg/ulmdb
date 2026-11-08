#[path = "cohort_support/mod.rs"]
mod support;
use std::{fs, path::Path, sync::Arc};
use support::*;
use ulmdb::{DatabaseKind, Environment, backup};

fn replace(env: &Environment, value: &[u8]) {
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"key", value).unwrap();
    w.commit().unwrap();
}

fn data_only_origins_do_not_retain_their_historical_inventories(parent: &Path) {
    let source = Directory::new(parent);
    let exported = Directory::new(parent);
    let env = cohort_environment(&source);
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"old").unwrap();
    w.commit().unwrap();
    let first = ulmdb_storage::read_current(&source.0).unwrap();
    replace(&env, b"new");
    let second = ulmdb_storage::read_current(&source.0).unwrap();
    assert!(
        second
            .segments
            .iter()
            .any(|entry| entry.cohort == first.generation)
    );
    let pruned = env.prune_inventory().unwrap();
    assert!(pruned.changed);
    let selected = ulmdb_storage::read_current(&source.0).unwrap();
    assert_eq!(selected.format_minor, 3);
    assert_eq!(selected.segments.len(), 1);
    assert_eq!(selected.segments[0].cohort, second.generation);
    assert!(
        selected
            .segments
            .iter()
            .all(|entry| entry.cohort != selected.generation),
        "selected root cohort is descriptor-only"
    );
    let pin = env.begin_read();
    env.retain_snapshot("old-cohort", &pin).unwrap();
    drop(pin);
    let compacted = env.compact().unwrap();
    let current = ulmdb_storage::read_current(&source.0).unwrap();
    assert_eq!(current.format_minor, 3);
    assert!(
        current.segments.iter().all(|entry| entry.cohort == 0),
        "builder data may remain legacy under a cohort descriptor"
    );
    assert_ne!(compacted.generation, selected.generation);
    let gc = env.reclaim().unwrap();
    assert!(gc.removed_cohorts >= 1);
    assert!(!cohort_path(&source.0, first.generation).exists());
    assert!(cohort_path(&source.0, second.generation).exists());
    assert!(cohort_path(&source.0, selected.generation).exists());

    // The physical second cohort still embeds its former inventory, including
    // a now-missing first cohort. As data for selected it must not activate or
    // recursively trace that historical descriptor's dependencies.
    let historical = ulmdb_storage::read_checkpoint(
        &source.0,
        &ulmdb_storage::checkpoint_bytes(&second).unwrap(),
    )
    .unwrap();
    assert!(
        historical
            .segments
            .iter()
            .any(|entry| entry.cohort == first.generation)
    );
    let old = env.open_retained("old-cohort").unwrap();
    let borrowed = old.get(b"kv", b"key").unwrap().unwrap();
    assert_eq!(borrowed, b"new");
    let summary = backup::export_snapshot(&env, &old, &exported.0).unwrap();
    assert_eq!(summary.generation, selected.generation);
    assert_eq!(summary.physical_bytes, bytes_on_disk(&source.0, &selected));
    assert_eq!(count(&exported.0, "cohorts"), 2);
    assert!(!cohort_path(&exported.0, first.generation).exists());
    for origin in [second.generation, selected.generation] {
        assert_eq!(
            fs::read(cohort_path(&source.0, origin)).unwrap(),
            fs::read(cohort_path(&exported.0, origin)).unwrap()
        );
    }
    let copy = Environment::open(&exported.0).unwrap();
    assert_eq!(
        copy.begin_read().get(b"kv", b"key").unwrap(),
        Some(b"new".as_slice())
    );
    assert!(ulmdb_storage::read_previous(&exported.0).unwrap().is_none());
    copy.reclaim().unwrap();
    backup::verify(&copy).unwrap();

    env.release_snapshot("old-cohort").unwrap();
    env.compact().unwrap();
    env.compact().unwrap();
    env.reclaim().unwrap();
    assert!(cohort_path(&source.0, second.generation).exists());
    assert!(cohort_path(&source.0, selected.generation).exists());
    assert_eq!(borrowed, b"new");
    drop(old);
    let gc = env.reclaim().unwrap();
    assert!(gc.removed_cohorts >= 2);
    assert!(!cohort_path(&source.0, second.generation).exists());
    assert!(!cohort_path(&source.0, selected.generation).exists());
    assert_eq!(
        env.begin_read().get(b"kv", b"key").unwrap(),
        Some(b"new".as_slice())
    );
    backup::verify(&env).unwrap();
}
#[test]
fn cohort_root_and_data_marking_pins_compaction_prune_and_export_on_disk() {
    data_only_origins_do_not_retain_their_historical_inventories(&std::env::temp_dir());
}
#[test]
fn cohort_root_and_data_marking_pins_compaction_prune_and_export_on_ramdrive() {
    assert!(Path::new("/dev/shm").is_dir());
    data_only_origins_do_not_retain_their_historical_inventories(Path::new("/dev/shm"));
}

#[test]
fn invalid_pin_or_current_prevents_any_cohort_unlink() {
    let parent = std::env::temp_dir();
    let source = Directory::new(&parent);
    let env = cohort_environment(&source);
    populate(&env);
    let read = env.begin_read();
    env.retain_snapshot("pin", &read).unwrap();
    let orphan = cohort_path(&source.0, 0xaaaa);
    fs::write(&orphan, b"unselected partial cohort").unwrap();
    let pin = source.0.join("retained/pin.pin");
    let pin_bytes = fs::read(&pin).unwrap();
    fs::write(&pin, b"broken").unwrap();
    assert!(env.reclaim().is_err());
    assert!(orphan.exists());
    fs::write(pin, pin_bytes).unwrap();
    let gc = env.reclaim().unwrap();
    assert!(gc.removed_cohorts >= 1);
    assert!(!orphan.exists());
    let current = source.0.join("CURRENT");
    let current_bytes = fs::read(&current).unwrap();
    fs::write(&orphan, b"another unselected partial cohort").unwrap();
    fs::write(&current, b"broken").unwrap();
    assert!(env.reclaim().is_err());
    assert!(orphan.exists());
    fs::write(current, current_bytes).unwrap();
}

#[test]
fn concurrent_old_cohort_exports_and_reclamation_preserve_physical_paths() {
    let parent = std::env::temp_dir();
    let source = Directory::new(&parent);
    let env = Arc::new(cohort_environment(&source));
    populate(&env);
    let old = env.begin_read();
    let manifest = ulmdb_storage::read_current(&source.0).unwrap();
    for n in 1..=5 {
        update(&env, n);
    }
    env.compact().unwrap();
    env.compact().unwrap();
    env.reclaim().unwrap();
    std::thread::scope(|scope| {
        let worker = Arc::clone(&env);
        scope.spawn(move || {
            for _ in 0..4 {
                worker.compact().unwrap();
                worker.reclaim().unwrap();
            }
        });
        for _ in 0..4 {
            let target = Directory::new(&parent);
            let result = backup::export_snapshot(&env, &old, &target.0).unwrap();
            assert_eq!(result.generation, manifest.generation);
            inspect(&Environment::open(&target.0).unwrap(), 0);
        }
    });
    for origin in cohort_names(&manifest) {
        assert!(cohort_path(&source.0, origin).exists());
    }
    assert_eq!(
        old.get(b"kv", b"counter").unwrap(),
        Some(0u64.to_le_bytes().as_slice())
    );
    drop(old);
    env.reclaim().unwrap();
    for origin in cohort_names(&manifest) {
        assert!(!cohort_path(&source.0, origin).exists());
    }
    inspect(&env, 5);
}

#[test]
fn stale_legacy_previous_preserves_actual_cohort_predecessor_before_any_unlink() {
    let directory = Directory::new(&std::env::temp_dir());
    let env = cohort_environment(&directory);
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    write.put(b"kv", b"key", b"old").unwrap();
    write.commit().unwrap();
    let older = ulmdb_storage::read_current(&directory.0).unwrap();
    // Compaction outputs legacy data segments and a cohort descriptor. This
    // makes a low-level format-2 publication valid without rewriting references.
    env.compact().unwrap();
    let predecessor = ulmdb_storage::read_current(&directory.0).unwrap();
    assert!(predecessor.segments.iter().all(|entry| entry.cohort == 0));
    drop(env);
    let mut legacy = predecessor.clone();
    legacy.format_minor = 2;
    legacy.previous = predecessor.generation;
    legacy.generation = ulmdb_storage::new_identity().unwrap();
    ulmdb_storage::publish(&directory.0, &legacy).unwrap();
    fs::write(
        directory.0.join("PREVIOUS"),
        ulmdb_storage::checkpoint_bytes(&older).unwrap(),
    )
    .unwrap();
    let actual = ulmdb_storage::descriptor_path(&directory.0, &predecessor);
    let before = fs::read(&actual).unwrap();
    let all_files = || {
        ["segments", "commits", "cohorts"]
            .into_iter()
            .flat_map(|name| {
                fs::read_dir(directory.0.join(name))
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
            })
            .collect::<std::collections::BTreeSet<_>>()
    };
    let files = all_files();
    let env = Environment::open(&directory.0).unwrap();
    assert!(matches!(env.reclaim(), Err(ulmdb::Error::Busy)));
    assert_eq!(fs::read(&actual).unwrap(), before);
    assert_eq!(all_files(), files);
    replace(&env, b"new");
    env.reclaim().unwrap();
    assert!(!actual.exists());
    assert_eq!(
        env.begin_read().get(b"kv", b"key").unwrap(),
        Some(b"new".as_slice())
    );
}
