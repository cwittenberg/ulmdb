use std::{collections::BTreeMap, ops::Bound, path::PathBuf};
use ulmdb::{Address, DatabaseKind, Environment, Error, PageSize, Prefix, PublicationMode};

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-batch-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn p(v6: bool, n: u32, length: u8) -> Prefix {
    Prefix::normalize(
        if v6 {
            Address::V6(u128::from(n) << 64)
        } else {
            Address::V4(n)
        },
        length,
    )
    .unwrap()
}

#[test]
fn existing_indexes_match_puts_preserve_snapshots_and_reopen_across_publication_and_page_sizes() {
    for (mode, size) in [
        (PublicationMode::Legacy, 4096),
        (PublicationMode::Cohort, 4096),
        (PublicationMode::Indexed, 512),
        (PublicationMode::Indexed, 1024),
        (PublicationMode::Range, 4096),
        (PublicationMode::Indexed, 65536),
    ] {
        let dir = Dir::new();
        let env =
            Environment::create_with_page_size(&dir.0, mode, PageSize::new(size).unwrap()).unwrap();
        let mut writer = env.begin_write().unwrap();
        writer.create_database(b"kv", DatabaseKind::Kv).unwrap();
        for (name, kind) in [
            (b"v4".as_slice(), DatabaseKind::Ipv4),
            (b"v6", DatabaseKind::Ipv6),
        ] {
            writer.create_database(name, kind).unwrap();
            let v6 = name == b"v6";
            writer
                .load_sorted_prefixes(
                    name,
                    (0..1000u32)
                        .map(|n| Ok((p(v6, n * 2, if v6 { 128 } else { 32 }), n.to_be_bytes()))),
                )
                .unwrap();
            writer.insert_prefix(name, p(v6, 0, 0), b"default").unwrap();
        }
        writer
            .load_sorted_kv(
                b"kv",
                (0..1000u32).map(|n| Ok(((n * 2).to_be_bytes(), n.to_be_bytes()))),
            )
            .unwrap();
        writer.commit().unwrap();
        let old = env.begin_read();
        let borrowed = old.get(b"kv", &14u32.to_be_bytes()).unwrap().unwrap();
        let mut writer = env.begin_write().unwrap();
        writer
            .upsert_sorted_kv(
                b"kv",
                (0..2400u32)
                    .filter(|n| n % 3 != 0)
                    .map(|n| Ok((n.to_be_bytes(), (n + 11).to_be_bytes()))),
            )
            .unwrap();
        for name in [b"v4".as_slice(), b"v6"] {
            let v6 = name == b"v6";
            // Ancestors sharing the same network sort before their descendants.
            let mut input = BTreeMap::new();
            input.insert(p(v6, 0, 0), b"new default".to_vec());
            input.insert(p(v6, 0, if v6 { 64 } else { 24 }), b"ancestor".to_vec());
            for n in (0..2400u32).filter(|n| n % 3 != 0) {
                input.insert(
                    p(v6, n, if v6 { 128 } else { 32 }),
                    (n + 11).to_be_bytes().to_vec(),
                );
            }
            writer
                .upsert_sorted_prefixes(name, input.into_iter().map(Ok))
                .unwrap();
        }
        writer.commit().unwrap();
        let current = env.begin_read();
        for n in 0..2400u32 {
            let expected = if n % 3 != 0 {
                Some((n + 11).to_be_bytes())
            } else if n < 2000 && n % 2 == 0 {
                Some((n / 2).to_be_bytes())
            } else {
                None
            };
            assert_eq!(
                current.get(b"kv", &n.to_be_bytes()).unwrap(),
                expected.as_ref().map(|v| v.as_slice())
            );
            for name in [b"v4".as_slice(), b"v6"] {
                assert_eq!(
                    current
                        .exact_prefix(
                            name,
                            p(name == b"v6", n, if name == b"v6" { 128 } else { 32 })
                        )
                        .unwrap()
                        .map(|(_, v)| v),
                    expected.as_ref().map(|v| v.as_slice())
                );
            }
        }
        assert_eq!(borrowed, 7u32.to_be_bytes());
        assert!(old.get(b"kv", &15u32.to_be_bytes()).unwrap().is_none());
        ulmdb::backup::verify(&env).unwrap();
        drop(current);
        drop(env);
        let env = Environment::open(&dir.0).unwrap();
        let current = env.begin_read();
        assert_eq!(
            current.get(b"kv", &14u32.to_be_bytes()).unwrap(),
            Some(25u32.to_be_bytes().as_slice())
        );
        for name in [b"v4".as_slice(), b"v6"] {
            assert_eq!(
                current
                    .exact_prefix(name, p(name == b"v6", 0, 0))
                    .unwrap()
                    .unwrap()
                    .1,
                b"new default"
            );
        }
        assert_eq!(borrowed, 7u32.to_be_bytes());
    }
}

