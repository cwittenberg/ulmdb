use std::{fs, path::PathBuf};
use ulmdb::{DatabaseKind, Environment, PublicationMode, ValueLimit};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-large-{}",
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
#[test]
fn jumbo_profile_preflight_savepoint_and_wire_defaults() {
    for mode in MODES {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        let before = w.dirty_bytes();
        assert!(w.reserve(b"kv", b"x", usize::MAX).is_err());
        assert!(
            w.reserve(b"kv", b"x", ValueLimit::EXTENDED.get() + 1)
                .is_err()
        );
        assert_eq!(w.dirty_bytes(), before);
        {
            let mut c = w.savepoint().unwrap();
            c.set_value_limit(ValueLimit::LARGE).unwrap();
            assert_eq!(c.value_limit(), ValueLimit::LARGE);
            assert!(
                c.reserve(b"kv", b"large", ValueLimit::LARGE.get() + 1)
                    .is_err()
            );
            c.abort().unwrap();
        }
        assert_eq!(w.value_limit(), ValueLimit::DEFAULT);
        w.put(b"kv", b"x", b"okay").unwrap();
        w.commit().unwrap();
        assert_eq!(
            e.begin_read().file_manifest().unwrap().value_limit,
            ValueLimit::DEFAULT
        );
    }
}
/// Explicit bounded resource journey; the default workspace suite does not
/// allocate65MiB values. Run this named case with--ignored when changing limits.
#[test]
#[ignore = "explicit129MiB file-value/copy/import resource journey"]
fn jumbo_reserve_borrow_copy_incremental_and_reopen() {
    use ulmdb::{OwnedFileCapture, ResidencyPolicy, backup};
    use ulmdb_storage::coordination::Coordination;
    const LENGTH: usize = 129 * 1024 * 1024;
    for mode in MODES {
        let d = Dir::new();
        let copied = Dir::new();
        let compact = Dir::new();
        let delta_target = Dir::new();
        let delta_dir = Dir::new();
        fs::create_dir(&delta_dir.0).unwrap();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"small", b"A").unwrap();
        w.commit().unwrap();
        backup::export(&e, &delta_target.0).unwrap();
        let old = e.begin_read();
        // A child allocation/upgrade can be fully rolled back before the real write.
        let mut w = e.begin_write().unwrap();
        {
            let mut child = w.savepoint().unwrap();
            child.set_value_limit(ValueLimit::LARGE).unwrap();
            let cap = child.reserve(b"kv", b"aborted", LENGTH).unwrap();
            child.reservation_value_mut(&cap).unwrap()[0] = 1;
            child.end_reservation(cap).unwrap();
            child.abort().unwrap();
        }
        assert!(w.get(b"kv", b"aborted").unwrap().is_none());
        assert_eq!(w.value_limit(), ValueLimit::DEFAULT);
        w.set_value_limit(ValueLimit::LARGE).unwrap();
        let cap = w.reserve(b"kv", b"large", LENGTH).unwrap();
        let p = {
            let bytes = w.reservation_value_mut(&cap).unwrap();
            bytes.fill(0x6d);
            bytes[0] = 0x11;
            bytes[LENGTH - 1] = 0x22;
            bytes.as_ptr()
        };
        assert_eq!(w.get(b"kv", b"large").unwrap().unwrap().as_ptr(), p);
        w.end_reservation(cap).unwrap();
        w.commit().unwrap();
        let mut delta = fs::File::create(delta_dir.0.join("delta")).unwrap();
        e.dump_incremental(1, &mut delta).unwrap();
        drop(delta);
        let target = Environment::open(&delta_target.0).unwrap();
        let mut delta = fs::File::open(delta_dir.0.join("delta")).unwrap();
        target.load_incremental(&mut delta).unwrap();
        assert_eq!(
            target.begin_read().file_manifest().unwrap().value_limit,
            ValueLimit::LARGE
        );
        assert_eq!(
            target
                .begin_read()
                .get(b"kv", b"large")
                .unwrap()
                .unwrap()
                .len(),
            LENGTH
        );
        drop(target);
        let r = e.begin_read();
        assert_eq!(
            r.structural_pairs(b"kv")
                .unwrap()
                .map(Result::unwrap)
                .count(),
            2
        );
        assert_eq!(r.file_manifest().unwrap().value_limit, ValueLimit::LARGE);
        let borrowed = r.get(b"kv", b"large").unwrap().unwrap();
        assert_eq!(
            (borrowed.len(), borrowed[0], borrowed[LENGTH - 1]),
            (LENGTH, 0x11, 0x22)
        );
        assert_eq!(old.get(b"kv", b"large").unwrap(), None);
        // Ordinary put consumes a mapped borrow, without a mandatory caller-sized Vec.
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"large", borrowed).unwrap();
        assert_eq!(
            w.structural_pairs(b"kv")
                .unwrap()
                .map(Result::unwrap)
                .count(),
            2
        );
        w.commit().unwrap();
        assert_eq!(borrowed[0], 0x11);
        let pair = OwnedFileCapture::current_for_copy(
            &d.0,
            Coordination::capture_directory(&d.0, Default::default()).unwrap(),
        )
        .unwrap()
        .into_copy_pair()
        .unwrap()
        .admit(ResidencyPolicy::Demand)
        .unwrap();
        backup::export_file_pair(&pair, &copied.0).unwrap();
        let view = OwnedFileCapture::current_for_copy(
            &d.0,
            Coordination::capture_directory(&d.0, Default::default()).unwrap(),
        )
        .unwrap()
        .into_copy_pair()
        .unwrap()
        .admit_selected_view(ResidencyPolicy::Demand)
        .unwrap();
        backup::compact_file_copy(&view, &compact.0).unwrap();
        for path in [&copied.0, &compact.0] {
            let out = Environment::open(path).unwrap();
            let read = out.begin_read();
            let bytes = read.get(b"kv", b"large").unwrap().unwrap();
            assert_eq!(
                (bytes.len(), bytes[0], bytes[LENGTH - 1]),
                (LENGTH, 0x11, 0x22)
            );
            assert_eq!(read.file_manifest().unwrap().value_limit, ValueLimit::LARGE);
            backup::verify(&out).unwrap();
        }
        e.rollback_last_commit(2).unwrap();
        assert_eq!(
            e.begin_read().file_manifest().unwrap().value_limit,
            ValueLimit::LARGE
        );
        assert_eq!(e.begin_read().get(b"kv", b"large").unwrap(), None);
        assert_eq!(borrowed[LENGTH - 1], 0x22);
        drop(view);
        drop(pair);
        drop(r);
        drop(old);
        drop(e);
        let e = Environment::open(&d.0).unwrap();
        assert_eq!(
            e.begin_read().file_manifest().unwrap().value_limit,
            ValueLimit::LARGE
        );
        assert_eq!(e.begin_read().get(b"kv", b"large").unwrap(), None);
    }
}

