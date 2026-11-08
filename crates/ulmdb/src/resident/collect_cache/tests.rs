use super::super::{Environment, Options, RedoEncoding, RedoFormat, Snapshot, audit as full};
use super::*;
use crate::{Address, Prefix};
use std::collections::{BTreeMap, BTreeSet};
use ulmdb_storage::{
    WriteStore,
    resident::{Arena, Config},
};
fn config() -> Config {
    Config {
        kv_pages: 128,
        tiny_pages: 128,
        max_reserved_bytes: 64 << 20,
        max_metadata_bytes: 64 << 20,
        max_views: 128,
    }
}
fn descriptor(
    a: &mut Arena,
    cat: Reference,
    name: &[u8],
    id: u64,
    kind: DatabaseKind,
    root: Reference,
) -> Reference {
    ulmdb_kv::put(
        a,
        cat,
        name,
        &catalog_codec::encode(&Database {
            ordering: crate::ordering::DatabaseOrdering::default(),
            id,
            kind,
            root,
        }),
    )
    .unwrap()
}
struct Fixture {
    pool: Arc<Pool>,
    view: View,
    cat: Reference,
    v4: Reference,
    v6: Reference,
}
fn fixture() -> Fixture {
    let pool = Pool::create(config()).unwrap();
    let mut a = pool.begin(pool.empty().unwrap()).unwrap();
    a.allocate(&[7; 4096], 4096).unwrap();
    let mut kv = Reference::NULL;
    for n in 0..256u32 {
        kv = ulmdb_kv::put(&mut a, kv, &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
    }
    kv = ulmdb_kv::put(&mut a, kv, &[8; 300], &[9; 700]).unwrap();
    let mut cat = descriptor(&mut a, Reference::NULL, b"kv", 1, DatabaseKind::Kv, kv);
    let mut roots = Vec::new();
    for (id, name, kind, width) in [
        (2, b"v4".as_slice(), DatabaseKind::Ipv4, 32),
        (3, b"v6".as_slice(), DatabaseKind::Ipv6, 128),
    ] {
        let mut root = Reference::NULL;
        for n in 0..48u128 {
            let address = if width == 32 {
                Address::V4((n as u32).wrapping_mul(0x04000001))
            } else {
                Address::V6(n << 120)
            };
            let value = if n % 5 == 0 {
                b"".as_slice()
            } else {
                b"network"
            };
            root = ulmdb_prefix::insert(&mut a, root, Prefix::new(address, width).unwrap(), value)
                .unwrap();
        }
        cat = descriptor(&mut a, cat, name, id, kind, root);
        roots.push(root);
    }
    cat = descriptor(&mut a, cat, b"alias", 4, DatabaseKind::Ipv4, roots[0]);
    Fixture {
        pool,
        view: a.freeze().unwrap().activate(),
        cat,
        v4: roots[0],
        v6: roots[1],
    }
}
fn same(actual: &full::Audit, expected: &full::Audit) {
    assert_eq!(actual.live, expected.live);
    assert_eq!(actual.kv_pages, expected.kv_pages);
    assert_eq!(actual.prefix_records, expected.prefix_records);
    let rows = |c: &BTreeMap<Vec<u8>, Database>| {
        c.iter()
            .map(|(n, d)| (n.clone(), catalog_codec::encode(d)))
            .collect::<Vec<_>>()
    };
    assert_eq!(rows(&actual.catalog), rows(&expected.catalog));
}
fn oracle(f: &Fixture) -> full::Audit {
    full::audit(&f.view, f.cat, 5, 2_000_000).unwrap()
}
#[test]
fn actual_closures_match_full_audit_shared_tiny_aliases_overflow_and_do_not_pin_versions() {
    let f = fixture();
    let before = f.pool.stats().unwrap();
    let cache = Cache::new(&f.pool, Limits::default()).unwrap();
    let expected = oracle(&f);
    let (first, cache) = cache.audit(&f.pool, &f.view, f.cat, 5, 2_000_000).unwrap();
    same(&first, &expected);
    assert_eq!(first.reads, expected.reads);
    assert_eq!(cache.stats.misses, 3);
    let (second, next) = cache.audit(&f.pool, &f.view, f.cat, 5, 2_000_000).unwrap();
    same(&second, &expected);
    assert_eq!(next.stats.hits, 3);
    assert_eq!(next.stats.misses, 0);
    assert!(second.reads < expected.reads);
    assert!(next.stats.membership_checks > 0);
    assert!(next.stats.endpoint_checks > 0);
    assert_eq!(before.view_roots, f.pool.stats().unwrap().view_roots);
    assert_eq!(before.held_maps, f.pool.stats().unwrap().held_maps);
    assert!(cache.charged <= cache.limits.phase());
    assert!(next.charged <= next.limits.phase());
    drop(f.view);
    assert_eq!(
        f.pool.stats().unwrap().held_maps,
        0,
        "cache must not retain any Version or View"
    );
    assert!(next.entries.iter().all(|e| !e.keys.is_empty()));
}
#[test]
fn changed_roots_ids_family_foreign_owner_and_stale_generations_miss_safely() {
    let f = fixture();
    let empty = Cache::new(&f.pool, Limits::default()).unwrap();
    let (_, cache) = empty.audit(&f.pool, &f.view, f.cat, 5, 2_000_000).unwrap();
    let mut a = f.pool.begin(f.view.clone()).unwrap();
    let changed = ulmdb_prefix::insert(
        &mut a,
        f.v4,
        Prefix::new(Address::V4(1), 32).unwrap(),
        b"new",
    )
    .unwrap();
    let cat = descriptor(&mut a, f.cat, b"v4", 2, DatabaseKind::Ipv4, changed);
    let view = a.freeze().unwrap().activate();
    let expected = full::audit(&view, cat, 5, 2_000_000).unwrap();
    let (actual, next) = cache.audit(&f.pool, &view, cat, 5, 2_000_000).unwrap();
    same(&actual, &expected);
    assert_eq!((next.stats.hits, next.stats.misses), (2, 1));
    let mut a = f.pool.begin(view.clone()).unwrap();
    let cat = descriptor(&mut a, cat, b"v4", 5, DatabaseKind::Ipv4, changed);
    let view = a.freeze().unwrap().activate();
    let expected = full::audit(&view, cat, 6, 2_000_000).unwrap();
    let (actual, next) = next.audit(&f.pool, &view, cat, 6, 2_000_000).unwrap();
    same(&actual, &expected);
    assert_eq!(next.stats.misses, 1);
    let mut a = f.pool.begin(view.clone()).unwrap();
    let wrong = descriptor(&mut a, cat, b"v4", 5, DatabaseKind::Ipv6, changed);
    let view_bad = a.freeze().unwrap().activate();
    let wanted = full::audit(&view_bad, wrong, 6, 2_000_000).err().unwrap();
    assert_eq!(
        format!("{wanted:?}"),
        format!(
            "{:?}",
            next.audit(&f.pool, &view_bad, wrong, 6, 2_000_000)
                .err()
                .unwrap()
        )
    );
    // An actually different physical owner with matching root/key integers must miss.
    let foreign = fixture();
    assert_eq!(foreign.v4, f.v4);
    assert_eq!(foreign.v6, f.v6);
    let (actual, next) = cache
        .audit(&foreign.pool, &foreign.view, foreign.cat, 5, 2_000_000)
        .unwrap();
    same(&actual, &oracle(&foreign));
    assert_eq!(next.stats.hits, 0);
    assert_eq!(next.stats.misses, 3);
    let mut stale = Cache::new(&f.pool, Limits::default()).unwrap();
    let (_, built) = stale.audit(&f.pool, &f.view, f.cat, 5, 2_000_000).unwrap();
    stale = built;
    for entry in &mut stale.entries {
        Arc::get_mut(entry).unwrap().keys[0].generation += 1;
    }
    let (actual, repaired) = stale.audit(&f.pool, &f.view, f.cat, 5, 2_000_000).unwrap();
    same(&actual, &oracle(&f));
    assert_eq!(repaired.stats.hits, 0);
}
#[test]
fn unchanged_root_empty_endpoint_neighbor_addition_and_removal_force_revalidation() {
    let pool = Pool::create(config()).unwrap();
    let mut a = pool.begin(pool.empty().unwrap()).unwrap();
    a.allocate(&[0; 4080], 16).unwrap();
    let payload = a.put_payload(b"").unwrap();
    let neighbor = a.allocate(&[0; 4000], 16).unwrap();
    a.allocate(&[0; 96], 16).unwrap();
    let mut node = [0; 96];
    node[..8].copy_from_slice(&[4, 0, 0, 0, 96, 0, 0, 0]);
    node[8] = 4;
    node[64..80].copy_from_slice(&payload.encode());
    let at = a.allocate(&node, 16).unwrap();
    let cat = descriptor(
        &mut a,
        Reference::NULL,
        b"endpoint",
        1,
        DatabaseKind::Ipv4,
        at,
    );
    let view = a.freeze().unwrap().activate();
    let neighbor_key = view.read_with_key(neighbor, 4000).unwrap().1;
    let minimal = view.without_versions(&[neighbor_key]).unwrap();
    let endpoint = Reference {
        offset: payload.offset + 16,
        ..payload
    };
    assert_ne!(
        view.read_with_key(endpoint, 0).unwrap().1,
        minimal.read_with_key(endpoint, 0).unwrap().1
    );
    let empty = Cache::new(&pool, Limits::default()).unwrap();
    let (_, cache) = empty.audit(&pool, &minimal, cat, 2, 100_000).unwrap();
    assert!(
        cache.entries[0]
            .keys
            .iter()
            .all(|k| view.contains_version(*k))
    );
    let expected = full::audit(&view, cat, 2, 100_000).unwrap();
    let (actual, added) = cache.audit(&pool, &view, cat, 2, 100_000).unwrap();
    same(&actual, &expected);
    assert_eq!((added.stats.hits, added.stats.misses), (0, 1));
    assert!(added.stats.endpoint_checks > 0);
    assert!(actual.live.contains(&neighbor_key));
    let (actual, removed) = added.audit(&pool, &minimal, cat, 2, 100_000).unwrap();
    same(&actual, &full::audit(&minimal, cat, 2, 100_000).unwrap());
    assert_eq!(removed.stats.hits, 0);
}
#[test]
fn validation_and_cache_capacity_limits_fall_back_without_weakening_graph_errors() {
    let f = fixture();
    let original = oracle(&f);
    let cache = Cache::new(
        &f.pool,
        Limits {
            max_entries: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    let (_, one) = cache.audit(&f.pool, &f.view, f.cat, 5, 2_000_000).unwrap();
    assert_eq!(one.entries.len(), 1);
    assert_eq!(one.stats.admission_skips, 2);
    let (actual, next) = one.audit(&f.pool, &f.view, f.cat, 5, 2_000_000).unwrap();
    same(&actual, &original);
    assert_eq!(next.stats.hits, 1);
    let cache = Cache::new(
        &f.pool,
        Limits {
            max_validations: 0,
            ..Limits::default()
        },
    )
    .unwrap();
    let (_, no_checks) = cache.audit(&f.pool, &f.view, f.cat, 5, 2_000_000).unwrap();
    let (actual, next) = no_checks
        .audit(&f.pool, &f.view, f.cat, 5, 2_000_000)
        .unwrap();
    same(&actual, &original);
    assert_eq!(actual.reads, original.reads);
    assert_eq!(next.stats.hits, 0);
    assert_eq!(next.stats.membership_checks, 0);
    let phase = size_of::<Cache>() + size_of::<Arc<Entry>>() + 64;
    let cache = Cache::new(
        &f.pool,
        Limits {
            max_bytes: phase * 3,
            max_entries: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    let (actual, tiny) = cache.audit(&f.pool, &f.view, f.cat, 5, 2_000_000).unwrap();
    same(&actual, &original);
    assert_eq!(tiny.entries.len(), 0);
    assert!(tiny.charged * 3 <= tiny.limits.max_bytes);
    assert!(matches!(
        no_checks.audit(&f.pool, &f.view, f.cat, 5, original.reads - 1),
        Err(Error::LimitExceeded)
    ));
    let mut a = f.pool.begin(f.view.clone()).unwrap();
    let stale = Reference {
        segment: 1,
        offset: (17 << 28) | 4096,
    };
    let bad = a.patch(f.v4, 96, 48, &stale.encode(), 16).unwrap();
    let cat = descriptor(&mut a, f.cat, b"v4", 2, DatabaseKind::Ipv4, bad);
    let view = a.freeze().unwrap().activate();
    let expected = full::audit(&view, cat, 5, 2_000_000).err().unwrap();
    assert_eq!(
        format!("{expected:?}"),
        format!(
            "{:?}",
            no_checks
                .audit(&f.pool, &view, cat, 5, 2_000_000)
                .err()
                .unwrap()
        )
    );
}
struct Path(std::path::PathBuf);
impl Drop for Path {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn options() -> Options {
    Options {
        pool: config(),
        wal_capacity: 16 << 20,
        graph_read_budget: 2_000_000,
        auto_collect_below: 0,
    }
}
#[test]
fn real_collect_matches_retired_sets_old_borrows_drop_recreate_and_full_cold_replay() {
    for base in [std::env::temp_dir(), std::path::PathBuf::from("/dev/shm")] {
        if !base.is_dir() {
            continue;
        }
        for (format, encoding) in [
            (RedoFormat::LiteralV1, RedoEncoding::Owned),
            (RedoFormat::FullPostimageV2, RedoEncoding::Owned),
            (RedoFormat::CompactV2, RedoEncoding::Borrowed),
        ] {
            let path = Path(base.join(format!(
                "ulmdb-collect-cache-{:032x}",
                ulmdb_storage::new_identity().unwrap()
            )));
            let env =
                Environment::create_with_encoding(&path.0, options(), format, encoding).unwrap();
            env.enable_collect_cache(Limits::default()).unwrap();
            let mut w = env.begin_write().unwrap();
            for (n, k) in [
                (b"kv".as_slice(), DatabaseKind::Kv),
                (b"v4", DatabaseKind::Ipv4),
                (b"v6", DatabaseKind::Ipv6),
            ] {
                w.create_database(n, k).unwrap();
            }
            w.put(b"kv", b"state", b"old").unwrap();
            for n in 0..32 {
                w.insert_prefix(
                    b"v4",
                    Prefix::new(Address::V4(n * 0x04000001), 32).unwrap(),
                    b"four",
                )
                .unwrap();
                w.insert_prefix(
                    b"v6",
                    Prefix::new(Address::V6((n as u128) << 120), 128).unwrap(),
                    b"six",
                )
                .unwrap();
            }
            w.commit().unwrap();
            let old = env.begin_read();
            let borrowed = old.get(b"kv", b"state").unwrap().unwrap();
            for round in 0..5u32 {
                let mut w = env.begin_write().unwrap();
                for n in 0..16u32 {
                    w.put(b"kv", &n.to_be_bytes(), &(round * 16 + n).to_be_bytes())
                        .unwrap();
                }
                w.commit().unwrap();
                let snapshot = env.current.load_full();
                let checked = full::audit(
                    &snapshot.image,
                    snapshot.root,
                    snapshot.next_database,
                    2_000_000,
                )
                .unwrap();
                let retired = snapshot
                    .image
                    .members()
                    .filter(|k| !checked.live.contains(k))
                    .count();
                drop(snapshot);
                let report = env.collect().unwrap();
                assert_eq!(report.retired_versions, retired);
                assert_eq!(report.prefix_records, checked.prefix_records);
                assert_eq!(
                    env.current.load().image.members().collect::<BTreeSet<_>>(),
                    checked.live
                );
                if round != 0 {
                    assert_eq!(env.collect_cache_stats().unwrap().unwrap().hits, 2);
                    assert!(report.graph_reads < checked.reads);
                }
            }
            let mut w = env.begin_write().unwrap();
            w.insert_prefix(b"v4", Prefix::new(Address::V4(7), 32).unwrap(), b"changed")
                .unwrap();
            w.commit().unwrap();
            env.collect().unwrap();
            assert_eq!(env.collect_cache_stats().unwrap().unwrap().misses, 1);
            let mut w = env.begin_write().unwrap();
            w.drop_database(b"v6").unwrap();
            w.create_database(b"v6", DatabaseKind::Ipv4).unwrap();
            w.commit().unwrap();
            env.collect().unwrap();
            assert_eq!(env.collect_cache_stats().unwrap().unwrap().misses, 1);
            assert_eq!(borrowed, b"old");
            let seq = env.sequence();
            drop(env);
            let reopened =
                Environment::open_with_encoding(&path.0, options(), format, encoding).unwrap();
            assert_eq!(reopened.sequence(), seq);
            assert!(reopened.collect_cache_stats().unwrap().is_none());
            assert_eq!(borrowed, b"old");
            assert_eq!(reopened.begin_read().database_count(), 3);
        }
    }
}
#[test]
fn finite_immutable_closure_model_includes_endpoint_dependency_counterexample() {
    // Edges only name immutable versions. Presence changes may add an adjacent
    // empty-read owner while the complete original object remains unchanged.
    fn closure(edges: &[u8; 4], roots: u8, present: u8, neighbor: bool) -> Option<u8> {
        let mut live = roots;
        loop {
            let mut next = live;
            for (n, edge) in edges.iter().enumerate() {
                if live & (1 << n) != 0 {
                    next |= *edge;
                }
            }
            if next == live {
                break;
            }
            live = next;
        }
        live |= 1; // The empty payload header always belongs to the original allocation.
        if live & !present != 0 {
            return None;
        }
        Some(live | if neighbor { 16 } else { 1 })
    }
    let mut cases = 0;
    let mut counterexample = false;
    for encoding in 0..64u32 {
        let edges = [
            0,
            (encoding & 1) as u8,
            ((encoding >> 1) & 3) as u8,
            ((encoding >> 3) & 7) as u8,
        ];
        for roots in 1..16u8 {
            for old_neighbor in [false, true] {
                let old = closure(&edges, roots, 15, old_neighbor).unwrap();
                for present in 0..32u8 {
                    let neighbor = present & 16 != 0;
                    let remaining = old & !present == 0;
                    let hit = remaining && neighbor == old_neighbor;
                    if hit {
                        assert_eq!(closure(&edges, roots, present, neighbor), Some(old));
                    }
                    if remaining
                        && neighbor != old_neighbor
                        && closure(&edges, roots, present, neighbor) != Some(old)
                    {
                        counterexample = true;
                    }
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 61440);
    assert!(
        counterexample,
        "membership alone must have a counterexample"
    );
}

#[test]
fn corrupt_collect_poison_discards_cache_before_any_retirement() {
    let path = Path(std::env::temp_dir().join(format!(
        "ulmdb-cache-poison-{:032x}",
        ulmdb_storage::new_identity().unwrap()
    )));
    let env = Environment::create_with_encoding(
        &path.0,
        options(),
        RedoFormat::CompactV2,
        RedoEncoding::Borrowed,
    )
    .unwrap();
    env.enable_collect_cache(Limits::default()).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
    w.insert_prefix(b"v4", Prefix::new(Address::V4(0), 0).unwrap(), b"safe")
        .unwrap();
    w.commit().unwrap();
    env.collect().unwrap();
    let old = env.current.load_full();
    let held = env.begin_read();
    let mut a = env.pool.begin(old.image.clone()).unwrap();
    let db = old.catalog.get(b"v4").unwrap();
    let bad_root = descriptor(&mut a, old.root, b"v4", db.id, DatabaseKind::Ipv6, db.root);
    let image = a.freeze().unwrap().activate();
    // Synthetic invalid current materialization, not a claimed valid Journal
    // transition: only exercise corruption-before-retirement handling.
    env.current.store(Arc::new(Snapshot {
        image,
        catalog: old.catalog.clone(),
        root: bad_root,
        next_database: old.next_database,
        receipt: old.receipt,
        commit: old.commit,
    }));
    assert!(matches!(env.collect(), Err(Error::Corrupt(_))));
    assert!(env.pool_stats().unwrap().poisoned);
    assert!(env.collect_cache_stats().unwrap().is_none());
    assert!(matches!(env.collect(), Err(Error::OutcomeUnknown)));
    assert_eq!(
        held.longest_match(b"v4", Address::V4(1))
            .unwrap()
            .unwrap()
            .1,
        b"safe"
    );
}

#[cfg(all(feature = "fault-injection", target_os = "linux"))]
#[test]
fn actual_cache_hit_collect_known_and_unknown_errors_preserve_committed_readers() {
    const WORKER: &str = "ULMDB_COLLECT_CACHE_WORKER";
    if let Some(path) = std::env::var_os(WORKER) {
        let env = Environment::open_with_encoding(
            path,
            options(),
            RedoFormat::CompactV2,
            RedoEncoding::Borrowed,
        )
        .unwrap();
        env.enable_collect_cache(Limits::default()).unwrap();
        // Test-only warmup through the actual complete auditor, without a
        // maintenance write that would consume the injected commit point.
        {
            let snapshot = env.current.load_full();
            let mut w = env.writer.lock().unwrap();
            let (_, next) = w
                .prefix_audit_cache
                .as_ref()
                .unwrap()
                .audit(
                    &env.pool,
                    &snapshot.image,
                    snapshot.root,
                    snapshot.next_database,
                    2_000_000,
                )
                .unwrap();
            w.prefix_audit_cache = Some(next);
        }
        let old = env.begin_read();
        let value = old.get(b"kv", b"state").unwrap().unwrap();
        assert_eq!(value, b"new");
        let point = std::env::var("ULMDB_RESIDENT_FAIL_AT").unwrap();
        let unknown = point == "resident2.commit.before_witness"
            || point == "resident2.commit.after_witness_sync";
        let result = env.collect();
        if unknown {
            assert!(matches!(result, Err(Error::OutcomeUnknown)));
            assert!(env.collect_cache_stats().unwrap().is_none());
            assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
        } else {
            assert!(matches!(result, Err(Error::Io(_))));
            assert!(env.collect_cache_stats().unwrap().is_some());
            assert!(!env.pool_stats().unwrap().poisoned);
        }
        assert_eq!(value, b"new");
        println!("CACHE_COLLECT_CHILD_PASS");
        return;
    }
    for base in [std::env::temp_dir(), std::path::PathBuf::from("/dev/shm")] {
        if !base.is_dir() {
            continue;
        }
        for point in [
            "resident2.commit.before_append",
            "resident2.commit.before_data_sync",
            "resident2.commit.before_witness",
            "resident2.commit.after_witness_sync",
        ] {
            let path = Path(base.join(format!(
                "ulmdb-cache-fault-{:032x}",
                ulmdb_storage::new_identity().unwrap()
            )));
            let env = Environment::create_with_encoding(
                &path.0,
                options(),
                RedoFormat::CompactV2,
                RedoEncoding::Borrowed,
            )
            .unwrap();
            let mut w = env.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            w.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
            w.put(b"kv", b"state", b"old").unwrap();
            w.insert_prefix(b"v4", Prefix::new(Address::V4(0), 0).unwrap(), b"ip")
                .unwrap();
            w.commit().unwrap();
            let held = env.begin_read();
            let escaped = held.get(b"kv", b"state").unwrap().unwrap();
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"state", b"new").unwrap();
            w.commit().unwrap();
            drop(env);
            let out=std::process::Command::new("timeout").args(["--signal=KILL","30s"]).arg(std::env::current_exe().unwrap()).args(["--exact","resident::collect_cache::tests::actual_cache_hit_collect_known_and_unknown_errors_preserve_committed_readers","--nocapture"]).env(WORKER,&path.0).env("ULMDB_RESIDENT_FAIL_AT",point).output().unwrap();
            assert!(
                out.status.success(),
                "{point}: {} {}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(String::from_utf8_lossy(&out.stdout).contains("CACHE_COLLECT_CHILD_PASS"));
            let reopened = Environment::open_with_encoding(
                &path.0,
                options(),
                RedoFormat::CompactV2,
                RedoEncoding::Borrowed,
            )
            .unwrap();
            assert_eq!(
                reopened.begin_read().get(b"kv", b"state").unwrap(),
                Some(b"new".as_slice())
            );
            assert_eq!(escaped, b"old");
        }
    }
}