#[test]
fn errors_rollback_and_empty_or_unchanged_batches_preserve_commit_identity() {
    let dir = Dir::new();
    let env = Environment::create(&dir.0).unwrap();
    let mut writer = env.begin_write().unwrap();
    writer.create_database(b"kv", DatabaseKind::Kv).unwrap();
    writer.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
    writer
        .upsert_sorted_kv(
            b"kv",
            (0..600u32).map(|n| Ok((n.to_be_bytes(), n.to_be_bytes()))),
        )
        .unwrap();
    writer
        .upsert_sorted_prefixes(
            b"v4",
            (0..600u32).map(|n| Ok((p(false, n, 32), n.to_be_bytes()))),
        )
        .unwrap();
    let sequence = writer.commit().unwrap();
    let mut writer = env.begin_write().unwrap();
    assert_eq!(
        writer
            .upsert_sorted_kv(b"kv", std::iter::empty::<ulmdb::Result<(&[u8], &[u8])>>())
            .unwrap(),
        0
    );
    writer
        .upsert_sorted_kv(
            b"kv",
            (0..600u32).map(|n| Ok((n.to_be_bytes(), n.to_be_bytes()))),
        )
        .unwrap();
    writer
        .upsert_sorted_prefixes(
            b"v4",
            (0..600u32).map(|n| Ok((p(false, n, 32), n.to_be_bytes()))),
        )
        .unwrap();
    assert_eq!(writer.commit().unwrap(), sequence);
    let mut writer = env.begin_write().unwrap();
    assert!(matches!(
        writer.upsert_sorted_kv(b"v4", [Ok((b"a", b"b"))]),
        Err(Error::WrongKind)
    ));
    assert!(matches!(
        writer.upsert_sorted_kv(b"missing", [Ok((b"a", b"b"))]),
        Err(Error::MissingDatabase)
    ));
    let before = writer.dirty_bytes();
    {
        let mut savepoint = writer.savepoint().unwrap();
        let input = (0..600u32)
            .map(|n| Ok((n.to_be_bytes(), b"bad")))
            .chain(std::iter::once(Err(Error::InvalidInput("source failed"))));
        assert!(savepoint.upsert_sorted_kv(b"kv", input).is_err());
        assert!(savepoint.put(b"kv", b"x", b"hidden").is_err());
        savepoint.rollback().unwrap();
        assert_eq!(savepoint.dirty_bytes(), before);
        let input = (0..256u32)
            .map(|n| Ok((p(false, n, 32), b"bad")))
            .chain(std::iter::once(Ok((p(false, 255, 32), b"bad"))));
        assert!(savepoint.upsert_sorted_prefixes(b"v4", input).is_err());
        savepoint.rollback().unwrap();
        assert_eq!(savepoint.dirty_bytes(), before);
        assert!(
            savepoint
                .upsert_sorted_prefixes(b"v4", [Ok((p(true, 0, 0), b"bad"))])
                .is_err()
        );
        savepoint.rollback().unwrap();
        savepoint.release().unwrap();
    }
    assert_eq!(
        writer.get(b"kv", &7u32.to_be_bytes()).unwrap(),
        Some(7u32.to_be_bytes().as_slice())
    );
    assert_eq!(writer.commit().unwrap(), sequence);
}

#[test]
fn custom_ordering_keeps_the_existing_representative_and_registration_is_required() {
    use ulmdb::ordering::{DatabaseComparators, DatabaseOrdering, OrderingSpec};
    let dir = Dir::new();
    let env = Environment::create(&dir.0).unwrap();
    let order = DatabaseComparators {
        keys: Some(std::sync::Arc::new(|a, b| {
            a.iter()
                .map(u8::to_ascii_lowercase)
                .cmp(b.iter().map(u8::to_ascii_lowercase))
        })),
        duplicates: None,
    };
    let mut writer = env.begin_write().unwrap();
    writer
        .create_database_ordered(
            b"kv",
            DatabaseKind::Kv,
            DatabaseOrdering {
                keys: OrderingSpec::Custom,
                ..Default::default()
            },
        )
        .unwrap();
    writer
        .upsert_sorted_kv_with_ordering(
            b"kv",
            (0..600).map(|n| Ok((format!("K{n:04}"), b"old"))),
            &order,
        )
        .unwrap();
    writer.commit().unwrap();
    let mut writer = env.begin_write().unwrap();
    assert!(
        writer
            .upsert_sorted_kv(b"kv", [Ok((b"k0001", b"bad"))])
            .is_err()
    );
    writer
        .upsert_sorted_kv_with_ordering(
            b"kv",
            (0..900).map(|n| Ok((format!("k{n:04}"), b"new"))),
            &order,
        )
        .unwrap();
    writer.commit().unwrap();
    let read = env.begin_read();
    let actual = read
        .kv_with_ordering(b"kv", &order)
        .unwrap()
        .cursor(Bound::Unbounded, Bound::Unbounded, false)
        .unwrap()
        .map(|p| p.unwrap().0.to_vec())
        .collect::<Vec<_>>();
    assert_eq!(actual[7], b"K0007");
    assert_eq!(actual[700], b"k0700");
}
