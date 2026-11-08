use std::path::PathBuf;
use ulmdb::{DatabaseKind, Environment, Error, PublicationMode, ReadTxn};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-client-snapshot-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn compatibility_default_database_is_a_read_only_catalog_projection() {
    let directory = Directory::new();
    let environment = Environment::create(&directory.0).unwrap();
    let before = std::fs::read(directory.0.join("CURRENT")).unwrap();
    let read = ReadTxn::open_snapshot_with_empty_kv(&directory.0, b"\0lmdb-main").unwrap();
    assert_eq!(read.get(b"\0lmdb-main", b"missing").unwrap(), None);
    assert_eq!(read.open_database(b"\0lmdb-main").unwrap().id(), u64::MAX);
    assert!(matches!(
        environment.begin_read().database_kind(b"\0lmdb-main"),
        Err(Error::MissingDatabase)
    ));
    assert_eq!(std::fs::read(directory.0.join("CURRENT")).unwrap(), before);
}

#[test]
fn independent_reader_opens_during_writer_and_pins_its_committed_snapshot() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let directory = Directory::new();
        let environment = Environment::create_with_publication(&directory.0, mode).unwrap();
        let mut writer = environment.begin_write().unwrap();
        writer
            .create_database(b"records", DatabaseKind::Kv)
            .unwrap();
        writer.put(b"records", b"key", b"committed").unwrap();
        writer.commit().unwrap();

        let mut writer = environment.begin_write().unwrap();
        writer.put(b"records", b"key", b"private").unwrap();
        writer
            .create_database(b"private", DatabaseKind::Kv)
            .unwrap();
        assert_eq!(
            writer.database_names().unwrap(),
            vec![b"private".to_vec(), b"records".to_vec()]
        );
        // This succeeds synchronously while the writer and its file fence are
        // live. Acquiring the ordinary writer capability would fail Busy.
        let old = ReadTxn::open_snapshot(&directory.0).unwrap();
        assert_eq!(old.database_names().unwrap(), vec![b"records".to_vec()]);
        let borrowed = old.get(b"records", b"key").unwrap().unwrap();
        assert_eq!(borrowed, b"committed");
        writer.commit().unwrap();
        let current = ReadTxn::open_snapshot(&directory.0).unwrap();
        assert_eq!(
            current.database_names().unwrap(),
            vec![b"private".to_vec(), b"records".to_vec()]
        );
        assert_eq!(
            current.get(b"records", b"key").unwrap(),
            Some(b"private".as_slice())
        );
        assert_eq!(borrowed, b"committed");

        // A standalone snapshot holds mappings, not the writer lease.
        drop(environment);
        let reopened = Environment::open(&directory.0).unwrap();
        assert_eq!(borrowed, b"committed");
        assert_eq!(
            reopened.begin_read().get(b"records", b"key").unwrap(),
            Some(b"private".as_slice())
        );
        let current_handle = current.open_database(b"records").unwrap();
        assert!(matches!(
            old.get_handle(&current_handle, b"key"),
            Err(Error::StaleHandle)
        ));
        assert!(matches!(
            reopened.begin_read().get_handle(&current_handle, b"key"),
            Err(Error::StaleHandle)
        ));
    }
}

#[test]
fn independent_reader_rejects_damaged_publication_without_repairing_it() {
    let directory = Directory::new();
    let environment = Environment::create(&directory.0).unwrap();
    let publication = directory.0.join("CURRENT");
    let mut bytes = std::fs::read(&publication).unwrap();
    bytes[0] ^= 1;
    std::fs::write(&publication, &bytes).unwrap();
    assert!(matches!(
        ReadTxn::open_snapshot(&directory.0),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(std::fs::read(&publication).unwrap(), bytes);
    drop(environment);
}

#[test]
fn independent_reader_rejects_recovery_backed_range_environments() {
    let directory = Directory::new();
    #[cfg(not(feature = "range-lifecycle"))]
    let mode = PublicationMode::Range;
    #[cfg(feature = "range-lifecycle")]
    let mode = PublicationMode::RangeV5;
    let _environment = Environment::create_with_publication(&directory.0, mode).unwrap();
    assert!(matches!(
        ReadTxn::open_snapshot(&directory.0),
        Err(Error::InvalidInput(_))
    ));
}
