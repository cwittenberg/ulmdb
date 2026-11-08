#![cfg(target_os = "linux")]
use std::{fs, path::PathBuf};
use ulmdb::{
    CatalogCapacity, DatabaseKind, Environment, Error, OwnedFileCapture, PublicationMode,
    ResidencyPolicy, backup,
};
use ulmdb_storage::coordination::Coordination;
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "catalog-profile-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn name(n: usize) -> Vec<u8> {
    format!("db{n:05}").into_bytes()
}
#[test]
fn file_catalog_5005_commit_cold_compact_and_independent_copies_preserve_profile() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new();
        let copied = Dir::new();
        let compact = Dir::new();
        let env = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = env.begin_write().unwrap();
        for n in 0..4096 {
            w.create_database(&name(n), DatabaseKind::Kv).unwrap();
        }
        assert!(matches!(
            w.create_database(b"excess", DatabaseKind::Kv),
            Err(Error::LimitExceeded)
        ));
        {
            let mut child = w.savepoint().unwrap();
            child
                .set_catalog_capacity(CatalogCapacity::EXTENDED)
                .unwrap();
            child.create_database(b"undo", DatabaseKind::Kv).unwrap();
            child.abort().unwrap();
        }
        assert_eq!(w.catalog_capacity(), CatalogCapacity::DEFAULT);
        assert!(w.open_database(b"undo").is_err());
        w.set_catalog_capacity(CatalogCapacity::EXTENDED).unwrap();
        for n in 4096..5005 {
            w.create_database(&name(n), DatabaseKind::Kv).unwrap();
        }
        w.put(&name(4096), b"k", b"4097").unwrap();
        w.put(&name(5004), b"k", b"5005").unwrap();
        assert!(w.set_catalog_capacity(CatalogCapacity::DEFAULT).is_err());
        w.commit().unwrap();
        let r = env.begin_read();
        assert_eq!(r.catalog_capacity().unwrap(), CatalogCapacity::EXTENDED);
        assert_eq!(r.database_names().unwrap().len(), 5005);
        let held = r.get(&name(5004), b"k").unwrap().unwrap();
        backup::export(&env, &copied.0).unwrap();
        env.compact().unwrap();
        assert_eq!(held, b"5005");
        let capture = Coordination::capture_directory(&d.0, Default::default()).unwrap();
        let view = OwnedFileCapture::current_for_copy(&d.0, capture)
            .unwrap()
            .admit_copy_view(ResidencyPolicy::Demand)
            .unwrap();
        backup::compact_file_copy(&view, &compact.0).unwrap();
        drop(view);
        drop(r);
        drop(env);
        for path in [&d.0, &copied.0, &compact.0] {
            let e = Environment::open(path).unwrap();
            let r = e.begin_read();
            assert_eq!(r.catalog_capacity().unwrap(), CatalogCapacity::EXTENDED);
            assert_eq!(r.database_names().unwrap().len(), 5005);
            assert_eq!(r.get(&name(4096), b"k").unwrap(), Some(b"4097".as_slice()));
            assert_eq!(r.get(&name(5004), b"k").unwrap(), Some(b"5005".as_slice()));
            backup::verify(&e).unwrap();
        }
    }
}
#[test]
fn resident_catalog_5005_checkpoint_capture_export_incremental_and_savepoint() {
    use ulmdb::resident::{
        Environment as Resident, IncrementalLimits, Options, ReadTxn as ResidentRead, RedoFormat,
    };
    let o = Options {
        pool: ulmdb_storage::resident::Config {
            kv_pages: 4096,
            tiny_pages: 128,
            max_reserved_bytes: 64 << 20,
            max_metadata_bytes: 64 << 20,
            max_views: 128,
        },
        wal_capacity: 32 << 20,
        graph_read_budget: 32_000_000,
        auto_collect_below: 16,
    };
    for format in [RedoFormat::LiteralV1, RedoFormat::CompactV2] {
        let d = Dir::new();
        let copy = Dir::new();
        let env = Resident::create_with_redo(&d.0, o, format).unwrap();
        let mut w = env.begin_write().unwrap();
        for n in 0..4096 {
            w.create_database(&name(n), DatabaseKind::Kv).unwrap();
        }
        assert!(w.create_database(b"excess", DatabaseKind::Kv).is_err());
        {
            let mut child = w.savepoint().unwrap();
            child
                .set_catalog_capacity(CatalogCapacity::EXTENDED)
                .unwrap();
            child.abort().unwrap();
        }
        assert_eq!(w.catalog_capacity(), CatalogCapacity::DEFAULT);
        w.set_catalog_capacity(CatalogCapacity::EXTENDED).unwrap();
        for n in 4096..5005 {
            w.create_database(&name(n), DatabaseKind::Kv).unwrap();
        }
        w.put(&name(5004), b"k", b"5005").unwrap();
        w.commit().unwrap();
        if format == RedoFormat::CompactV2 {
            env.enable_incremental(IncrementalLimits::default())
                .unwrap();
        }
        let old = env.begin_read();
        let held = old.get(&name(5004), b"k").unwrap().unwrap();
        let mut w = env.begin_write().unwrap();
        assert!(w.set_catalog_capacity(CatalogCapacity::DEFAULT).is_err());
        w.put(&name(4096), b"k", b"4097").unwrap();
        w.commit().unwrap();
        env.collect().unwrap();
        env.checkpoint().unwrap();
        assert_eq!(held, b"5005");
        env.enable_readonly_capture().unwrap();
        let capture = ResidentRead::open_coordinated_snapshot(&d.0, o).unwrap();
        assert_eq!(capture.database_count(), 5005);
        assert_eq!(
            capture.catalog_capacity().unwrap(),
            CatalogCapacity::EXTENDED
        );
        drop(capture);
        env.export_snapshot(&copy.0).unwrap();
        drop(old);
        drop(env);
        for path in [&d.0, &copy.0] {
            let e = Resident::open(path, o).unwrap();
            let r = e.begin_read();
            assert_eq!(r.database_count(), 5005);
            assert_eq!(r.catalog_capacity().unwrap(), CatalogCapacity::EXTENDED);
            assert_eq!(r.get(&name(4096), b"k").unwrap(), Some(b"4097".as_slice()));
        }
    }
}
#[test]
fn exact_profile_codec_rejects_unknown_empty_metadata_and_unsupported_caps() {
    assert!(CatalogCapacity::DEFAULT.marker().is_none());
    let good = CatalogCapacity::EXTENDED.marker().unwrap();
    assert_eq!(
        CatalogCapacity::decode_marker(&good).unwrap(),
        CatalogCapacity::EXTENDED
    );
    for i in 0..24 {
        let mut bad = good;
        bad[i] ^= 128;
        assert!(CatalogCapacity::decode_marker(&bad).is_err());
    }
    for n in [0, 1, 4095, 4097, 65535, 65537] {
        assert!(CatalogCapacity::new(n).is_err());
    }
}
