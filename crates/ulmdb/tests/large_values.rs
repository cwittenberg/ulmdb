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
fn unsupported_lengths_preflight_without_allocation_or_profile_change() {
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
#[ignore = "explicit65MiB file-value/copy/import resource journey"]
fn sixty_five_mib_reserve_borrow_copy_incremental_and_reopen() {
    use ulmdb::{OwnedFileCapture, ResidencyPolicy, backup};
    use ulmdb_storage::coordination::Coordination;
    const LENGTH: usize = 65 * 1024 * 1024;
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
            let cap = child.reserve(b"kv", b"aborted", LENGTH).unwrap();
            child.reservation_value_mut(&cap).unwrap()[0] = 1;
            child.end_reservation(cap).unwrap();
            child.abort().unwrap();
        }
        assert!(w.get(b"kv", b"aborted").unwrap().is_none());
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
            ValueLimit::EXTENDED
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
        assert_eq!(r.structural_pairs(b"kv").unwrap().map(Result::unwrap).count(), 2);
        assert_eq!(r.file_manifest().unwrap().value_limit, ValueLimit::EXTENDED);
        let borrowed = r.get(b"kv", b"large").unwrap().unwrap();
        assert_eq!(
            (borrowed.len(), borrowed[0], borrowed[LENGTH - 1]),
            (LENGTH, 0x11, 0x22)
        );
        assert_eq!(old.get(b"kv", b"large").unwrap(), None);
        // Ordinary put consumes a mapped borrow, without a mandatory caller-sized Vec.
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"large", borrowed).unwrap();
        assert_eq!(w.structural_pairs(b"kv").unwrap().map(Result::unwrap).count(), 2);
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
            assert_eq!(
                read.file_manifest().unwrap().value_limit,
                ValueLimit::EXTENDED
            );
            backup::verify(&out).unwrap();
        }
        e.rollback_last_commit(2).unwrap();
        assert_eq!(
            e.begin_read().file_manifest().unwrap().value_limit,
            ValueLimit::EXTENDED
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
            ValueLimit::EXTENDED
        );
        assert_eq!(e.begin_read().get(b"kv", b"large").unwrap(), None);
    }
}
#[test]
#[ignore = "explicit65MiB retained/prefix/bulk resource journey"]
fn retained_roots_and_prefix_bulk_keep_large_profile() {
    use ulmdb::{Address, DatabaseViewTransfer, Prefix};
    const LENGTH: usize = 65 * 1024 * 1024;
    for mode in MODES {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"X", DatabaseKind::Kv).unwrap();
        w.put(b"X", b"k", b"old").unwrap();
        w.commit().unwrap();
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"X").unwrap();
        let v = w.open_database_view(&h).unwrap();
        let group = w.retain_database_views(&h, &[&v]).unwrap();
        let retained = w.retained_database_view(&group, &v).unwrap();
        let cap = w.retained_view_reserve(&retained, b"k", LENGTH).unwrap();
        w.reservation_value_mut(&cap).unwrap().fill(0x39);
        w.end_reservation(cap).unwrap();
        w.create_database(b"X", DatabaseKind::Kv).unwrap();
        let dest = w.open_database(b"X").unwrap();
        w.finalize_database_view_groups(
            &[],
            &[DatabaseViewTransfer {
                source: &retained,
                destination: &dest,
            }],
            &[],
            &[&dest],
        )
        .unwrap();
        w.prepare().unwrap().commit().unwrap();
        let r = e.begin_read();
        let bytes = r.get(b"X", b"k").unwrap().unwrap();
        assert_eq!(bytes.len(), LENGTH);
        let mut w = e.begin_write().unwrap();
        w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
        w.load_sorted_prefixes(
            b"ip",
            [Ok((
                Prefix::new(Address::V4(0x0a000000), 8).unwrap(),
                bytes,
            ))],
        )
        .unwrap();
        w.commit().unwrap();
        e.compact().unwrap();
        assert_eq!(
            e.begin_read()
                .longest_match(b"ip", Address::V4(0x0a010101))
                .unwrap()
                .unwrap()
                .1
                .len(),
            LENGTH
        );
        ulmdb::backup::verify(&e).unwrap();
    }
}
