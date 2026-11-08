//! Real million-row regression for the former 4096-slot Resident ceiling.
//! Run in release mode with the Resident benchmark feature set. This exercises
//! actual KV pages, transaction publication, retained snapshots and cold replay;
//! it does not substitute synthetic directory entries for an Environment.
#![cfg(feature = "resident-compact-pages")]

use std::{ops::Bound, path::PathBuf};
use ulmdb::{
    DatabaseKind,
    resident::{Environment, Options, ReadTxn, RedoEncoding, RedoFormat},
};

const KEYS: usize = 1_000_000;
const BATCH: usize = 1000;
const PAGES: usize = 8192;
const FORMER_CEILING: usize = 4096;

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-resident-capacity-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn options() -> Options {
    Options {
        pool: ulmdb_storage::resident::Config {
            kv_pages: PAGES,
            tiny_pages: 0,
            max_reserved_bytes: 64 << 20,
            max_metadata_bytes: 64 << 20,
            max_views: 16,
        },
        // Capacity and recovery work are explicit. Neither WAL truncation nor a
        // reduced recovery audit is used to make the regression fit.
        wal_capacity: 256 << 20,
        graph_read_budget: 1_000_000_000,
        // Preserve the quarter-pool collection headroom used by the smaller
        // Resident workload. A fixed 256-page threshold is too late for a
        // dispersed 1000-row update after loading this many pages.
        auto_collect_below: PAGES / 4,
    }
}

fn mix(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn key(index: usize) -> [u8; 8] {
    (2 * index as u64).to_be_bytes()
}

fn shuffled_batch() -> Vec<usize> {
    let mut order: Vec<_> = (0..KEYS).collect();
    let mut state = 73u64;
    for index in (1..KEYS).rev() {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let other = (mix(state) % (index as u64 + 1)) as usize;
        order.swap(index, other);
    }
    order.truncate(BATCH);
    order
}

fn audit(read: &ReadTxn, expected: &[u64]) {
    let view = read.kv(b"kv").unwrap();
    let mut cursor = view
        .cursor(Bound::Unbounded, Bound::Unbounded, false)
        .unwrap();
    for (index, value) in expected.iter().enumerate() {
        let key = key(index);
        let value = value.to_be_bytes();
        assert_eq!(
            view.get(&key).unwrap(),
            Some(value.as_slice()),
            "point value at row {index}"
        );
        assert_eq!(
            cursor.next().transpose().unwrap(),
            Some((key.as_slice(), value.as_slice())),
            "ordered cursor at row {index}"
        );
    }
    assert!(cursor.next().transpose().unwrap().is_none());
    // Odd keys never existed, including across leaf and high-slot boundaries.
    for index in (0..KEYS).step_by(7919) {
        assert_eq!(
            view.get(&(2 * index as u64 + 1).to_be_bytes()).unwrap(),
            None
        );
    }
    assert_eq!(view.get(&key(KEYS)).unwrap(), None);
}

#[test]
fn million_rows_cross_old_slot_ceiling_keep_old_borrows_and_replay() {
    let dir = Directory::new();
    let options = options();
    let env = Environment::create_with_encoding(
        &dir.0,
        options,
        RedoFormat::CompactV2,
        RedoEncoding::Borrowed,
    )
    .unwrap();
    let mut expected: Vec<_> = (0..KEYS).map(|index| mix(index as u64 ^ 73)).collect();
    {
        let mut write = env.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        write.commit().unwrap();
    }
    #[cfg(feature = "resident-prefix-audit-cache")]
    env.enable_collect_cache(ulmdb::resident::CollectCacheLimits::default())
        .unwrap();
    for first in (0..KEYS).step_by(BATCH) {
        let mut write = env.begin_write().unwrap();
        for (index, value) in expected.iter().enumerate().skip(first).take(BATCH) {
            write.put(b"kv", &key(index), &value.to_be_bytes()).unwrap();
        }
        write.commit().unwrap();
    }

    // Pin the committed million-row snapshot immediately after normal loading,
    // without a manual sweep hiding a broken automatic-collection threshold.
    // The first update must coexist with its old pages until publication.
    let initial = env.pool_stats().unwrap();
    assert_eq!(initial.maps_created, PAGES);
    assert_eq!(initial.held_maps + initial.free_pages, PAGES);
    assert!(initial.held_maps > 3000);
    assert!(initial.free_pages >= 2 * BATCH, "{initial:?}");
    assert!(!initial.poisoned);
    assert!(
        env.physical_lsn() > env.sequence(),
        "normal loading must exercise automatic Collect before the first update"
    );
    let old = env.begin_read();
    let original = expected.clone();
    let batch = shuffled_batch();
    let held_index = batch[0];
    let held = old.get(b"kv", &key(held_index)).unwrap().unwrap();
    let held_address = held.as_ptr();
    let held_value = original[held_index].to_be_bytes();
    let old_sequence = old.sequence();
    {
        let mut write = env.begin_write().unwrap();
        for (step, &index) in batch.iter().enumerate() {
            expected[index] ^= step as u64 + 1;
            write
                .put(b"kv", &key(index), &expected[index].to_be_bytes())
                .unwrap();
        }
        assert_eq!(write.commit().unwrap(), old_sequence + 1);
    }
    let updated = env.pool_stats().unwrap();
    // More than 4096 simultaneously held physical maps necessarily exercise
    // slot coordinates beyond the former fixed directory, regardless of reuse.
    assert!(updated.held_maps > FORMER_CEILING, "{updated:?}");
    assert_eq!(old.sequence(), old_sequence);
    assert_eq!(held, held_value);
    assert_eq!(held.as_ptr(), held_address);
    audit(&old, &original);
    audit(&env.begin_read(), &expected);

    let selected = (env.sequence(), env.physical_lsn(), env.wal_bytes());
    {
        let mut aborted = env.begin_write().unwrap();
        for &index in &batch {
            aborted.put(b"kv", &key(index), b"aborted!").unwrap();
        }
        // Dropping the transaction must discard all its high-slot versions.
    }
    assert_eq!(
        (env.sequence(), env.physical_lsn(), env.wal_bytes()),
        selected
    );
    let collection = env.collect().unwrap();
    assert!(collection.retired_versions > 0);
    assert_eq!(collection.sequence, selected.0);
    assert_eq!(held, held_value);
    audit(&env.begin_read(), &expected);

    let selected_sequence = env.sequence();
    let selected_lsn = env.physical_lsn();
    let incarnation = env.pool_stats().unwrap().physical_incarnation;
    drop(env);
    let reopened = Environment::open_with_encoding(
        &dir.0,
        options,
        RedoFormat::CompactV2,
        RedoEncoding::Borrowed,
    )
    .unwrap();
    assert_eq!(reopened.options().pool.kv_pages, PAGES);
    assert_eq!(reopened.options().pool.tiny_pages, 0);
    assert_eq!(reopened.pool_stats().unwrap().maps_created, PAGES);
    assert_ne!(
        reopened.pool_stats().unwrap().physical_incarnation,
        incarnation
    );
    assert_eq!(
        (reopened.sequence(), reopened.physical_lsn()),
        (selected_sequence, selected_lsn)
    );
    audit(&reopened.begin_read(), &expected);
    audit(&old, &original);
    assert_eq!(held, held_value);
    assert_eq!(held.as_ptr(), held_address);
    drop(old);
    let collected = reopened.collect().unwrap();
    assert_eq!(collected.sequence, selected_sequence);
    assert!(!reopened.pool_stats().unwrap().poisoned);
}
