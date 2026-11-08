use std::{fs, path::PathBuf};
use ulmdb::{DatabaseKind, Environment, Error, PublicationMode, ReadTxn, backup};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-client-export-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn populate(environment: &Environment) {
    let mut writer = environment.begin_write().unwrap();
    writer.create_database(b"kv", DatabaseKind::Kv).unwrap();
    writer.put(b"kv", b"key", b"committed").unwrap();
    writer.commit().unwrap();
}
#[test]
fn independent_export_copies_committed_snapshot_while_source_writer_is_active() {
    let directory = Directory::new();
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let source = directory.0.join(format!("source-{mode:?}"));
        let target = directory.0.join(format!("target-{mode:?}"));
        let environment = Environment::create_with_publication(&source, mode).unwrap();
        populate(&environment);
        let read = ReadTxn::open_snapshot(&source).unwrap();
        let borrowed = read.get(b"kv", b"key").unwrap().unwrap();
        let pointer = fs::read(source.join("CURRENT")).unwrap();
        let mut writer = environment.begin_write().unwrap();
        writer.put(b"kv", b"key", b"private").unwrap();
        writer.put(b"kv", b"uncommitted", b"secret").unwrap();
        assert_eq!(
            writer.get(b"kv", b"key").unwrap(),
            Some(b"private".as_slice())
        );
        let summary = backup::export_independent_snapshot(&source, &read, &target).unwrap();
        assert_eq!(summary.sequence, read.sequence());
        assert_eq!(summary.generation, read.generation());
        assert_eq!(summary.commit, read.commit_identity());
        assert_eq!(fs::read(source.join("CURRENT")).unwrap(), pointer);
        assert_eq!(borrowed, b"committed");
        let copied = Environment::open(&target).unwrap();
        let copied_read = copied.begin_read();
        assert_eq!(
            copied_read.get(b"kv", b"key").unwrap(),
            Some(b"committed".as_slice())
        );
        assert_eq!(copied_read.get(b"kv", b"uncommitted").unwrap(), None);
        backup::verify(&copied).unwrap();
        writer.abort();
        assert_eq!(
            environment.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"committed".as_slice())
        );
        assert_eq!(borrowed, b"committed");
    }
}
#[test]
fn unrelated_or_damaged_sources_never_publish_an_export() {
    let directory = Directory::new();
    let source = directory.0.join("source");
    let other = directory.0.join("other");
    let environment = Environment::create(&source).unwrap();
    populate(&environment);
    let unrelated = Environment::create(&other).unwrap();
    let read = ReadTxn::open_snapshot(&source).unwrap();
    let target = directory.0.join("wrong-source");
    assert!(matches!(
        backup::export_independent_snapshot(&other, &read, &target),
        Err(Error::InvalidInput(_))
    ));
    assert!(!target.exists());
    drop(unrelated);
    let manifest = ulmdb_storage::read_current(&source).unwrap();
    let segment = ulmdb_storage::segment_path(&source, &manifest.segments[0]);
    fs::remove_file(segment).unwrap();
    let target = directory.0.join("missing-file");
    assert!(backup::export_independent_snapshot(&source, &read, &target).is_err());
    assert!(!target.exists());
}
