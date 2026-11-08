mod lookup_strategy_support;

use lookup_strategy_support::{Dir, hash, resident_options};
use std::ops::Bound;
use ulmdb::{
    DatabaseKind,
    lookup::{Cursor, Options},
};

type Pair<'a> = ulmdb::Result<(&'a [u8], &'a [u8])>;

fn checksum((count, sum): (usize, u64), pair: Pair<'_>) -> (usize, u64) {
    let (key, value) = pair.unwrap();
    let digest = key
        .iter()
        .chain(value)
        .fold(0u64, |n, b| n.rotate_left(5).wrapping_add(u64::from(*b)));
    (count + 1, sum.wrapping_add(digest))
}

fn partial_and_fold_match_native<'a>(
    mut cursor: Cursor<'a>,
    mut native: impl Iterator<Item = Pair<'a>>,
) {
    assert_eq!(cursor.size_hint(), native.size_hint());
    for _ in 0..3 {
        assert_eq!(
            cursor.next().transpose().unwrap(),
            native.next().transpose().unwrap()
        );
    }
    assert_eq!(cursor.size_hint(), native.size_hint());
    assert_eq!(
        cursor.by_ref().take(7).fold((0, 0), checksum),
        native.by_ref().take(7).fold((0, 0), checksum)
    );
    assert_eq!(cursor.size_hint(), native.size_hint());
    // Move the remaining cursor into its explicit fold implementation.
    assert_eq!(cursor.fold((0, 0), checksum), native.fold((0, 0), checksum));
}

fn take_and_exhaustion_match_native<'a>(
    mut cursor: Cursor<'a>,
    mut native: impl Iterator<Item = Pair<'a>>,
) {
    for limit in [0, 1, 4] {
        assert_eq!(
            cursor.by_ref().take(limit).fold((0, 0), checksum),
            native.by_ref().take(limit).fold((0, 0), checksum)
        );
    }
    loop {
        let expected = native.next().transpose().unwrap();
        assert_eq!(cursor.next().transpose().unwrap(), expected);
        if expected.is_none() {
            break;
        }
    }
    for _ in 0..3 {
        assert!(cursor.next().is_none());
        assert!(native.next().is_none());
        assert_eq!(cursor.size_hint(), native.size_hint());
    }
}

#[test]
fn file_cursor_forwarding_preserves_partial_fold_take_bounds_and_exhaustion() {
    let dir = Dir::new();
    let env = ulmdb::Environment::create(&dir.0).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    for id in 0..32u8 {
        write
            .put(b"kv", format!("key-{id:03}").as_bytes(), &[id])
            .unwrap();
    }
    write.commit().unwrap();
    let read = env.begin_read();
    let native = read.kv(b"kv").unwrap();
    for options in [Options::Ordered, hash()] {
        let prepared = read.prepare_lookup(b"kv", options).unwrap();
        for reverse in [false, true] {
            for (lower, upper) in [
                (Bound::Unbounded, Bound::Unbounded),
                (
                    Bound::Included(b"key-005".as_slice()),
                    Bound::Excluded(b"key-024".as_slice()),
                ),
                (Bound::Excluded(b"key-099".as_slice()), Bound::Unbounded),
            ] {
                partial_and_fold_match_native(
                    prepared.cursor(lower, upper, reverse).unwrap(),
                    native.cursor(lower, upper, reverse).unwrap(),
                );
                take_and_exhaustion_match_native(
                    prepared.cursor(lower, upper, reverse).unwrap(),
                    native.cursor(lower, upper, reverse).unwrap(),
                );
            }
        }
    }
}

#[test]
fn resident_cursor_forwarding_preserves_partial_fold_take_bounds_and_exhaustion() {
    let dir = Dir::new();
    let env = ulmdb::resident::Environment::create(&dir.0, resident_options()).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    for id in 0..32u8 {
        write
            .put(b"kv", format!("key-{id:03}").as_bytes(), &[id])
            .unwrap();
    }
    write.commit().unwrap();
    let read = env.begin_read();
    let native = read.kv(b"kv").unwrap();
    for options in [Options::Ordered, hash()] {
        let prepared = read.prepare_lookup(b"kv", options).unwrap();
        for reverse in [false, true] {
            for (lower, upper) in [
                (Bound::Unbounded, Bound::Unbounded),
                (
                    Bound::Included(b"key-005".as_slice()),
                    Bound::Excluded(b"key-024".as_slice()),
                ),
                (Bound::Excluded(b"key-099".as_slice()), Bound::Unbounded),
            ] {
                partial_and_fold_match_native(
                    prepared.cursor(lower, upper, reverse).unwrap(),
                    native.cursor(lower, upper, reverse).unwrap(),
                );
                take_and_exhaustion_match_native(
                    prepared.cursor(lower, upper, reverse).unwrap(),
                    native.cursor(lower, upper, reverse).unwrap(),
                );
            }
        }
    }
}
