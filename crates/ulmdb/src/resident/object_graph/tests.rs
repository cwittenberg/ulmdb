use super::*;
use crate::{Address, Prefix, catalog::Database};
use ulmdb_core::{object_graph::ObjectKind, storage::WriteStore};
use ulmdb_storage::resident::{Arena, Config, Pool};
fn arena() -> Arena {
    let pool = Pool::create(Config {
        kv_pages: 128,
        tiny_pages: 256,
        max_reserved_bytes: 64 << 20,
        max_metadata_bytes: 64 << 20,
        max_views: 64,
    })
    .unwrap();
    pool.begin(pool.empty().unwrap()).unwrap()
}
fn limits() -> Limits {
    Limits {
        reads: 2_000_000,
        objects: 10_000,
        edges: 20_000,
    }
}
fn receipt() -> WalReceipt {
    ulmdb_storage::resident::wal_codec::genesis(&ulmdb_storage::resident::wal_codec::Boot {
        uuid: [7; 16],
        epoch: 1,
        commit: 3,
        kv_pages: 128,
        tiny_pages: 256,
    })
    .unwrap()
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
#[test]
fn actual_kv_overflows_first_children_both_families_and_shared_tiny_match_full_audit() {
    let mut a = arena();
    let mut kv = Reference::NULL;
    for n in 0..512u32 {
        kv = ulmdb_kv::put(&mut a, kv, &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
    }
    kv = ulmdb_kv::put(&mut a, kv, &[0x70; 300], &[0x31; 700]).unwrap();
    let mut cat = descriptor(&mut a, Reference::NULL, b"kv", 1, DatabaseKind::Kv, kv);
    // Aliased root remains TWO root multiset entries, not two physical objects.
    cat = descriptor(&mut a, cat, b"alias", 2, DatabaseKind::Kv, kv);
    for (id, name, kind, left, right) in [
        (
            3,
            b"v4".as_slice(),
            DatabaseKind::Ipv4,
            Address::V4(0),
            Address::V4(u32::MAX),
        ),
        (
            4,
            b"v6".as_slice(),
            DatabaseKind::Ipv6,
            Address::V6(0),
            Address::V6(u128::MAX),
        ),
    ] {
        let width = if id == 3 { 32 } else { 128 };
        let mut root = ulmdb_prefix::insert(
            &mut a,
            Reference::NULL,
            Prefix::new(left, width).unwrap(),
            b"",
        )
        .unwrap();
        root = ulmdb_prefix::insert(&mut a, root, Prefix::new(right, width).unwrap(), b"right")
            .unwrap();
        cat = descriptor(&mut a, cat, name, id, kind, root);
    }
    cat = descriptor(&mut a, cat, b"empty", 5, DatabaseKind::Kv, Reference::NULL);
    let view = a.freeze().unwrap().activate();
    let graph = bootstrap(&view, cat, 6, receipt(), 3, limits()).unwrap();
    let original = audit::audit(&view, cat, 6, limits().reads).unwrap();
    assert_eq!(graph.live, original.live);
    assert_eq!(graph.roots.len(), 6);
    assert_eq!(graph.roots.iter().filter(|r| r.reference == kv).count(), 2);
    for want in [
        EdgeKind::KvChild { ordinal: 0 },
        EdgeKind::RadixChild { side: 0 },
        EdgeKind::RadixChild { side: 1 },
    ] {
        assert!(graph.edges.iter().any(|e| e.kind == want));
    }
    assert!(
        graph
            .edges
            .iter()
            .any(|e| matches!(e.kind, EdgeKind::OverflowKey { .. }))
    );
    assert!(
        graph
            .edges
            .iter()
            .any(|e| matches!(e.kind, EdgeKind::OverflowValue { .. }))
    );
    assert_eq!(
        graph
            .objects
            .keys()
            .filter(|o| matches!(o.kind, ObjectKind::RadixNode { .. }))
            .count(),
        6
    );
    assert!(
        graph
            .objects
            .keys()
            .any(|o| o.kind == ObjectKind::Payload && o.length == 16)
    );
    let mut by_slot = BTreeMap::new();
    for b in graph.objects.values() {
        *by_slot.entry(b.allocation).or_insert(0) += 1;
    }
    assert!(
        by_slot.values().any(|&count| count > 1),
        "Tiny objects retain individual extents"
    );
    let total = graph.baseline_reads + graph.typed_reads;
    assert!(matches!(
        bootstrap(
            &view,
            cat,
            6,
            receipt(),
            3,
            Limits {
                reads: total - 1,
                ..limits()
            }
        ),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(
        bootstrap(
            &view,
            cat,
            6,
            receipt(),
            3,
            Limits {
                reads: total,
                ..limits()
            }
        )
        .unwrap()
        .typed_reads,
        graph.typed_reads
    );
    assert!(matches!(
        bootstrap(
            &view,
            cat,
            6,
            receipt(),
            3,
            Limits {
                objects: graph.objects.len() - 1,
                ..limits()
            }
        ),
        Err(Error::LimitExceeded)
    ));
    assert!(matches!(
        bootstrap(
            &view,
            cat,
            6,
            receipt(),
            3,
            Limits {
                edges: graph.edges.len() - 1,
                ..limits()
            }
        ),
        Err(Error::LimitExceeded)
    ));
}
#[test]
fn empty_payload_endpoint_preserves_current_first_actual_returned_version_dependency() {
    let mut a = arena();
    a.allocate(&[0; 4080], 16).unwrap();
    let payload = a.put_payload(b"").unwrap();
    let mut bytes = [0; 96];
    bytes[..8].copy_from_slice(&[4, 0, 0, 0, 96, 0, 0, 0]);
    bytes[8] = 4;
    bytes[64..80].copy_from_slice(&payload.encode());
    let node = a.allocate(&bytes, 16).unwrap();
    let cat = descriptor(
        &mut a,
        Reference::NULL,
        b"endpoint",
        1,
        DatabaseKind::Ipv4,
        node,
    );
    let view = a.freeze().unwrap().activate();
    let graph = bootstrap(&view, cat, 2, receipt(), 3, limits()).unwrap();
    let payload_id = ObjectId {
        reference: payload,
        length: 16,
        kind: ObjectKind::Payload,
    };
    let allocation = graph.objects[&payload_id].allocation;
    let endpoint = Reference {
        offset: payload.offset + 16,
        ..payload
    };
    let returned = view.read_with_key(endpoint, 0).unwrap().1;
    assert_ne!(returned, allocation);
    assert_eq!(returned, view.read_with_key(node, 96).unwrap().1);
    assert!(graph.read_bindings.contains(&ReadBinding {
        reference: endpoint,
        length: 0,
        version: returned
    }));
    // Removing the neighbor changes actual zero-byte owner despite unchanged payload.
    let keep = BTreeSet::from([allocation]);
    let previous_only = view.retain(&keep).unwrap();
    assert_eq!(
        previous_only.read_with_key(endpoint, 0).unwrap().1,
        allocation
    );
}
#[test]
fn full_audit_precedes_typed_output_for_cross_family_and_malformed_unselected_edge() {
    let mut a = arena();
    let v6 = ulmdb_prefix::insert(
        &mut a,
        Reference::NULL,
        Prefix::new(Address::V6(0), 0).unwrap(),
        b"v6",
    )
    .unwrap();
    let bad_family = descriptor(&mut a, Reference::NULL, b"bad", 1, DatabaseKind::Ipv4, v6);
    let left = ulmdb_prefix::insert(
        &mut a,
        Reference::NULL,
        Prefix::new(Address::V4(0), 32).unwrap(),
        b"left",
    )
    .unwrap();
    let root = ulmdb_prefix::insert(
        &mut a,
        left,
        Prefix::new(Address::V4(u32::MAX), 32).unwrap(),
        b"right",
    )
    .unwrap();
    let stale = Reference {
        segment: 1,
        offset: (17 << 28) | 4096,
    };
    let root = a.patch(root, 96, 48, &stale.encode(), 16).unwrap();
    let bad_edge = descriptor(&mut a, Reference::NULL, b"bad", 1, DatabaseKind::Ipv4, root);
    let view = a.freeze().unwrap().activate();
    assert_eq!(
        ulmdb_prefix::get(&view, root, Address::V4(0))
            .unwrap()
            .unwrap()
            .1,
        b"left"
    );
    for cat in [bad_family, bad_edge] {
        let old = audit::audit(&view, cat, 2, limits().reads).err().unwrap();
        let new = bootstrap(&view, cat, 2, receipt(), 3, limits())
            .err()
            .unwrap();
        assert_eq!(old.to_string(), new.to_string());
    }
}
#[test]
fn actual_engine_bootstrap_stays_bound_to_old_snapshot_across_commit_and_cold_open() {
    for base in ["/tmp", "/dev/shm"] {
        let path = std::path::Path::new(base).join(format!(
            "ulmdb-w1-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let options = super::super::Options {
            pool: Config {
                kv_pages: 64,
                tiny_pages: 64,
                max_reserved_bytes: 64 << 20,
                max_metadata_bytes: 64 << 20,
                max_views: 64,
            },
            wal_capacity: 8 << 20,
            graph_read_budget: 100_000,
            auto_collect_below: 8,
        };
        let env = super::super::Environment::create_with_redo(
            &path,
            options,
            super::super::RedoFormat::CompactV2,
        )
        .unwrap();
        let mut w = env.begin_write().unwrap();
        for (name, kind) in [
            (b"kv".as_slice(), DatabaseKind::Kv),
            (b"v4", DatabaseKind::Ipv4),
            (b"v6", DatabaseKind::Ipv6),
        ] {
            w.create_database(name, kind).unwrap();
        }
        w.put(b"kv", b"x", b"old").unwrap();
        w.insert_prefix(b"v4", Prefix::new(Address::V4(0), 0).unwrap(), b"")
            .unwrap();
        w.insert_prefix(b"v6", Prefix::new(Address::V6(0), 0).unwrap(), b"v6")
            .unwrap();
        w.commit().unwrap();
        let old = env.begin_read();
        let graph = old.bootstrap_objects(limits()).unwrap();
        let objects = graph.objects.clone();
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"x", b"new").unwrap();
        w.commit().unwrap();
        drop(env);
        let reopened = super::super::Environment::open(&path, options).unwrap();
        assert_eq!(old.get(b"kv", b"x").unwrap(), Some(b"old".as_slice()));
        assert_eq!(old.bootstrap_objects(limits()).unwrap().objects, objects);
        let fresh = reopened.begin_read();
        let fresh_graph = fresh.bootstrap_objects(limits()).unwrap();
        assert_eq!(fresh_graph.receipt.sequence, 2);
        assert_eq!(fresh.get(b"kv", b"x").unwrap(), Some(b"new".as_slice()));
        drop(fresh_graph);
        drop(fresh);
        drop(reopened);
        drop(graph);
        drop(old);
        std::fs::remove_dir_all(path).unwrap();
    }
}
