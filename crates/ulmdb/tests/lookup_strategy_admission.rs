mod lookup_strategy_support;
use lookup_strategy_support::*;
use std::{ops::Bound, sync::Arc};
use ulmdb::{
    DatabaseKind, Environment, Error, PublicationMode,
    lookup::Options,
    ordering::{DatabaseOrdering, OrderingSpec},
};
use ulmdb_storage::transform::{Checksum, ChecksumConfig, TransformContext};

#[test]
fn hash_refuses_non_bytewise_and_non_kv_sources_while_ordered_keeps_native_rules() {
    let d = Dir::new();
    let env = Environment::create(&d.0).unwrap();
    let mut w = env.begin_write().unwrap();
    for order in [
        OrderingSpec::ReverseBytes,
        OrderingSpec::UnsignedInteger {
            width: ulmdb::ordering::Width::U32,
            endian: ulmdb::ordering::Endian::Little,
        },
        OrderingSpec::Custom,
    ] {
        let name = format!("order-{order:?}").into_bytes();
        w.create_database_ordered(
            &name,
            DatabaseKind::Kv,
            DatabaseOrdering {
                keys: order,
                duplicates: OrderingSpec::Bytewise,
            },
        )
        .unwrap();
    }
    w.create_database(b"reverse", DatabaseKind::Kv).unwrap();
    w.set_database_ordering(
        b"reverse",
        DatabaseOrdering {
            keys: OrderingSpec::ReverseBytes,
            duplicates: OrderingSpec::Bytewise,
        },
    )
    .unwrap();
    for key in [b"az".as_slice(), b"ba", b"a"] {
        w.put(b"reverse", key, key).unwrap();
    }
    w.create_database(b"dups", DatabaseKind::Duplicates { width: None })
        .unwrap();
    w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
    w.commit().unwrap();
    let r = env.begin_read();
    for order in [
        OrderingSpec::ReverseBytes,
        OrderingSpec::UnsignedInteger {
            width: ulmdb::ordering::Width::U32,
            endian: ulmdb::ordering::Endian::Little,
        },
        OrderingSpec::Custom,
    ] {
        let name = format!("order-{order:?}").into_bytes();
        assert!(matches!(
            r.prepare_lookup(&name, hash()),
            Err(Error::InvalidInput(
                "hash lookup requires bytewise ordering"
            ))
        ));
    }
    for name in [b"dups".as_slice(), b"ip"] {
        assert!(matches!(
            r.prepare_lookup(name, hash()),
            Err(Error::WrongKind)
        ));
    }
    assert!(matches!(
        r.prepare_lookup(b"missing", hash()),
        Err(Error::MissingDatabase)
    ));
    let ordered = r.prepare_lookup(b"reverse", Options::Ordered).unwrap();
    assert_eq!(
        ordered
            .cursor(Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .collect::<ulmdb::Result<Vec<_>>>()
            .unwrap(),
        r.kv(b"reverse")
            .unwrap()
            .cursor(Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .collect::<ulmdb::Result<Vec<_>>>()
            .unwrap()
    );
    let rd = Dir::new();
    let resident = ulmdb::resident::Environment::create(&rd.0, resident_options()).unwrap();
    let mut w = resident.begin_write().unwrap();
    w.create_database(b"dups", DatabaseKind::Duplicates { width: None })
        .unwrap();
    w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
    w.commit().unwrap();
    let rr = resident.begin_read();
    for name in [b"dups".as_slice(), b"ip"] {
        assert!(matches!(
            rr.prepare_lookup(name, hash()),
            Err(Error::WrongKind)
        ));
    }
    assert!(matches!(
        rr.prepare_lookup(b"missing", hash()),
        Err(Error::MissingDatabase)
    ));
}

struct TestSum;
impl Checksum for TestSum {
    fn compute(&self, source: &[u8], dest: &mut [u8], _: Option<&[u8]>) -> ulmdb::Result<()> {
        dest.fill(source.iter().fold(0u8, |sum, v| sum.wrapping_add(*v)));
        Ok(())
    }
}
fn context() -> Arc<TransformContext> {
    Arc::new(
        TransformContext::new(
            Some(ChecksumConfig {
                callback: Arc::new(TestSum),
                bytes: 32,
            }),
            None,
            64 << 20,
        )
        .unwrap(),
    )
}

#[test]
fn hash_rejects_transforms_for_both_backends_and_ordered_stays_usable() {
    let d = Dir::new();
    let env = Environment::create_with_transform(&d.0, PublicationMode::Legacy, context()).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"value").unwrap();
    w.commit().unwrap();
    let r = env.begin_read();
    assert!(matches!(
        r.prepare_lookup(b"kv", hash()),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        r.prepare_lookup(b"kv", Options::Ordered)
            .unwrap()
            .get(b"key")
            .unwrap(),
        Some(b"value".as_slice())
    );
    let d = Dir::new();
    let env =
        ulmdb::resident::Environment::create_with_transform(&d.0, resident_options(), context())
            .unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"value").unwrap();
    w.commit().unwrap();
    let r = env.begin_read();
    assert!(matches!(
        r.prepare_lookup(b"kv", hash()),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        r.prepare_lookup(b"kv", Options::Ordered)
            .unwrap()
            .get(b"key")
            .unwrap(),
        Some(b"value".as_slice())
    );
}

#[test]
fn hash_refuses_caller_managed_external_snapshot() {
    let d = Dir::new();
    let env = Environment::create(&d.0).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"value").unwrap();
    w.commit().unwrap();
    drop(env);
    let r = ulmdb::ReadTxn::open_externally_synchronized(&d.0).unwrap();
    assert!(matches!(
        r.prepare_lookup(b"kv", hash()),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        r.prepare_lookup(b"kv", Options::Ordered)
            .unwrap()
            .get(b"key")
            .unwrap(),
        Some(b"value".as_slice())
    );
}