#[test]
#[ignore = "explicit257MiB contiguous mapped value across the old256MiB segment boundary"]
fn jumbo_mapped_segment_and_cached_cold_read() {
    use ulmdb::{MappedFlush, chunked::ChunkedReadTxn};
    use ulmdb_storage::chunk_cache::{ChunkCache, Limits};
    const LENGTH: usize = 257 << 20;
    for mode in MODES {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"old", b"A").unwrap();
        w.commit().unwrap();
        let old = e.begin_read();
        let mut w = e.begin_write().unwrap();
        w.enable_write_map(600 << 20, MappedFlush::Synchronous)
            .unwrap();
        w.set_value_limit(ValueLimit::LARGE).unwrap();
        let info = w.mapped_allocation().unwrap().unwrap();
        let cap = w.reserve(b"kv", b"large", LENGTH).unwrap();
        let bytes = w.reservation_value_mut(&cap).unwrap();
        bytes.fill(0x4d);
        bytes[0] = 0x11;
        bytes[LENGTH - 1] = 0x22;
        let at = bytes.as_ptr() as usize;
        assert!((info.address..info.address + info.capacity).contains(&at));
        assert!(at.checked_add(LENGTH).unwrap() <= info.address + info.capacity);
        w.end_reservation(cap).unwrap();
        w.prepare().unwrap().commit().unwrap();
        assert_eq!(old.get(b"kv", b"old").unwrap(), Some(b"A".as_slice()));
        assert!(old.get(b"kv", b"large").unwrap().is_none());
        let m = e.begin_read().file_manifest().unwrap().clone();
        assert!(m.segments.iter().any(|s| s.length > 256 << 20));
        // An unprofiled low-level loader must not silently accept the new asset.
        assert!(ulmdb_storage::SegmentTable::load(&d.0, &m.segments).is_err());
        drop(old);
        drop(e);
        let e = Environment::open(&d.0).unwrap();
        let r = e.begin_read();
        let b = r.get(b"kv", b"large").unwrap().unwrap();
        assert_eq!((b.len(), b[0], b[LENGTH - 1]), (LENGTH, 0x11, 0x22));
        let cache = ChunkCache::new(Limits {
            mapped_bytes: 2 << 30,
            windows: 512,
            chunk_bytes: 64 << 10,
            max_window_bytes: 1 << 30,
        })
        .unwrap();
        let cached = ChunkedReadTxn::open_coordinated_snapshot(&d.0, cache).unwrap();
        let b = cached.get(b"kv", b"large").unwrap().unwrap();
        assert_eq!((b.len(), b[0], b[LENGTH - 1]), (LENGTH, 0x11, 0x22));
        println!("JUMBO_MAPPED_PASS {mode:?} len={LENGTH}");
    }
}
