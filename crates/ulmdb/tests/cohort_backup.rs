#[path = "cohort_support/mod.rs"]
mod support;
use std::{fs, path::Path};
use support::*;
use ulmdb::{Environment, PublicationMode, backup};

fn exercise(parent: &Path) {
    let source = Directory::new(parent);
    let target = Directory::new(parent);
    let restored = Directory::new(parent);
    let env = cohort_environment(&source);
    assert_eq!(env.publication_mode(), PublicationMode::Cohort);
    populate(&env);
    let before = ulmdb_storage::read_current(&source.0).unwrap();
    assert_eq!(before.format_minor, 3);
    assert_eq!(before.segments.len(), 1);
    assert_eq!(before.segments[0].cohort, before.generation);
    let selected = cohort_path(&source.0, before.generation);
    assert!(fs::metadata(&selected).unwrap().len() > before.segments[0].length);
    let summary = backup::export(&env, &target.0).unwrap();
    assert_eq!(summary.physical_bytes, bytes_on_disk(&source.0, &before));
    assert!(summary.physical_bytes > summary.segment_bytes);
    assert_eq!(
        summary.physical_bytes,
        fs::metadata(&selected).unwrap().len()
    );
    assert_eq!(
        count(&target.0, "cohorts"),
        1,
        "root and data share exactly one copied physical cohort"
    );
    assert_eq!(count(&target.0, "segments"), 0);
    assert_eq!(count(&target.0, "commits"), 0);
    assert_eq!(
        fs::read(&selected).unwrap(),
        fs::read(cohort_path(&target.0, before.generation)).unwrap()
    );
    assert!(ulmdb_storage::read_previous(&target.0).unwrap().is_none());
    same_entry_origins(&before, &ulmdb_storage::read_current(&target.0).unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let copied = fs::metadata(cohort_path(&target.0, before.generation)).unwrap();
        assert_ne!(copied.ino(), fs::metadata(&selected).unwrap().ino());
        assert_eq!(copied.nlink(), 1);
    }
    let copy = Environment::open(&target.0).unwrap();
    inspect(&copy, 0);
    copy.reclaim().unwrap();
    drop(copy);
    assert_eq!(backup::restore(&target.0, &restored.0).unwrap(), summary);
    let copy = Environment::open(&restored.0).unwrap();
    inspect(&copy, 0);
    update(&copy, 7);
    inspect(&copy, 7);
    inspect(&env, 0);
    drop(copy);
    inspect(&Environment::open(&restored.0).unwrap(), 7);
}
#[test]
fn exact_cohort_files_are_deduplicated_copied_and_restored_on_disk() {
    exercise(&std::env::temp_dir());
}
#[test]
fn exact_cohort_files_are_deduplicated_copied_and_restored_on_ramdrive() {
    assert!(Path::new("/dev/shm").is_dir());
    exercise(Path::new("/dev/shm"));
}

#[test]
fn mixed_origins_and_old_legacy_snapshot_export_preserve_format_and_bytes() {
    let parent = std::env::temp_dir();
    let source = Directory::new(&parent);
    let target = Directory::new(&parent);
    let old_target = Directory::new(&parent);
    let env = Environment::create(&source.0).unwrap();
    populate(&env);
    let old = env.begin_read();
    let legacy = ulmdb_storage::read_current(&source.0).unwrap();
    assert_eq!(legacy.format_minor, 2);
    assert!(legacy.segments.iter().all(|entry| entry.cohort == 0));
    env.upgrade_publication().unwrap();
    update(&env, 9);
    let before = ulmdb_storage::read_current(&source.0).unwrap();
    assert!(before.segments.iter().any(|entry| entry.cohort == 0));
    assert!(before.segments.iter().any(|entry| entry.cohort != 0));
    let summary = backup::export(&env, &target.0).unwrap();
    assert_eq!(summary.physical_bytes, bytes_on_disk(&source.0, &before));
    same_entry_origins(&before, &ulmdb_storage::read_current(&target.0).unwrap());
    for path in physical_paths(&source.0, &before) {
        let relative = path.strip_prefix(&source.0).unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            fs::read(target.0.join(relative)).unwrap()
        );
    }
    inspect(&Environment::open(&target.0).unwrap(), 9);
    let old_summary = backup::export_snapshot(&env, &old, &old_target.0).unwrap();
    assert_eq!(old_summary.generation, legacy.generation);
    same_entry_origins(
        &legacy,
        &ulmdb_storage::read_current(&old_target.0).unwrap(),
    );
    assert_eq!(count(&old_target.0, "cohorts"), 0);
    inspect(&Environment::open(&old_target.0).unwrap(), 0);
}

#[test]
fn empty_descriptor_only_cohort_round_trips_and_accepts_first_commit() {
    let parent = std::env::temp_dir();
    let source = Directory::new(&parent);
    let target = Directory::new(&parent);
    let restored = Directory::new(&parent);
    let env = cohort_environment(&source);
    let before = ulmdb_storage::read_current(&source.0).unwrap();
    let summary = backup::export(&env, &target.0).unwrap();
    assert_eq!((summary.segments, summary.segment_bytes), (0, 0));
    assert_eq!(summary.physical_bytes, bytes_on_disk(&source.0, &before));
    assert_eq!(count(&target.0, "cohorts"), 1);
    assert_eq!(backup::restore(&target.0, &restored.0).unwrap(), summary);
    let copy = Environment::open(&restored.0).unwrap();
    assert_eq!(copy.publication_mode(), PublicationMode::Cohort);
    let mut writer = copy.begin_write().unwrap();
    writer
        .create_database(b"first", ulmdb::DatabaseKind::Kv)
        .unwrap();
    writer.put(b"first", b"key", b"value").unwrap();
    assert_eq!(writer.commit().unwrap(), 1);
    assert_eq!(
        copy.begin_read().get(b"first", b"key").unwrap(),
        Some(b"value".as_slice())
    );
}

#[test]
fn copied_prefix_corruption_and_selected_footer_corruption_never_activate_restore() {
    for footer in [false, true] {
        let parent = std::env::temp_dir();
        let source = Directory::new(&parent);
        let exported = Directory::new(&parent);
        let rejected = Directory::new(&parent);
        let env = cohort_environment(&source);
        populate(&env);
        backup::export(&env, &exported.0).unwrap();
        let manifest = ulmdb_storage::read_current(&exported.0).unwrap();
        let path = ulmdb_storage::descriptor_path(&exported.0, &manifest);
        let mut bytes = fs::read(&path).unwrap();
        let offset = if footer { bytes.len() - 32 } else { 4100 };
        bytes[offset] ^= 1;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        fs::write(path, bytes).unwrap();
        assert!(backup::restore(&exported.0, &rejected.0).is_err());
        assert!(!rejected.0.join("CURRENT").exists());
    }
}
