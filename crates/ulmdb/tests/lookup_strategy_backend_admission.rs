//! Admission at the live Resident ordering and managed Range boundaries.
//! These are integration controls, independent of the frozen bytewise benchmark.
mod lookup_strategy_support;

use lookup_strategy_support::{Dir, hash, resident_options};
use std::{
    ops::Bound,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use ulmdb::{
    DatabaseKind, Error,
    lookup::Options,
    ordering::{DatabaseComparators, DatabaseOrdering, Endian, OrderingSpec, Width},
    resident::{Environment, ReadTxn},
};

type Rows = Vec<(Vec<u8>, Vec<u8>)>;

fn own<'a>(cursor: impl Iterator<Item = ulmdb::Result<(&'a [u8], &'a [u8])>>) -> Rows {
    cursor
        .map(|item| {
            let (key, value) = item.unwrap();
            (key.to_vec(), value.to_vec())
        })
        .collect()
}

fn builtin_ordering(spec: OrderingSpec) -> DatabaseOrdering {
    DatabaseOrdering {
        keys: spec,
        duplicates: OrderingSpec::Bytewise,
    }
}

fn check_resident_builtin(read: &ReadTxn, name: &[u8], rows: &Rows) {
    assert!(matches!(
        read.prepare_lookup(name, hash()),
        Err(Error::InvalidInput(
            "hash lookup requires bytewise ordering"
        ))
    ));
    let native = read.kv(name).unwrap();
    let ordered = read.prepare_lookup(name, Options::Ordered).unwrap();
    assert!(!ordered.stats().enumerated);
    assert_eq!(ordered.stats().index_bytes, 0);
    assert_eq!(ordered.identity().commit, read.commit_identity());
    for (key, value) in rows {
        assert_eq!(native.get(key).unwrap(), Some(value.as_slice()));
        let actual = ordered.get(key).unwrap().unwrap();
        assert_eq!(actual, value);
        assert_eq!(actual.as_ptr(), native.get(key).unwrap().unwrap().as_ptr());
    }
    for (lower, upper, first, end) in [
        (Bound::Unbounded, Bound::Unbounded, 0, rows.len()),
        (
            Bound::Included(rows[3].0.as_slice()),
            Bound::Excluded(rows[rows.len() - 3].0.as_slice()),
            3,
            rows.len() - 3,
        ),
        (
            Bound::Excluded(rows[3].0.as_slice()),
            Bound::Included(rows[rows.len() - 3].0.as_slice()),
            4,
            rows.len() - 2,
        ),
        (
            Bound::Included(rows[17].0.as_slice()),
            Bound::Excluded(rows[17].0.as_slice()),
            17,
            17,
        ),
    ] {
        for reverse in [false, true] {
            let mut expected = rows[first..end].to_vec();
            if reverse {
                expected.reverse();
            }
            assert_eq!(own(native.cursor(lower, upper, reverse).unwrap()), expected);
            assert_eq!(
                own(ordered.cursor(lower, upper, reverse).unwrap()),
                expected
            );
        }
    }
}

#[test]
fn resident_integer_reverse_hash_refusal_preserves_native_ordered_get_and_ranges() {
    let dir = Dir::new();
    let options = resident_options();
    let env = Environment::create(&dir.0, options).unwrap();
    let mut integer: Rows = (0u32..520)
        .chain([65_535, 65_536, 131_071])
        .map(|id| (id.to_le_bytes().to_vec(), id.to_be_bytes().to_vec()))
        .collect();
    // Numeric little-endian order differs from bytewise at255/256 and65535/65536.
    integer.sort_unstable_by_key(|(key, _)| u32::from_le_bytes(key.as_slice().try_into().unwrap()));
    let mut reverse: Rows = (0..180u32)
        .map(|id| {
            (
                format!("item-{id:03}").into_bytes(),
                id.to_be_bytes().to_vec(),
            )
        })
        .collect();
    reverse.sort_unstable_by(|a, b| a.0.iter().rev().cmp(b.0.iter().rev()));
    let mut write = env.begin_write().unwrap();
    write
        .create_database_with_ordering(
            b"integer",
            DatabaseKind::Kv,
            builtin_ordering(OrderingSpec::UnsignedInteger {
                width: Width::U32,
                endian: Endian::Little,
            }),
        )
        .unwrap();
    write
        .create_database_with_ordering(
            b"reverse",
            DatabaseKind::Kv,
            builtin_ordering(OrderingSpec::ReverseBytes),
        )
        .unwrap();
    for (key, value) in integer.iter().rev() {
        write.put(b"integer", key, value).unwrap();
    }
    for (key, value) in reverse.iter().rev() {
        write.put(b"reverse", key, value).unwrap();
    }
    write.commit().unwrap();
    let read = env.begin_read();
    let identity = (read.sequence(), read.commit_identity());
    check_resident_builtin(&read, b"integer", &integer);
    check_resident_builtin(&read, b"reverse", &reverse);
    let native = read.kv(b"integer").unwrap();
    let ordered = read.prepare_lookup(b"integer", Options::Ordered).unwrap();
    assert_eq!(
        ordered.get(b"bad").unwrap_err().to_string(),
        native.get(b"bad").unwrap_err().to_string(),
    );
    assert_eq!((read.sequence(), read.commit_identity()), identity);
    drop(ordered);
    drop(native);
    drop(read);
    drop(env);
    let reopened = Environment::open(&dir.0, options).unwrap();
    let read = reopened.begin_read();
    check_resident_builtin(&read, b"integer", &integer);
    check_resident_builtin(&read, b"reverse", &reverse);
}

