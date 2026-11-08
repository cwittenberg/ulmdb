use std::fs;
use ulmdb::{DatabaseKind, Environment, Error, PublicationMode};
struct Dir(std::path::PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-reserve-{}",
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
fn reserve_is_actual_initialized_payload_without_retirement_copy() {
    for mode in MODES {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"old", b"A").unwrap();
        w.commit().unwrap();
        let old = e.begin_read();
        let mut w = e.begin_write().unwrap();
        let before = w.dirty_bytes();
        let cap = w.reserve(b"kv", b"large", 2 * 1024 * 1024).unwrap();
        let after = w.dirty_bytes();
        assert!(
            after - before < cap.len() + 4 * 65536,
            "reserve may not retain a second value-sized allocation"
        );
        assert!(w.reservation_value(&cap).unwrap().iter().all(|b| *b == 0));
        let pointer = {
            let v = w.reservation_value_mut(&cap).unwrap();
            v.fill(0x45);
            v.as_ptr()
        };
        assert_eq!(w.get(b"kv", b"large").unwrap().unwrap().as_ptr(), pointer);
        for _ in 0..10 {
            assert_eq!(w.get(b"kv", b"large").unwrap().unwrap().len(), cap.len());
            assert_eq!(w.reservation_value(&cap).unwrap().as_ptr(), pointer);
        }
        w.reservation_value_mut(&cap).unwrap()[17] = 0x99;
        let revision_bytes = w.dirty_bytes();
        w.end_reservation(cap).unwrap();
        assert_eq!(w.dirty_bytes(), revision_bytes);
        assert_eq!(w.get(b"kv", b"large").unwrap().unwrap()[17], 0x99);
        w.commit().unwrap();
        assert_eq!(old.get(b"kv", b"large").unwrap(), None);
        drop(old);
        drop(e);
        let e = Environment::open(&d.0).unwrap();
        assert_eq!(
            e.begin_read().get(b"kv", b"large").unwrap().unwrap()[17],
            0x99
        );
    }
}
#[test]
fn foreign_stale_savepoint_and_private_reader_guards() {
    for mode in MODES {
        let d = Dir::new();
        let d2 = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let e2 = Environment::create_with_publication(&d2.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        let mut other = e2.begin_write().unwrap();
        other.create_database(b"kv", DatabaseKind::Kv).unwrap();
        let cap = w.reserve(b"kv", b"k", 32).unwrap();
        w.reservation_value_mut(&cap).unwrap().fill(9);
        assert!(matches!(
            other.reservation_value_mut(&cap),
            Err(Error::StaleHandle)
        ));
        {
            let mut child = w.savepoint().unwrap();
            assert!(matches!(
                child.reservation_value(&cap),
                Err(Error::StaleHandle)
            ));
            let child_cap = child.reserve(b"kv", b"k", 32).unwrap();
            child.reservation_value_mut(&child_cap).unwrap().fill(3);
            child.abort().unwrap();
            assert!(matches!(
                w.reservation_value(&child_cap),
                Err(Error::StaleHandle)
            ));
        }
        assert_eq!(w.get(b"kv", b"k").unwrap(), Some([9; 32].as_slice()));
        let next = w.reserve(b"kv", b"next", 0).unwrap();
        assert!(w.reservation_value_mut(&next).unwrap().is_empty());
        w.end_reservation(next).unwrap();
        let handle = w.open_database(b"kv").unwrap();
        let view = w.open_database_view(&handle).unwrap();
        let reserved = w.view_reserve(&view, b"branch", 512).unwrap();
        w.reservation_value_mut(&reserved).unwrap().fill(7);
        w.end_reservation(reserved).unwrap();
        {
            let read = w.database_read_scope().unwrap();
            let loan = read.loan_database_view(&view).unwrap();
            assert_eq!(
                read.get(&loan, b"branch").unwrap(),
                Some([7; 512].as_slice())
            );
        }
        w.select_database_views(&[&view]).unwrap();
        w.commit().unwrap();
    }
}
#[test]
fn retained_branch_reservation_prepares_and_publishes_actual_bytes() {
    for mode in MODES {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"X", DatabaseKind::Kv).unwrap();
        w.put(b"X", b"key", b"base").unwrap();
        w.commit().unwrap();
        let mut w = e.begin_write().unwrap();
        let a = w.open_database(b"X").unwrap();
        let view = w.open_database_view(&a).unwrap();
        let group = w.retain_database_views(&a, &[&view]).unwrap();
        let retained = w.retained_database_view(&group, &view).unwrap();
        assert!(matches!(
            w.view_reserve(&view, b"key", 8192),
            Err(Error::MissingDatabase) | Err(Error::StaleHandle)
        ));
        let cap = w.retained_view_reserve(&retained, b"key", 8192).unwrap();
        w.reservation_value_mut(&cap).unwrap().fill(0x55);
        w.end_reservation(cap).unwrap();
        w.create_database(b"X", DatabaseKind::Kv).unwrap();
        let destination = w.open_database(b"X").unwrap();
        w.finalize_database_view_groups(
            &[],
            &[ulmdb::DatabaseViewTransfer {
                source: &retained,
                destination: &destination,
            }],
            &[],
            &[&destination],
        )
        .unwrap();
        let prepared = w.prepare().unwrap();
        assert_eq!(
            e.begin_read().get(b"X", b"key").unwrap(),
            Some(b"base".as_slice())
        );
        prepared.commit().unwrap();
        assert_eq!(
            e.begin_read().get(b"X", b"key").unwrap(),
            Some([0x55; 8192].as_slice())
        );
    }
}
