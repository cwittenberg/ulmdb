use super::super::{Environment, Options, RedoEncoding, RedoFormat, audit};
use super::*;
use crate::{Address, Prefix};
use ulmdb_core::storage::WriteStore;
use ulmdb_storage::resident::{Config, Pool};
#[test]
fn persistent_path_map_matches_btree_and_preserves_old_roots() {
    let mut m = Map::default();
    let mut oracle = std::collections::BTreeMap::new();
    let mut x = 73u64;
    for n in 0..4000u32 {
        let old = m.clone();
        let old_values = old.entries();
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
        let k = (x >> 32) % 513;
        if n % 3 == 0 {
            m.remove(&k);
            oracle.remove(&k);
        } else {
            m.insert(k, n);
            oracle.insert(k, n);
        }
        assert_eq!(
            m.entries(),
            oracle.iter().map(|(a, b)| (*a, *b)).collect::<Vec<_>>()
        );
        assert_eq!(old.entries(), old_values);
        assert_eq!(m.len(), oracle.len());
    }
}
fn limits() -> Limits {
    Limits::default()
}
fn opts() -> Options {
    Options {
        pool: Config {
            kv_pages: 128,
            tiny_pages: 128,
            max_reserved_bytes: 64 << 20,
            max_metadata_bytes: 64 << 20,
            max_views: 128,
        },
        wal_capacity: 16 << 20,
        graph_read_budget: 2_000_000,
        auto_collect_below: 0,
    }
}
struct Dir(std::path::PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn directory() -> Dir {
    Dir(std::env::temp_dir().join(format!(
        "w2-unit-{:032x}",
        ulmdb_storage::new_identity().unwrap()
    )))
}
#[test]
fn actual_candidate_updates_match_full_audit_without_expanding_unchanged_indexes() {
    let d = directory();
    let e = Environment::create_with_encoding(
        &d.0,
        opts(),
        RedoFormat::CompactV2,
        RedoEncoding::Borrowed,
    )
    .unwrap();
    let mut w = e.begin_write().unwrap();
    for (name, kind) in [
        (b"kv".as_slice(), DatabaseKind::Kv),
        (b"v4", DatabaseKind::Ipv4),
        (b"v6", DatabaseKind::Ipv6),
    ] {
        w.create_database(name, kind).unwrap();
    }
    for n in 0..512u32 {
        w.put(b"kv", &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
    }
    for n in 0..40u32 {
        w.insert_prefix(b"v4", Prefix::new(Address::V4(n << 24), 8).unwrap(), b"v4")
            .unwrap();
        w.insert_prefix(
            b"v6",
            Prefix::new(Address::V6((n as u128) << 120), 8).unwrap(),
            b"v6",
        )
        .unwrap();
    }
    w.commit().unwrap();
    e.enable_incremental(limits()).unwrap();
    let old = e.begin_read();
    for n in 0..48u32 {
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", &(n % 512).to_be_bytes(), &n.to_be_bytes())
            .unwrap();
        w.commit().unwrap();
        let s = e.current.load_full();
        let checked = audit::audit(&s.image, s.root, s.next_database, 2_000_000).unwrap();
        let guard = e.writer.lock().unwrap();
        let c = guard.incremental.as_ref().unwrap();
        assert!(c.matches(&s));
        assert_eq!(c.live_versions(), checked.live);
        let m = guard.incremental_metrics;
        assert!(
            m.expanded_objects < 30,
            "only changed KV/catalog pages and their new objects expand: {m:?}"
        );
    }
    assert_eq!(
        old.get(b"kv", &0u32.to_be_bytes()).unwrap().unwrap(),
        0u32.to_le_bytes()
    );
    e.collect().unwrap();
    let s = e.current.load_full();
    assert!(
        e.writer
            .lock()
            .unwrap()
            .incremental
            .as_ref()
            .unwrap()
            .matches(&s)
    );
}
#[test]
fn source_identity_budget_failure_and_dropped_pending_preserve_certificate() {
    let pool = Pool::create(opts().pool).unwrap();
    let view = pool.empty().unwrap();
    let receipt =
        ulmdb_storage::resident::wal_codec::genesis(&ulmdb_storage::resident::wal_codec::Boot {
            uuid: [1; 16],
            epoch: 1,
            commit: 7,
            kv_pages: 128,
            tiny_pages: 128,
        })
        .unwrap();
    let base = Snapshot {
        image: view.clone(),
        catalog: Default::default(),
        root: Reference::NULL,
        next_database: 1,
        receipt,
        commit: 7,
    };
    let c = Certificate::bootstrap(&base, limits()).unwrap();
    let mut a = pool.begin(view).unwrap();
    a.allocate(&[0; 4096], 4096).unwrap();
    let candidate = a.freeze().unwrap();
    let pending = c
        .prepare(&base, &candidate, &[], Reference::NULL, 1, 64)
        .unwrap();
    let mut header = ulmdb_storage::resident::wal_codec_v2::Record {
        uuid: [1; 16],
        epoch: 1,
        lsn: 1,
        sequence: 1,
        commit: 8,
        parent_commit: 7,
        parent: base.receipt,
        root: Reference::NULL,
        next_database: 1,
        kind: ulmdb_storage::resident::wal_codec_v2::Kind::Write,
        allocations: Vec::new(),
        retired: pending.retired().to_vec(),
    };
    pending.validate_record(&header).unwrap();
    header.next_database = 2;
    assert!(pending.validate_record(&header).is_err());
    header.next_database = 1;
    header.commit = 9;
    assert!(pending.validate_record(&header).is_err());
    header.commit = 8;
    header.retired.push(VersionKey {
        slot: 99,
        generation: 1,
    });
    assert!(pending.validate_record(&header).is_err());
    let future = WalReceipt {
        lsn: 1,
        sequence: 1,
        offset: base.receipt.end().unwrap(),
        length: 256,
        digest: [3; 32],
    };
    assert!(pending.bind(future, 9).is_err());
    assert!(c.matches(&base));
    assert!(c.live_versions().is_empty());
    let other = Pool::create(opts().pool).unwrap();
    let foreign = other
        .begin(other.empty().unwrap())
        .unwrap()
        .freeze()
        .unwrap();
    assert!(
        c.prepare(&base, &foreign, &[], Reference::NULL, 1, 64)
            .is_err()
    );
    let mut tiny = c.clone();
    tiny.limits.max_metadata_bytes = 0;
    assert!(
        tiny.prepare(&base, &candidate, &[], Reference::NULL, 1, 64)
            .is_err()
    );
    assert!(c.matches(&base));
}
fn synthetic_snapshot(
    pool: &Arc<Pool>,
    image: View,
    root: Reference,
    catalog: std::collections::BTreeMap<Vec<u8>, crate::catalog::Database>,
    next_database: u64,
) -> Snapshot {
    let (kv, tiny) = pool.geometry();
    let receipt =
        ulmdb_storage::resident::wal_codec::genesis(&ulmdb_storage::resident::wal_codec::Boot {
            uuid: [9; 16],
            epoch: 1,
            commit: 7,
            kv_pages: kv as u32,
            tiny_pages: tiny as u32,
        })
        .unwrap();
    Snapshot {
        image,
        catalog: super::super::catalog::Catalog::from_map(catalog).unwrap(),
        root,
        next_database,
        receipt,
        commit: 7,
    }
}
#[test]
fn shared_overflow_descriptor_names_reject_and_catalog_root_alias_has_no_ownership_cycle() {
    let pool = Pool::create(opts().pool).unwrap();
    let empty = pool.empty().unwrap();
    let base = synthetic_snapshot(&pool, empty.clone(), Reference::NULL, Default::default(), 1);
    let c = Certificate::bootstrap(&base, limits()).unwrap();
    let mut a = pool.begin(empty).unwrap();
    let desc = crate::catalog::Database {
        ordering: crate::ordering::DatabaseOrdering::default(),
        id: 1,
        kind: DatabaseKind::Kv,
        root: Reference::NULL,
    };
    let value = super::super::catalog_codec::encode(&desc);
    let payload = a.put_payload(&value).unwrap();
    let mut root = ulmdb_kv::put(&mut a, Reference::NULL, b"a", &[0; 700]).unwrap();
    root = ulmdb_kv::put(&mut a, root, b"b", &[0; 700]).unwrap();
    let mut bytes = a.read(root, 4096).unwrap().to_vec();
    for i in 0..2 {
        let at = u16::from_le_bytes(bytes[32 + i * 2..34 + i * 2].try_into().unwrap()) as usize;
        assert_eq!(
            u16::from_le_bytes(bytes[at + 2..at + 4].try_into().unwrap()) & 2,
            2
        );
        bytes[at + 4..at + 8].copy_from_slice(&48u32.to_le_bytes());
        bytes[at + 9..at + 25].copy_from_slice(&payload.encode());
    }
    root = a.rewrite(root, &bytes, 4096).unwrap();
    let candidate = a.freeze().unwrap();
    assert!(audit::audit(candidate.view(), root, 2, 2_000_000).is_err());
    let change = RootChange {
        before: None,
        after: Some(RootSpec {
            value_limit: 4080,
            name: b"a".to_vec(),
            id: 1,
            kind: DatabaseKind::Kv,
            ordering: Default::default(),
            root: Reference::NULL,
        }),
    };
    assert!(
        c.prepare(&base, &candidate, &[change], root, 2, 64)
            .is_err()
    );
    let mut a = pool.begin(base.image.clone()).unwrap();
    let mut root = ulmdb_kv::put(&mut a, Reference::NULL, b"self", &value).unwrap();
    let alias = crate::catalog::Database { root, ..desc };
    let previous = root;
    root = ulmdb_kv::put(&mut a, root, b"self", &catalog_codec::encode(&alias)).unwrap();
    assert_eq!(root, previous);
    let image = a.freeze().unwrap().activate();
    let catalog = std::collections::BTreeMap::from([(b"self".to_vec(), alias)]);
    let aliased = synthetic_snapshot(&pool, image, root, catalog, 2);
    let checked = audit::audit(&aliased.image, root, 2, 2_000_000).unwrap();
    let cert = Certificate::bootstrap(&aliased, limits()).unwrap();
    assert_eq!(cert.live_versions(), checked.live);
}
#[test]
fn neighbor_subscription_revalidates_present_then_reused_generation_without_rescanning_objects() {
    use ulmdb_storage::resident::wal_codec::{Kind, LiteralSlot, Record};
    let cfg = Config {
        kv_pages: 16,
        tiny_pages: 8,
        max_reserved_bytes: 8 << 20,
        max_metadata_bytes: 16 << 20,
        max_views: 64,
    };
    let pool = Pool::create(cfg).unwrap();
    let at = |slot: u64, offset: u64| Reference {
        segment: 1,
        offset: (1 << 28) | ((slot + 1) * 4096 + offset),
    };
    let payload = at(16, 4080);
    let node = at(18, 0);
    let db = crate::catalog::Database {
        ordering: crate::ordering::DatabaseOrdering::default(),
        id: 1,
        kind: DatabaseKind::Ipv4,
        root: node,
    };
    // Actual KV encoder emits the catalogue page; only materialization geometry is synthetic.
    let aux = Pool::create(cfg).unwrap();
    let mut a = aux.begin(aux.empty().unwrap()).unwrap();
    let root = ulmdb_kv::put(&mut a, Reference::NULL, b"v4", &catalog_codec::encode(&db)).unwrap();
    let prepared = a.freeze().unwrap();
    let cat = prepared
        .slots()
        .find(|s| s.key.slot == 0)
        .unwrap()
        .bytes
        .to_vec();
    let mut body = vec![0; 4096];
    body[4080..4088].copy_from_slice(&[1, 0, 0, 0, 16, 0, 0, 0]);
    let mut node_bytes = vec![0; 4096];
    node_bytes[..8].copy_from_slice(&[4, 0, 0, 0, 96, 0, 0, 0]);
    node_bytes[8] = 4;
    node_bytes[64..80].copy_from_slice(&payload.encode());
    let receipt =
        ulmdb_storage::resident::wal_codec::genesis(&ulmdb_storage::resident::wal_codec::Boot {
            uuid: [9; 16],
            epoch: 1,
            commit: 7,
            kv_pages: 16,
            tiny_pages: 8,
        })
        .unwrap();
    let record = Record {
        uuid: [9; 16],
        epoch: 1,
        lsn: 1,
        sequence: 1,
        commit: 8,
        parent_commit: 7,
        parent: receipt,
        root,
        next_database: 2,
        kind: Kind::Write,
        retired: Vec::new(),
        allocations: vec![
            LiteralSlot {
                key: VersionKey {
                    slot: 0,
                    generation: 1,
                },
                expected_previous_generation: 0,
                ranges: vec![(0, 4096)],
                bytes: cat,
            },
            LiteralSlot {
                key: VersionKey {
                    slot: 16,
                    generation: 1,
                },
                expected_previous_generation: 0,
                ranges: vec![(4080, 16)],
                bytes: body,
            },
            LiteralSlot {
                key: VersionKey {
                    slot: 18,
                    generation: 1,
                },
                expected_previous_generation: 0,
                ranges: vec![(0, 96)],
                bytes: node_bytes,
            },
        ],
    };
    let image = pool
        .empty()
        .unwrap()
        .replay_literals(&record)
        .unwrap()
        .activate();
    let base = synthetic_snapshot(
        &pool,
        image,
        root,
        std::collections::BTreeMap::from([(b"v4".to_vec(), db)]),
        2,
    );
    let cert = Certificate::bootstrap(&base, limits()).unwrap();
    let endpoint = Reference {
        offset: payload.offset + 16,
        ..payload
    };
    assert_eq!(base.image.read_with_key(endpoint, 0).unwrap().1.slot, 16);
    for generation in [1, 2] {
        let mut a = pool.begin(base.image.clone()).unwrap();
        a.allocate(&[7], 1).unwrap();
        let candidate = a.freeze().unwrap();
        let slot = candidate.slots().next().unwrap();
        assert_eq!(
            slot.key,
            VersionKey {
                slot: 17,
                generation
            }
        );
        let pending = cert.prepare(&base, &candidate, &[], root, 2, 64).unwrap();
        let checked = audit::audit(pending.image(), root, 2, 2_000_000).unwrap();
        assert_eq!(pending.certificate.live_versions(), checked.live);
        assert_eq!(pending.metrics.expanded_objects, 0);
        assert!(pending.metrics.endpoint_revalidations > 0);
        assert_eq!(
            pending.image().read_with_key(endpoint, 0).unwrap().1.slot,
            if generation == 1 { 17 } else { 16 }
        );
        drop(pending);
        drop(candidate);
        assert!(cert.matches(&base));
        assert_eq!(base.image.read_with_key(endpoint, 0).unwrap().1.slot, 16);
    }
}
