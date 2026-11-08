use super::*;
use crate::ModelPrefix;
use ulmdb_core::Address;

#[test]
fn named_databases_commit_as_one_state_with_old_snapshots() {
    let mut env = Environment::new(7);
    let old = env.snapshot();
    let mut w = env.begin_write().unwrap();
    let names = w.create(b"names", Kind::Kv).unwrap();
    let reverse = w
        .create(b"reverse", Kind::Duplicates { width: Some(8) })
        .unwrap();
    let routes = w.create(b"routes", Kind::Ipv6).unwrap();
    w.private.put(names, b"example", b"::1").unwrap();
    w.private
        .insert_pair(reverse, b"::1", &17u64.to_be_bytes())
        .unwrap();
    w.private
        .insert_prefix(
            routes,
            ModelPrefix::normalize(Address::V6(1), 128).unwrap(),
            b"loopback",
        )
        .unwrap();
    assert_eq!(w.commit(), Ok(1));
    assert_eq!(old.1.open(b"names", Kind::Kv), Err(Error::DatabaseNotFound));
    let (seq, new) = env.snapshot();
    assert_eq!(seq, 1);
    assert_eq!(
        new.kv(names).unwrap().get(b"example".as_slice()).unwrap(),
        b"::1"
    );
    assert_eq!(new.pairs(reverse).unwrap().len(), 1);
    assert_eq!(
        new.prefixes(routes)
            .unwrap()
            .longest_match(Address::V6(1))
            .unwrap()
            .1,
        b"loopback"
    );
}

#[test]
fn handles_never_bind_to_recreated_or_aborted_objects() {
    let mut env = Environment::new(10);
    let mut w = env.begin_write().unwrap();
    let h = w.create(b"x", Kind::Kv).unwrap();
    w.abort();
    let mut w = env.begin_write().unwrap();
    let first = w.create(b"x", Kind::Kv).unwrap();
    assert_ne!(h, first);
    assert_eq!(w.private.kv(h), Err(Error::StaleHandle));
    w.commit().unwrap();
    let old = env.snapshot().1;
    let mut w = env.begin_write().unwrap();
    w.private.drop_database(first).unwrap();
    let second = w.create(b"x", Kind::Kv).unwrap();
    assert_ne!(first, second);
    assert_eq!(w.private.kv(first), Err(Error::StaleHandle));
    w.commit().unwrap();
    assert!(old.kv(first).is_ok());
    assert_eq!(env.snapshot().1.kv(first), Err(Error::StaleHandle));
    let other = Environment::new(11);
    assert_eq!(other.snapshot().1.kv(second), Err(Error::StaleHandle));
}

#[test]
fn savepoint_rollback_restores_catalog_data_and_invalidates_descendants() {
    let mut env = Environment::new(12);
    let mut w = env.begin_write().unwrap();
    let h = w.create(b"x", Kind::Kv).unwrap();
    w.private.put(h, b"a", b"old").unwrap();
    let a = w.savepoint().unwrap();
    w.private.put(h, b"a", b"new").unwrap();
    let b = w.savepoint().unwrap();
    let gone = w.create(b"later", Kind::Ipv4).unwrap();
    w.rollback(a).unwrap();
    assert_eq!(w.rollback(b), Err(Error::InvalidSavepoint));
    assert_eq!(w.private.prefixes(gone), Err(Error::StaleHandle));
    assert_eq!(w.private.kv(h).unwrap()[b"a".as_slice()], b"old");
    let fresh = w.create(b"later", Kind::Ipv4).unwrap();
    assert_ne!(gone, fresh);
    w.commit().unwrap();
    let mut w = env.begin_write().unwrap();
    assert_eq!(w.rollback(a), Err(Error::InvalidSavepoint));
}

#[test]
fn duplicates_and_types_enforce_limits_without_overwriting_evidence() {
    let mut env = Environment::new(13);
    let mut w = env.begin_write().unwrap();
    let h = w.create(b"d", Kind::Duplicates { width: Some(2) }).unwrap();
    assert!(w.private.insert_pair(h, b"k", b"ab").unwrap());
    assert!(!w.private.insert_pair(h, b"k", b"ab").unwrap());
    assert_eq!(
        w.private.insert_pair(h, b"k", b"x"),
        Err(Error::WidthMismatch)
    );
    assert_eq!(w.private.put(h, b"k", b"v"), Err(Error::WrongKind));
    assert_eq!(
        w.private.insert_pair(h, &vec![0; 4097], b"ab"),
        Err(Error::KeyTooLarge)
    );
    assert_eq!(
        w.private.insert_pair(h, b"k", &vec![0; 65_537]),
        Err(Error::DuplicateTooLarge)
    );
    assert_eq!(w.private.delete_pairs(h, b"k"), Ok(1));
    assert_eq!(w.private.delete_pair(h, b"k", b"ab"), Ok(false));
    let p = w.create(b"p", Kind::Ipv4).unwrap();
    assert_eq!(
        w.private.insert_prefix(
            p,
            ModelPrefix::normalize(Address::V6(0), 0).unwrap(),
            b"wrong"
        ),
        Err(Error::WrongFamily)
    );
    assert_eq!(w.private.open(b"p", Kind::Ipv6), Err(Error::WrongKind));
    assert_eq!(w.create(b"", Kind::Kv), Err(Error::InvalidName));
    assert_eq!(w.create(b"p", Kind::Kv), Err(Error::DatabaseExists));
}

#[test]
fn exhausted_commit_sequence_leaves_current_state_unchanged() {
    let mut env = Environment::new(15);
    env.sequence = u64::MAX;
    let before = env.snapshot();
    let mut w = env.begin_write().unwrap();
    w.create(b"never", Kind::Kv).unwrap();
    assert_eq!(w.commit(), Err(Error::LimitExceeded));
    assert_eq!(env.snapshot(), before);
}
