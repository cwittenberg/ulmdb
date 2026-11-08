#![cfg(target_os = "linux")]
use std::{fs, os::unix::fs::MetadataExt, path::PathBuf};
use ulmdb::{DatabaseKind, Environment, MappedFlush, PublicationMode};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-no-mem-init-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
fn blocks(directory: &Dir, inode: u64) -> u64 {
    fs::read_dir(directory.0.join("transactions"))
        .unwrap()
        .map(|p| p.unwrap().metadata().unwrap())
        .find(|m| m.ino() == inode)
        .unwrap()
        .blocks()
        * 512
}
#[test]
fn fresh_zero_backed_reserve_skips_physical_zero_stores_only_when_enabled() {
    for mode in MODES {
        let mut allocation = [0; 2];
        for (index, enabled) in [false, true].into_iter().enumerate() {
            let d = Dir::new();
            let env = Environment::create_with_publication(&d.0, mode).unwrap();
            let mut seed = env.begin_write().unwrap();
            seed.create_database(b"data", DatabaseKind::Kv).unwrap();
            seed.commit().unwrap();
            let mut writer = env.begin_write().unwrap();
            writer
                .enable_write_map(8 << 20, MappedFlush::Synchronous)
                .unwrap();
            writer.set_mapped_no_mem_init(enabled).unwrap();
            let info = writer.mapped_allocation().unwrap().unwrap();
            let start = blocks(&d, info.file_inode);
            let reserve = writer.reserve(b"data", b"sparse", 4 << 20).unwrap();
            assert!(
                writer
                    .reservation_value(&reserve)
                    .unwrap()
                    .iter()
                    .all(|b| *b == 0)
            );
            allocation[index] = blocks(&d, info.file_inode) - start;
            writer.reservation_value_mut(&reserve).unwrap()[0] = 0x42;
            writer.end_reservation(reserve).unwrap();
            writer.commit().unwrap();
            drop(env);
            let env = Environment::open(&d.0).unwrap();
            let read = env.begin_read();
            let value = read.get(b"data", b"sparse").unwrap().unwrap();
            assert_eq!(value[0], 0x42);
            assert!(value[1..].iter().all(|b| *b == 0));
        }
        eprintln!(
            "NOMEMINIT {mode:?}: default={} sparse={} physical bytes",
            allocation[0], allocation[1]
        );
        assert!(
            allocation[0] >= 4 << 20,
            "default must really initialize backing: {allocation:?}"
        );
        assert!(
            allocation[1] < 128 << 10,
            "fresh suffix must remain sparse: {allocation:?}"
        );
    }
}
#[test]
fn savepoint_put_and_reserve_rollback_bytes_are_zeroed_before_reuse() {
    for mode in MODES {
        let d = Dir::new();
        let env = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut seed = env.begin_write().unwrap();
        seed.create_database(b"data", DatabaseKind::Kv).unwrap();
        seed.put(b"data", b"old", b"A").unwrap();
        seed.commit().unwrap();
        let old = env.begin_read();
        let mut writer = env.begin_write().unwrap();
        writer
            .enable_write_map(8 << 20, MappedFlush::Synchronous)
            .unwrap();
        writer.set_mapped_no_mem_init(true).unwrap();
        {
            let mut child = writer.savepoint().unwrap();
            // Ordinary put takes the extend_from_slice path, which must also
            // advance the monotonic exposure watermark across rollback.
            child
                .put(b"data", b"discarded", &vec![0xab; 2 << 20])
                .unwrap();
            child.abort().unwrap();
        }
        let reserve = writer.reserve(b"data", b"live", 1536 << 10).unwrap();
        assert!(
            writer
                .reservation_value(&reserve)
                .unwrap()
                .iter()
                .all(|b| *b == 0)
        );
        writer.reservation_value_mut(&reserve).unwrap().fill(0x37);
        writer.end_reservation(reserve).unwrap();
        {
            let mut child = writer.savepoint().unwrap();
            let reserve = child.reserve(b"data", b"discarded", 1 << 20).unwrap();
            child.reservation_value_mut(&reserve).unwrap().fill(0xcd);
            child.end_reservation(reserve).unwrap();
            child.abort().unwrap();
        }
        let reserve = writer.reserve(b"data", b"zero", (1 << 20) + 17).unwrap();
        assert!(
            writer
                .reservation_value(&reserve)
                .unwrap()
                .iter()
                .all(|b| *b == 0)
        );
        writer.end_reservation(reserve).unwrap();
        writer.commit().unwrap();
        assert_eq!(old.get(b"data", b"old").unwrap(), Some(b"A".as_slice()));
        assert_eq!(old.get(b"data", b"live").unwrap(), None);
        let read = env.begin_read();
        assert!(
            read.get(b"data", b"live")
                .unwrap()
                .unwrap()
                .iter()
                .all(|b| *b == 0x37)
        );
        assert!(
            read.get(b"data", b"zero")
                .unwrap()
                .unwrap()
                .iter()
                .all(|b| *b == 0)
        );
        assert_eq!(read.get(b"data", b"discarded").unwrap(), None);
    }
}
#[test]
fn heap_reservation_stays_initialized_when_mapped_optimization_is_requested() {
    let d = Dir::new();
    let env = Environment::create(&d.0).unwrap();
    let mut writer = env.begin_write().unwrap();
    writer.set_mapped_no_mem_init(true).unwrap();
    writer.create_database(b"data", DatabaseKind::Kv).unwrap();
    let reserve = writer.reserve(b"data", b"heap", 65536).unwrap();
    assert!(
        writer
            .reservation_value(&reserve)
            .unwrap()
            .iter()
            .all(|b| *b == 0)
    );
}
