#![cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
use std::{fs, path::PathBuf};
use ulmdb::{DatabaseKind, Environment, MappedFlush, PublicationMode};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-write-map-{}",
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
#[test]
fn real_shared_file_mapping_reservation_savepoint_seal_inode_and_old_reader() {
    for mode in modes() {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        assert_eq!(w.mapped_allocation().unwrap(), None);
        w.create_database(b"data", DatabaseKind::Kv).unwrap();
        w.put(b"data", b"k", b"A").unwrap();
        w.commit().unwrap();
        let old = e.begin_read();
        let mut w = e.begin_write().unwrap();
        w.enable_write_map(2 << 20, MappedFlush::Synchronous)
            .unwrap();
        let info = w.mapped_allocation().unwrap().unwrap();
        let maps = fs::read_to_string("/proc/self/maps").unwrap();
        let line = maps
            .lines()
            .find(|line| line.starts_with(&format!("{:x}-", info.address)))
            .unwrap();
        assert!(
            line.contains("rw-s") && line.contains("writemap.tmp"),
            "{line}"
        );
        assert!(info.file_inode > 0);
        let reserve = w.reserve(b"data", b"mapped", 48_000).unwrap();
        let bytes = w.reservation_value_mut(&reserve).unwrap();
        let addr = bytes.as_ptr() as usize;
        assert!((info.address..info.address + info.capacity).contains(&addr));
        bytes.fill(0x76);
        w.end_reservation(reserve).unwrap();
        {
            let mut child = w.savepoint().unwrap();
            child.put(b"data", b"k", b"discard").unwrap();
            child.abort().unwrap();
        }
        assert_eq!(w.get(b"data", b"k").unwrap(), Some(b"A".as_slice()));
        w.put(b"data", b"k", b"B").unwrap();
        let id = w.mapped_allocation().unwrap().unwrap();
        assert_eq!(id.address, info.address);
        w.commit().unwrap();
        assert_eq!(old.get(b"data", b"k").unwrap(), Some(b"A".as_slice()));
        assert_eq!(
            e.begin_read().get(b"data", b"mapped").unwrap().unwrap(),
            vec![0x76; 48_000]
        );
        let manifest = ulmdb_storage::read_current(&d.0).unwrap();
        let file = ulmdb_storage::segment_path(&d.0, manifest.segments.last().unwrap());
        assert_eq!(fs::metadata(file).unwrap().ino(), info.file_inode);
        assert!(
            fs::read_dir(d.0.join("transactions"))
                .unwrap()
                .next()
                .is_none()
        );
        drop(old);
        drop(e);
        let e = Environment::open(&d.0).unwrap();
        assert_eq!(
            e.begin_read().get(b"data", b"k").unwrap(),
            Some(b"B".as_slice())
        );
    }
}
#[test]
fn abort_and_capacity_failure_keep_head_and_remove_only_owned_mapping() {
    let d = Dir::new();
    let e = Environment::create(&d.0).unwrap();
    let before = fs::read(d.0.join("CURRENT")).unwrap();
    let mut w = e.begin_write().unwrap();
    w.enable_write_map(8192, MappedFlush::Synchronous).unwrap();
    w.create_database(b"data", DatabaseKind::Kv).unwrap();
    assert!(w.put(b"data", b"k", &vec![7; 16_000]).is_err());
    drop(w);
    assert_eq!(before, fs::read(d.0.join("CURRENT")).unwrap());
    assert!(
        fs::read_dir(d.0.join("transactions"))
            .unwrap()
            .next()
            .is_none()
    );
    let mut w = e.begin_write().unwrap();
    w.create_database(b"data", DatabaseKind::Kv).unwrap();
    w.put(b"data", b"k", b"ok").unwrap();
    w.commit().unwrap();
}
#[test]
fn asynchronous_legacy_mapping_selects_real_weak_debt_then_force_sync_repairs_files() {
    for mode in modes() {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.enable_write_map(1 << 20, MappedFlush::Asynchronous)
            .unwrap();
        w.create_database(b"data", DatabaseKind::Kv).unwrap();
        w.put(b"data", b"k", b"async").unwrap();
        let prepared = w.prepare().unwrap();
        assert_eq!(prepared.data_policy(), ulmdb::Durability::NoSync);
        prepared.commit().unwrap();
        let (_, capture, pin, state) =
            ulmdb_storage::coordination::Coordination::capture_directory(&d.0, Default::default())
                .unwrap()
                .capture_current_with_durability()
                .unwrap();
        assert!(state.has_data_debt());
        drop(pin);
        drop(capture);
        assert!(e.sync().unwrap().files_synced > 0);
        drop(e);
        assert_eq!(
            Environment::open(&d.0)
                .unwrap()
                .begin_read()
                .get(b"data", b"k")
                .unwrap(),
            Some(b"async".as_slice())
        );
    }
}
