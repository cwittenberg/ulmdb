mod lookup_strategy_support;
use lookup_strategy_support::*;
use std::ops::Bound;
use ulmdb::{
    DatabaseKind, Error,
    lookup::{Backend, HashLimits, Options, hash_index_bytes},
    resident::{Environment, RedoEncoding, RedoFormat},
};

#[test]
fn resident_bytes_pointer_identity_key_limit_and_ordered_fallback_match_native() {
    let d = Dir::new();
    let env = Environment::create_with_encoding(
        &d.0,
        resident_options(),
        RedoFormat::CompactV2,
        RedoEncoding::Borrowed,
    )
    .unwrap();
    let rows = fixtures();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    for (k, v) in &rows {
        w.put(b"kv", k, v).unwrap();
    }
    w.commit().unwrap();
    let read = env.begin_read();
    let native = read.kv(b"kv").unwrap();
    for options in [Options::Ordered, hash()] {
        let lookup = read.prepare_lookup(b"kv", options).unwrap();
        assert_eq!(lookup.identity().backend, Backend::Resident);
        assert_eq!(lookup.identity().commit, read.commit_identity());
        for (k, v) in &rows {
            let actual = lookup.get(k).unwrap().unwrap();
            assert_eq!(actual, v);
            assert_eq!(actual.as_ptr(), native.get(k).unwrap().unwrap().as_ptr());
        }
        assert_eq!(lookup.get(b"absent").unwrap(), None);
        let bad = vec![0; 4081];
        assert_eq!(error_key(lookup.get(&bad)), error_key(native.get(&bad)));
        for reverse in [false, true] {
            let lower = Bound::Included(b"key-00000019".as_slice());
            let upper = Bound::Excluded(b"key-00000100".as_slice());
            assert_eq!(
                lookup
                    .cursor(lower, upper, reverse)
                    .unwrap()
                    .collect::<ulmdb::Result<Vec<_>>>()
                    .unwrap(),
                native
                    .cursor(lower, upper, reverse)
                    .unwrap()
                    .collect::<ulmdb::Result<Vec<_>>>()
                    .unwrap()
            );
            assert_eq!(
                lookup
                    .cursor(Bound::Included(bad.as_slice()), Bound::Unbounded, reverse)
                    .err()
                    .unwrap()
                    .to_string(),
                native
                    .cursor(Bound::Included(bad.as_slice()), Bound::Unbounded, reverse)
                    .err()
                    .unwrap()
                    .to_string()
            );
        }
    }
}

#[test]
fn resident_index_adds_no_snapshot_owner_and_survives_collection_checkpoint_reopen() {
    let d = Dir::new();
    let options = resident_options();
    let env = Environment::create_with_encoding(
        &d.0,
        options,
        RedoFormat::CompactV2,
        RedoEncoding::Borrowed,
    )
    .unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"old").unwrap();
    w.commit().unwrap();
    let old = env.begin_read();
    let roots = env.pool_stats().unwrap().view_roots;
    let lookup = old.prepare_lookup(b"kv", hash()).unwrap();
    assert_eq!(env.pool_stats().unwrap().view_roots, roots);
    let identity = lookup.identity();
    for i in 0..20u64 {
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", &i.to_le_bytes()).unwrap();
        w.commit().unwrap();
    }
    env.collect().unwrap();
    env.checkpoint().unwrap();
    assert_eq!(lookup.identity(), identity);
    assert_eq!(lookup.get(b"key").unwrap(), Some(b"old".as_slice()));
    let held_before = env.pool_stats().unwrap().held_maps;
    drop(lookup);
    assert_eq!(
        env.pool_stats().unwrap().held_maps,
        held_before,
        "dropping lookup does not release its caller-owned txn"
    );
    drop(old);
    assert!(
        env.pool_stats().unwrap().held_maps < held_before,
        "last caller read releases old pages"
    );
    let read = env.begin_read();
    let current = read.prepare_lookup(b"kv", hash()).unwrap();
    drop(env);
    let reopened = Environment::open_with_encoding(
        &d.0,
        options,
        RedoFormat::CompactV2,
        RedoEncoding::Borrowed,
    )
    .unwrap();
    let rr = reopened.begin_read();
    let lookup = rr.prepare_lookup(b"kv", hash()).unwrap();
    assert_eq!(lookup.identity().commit, current.identity().commit);
    assert_eq!(
        lookup.get(b"key").unwrap(),
        Some(19u64.to_le_bytes().as_slice())
    );
    drop(reopened);
    assert_eq!(
        current.get(b"key").unwrap(),
        Some(19u64.to_le_bytes().as_slice())
    );
}

#[test]
fn resident_limits_reject_complete_index_and_old_lookup_does_not_rebuild() {
    let d = Dir::new();
    let env = Environment::create(&d.0, resident_options()).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"a", b"one").unwrap();
    w.put(b"kv", b"b", b"two").unwrap();
    w.commit().unwrap();
    let r = env.begin_read();
    let exact = HashLimits {
        max_entries: 2,
        max_index_bytes: hash_index_bytes(2).unwrap(),
        max_value_bytes: 3,
        max_total_value_bytes: 6,
    };
    for bad in [
        HashLimits {
            max_entries: 1,
            ..exact
        },
        HashLimits {
            max_index_bytes: exact.max_index_bytes - 1,
            ..exact
        },
        HashLimits {
            max_value_bytes: 2,
            ..exact
        },
        HashLimits {
            max_total_value_bytes: 5,
            ..exact
        },
    ] {
        assert!(matches!(
            r.prepare_lookup(b"kv", Options::Hash(bad)),
            Err(Error::LimitExceeded)
        ));
    }
    let lookup = r.prepare_lookup(b"kv", Options::Hash(exact)).unwrap();
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"c", b"three").unwrap();
    w.commit().unwrap();
    assert_eq!(lookup.stats().entries, 2);
    assert_eq!(lookup.get(b"c").unwrap(), None);
    let new = env.begin_read();
    assert!(matches!(
        new.prepare_lookup(b"kv", Options::Hash(exact)),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(new.get(b"kv", b"c").unwrap(), Some(b"three".as_slice()));
}
