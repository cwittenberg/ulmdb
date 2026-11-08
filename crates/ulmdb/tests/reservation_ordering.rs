use std::{fs, ops::Bound, sync::Arc};
use ulmdb::ordering::{DatabaseComparators, DatabaseOrdering, Endian, OrderingSpec, Width};
use ulmdb::{DatabaseKind, DatabaseViewTransfer, Environment, PageSize, PublicationMode};
struct Dir(std::path::PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "reserve-order-{}",
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
fn ordered_reserved_payloads_split_and_reopen_under_real_builtin_order() {
    for mode in MODES {
        for order in [
            OrderingSpec::ReverseBytes,
            OrderingSpec::UnsignedInteger {
                width: Width::U32,
                endian: Endian::Little,
            },
        ] {
            let d = Dir::new();
            let e = Environment::create_with_page_size(&d.0, mode, PageSize::new(512).unwrap())
                .unwrap();
            let mut w = e.begin_write().unwrap();
            w.create_database_ordered(
                b"kv",
                DatabaseKind::Kv,
                DatabaseOrdering {
                    keys: order,
                    duplicates: OrderingSpec::Bytewise,
                },
            )
            .unwrap();
            for n in 0..330u32 {
                let cap = w.reserve(b"kv", &n.to_le_bytes(), 12).unwrap();
                w.reservation_value_mut(&cap).unwrap().fill((n % 251) as u8);
                w.end_reservation(cap).unwrap();
            }
            if matches!(order, OrderingSpec::UnsignedInteger { .. }) {
                let before = w.dirty_bytes();
                assert!(w.reserve(b"kv", b"bad", 8).is_err());
                assert_eq!(w.dirty_bytes(), before);
            }
            w.commit().unwrap();
            drop(e);
            let e = Environment::open(&d.0).unwrap();
            let read = e.begin_read();
            for n in 0..330u32 {
                assert_eq!(
                    read.get(b"kv", &n.to_le_bytes()).unwrap(),
                    Some([(n % 251) as u8; 12].as_slice())
                );
            }
            let mut expected = (0..330u32)
                .map(|n| n.to_le_bytes().to_vec())
                .collect::<Vec<_>>();
            if order == OrderingSpec::ReverseBytes {
                expected.sort_by(|a, b| a.iter().rev().cmp(b.iter().rev()));
            }
            assert_eq!(
                read.kv(b"kv")
                    .unwrap()
                    .cursor(Bound::Unbounded, Bound::Unbounded, false)
                    .unwrap()
                    .map(|r| r.unwrap().0.to_vec())
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
}
#[test]
fn custom_reserved_equality_preserves_representative_and_retained_authority() {
    let callbacks = DatabaseComparators {
        keys: Some(Arc::new(|a, b| {
            a.iter()
                .map(u8::to_ascii_lowercase)
                .cmp(b.iter().map(u8::to_ascii_lowercase))
        })),
        duplicates: None,
    };
    for mode in MODES {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let spec = DatabaseOrdering {
            keys: OrderingSpec::Custom,
            duplicates: OrderingSpec::Bytewise,
        };
        let mut w = e.begin_write().unwrap();
        w.create_database_ordered(b"kv", DatabaseKind::Kv, spec)
            .unwrap();
        let before = w.dirty_bytes();
        assert!(w.reserve(b"kv", b"KEY", 32).is_err());
        assert_eq!(w.dirty_bytes(), before);
        for n in 0..250 {
            let k = format!("KEY-{n:04}");
            let cap = w
                .reserve_with_ordering(b"kv", k.as_bytes(), 32, &callbacks)
                .unwrap();
            w.reservation_value_mut(&cap).unwrap().fill(1);
            w.end_reservation(cap).unwrap();
        }
        let cap = w
            .reserve_with_ordering(b"kv", b"key-0001", 32, &callbacks)
            .unwrap();
        w.reservation_value_mut(&cap).unwrap().fill(2);
        w.end_reservation(cap).unwrap();
        w.commit().unwrap();
        let read = e.begin_read();
        let rows = read
            .kv_with_ordering(b"kv", &callbacks)
            .unwrap()
            .cursor(Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .map(|r| r.unwrap().0.to_vec())
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 250);
        assert_eq!(rows[1], b"KEY-0001");
        let mut w = e.begin_write().unwrap();
        let h = w.open_database(b"kv").unwrap();
        let v = w.open_database_view_with_ordering(&h, &callbacks).unwrap();
        assert!(w.view_reserve(&v, b"KEY-0001", 10).is_err());
        let group = w.retain_database_views(&h, &[&v]).unwrap();
        let retained = w.retained_database_view(&group, &v).unwrap();
        let cap = w
            .retained_view_reserve_with_ordering(&retained, b"key-0001", 16, &callbacks)
            .unwrap();
        w.reservation_value_mut(&cap).unwrap().fill(9);
        w.end_reservation(cap).unwrap();
        w.create_database_ordered(b"kv", DatabaseKind::Kv, spec)
            .unwrap();
        let dest = w.open_database(b"kv").unwrap();
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
        assert_eq!(
            e.begin_read()
                .get_with_ordering(b"kv", b"key-0001", &callbacks)
                .unwrap(),
            Some([9; 16].as_slice())
        );
    }
}
