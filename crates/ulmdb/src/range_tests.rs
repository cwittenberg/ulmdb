//! Rehashed semantic corruption must fail before head healing or tail cleanup.
use crate::{DatabaseKind, Environment, Error, PublicationMode, catalog::Database};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};
use ulmdb_core::storage::{Reference, WriteStore};
use ulmdb_platform::FileExt;
use ulmdb_storage::{Arena, SegmentTable};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-range-semantic-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn bad_root(kind: u8, heal: bool) {
    let d = Dir::new();
    let mode = if std::env::var_os("ULMDB_TEST_RANGE_LOCATORS").is_some() {
        PublicationMode::Range
    } else {
        PublicationMode::RangeV5
    };
    let env = Environment::create_with_publication(&d.0, mode).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"stable").unwrap();
    w.commit().unwrap();
    let base = env.current.load_full();
    let mut arena = Arena::new(base.segments.clone(), base.manifest.next_segment).unwrap();
    let catalog = if kind == 0 {
        ulmdb_kv::put(
            &mut arena,
            Reference::NULL,
            b"malformed",
            b"not an eighty-byte descriptor",
        )
        .unwrap()
    } else {
        let db = Database {
            ordering: crate::ordering::DatabaseOrdering::default(),
            id: 1,
            kind: if kind == 1 {
                DatabaseKind::Kv
            } else {
                DatabaseKind::Ipv4
            },
            root: arena
                .allocate(
                    &vec![0; if kind == 1 { 4096 } else { 96 }],
                    if kind == 1 { 4096 } else { 16 },
                )
                .unwrap(),
        };
        ulmdb_kv::put(&mut arena, Reference::NULL, b"malformed", &db.encode()).unwrap()
    };
    let mut manifest = base.manifest.clone_metadata();
    manifest.previous = manifest.generation;
    manifest.parent = manifest.commit;
    manifest.generation = ulmdb_storage::new_identity().unwrap();
    manifest.commit = manifest.generation;
    manifest.sequence += 1;
    manifest.catalog = catalog;
    // The low-level substrate receives intentionally invalid typed roots but
    // computes every segment, descriptor, envelope and head hash correctly.
    let state = env
        .range
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .commit_arena(arena, manifest, &base.manifest, &env.residency)
        .unwrap();
    assert!(matches!(
        crate::backup::validate(&state.segments, &state.manifest),
        Err(Error::Corrupt(_))
    ));
    drop(state);
    drop(base);
    drop(env);
    let path = d.0.join("RANGE.pack");
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    if heal {
        let mut latest = (0, 0);
        for offset in [65536, 131072] {
            let mut b = [0; 4096];
            file.read_exact_at(&mut b, offset).unwrap();
            let ordinal = u64::from_le_bytes(b[40..48].try_into().unwrap());
            if ordinal > latest.0 {
                latest = (ordinal, offset);
            }
        }
        file.write_all_at(&[0x42], latest.1).unwrap();
    } else {
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::End(0)).unwrap();
        file.write_all(b"tail").unwrap();
    }
    file.sync_all().unwrap();
    drop(file);
    let before = fs::read(&path).unwrap();
    assert!(matches!(Environment::open(&d.0), Err(Error::Corrupt(_))));
    assert_eq!(fs::read(&path).unwrap(), before);
}
#[test]
fn bad_catalog_prevents_heal_and_trim() {
    for heal in [false, true] {
        bad_root(0, heal);
    }
}
#[test]
fn bad_kv_tree_prevents_heal_and_trim() {
    for heal in [false, true] {
        bad_root(1, heal);
    }
}
#[test]
fn bad_prefix_tree_prevents_heal_and_trim() {
    for heal in [false, true] {
        bad_root(2, heal);
    }
}
#[test]
fn logical_entry_equality_never_reuses_range_mapping_as_file_mapping() {
    let d = Dir::new();
    let mode = if std::env::var_os("ULMDB_TEST_RANGE_LOCATORS").is_some() {
        PublicationMode::Range
    } else {
        PublicationMode::RangeV5
    };
    let env = Environment::create_with_publication(&d.0, mode).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"a", b"b").unwrap();
    w.commit().unwrap();
    let s = env.current.load_full();
    let e = &s.manifest.segments[0];
    assert!(matches!(
        SegmentTable::default().reuse_from(&s.segments, e),
        Err(Error::InvalidInput(_))
    ));
}
