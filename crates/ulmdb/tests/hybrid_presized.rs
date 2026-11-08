use std::path::{Path, PathBuf};
use ulmdb::{
    Address, DatabaseKind, Prefix,
    hybrid::{Environment, ImageMode, Options, WalLayout},
};
use ulmdb_platform::FileExt;
use ulmdb_storage::hybrid::checkpoint::{CheckpointLimits, read_descriptor};
const CAP: u64 = 16 * 1024 * 1024;
struct Temp(PathBuf);
impl Temp {
    fn new(base: &Path) -> Self {
        let p = base.join(format!(
            "ulm-presized-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn options(image: ImageMode) -> Options {
    let mut o = Options {
        image,
        wal_layout: WalLayout::PreSized { capacity: CAP },
        ..Options::default()
    };
    o.chunk.capacity = 2 * 1024 * 1024;
    o.chunk.reserve_quantum = 65536;
    o.chunk.max_burned_bytes = 1024 * 1024;
    o.max_image_bytes = 8 * 1024 * 1024;
    o
}
#[test]
fn real_kv_ip_noop_abort_old_borrows_reopen_and_checkpoint_receipts() {
    for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
        for mode in [ImageMode::File, ImageMode::Anonymous] {
            let t = Temp::new(base);
            let o = options(mode);
            let e = Environment::create(&t.0, o).unwrap();
            let g = e.capture_checkpoint().unwrap();
            let a = g
                .stage(t.0.join("genesis"), 71, CheckpointLimits::default())
                .unwrap();
            let d = read_descriptor(&a.directory().join("CHECKPOINT"), 1).unwrap();
            assert_eq!(d.selected, g.selection());
            let v4 = Prefix::new(Address::V4(0x0a000000), 8).unwrap();
            let v6 = Prefix::new(Address::V6(0x20010db8000000000000000000000000), 32).unwrap();
            let mut w = e.begin_write().unwrap();
            for (name, kind) in [
                (b"kv".as_slice(), DatabaseKind::Kv),
                (b"v4", DatabaseKind::Ipv4),
                (b"v6", DatabaseKind::Ipv6),
            ] {
                w.create_database(name, kind).unwrap();
            }
            for i in 0..256u64 {
                w.put(b"kv", &i.to_be_bytes(), &i.to_le_bytes()).unwrap();
            }
            w.insert_prefix(b"v4", v4, b"old").unwrap();
            w.insert_prefix(b"v6", v6, b"old").unwrap();
            w.commit().unwrap();
            let old = e.begin_read();
            let held = old.get(b"kv", &7u64.to_be_bytes()).unwrap().unwrap();
            let wal = t.0.join("HYBRID.wal");
            let before = std::fs::read(&wal).unwrap();
            e.begin_write().unwrap().commit().unwrap();
            assert_eq!(std::fs::read(&wal).unwrap(), before);
            {
                let mut w = e.begin_write().unwrap();
                w.put(b"kv", &7u64.to_be_bytes(), b"abort").unwrap();
            }
            assert_eq!(std::fs::read(&wal).unwrap(), before);
            let mut w = e.begin_write().unwrap();
            w.put(b"kv", &7u64.to_be_bytes(), b"new").unwrap();
            w.insert_prefix(b"v4", v4, b"new").unwrap();
            w.insert_prefix(b"v6", v6, b"new").unwrap();
            w.commit().unwrap();
            assert_eq!(held, 7u64.to_le_bytes());
            assert_eq!(
                old.prefix_index(b"v4")
                    .unwrap()
                    .exact(v4)
                    .unwrap()
                    .unwrap()
                    .1,
                b"old"
            );
            let c = e.capture_checkpoint().unwrap();
            let a = c
                .stage(t.0.join("checkpoint"), 72, CheckpointLimits::default())
                .unwrap();
            let d = read_descriptor(&a.directory().join("CHECKPOINT"), 8).unwrap();
            assert_eq!(d.selected, c.selection());
            assert_eq!(std::fs::metadata(&wal).unwrap().len(), CAP);
            assert!(
                ulmdb_platform::file_identity(&std::fs::File::open(&wal).unwrap())
                    .unwrap()
                    .allocated_bytes
                    >= CAP
            );
            drop(e);
            let frozen = std::fs::read(&wal).unwrap();
            assert!(
                Environment::open(
                    &t.0,
                    Options {
                        wal_layout: WalLayout::Dense,
                        ..o
                    }
                )
                .is_err()
            );
            assert_eq!(std::fs::read(&wal).unwrap(), frozen);
            let e = Environment::open(&t.0, o).unwrap();
            let r = e.begin_read();
            assert_eq!(
                r.get(b"kv", &7u64.to_be_bytes()).unwrap(),
                Some(b"new".as_slice())
            );
            assert_eq!(held, 7u64.to_le_bytes());
            assert_eq!(
                r.prefix_index(b"v4")
                    .unwrap()
                    .longest_match(Address::V4(0x0a010101))
                    .unwrap()
                    .unwrap()
                    .1,
                b"new"
            );
            assert_eq!(
                r.prefix_index(b"v6")
                    .unwrap()
                    .longest_match(v6.address_bounds().0)
                    .unwrap()
                    .unwrap()
                    .1,
                b"new"
            );
        }
    }
}
#[test]
fn correctly_hashed_invalid_catalog_fails_before_heal_or_private_clear() {
    use ulmdb_storage::hybrid::{
        journal::{Bootstrap, Journal},
        redo::*,
    };
    struct NoSources;
    impl SourceResolver for NoSources {
        fn resolve_committed<'a>(
            &'a self,
            _: &Extent,
            _: &SourceReceipt,
        ) -> ulmdb_storage::Result<&'a [u8]> {
            Err(ulmdb_storage::Error::Corrupt("no sources"))
        }
    }
    for torn in [false, true] {
        let t = Temp::new(Path::new("/tmp"));
        std::fs::create_dir(&t.0).unwrap();
        let wal = t.0.join("HYBRID.wal");
        let h = Bootstrap {
            uuid: [7; 16],
            epoch: 1,
            commit: 9,
        };
        let mut j =
            Journal::create_with_layout(&wal, h, WalLayout::PreSized { capacity: CAP }).unwrap();
        let at = ulmdb_storage::Reference {
            segment: 1,
            offset: 4096,
        };
        let r = Record {
            uuid: h.uuid,
            epoch: h.epoch,
            lsn: 1,
            sequence: 1,
            commit: 10,
            parent_commit: j.commit_id(),
            parent: j.selected(),
            root: RootState {
                catalog: at,
                next_chunk: 2,
                next_database: 2,
            },
            chunks: vec![ChunkDelta {
                chunk: 1,
                logical_generation: 7,
                before: 4096,
                after: 4160,
                capacity: 2 * 1024 * 1024,
            }],
            objects: vec![ObjectRedo::Literal {
                dest: Extent {
                    at,
                    logical_generation: 7,
                    length: 64,
                },
                bytes: vec![0; 64],
            }],
        };
        j.commit_checked(&r, &NoSources, |_| Ok(())).unwrap();
        drop(j);
        let f = std::fs::OpenOptions::new().write(true).open(&wal).unwrap();
        if torn {
            f.write_all_at(&[0xff], 131072).unwrap();
        } else {
            f.write_all_at(&[1], CAP - 1).unwrap();
        }
        drop(f);
        let before = std::fs::read(&wal).unwrap();
        assert!(Environment::open(&t.0, options(ImageMode::Anonymous)).is_err());
        assert_eq!(std::fs::read(&wal).unwrap(), before);
    }
}