#[test]
fn resident_custom_hash_refuses_before_callback_and_ordered_retains_binding_requirement() {
    let dir = Dir::new();
    let env = Environment::create(&dir.0, resident_options()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let captured = calls.clone();
    let callbacks = DatabaseComparators {
        keys: Some(Arc::new(move |a: &[u8], b: &[u8]| {
            captured.fetch_add(1, Ordering::Relaxed);
            a.iter()
                .map(u8::to_ascii_lowercase)
                .cmp(b.iter().map(u8::to_ascii_lowercase))
        })),
        ..Default::default()
    };
    let mut write = env.begin_write().unwrap();
    write
        .create_database_with_ordering(
            b"custom",
            DatabaseKind::Kv,
            builtin_ordering(OrderingSpec::Custom),
        )
        .unwrap();
    for (key, value) in [
        (b"Alpha".as_slice(), b"old".as_slice()),
        (b"ALPHA", b"replacement"),
        (b"beta", b"second"),
        (b"Gamma", b"third"),
    ] {
        write
            .put_with_ordering(b"custom", key, value, &callbacks)
            .unwrap();
    }
    write.commit().unwrap();
    let read = env.begin_read();
    let before = calls.load(Ordering::Relaxed);
    assert!(matches!(
        read.prepare_lookup(b"custom", hash()),
        Err(Error::InvalidInput(
            "hash lookup requires bytewise ordering"
        ))
    ));
    assert_eq!(calls.load(Ordering::Relaxed), before);
    let native_error = read
        .kv(b"custom")
        .err()
        .expect("unbound native custom reader must refuse");
    let ordered_error = read
        .prepare_lookup(b"custom", Options::Ordered)
        .err()
        .expect("Ordered has no comparator callback parameter and must refuse");
    assert_eq!(ordered_error.to_string(), native_error.to_string());
    assert_eq!(calls.load(Ordering::Relaxed), before);
    // Existing explicitly bound custom access remains usable after both refusals.
    let bound = read.kv_with_ordering(b"custom", &callbacks).unwrap();
    assert_eq!(
        bound.get(b"aLpHa").unwrap(),
        Some(b"replacement".as_slice())
    );
    let expected = vec![
        (b"Alpha".to_vec(), b"replacement".to_vec()),
        (b"beta".to_vec(), b"second".to_vec()),
        (b"Gamma".to_vec(), b"third".to_vec()),
    ];
    assert_eq!(
        own(bound
            .cursor(Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()),
        expected
    );
    assert_eq!(
        own(bound
            .cursor(Bound::Included(b"ALPHA"), Bound::Excluded(b"GAMMA"), true)
            .unwrap()),
        vec![
            (b"beta".to_vec(), b"second".to_vec()),
            (b"Alpha".to_vec(), b"replacement".to_vec())
        ],
    );
}

#[cfg(feature = "range-lifecycle")]
#[test]
fn managed_range_hash_refusal_preserves_ordered_native_snapshot_and_ranges() {
    for mode in [
        ulmdb::PublicationMode::RangeV5,
        ulmdb::PublicationMode::Range,
    ] {
        let dir = Dir::new();
        let env = ulmdb::Environment::create_with_publication(&dir.0, mode).unwrap();
        assert!(env.is_range_backed());
        let mut write = env.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        for id in 0..128u64 {
            write
                .put(b"kv", &id.to_be_bytes(), &id.to_le_bytes())
                .unwrap();
        }
        write.commit().unwrap();
        let read = env.begin_read();
        let identity = (read.sequence(), read.commit_identity());
        assert!(matches!(
            read.prepare_lookup(b"kv", hash()),
            Err(Error::InvalidInput(
                "hash lookup requires immutable untransformed managed storage"
            ))
        ));
        let native = read.kv(b"kv").unwrap();
        let ordered = read.prepare_lookup(b"kv", Options::Ordered).unwrap();
        assert!(!ordered.stats().enumerated);
        assert_eq!(ordered.stats().index_bytes, 0);
        for id in 0..128u64 {
            let key = id.to_be_bytes();
            let value = id.to_le_bytes();
            assert_eq!(native.get(&key).unwrap(), Some(value.as_slice()));
            let actual = ordered.get(&key).unwrap().unwrap();
            assert_eq!(actual, value);
            assert_eq!(actual.as_ptr(), native.get(&key).unwrap().unwrap().as_ptr());
        }
        let lower = 17u64.to_be_bytes();
        let upper = 39u64.to_be_bytes();
        for reverse in [false, true] {
            let mut expected: Rows = (18u64..=39)
                .map(|id| (id.to_be_bytes().to_vec(), id.to_le_bytes().to_vec()))
                .collect();
            if reverse {
                expected.reverse();
            }
            assert_eq!(
                own(native
                    .cursor(Bound::Excluded(&lower), Bound::Included(&upper), reverse)
                    .unwrap()),
                expected
            );
            assert_eq!(
                own(ordered
                    .cursor(Bound::Excluded(&lower), Bound::Included(&upper), reverse)
                    .unwrap()),
                expected
            );
        }
        assert_eq!((read.sequence(), read.commit_identity()), identity);
    }
}
