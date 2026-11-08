#![cfg(target_os = "linux")]
use std::{
    fs,
    path::{Path, PathBuf},
};
use ulmdb::{
    Address, DatabaseKind, Durability, Environment, Error, MaintenancePolicy, OwnedFileCapture,
    Prefix, PublicationMode, ReadTxn, ResidencyPolicy, backup,
};
use ulmdb_storage::coordination::{Coordination, Limits};
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "client-copy-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn capture(path: &Path) -> OwnedFileCapture {
    OwnedFileCapture::current_for_copy(
        path,
        Coordination::capture_directory(path, Default::default()).unwrap(),
    )
    .unwrap()
}
fn pair(path: &Path) -> ulmdb::FileCopyPair {
    capture(path)
        .into_copy_pair()
        .unwrap()
        .admit(ResidencyPolicy::Demand)
        .unwrap()
}
fn pins(path: &Path) -> usize {
    fs::read_dir(path.join(".ulmdb-coordination/pins"))
        .unwrap()
        .count()
}
fn previous(path: &Path) -> ReadTxn {
    ReadTxn::open_previous_coordinated_snapshot(path, ResidencyPolicy::Demand).unwrap()
}
fn assets(path: &Path) -> std::collections::BTreeSet<PathBuf> {
    ["segments", "commits", "cohorts"]
        .into_iter()
        .flat_map(|name| {
            fs::read_dir(path.join(name))
                .unwrap()
                .map(|p| p.unwrap().path().strip_prefix(path).unwrap().to_path_buf())
        })
        .collect()
}
fn seed(path: &Path, mode: PublicationMode, policy: Durability) -> Environment {
    let env = Environment::create_with_publication(path, mode).unwrap();
    env.set_durability(policy).unwrap();
    let mut w = env.begin_write().unwrap();
    for (name, kind) in [
        (b"kv".as_slice(), DatabaseKind::Kv),
        (b"dups", DatabaseKind::Duplicates { width: None }),
        (b"fixed", DatabaseKind::Duplicates { width: Some(4) }),
        (b"v4", DatabaseKind::Ipv4),
        (b"v6", DatabaseKind::Ipv6),
    ] {
        w.create_database(name, kind).unwrap();
    }
    w.put(b"kv", b"k", b"A").unwrap();
    w.put(b"kv", b"large", &vec![42; 8192]).unwrap();
    for n in 0..40u32 {
        w.insert_duplicate(b"dups", b"k", &n.to_le_bytes()).unwrap();
        w.insert_duplicate(b"fixed", b"k", &n.to_be_bytes())
            .unwrap();
    }
    w.insert_prefix(
        b"v4",
        Prefix::new(Address::V4(0x0a00_0000), 8).unwrap(),
        b"four",
    )
    .unwrap();
    w.insert_prefix(
        b"v6",
        Prefix::new(Address::V6(0x20010db8u128 << 96), 32).unwrap(),
        b"six",
    )
    .unwrap();
    assert_eq!(w.commit().unwrap(), 1);
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"k", b"B").unwrap();
    assert_eq!(w.commit().unwrap(), 2);
    env
}
fn check(read: &ReadTxn, value: &[u8]) {
    assert_eq!(read.get(b"kv", b"k").unwrap(), Some(value));
    assert_eq!(
        read.get(b"kv", b"large").unwrap(),
        Some(vec![42; 8192].as_slice())
    );
    assert_eq!(read.duplicate_count(b"dups", b"k").unwrap(), 40);
    assert_eq!(read.duplicate_count(b"fixed", b"k").unwrap(), 40);
    assert_eq!(
        read.longest_match(b"v4", Address::V4(0x0a01_0001))
            .unwrap()
            .unwrap()
            .1,
        b"four"
    );
    assert_eq!(
        read.longest_match(b"v6", Address::V6((0x20010db8u128 << 96) | 1))
            .unwrap()
            .unwrap()
            .1,
        b"six"
    );
}
#[test]
fn exact_pair_copy_preserves_both_commits_and_all_kinds_while_writer_is_prepared() {
    for mode in MODES {
        for policy in [Durability::Full, Durability::NoMetaSync, Durability::NoSync] {
            let source = Directory::new();
            let output = Directory::new();
            let env = seed(&source.0, mode, policy);
            let old = env.begin_read();
            let borrowed = old.get(b"kv", b"large").unwrap().unwrap();
            let before = fs::read(source.0.join("CURRENT")).unwrap();
            let count = pins(&source.0);
            let committed_assets = assets(&source.0);
            let mut writer = env.begin_write().unwrap();
            writer.put(b"kv", b"k", b"private").unwrap();
            let prepared = writer.prepare().unwrap();
            let unselected_assets: Vec<_> = assets(&source.0)
                .difference(&committed_assets)
                .cloned()
                .collect();
            assert!(!unselected_assets.is_empty());
            let pair = pair(&source.0);
            assert_eq!(pair.selected().sequence(), 2);
            assert_eq!(pair.predecessor().unwrap().sequence(), 1);
            check(pair.selected(), b"B");
            check(pair.predecessor().unwrap(), b"A");
            let report = backup::export_file_pair(&pair, &output.0).unwrap();
            assert_eq!(report.selected.sequence, 2);
            assert_eq!(
                report.source_selected_checkpoint,
                report.selected_checkpoint
            );
            assert_eq!(report.normalization, None);
            assert_eq!(report.predecessor.as_ref().unwrap().sequence, 1);
            assert_eq!(fs::read(source.0.join("CURRENT")).unwrap(), before);
            let copied = Environment::open(&output.0).unwrap();
            check(&copied.begin_read(), b"B");
            let previous = previous(&output.0);
            check(&previous, b"A");
            assert_eq!(
                previous.file_checkpoint().unwrap(),
                report.predecessor_checkpoint.unwrap()
            );
            assert_eq!(
                copied.file_checkpoint().unwrap(),
                report.selected_checkpoint
            );
            assert_eq!(
                copied.begin_read().file_manifest().unwrap().next_database,
                pair.selected().file_manifest().unwrap().next_database
            );
            for path in unselected_assets {
                assert!(!output.0.join(path).exists());
            }
            assert!(fs::metadata(output.0.join("CURRENT")).unwrap().len() <= 144);
            assert!(!output.0.join("LMDB-VIEW").exists());
            let mut w = copied.begin_write().unwrap();
            w.put(b"kv", b"destination", b"only").unwrap();
            assert_eq!(w.commit().unwrap(), 3);
            prepared.abort();
            assert_eq!(borrowed, vec![42; 8192]);
            check(&old, b"B");
            assert_eq!(env.begin_read().get(b"kv", b"destination").unwrap(), None);
            drop(previous);
            drop(pair);
            assert_eq!(pins(&source.0), count);
        }
    }
}
#[test]
fn compact_copy_uses_genuine_chosen_a_and_new_business_one_with_genesis_zero() {
    for mode in MODES {
        let source = Directory::new();
        let output = Directory::new();
        let env = seed(&source.0, mode, Durability::NoSync);
        let cp = env.file_checkpoint().unwrap();
        let copy = capture(&source.0)
            .select_previous()
            .unwrap()
            .admit_copy_view(ResidencyPolicy::Demand)
            .unwrap();
        assert_ne!(*copy.view_checkpoint(), cp);
        assert_eq!(*copy.selected_checkpoint(), cp);
        let original: Vec<_> = copy
            .read()
            .database_names()
            .unwrap()
            .into_iter()
            .map(|n| (n.clone(), copy.read().open_database(&n).unwrap().id()))
            .collect();
        let report = backup::compact_file_copy(&copy, &output.0).unwrap();
        assert_eq!(report.selected.sequence, 1);
        assert_eq!(report.source_checkpoint, Some(*copy.view_checkpoint()));
        assert_ne!(
            report.selected.uuid,
            copy.read().file_manifest().unwrap().uuid
        );
        let out = Environment::open(&output.0).unwrap();
        check(&out.begin_read(), b"A");
        let p = previous(&output.0);
        assert_eq!(p.sequence(), 0);
        assert!(p.database_names().unwrap().is_empty());
        assert_eq!(p.file_checkpoint().unwrap(), report.genesis_checkpoint);
        for (n, id) in original {
            assert_eq!(out.begin_read().open_database(&n).unwrap().id(), id);
        }
        assert_eq!(
            out.database_id_floor(),
            copy.read().file_manifest().unwrap().next_database
        );
        assert_eq!(env.file_checkpoint().unwrap(), cp);
        assert_eq!(backup::verify(&out).unwrap().sequence, 1);
        assert!(fs::metadata(output.0.join("CURRENT")).unwrap().len() <= 144);
    }
}
#[test]
fn explicit_empty_copy_is_actual_fresh_genesis_not_a_counter_offset() {
    for mode in MODES {
        let out = Directory::new();
        let r = backup::create_empty_file_copy(&out.0, mode).unwrap();
        let e = Environment::open(&out.0).unwrap();
        assert_eq!(e.begin_read().sequence(), 0);
        assert_eq!(e.database_id_floor(), 1);
        assert_eq!(r.database_id_floor, 1);
        assert_eq!(r.selected_checkpoint, r.genesis_checkpoint);
        assert_eq!(previous(&out.0).sequence(), 0);
        let mut w = e.begin_write().unwrap();
        w.create_database(b"first", DatabaseKind::Kv).unwrap();
        assert_eq!(w.open_database(b"first").unwrap().id(), 1);
        assert_eq!(w.commit().unwrap(), 1);
    }
}
#[test]
fn pair_capture_failure_and_residency_failure_release_every_pin() {
    for mode in MODES {
        let d = Directory::new();
        let e = seed(&d.0, mode, Durability::Full);
        // Internal discovery has its own pending-source allowance. Release that
        // optional progress before testing the ordinary D/P admission quota;
        // generation directories themselves are not reader records.
        e.set_maintenance_policy(MaintenancePolicy::Disabled)
            .unwrap();
        let coordinator = Coordination::open(
            &d.0,
            e.begin_read().file_manifest().unwrap().uuid,
            Default::default(),
        )
        .unwrap();
        {
            let mut maintenance = coordinator.try_maintenance().unwrap();
            maintenance.scan().unwrap().cleanup_stale().unwrap();
        }
        let live = || {
            let mut maintenance = coordinator.try_maintenance().unwrap();
            let scan = maintenance.scan().unwrap();
            assert_eq!(scan.stale_records(), 0, "{mode:?}");
            scan.live_roots()
                .iter()
                .map(|root| *root.checkpoint())
                .collect::<Vec<_>>()
        };
        let checkpoint = e.file_checkpoint().unwrap();
        let head = fs::read(d.0.join("CURRENT")).unwrap();
        assert_eq!(live(), [checkpoint]);
        let before = live().len();
        let limits = Limits {
            max_pin_records: before + 1,
            ..Default::default()
        };
        let c =
            OwnedFileCapture::current(&d.0, Coordination::capture_directory(&d.0, limits).unwrap())
                .unwrap();
        assert!(matches!(c.into_copy_pair(), Err(Error::LimitExceeded)));
        assert_eq!(live(), [checkpoint]);
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head);
        check(&e.begin_read(), b"B");
        // A chosen-A capture must cost only the two result records, not A+D+P.
        let limits = Limits {
            max_pin_records: before + 2,
            ..Default::default()
        };
        let c = OwnedFileCapture::previous(
            &d.0,
            Coordination::capture_directory(&d.0, limits).unwrap(),
        )
        .unwrap();
        let p = c
            .into_copy_pair()
            .unwrap()
            .admit(ResidencyPolicy::Demand)
            .unwrap();
        let mut expected = vec![
            checkpoint,
            p.selected().file_checkpoint().unwrap(),
            p.predecessor().unwrap().file_checkpoint().unwrap(),
        ];
        expected.sort();
        let mut actual = live();
        actual.sort();
        assert_eq!(actual, expected);
        check(p.selected(), b"B");
        check(p.predecessor().unwrap(), b"A");
        drop(p);
        assert_eq!(live(), [checkpoint]);
        let c = capture(&d.0).into_copy_pair().unwrap();
        assert!(c.admit(ResidencyPolicy::Prefault { max_bytes: 0 }).is_err());
        assert_eq!(live(), [checkpoint]);
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), head);
        check(&e.begin_read(), b"B");
        assert!(
            Coordination::open(
                &d.0,
                e.begin_read().file_manifest().unwrap().uuid,
                Default::default()
            )
            .unwrap()
            .try_maintenance()
            .is_ok()
        );
        assert_eq!(e.begin_write().unwrap().commit().unwrap(), 2);
    }
}
#[test]
fn old_captured_d_head_change_is_busy_and_historyless_export_works() {
    for mode in MODES {
        let d = Directory::new();
        let single = Directory::new();
        let output = Directory::new();
        let e = seed(&d.0, mode, Durability::Full);
        let old = capture(&d.0);
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"k", b"C").unwrap();
        w.commit().unwrap();
        assert!(matches!(old.into_copy_pair(), Err(Error::Busy)));
        backup::export(&e, &single.0).unwrap();
        let p = pair(&single.0);
        assert!(p.predecessor().is_none());
        backup::export_file_pair(&p, &output.0).unwrap();
        assert_eq!(
            Environment::open(&output.0)
                .unwrap()
                .begin_read()
                .sequence(),
            3
        );
    }
}
#[test]
fn genuine_genesis_pair_copy_and_namespace_and_existing_target_rejections() {
    for mode in MODES {
        let source = Directory::new();
        let other = Directory::new();
        let out = Directory::new();
        let e = Environment::create_with_publication(&source.0, mode).unwrap();
        let other_env = Environment::create_with_publication(&other.0, mode).unwrap();
        let wrong = Coordination::capture_directory(&other.0, Default::default()).unwrap();
        assert!(OwnedFileCapture::current_for_copy(&source.0, wrong).is_err());
        let p = pair(&source.0);
        assert_eq!(p.selected().sequence(), 0);
        assert!(p.predecessor().is_none());
        backup::export_file_pair(&p, &out.0).unwrap();
        assert_eq!(
            Environment::open(&out.0).unwrap().begin_read().sequence(),
            0
        );
        assert_eq!(previous(&out.0).sequence(), 0);
        assert!(backup::export_file_pair(&p, &other.0).is_err());
        assert_eq!(other_env.begin_read().sequence(), 0);
        assert_eq!(e.begin_read().sequence(), 0);
        let empty = Directory::new();
        fs::create_dir(&empty.0).unwrap();
        assert!(backup::export_file_pair(&p, &empty.0).is_err());
        assert_eq!(fs::read_dir(&empty.0).unwrap().count(), 0);
    }
}
#[test]
fn new_copy_rejects_real_replica_and_range_and_preserves_generic_export() {
    use ulmdb::replication::{self, Cursor, PublisherKey, TransferLimits};
    for mode in MODES {
        let source = Directory::new();
        let replica = Directory::new();
        let staging = Directory::new();
        fs::create_dir(&staging.0).unwrap();
        let e = seed(&source.0, mode, Durability::Full);
        let r = e.begin_read();
        let cursor = Cursor::from_read(&r).unwrap();
        let key = PublisherKey::new(1, cursor.uuid, cursor.epoch, [37; 32]).unwrap();
        let mut wire = Vec::new();
        replication::send_snapshot(&e, &r, None, &key, &mut wire).unwrap();
        let frame = replication::receive(
            &mut std::io::Cursor::new(wire),
            &key,
            TransferLimits::default(),
            &staging.0,
        )
        .unwrap();
        let replica_env = replication::bootstrap(&frame, &replica.0).unwrap();
        assert!(replica_env.is_replica());
        let gate = Coordination::capture_directory(&replica.0, Default::default()).unwrap();
        assert!(matches!(
            OwnedFileCapture::current_for_copy(&replica.0, gate),
            Err(Error::ReadOnly)
        ));
        let legacy = Directory::new();
        backup::export(&e, &legacy.0).unwrap();
        assert_eq!(
            Environment::open(&legacy.0)
                .unwrap()
                .file_checkpoint()
                .unwrap(),
            e.file_checkpoint().unwrap()
        );
    }
    for mode in [PublicationMode::RangeV5, PublicationMode::Range] {
        let d = Directory::new();
        assert!(backup::create_empty_file_copy(&d.0, mode).is_err());
        assert!(!d.0.exists());
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let capture = Coordination::capture_directory(&d.0, Default::default()).unwrap();
        assert!(OwnedFileCapture::current_for_copy(&d.0, capture).is_err());
        drop(e);
    }
}
fn publish_bad_business_root(
    path: &Path,
    old: &ulmdb_storage::Manifest,
    mode: PublicationMode,
) -> ulmdb_storage::Manifest {
    use ulmdb_storage::{Arena, SegmentTable, WriteStore};
    let table = SegmentTable::load(path, &old.segments).unwrap();
    let mut db = ulmdb_kv::get(&table, old.catalog, b"kv")
        .unwrap()
        .unwrap()
        .to_vec();
    let mut arena = Arena::new(table, old.next_segment).unwrap();
    let invalid = arena.allocate(b"not a tree", 8).unwrap();
    db[16..32].copy_from_slice(&invalid.encode());
    let mut next = old.clone();
    next.catalog = ulmdb_kv::put(&mut arena, old.catalog, b"kv", &db).unwrap();
    let (table, sealed_entry) = arena.seal(path).unwrap();
    next.segments = table.entries().into();
    next.next_segment = sealed_entry.map_or(old.next_segment, |e| e.id.checked_add(1).unwrap());
    next.generation = ulmdb_storage::new_identity().unwrap();
    next.commit = ulmdb_storage::new_identity().unwrap();
    next.sequence = old.sequence + 1;
    next.parent = old.commit;
    next.previous = old.generation;
    if mode == PublicationMode::Indexed {
        next.prepare_indexed(None).unwrap();
    }
    ulmdb_storage::publish(path, &next).unwrap();
    next
}
#[test]
fn pair_admission_audits_both_graphs_but_chosen_view_does_not_audit_displaced_graph() {
    for mode in MODES {
        let d = Directory::new();
        let output = Directory::new();
        let e = seed(&d.0, mode, Durability::Full);
        let good = ulmdb_storage::read_current(&d.0).unwrap();
        let bad = publish_bad_business_root(&d.0, &good, mode);
        let count = pins(&d.0);
        let p = capture(&d.0).into_copy_pair().unwrap();
        assert!(matches!(
            p.admit(ResidencyPolicy::Demand),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(pins(&d.0), count);
        assert!(!output.0.exists());
        // D is metadata-valid but graph-invalid; choose its fully audited P.
        let view = capture(&d.0)
            .into_copy_pair()
            .unwrap()
            .admit_predecessor_view(ResidencyPolicy::Demand)
            .unwrap();
        check(view.read(), b"B");
        backup::compact_file_copy(&view, &output.0).unwrap();
        check(&Environment::open(&output.0).unwrap().begin_read(), b"B");
        drop(view);
        // Publish a full good D with the bad graph as its real alternate P.
        let mut next = good.clone();
        next.generation = ulmdb_storage::new_identity().unwrap();
        next.commit = ulmdb_storage::new_identity().unwrap();
        next.sequence = bad.sequence + 1;
        next.parent = bad.commit;
        next.previous = bad.generation;
        if mode == PublicationMode::Indexed {
            next.prepare_indexed(None).unwrap();
        }
        ulmdb_storage::publish(&d.0, &next).unwrap();
        assert!(
            capture(&d.0)
                .into_copy_pair()
                .unwrap()
                .admit(ResidencyPolicy::Demand)
                .is_err()
        );
        let view = capture(&d.0)
            .into_copy_pair()
            .unwrap()
            .admit_selected_view(ResidencyPolicy::Demand)
            .unwrap();
        check(view.read(), b"B");
        drop(e);
    }
}
#[test]
fn genuine_historyless_export_normalizes_only_physical_metadata_and_reports_actual_inventory() {
    for mode in MODES {
        for source_policy in [Durability::Full, Durability::NoSync] {
            let original = Directory::new();
            let historyless = Directory::new();
            let output = Directory::new();
            let e = seed(&original.0, mode, source_policy);
            backup::export(&e, &historyless.0).unwrap();
            let before = fs::read(historyless.0.join("CURRENT")).unwrap();
            let source = pair(&historyless.0);
            assert!(source.predecessor().is_none());
            let m = source.selected().file_manifest().unwrap().clone();
            assert_ne!(m.previous, 0);
            let cp = source.selected().file_checkpoint().unwrap();
            let borrowed = source.selected().get(b"kv", b"large").unwrap().unwrap();
            let r = backup::export_file_pair(&source, &output.0).unwrap();
            assert_eq!(r.source_selected_checkpoint, cp);
            assert_ne!(r.selected_checkpoint, cp);
            assert_eq!(r.source_selected.generation, m.generation);
            assert_ne!(r.selected.generation, m.generation);
            assert_eq!(
                r.normalization,
                Some(backup::PairCopyNormalization::UnavailablePredecessor)
            );
            assert!(r.predecessor.is_none());
            assert!(r.predecessor_checkpoint.is_none());
            let out = Environment::open(&output.0).unwrap();
            check(&out.begin_read(), b"B");
            let after = out.begin_read();
            let n = after.file_manifest().unwrap();
            assert_eq!(
                (
                    n.uuid,
                    n.epoch,
                    n.sequence,
                    n.commit,
                    n.parent,
                    n.next_database,
                    n.next_segment
                ),
                (
                    m.uuid,
                    m.epoch,
                    m.sequence,
                    m.commit,
                    m.parent,
                    m.next_database,
                    m.next_segment
                )
            );
            assert_eq!(n.catalog, m.catalog);
            assert_eq!(n.previous, 0);
            assert_eq!(after.file_checkpoint().unwrap(), r.selected_checkpoint);
            assert!(matches!(
                ulmdb::ReadTxn::open_previous_coordinated_snapshot(
                    &output.0,
                    ResidencyPolicy::Demand
                ),
                Err(ulmdb::PreviousError::NoPrevious)
            ));
            assert_eq!(assets(&output.0).len(), r.physical_files);
            let bytes: u64 = assets(&output.0)
                .into_iter()
                .map(|p| fs::metadata(output.0.join(p)).unwrap().len())
                .sum();
            assert_eq!(bytes, r.physical_bytes);
            assert_eq!(r.selected.physical_bytes, bytes);
            assert_eq!(borrowed, vec![42; 8192]);
            assert_eq!(fs::read(historyless.0.join("CURRENT")).unwrap(), before);
            // Generic export keeps its original identity/historyless contract.
            assert_eq!(
                ulmdb_storage::read_current(&historyless.0)
                    .unwrap()
                    .generation,
                m.generation
            );
            let mut w = out.begin_write().unwrap();
            w.put(b"kv", b"next", b"only-output").unwrap();
            assert_eq!(w.commit().unwrap(), 3);
            assert_eq!(source.selected().get(b"kv", b"next").unwrap(), None);
        }
    }
}
