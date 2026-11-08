#![cfg(target_os = "linux")]
use std::{fs, path::PathBuf};
use ulmdb::{
    DatabaseKind, Environment, Error, MaintenancePolicy, PublicationMode, ReadTxn,
    ResumableReclaimBudget, backup,
};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "auto-resumable-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn seed(d: &Dir) -> Environment {
    let e = Environment::create(&d.0).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"k", &[1; 12000]).unwrap();
    w.commit().unwrap();
    e
}
fn update(e: &Environment, n: u8) {
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", b"k", &[n; 12000]).unwrap();
    w.commit().unwrap();
}
fn value(r: &ReadTxn) -> u8 {
    r.get(b"kv", b"k").unwrap().unwrap()[0]
}
fn policy(units: usize) -> MaintenancePolicy {
    MaintenancePolicy::LegacyResumable {
        segment_threshold: usize::MAX,
        rewrite_budget_bytes: 16 << 20,
        reclaim: ResumableReclaimBudget {
            mark_units: units,
            unlink_files: 2,
            ..Default::default()
        },
    }
}
#[test]
fn forward_churn_finishes_marks_without_lifetime_gates_and_preserves_incremental_history() {
    let d = Dir::new();
    let target = Dir::new();
    let e = seed(&d);
    let old = e.begin_read();
    backup::export(&e, &target.0).unwrap();
    e.retain_snapshot("first", &old).unwrap();
    for n in 2..=12 {
        update(&e, n);
    }
    let remote = ReadTxn::open_coordinated_snapshot(&d.0).unwrap();
    for n in 13..=40 {
        update(&e, n);
    }
    e.set_maintenance_policy(policy(8)).unwrap();
    e.prune_inventory().unwrap();
    let current = e.begin_read();
    let uuid = current.file_manifest().unwrap().uuid;
    let c =
        ulmdb_storage::coordination::Coordination::open(&d.0, uuid, Default::default()).unwrap();
    drop(current);
    for n in 0..180u8 {
        update(&e, n);
        let status = e.maintenance_status().unwrap();
        assert!(status.last_resumable_mark_units <= 8, "{status:?}");
        assert_eq!((value(&old), value(&remote)), (1, 12));
        drop(c.try_capture().unwrap());
        drop(c.try_maintenance().unwrap());
    }
    let status = e.maintenance_status().unwrap();
    assert!(status.resumable_windows_completed >= 2, "{status:?}");
    assert!(status.reclaimed_files > 0, "{status:?}");
    assert_eq!(
        status.resumable_nonforward_resets, 1,
        "forward heads must not restart marks: {status:?}"
    );
    assert_eq!(status.last_reclaim_error, None, "{status:?}");
    assert!(status.resumable_released_snapshot_pins > 0);
    assert_eq!(value(&e.open_retained("first").unwrap()), 1);
    let mut delta = Vec::new();
    e.dump_incremental(1, &mut delta).unwrap();
    let dest = Environment::open(&target.0).unwrap();
    dest.load_incremental(&mut &delta[..]).unwrap();
    assert_eq!(value(&dest.begin_read()), 179);
    backup::verify(&dest).unwrap();
    let retention_changes = e.maintenance_status().unwrap().resumable_retention_changes;
    e.release_snapshot("first").unwrap();
    drop((old, remote));
    update(&e, 180);
    assert_eq!(
        e.maintenance_status().unwrap().resumable_retention_changes,
        retention_changes + 1
    );
    e.prune_inventory().unwrap();
    update(&e, 181);
    assert_eq!(
        e.maintenance_status().unwrap().resumable_nonforward_resets,
        2
    );
    drop(e);
    let reopened = Environment::open(&d.0).unwrap();
    assert_eq!(value(&reopened.begin_read()), 181);
    assert_eq!(
        reopened.maintenance_policy().unwrap(),
        MaintenancePolicy::AUTOMATIC
    );
}
#[test]
fn unknown_retained_root_defers_and_discovery_quota_never_breaks_valid_commit() {
    let d = Dir::new();
    let e = seed(&d);
    for n in 2..=20 {
        update(&e, n);
    }
    let late = e.begin_read();
    for n in 21..=30 {
        update(&e, n);
    }
    e.set_maintenance_policy(policy(2)).unwrap();
    e.prune_inventory().unwrap();
    update(&e, 31);
    let marked_before = e.maintenance_status().unwrap().resumable_marked_units;
    e.retain_snapshot("late", &late).unwrap();
    for n in 32..=40 {
        update(&e, n);
        assert_eq!(value(&late), 20);
        assert!(e.maintenance_status().unwrap().last_resumable_mark_units <= 2);
    }
    // Pending graph roots excludes registry/descriptor discovery. Require work
    // to progress, and fully reopen the retained source rather than asserting
    // that one particular internal queue is nonempty at this scheduling point.
    assert!(e.maintenance_status().unwrap().resumable_marked_units > marked_before);
    assert_eq!(value(&e.open_retained("late").unwrap()), 20);
    let before = e.begin_read().sequence();
    let mut b = ResumableReclaimBudget::default();
    b.max_inventory_entries = 1;
    e.set_maintenance_policy(MaintenancePolicy::LegacyResumable {
        segment_threshold: usize::MAX,
        rewrite_budget_bytes: 16 << 20,
        reclaim: b,
    })
    .unwrap();
    e.prune_inventory().unwrap();
    update(&e, 41);
    assert_eq!(e.begin_read().sequence(), before + 1);
    assert!(e.maintenance_status().unwrap().last_reclaim_error.is_some());
    assert_eq!(value(&e.open_retained("late").unwrap()), 20);
    for mode in [PublicationMode::Cohort, PublicationMode::Indexed] {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        assert!(matches!(
            e.set_maintenance_policy(policy(4)),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(e.maintenance_policy().unwrap(), MaintenancePolicy::AUTOMATIC);
    }
}

#[test]
fn automatic_rebuild_retirements_progress_without_manual_prune_or_weak_head_deletion() {
    let d = Dir::new();
    let e = seed(&d);
    let old = e.begin_read();
    e.set_maintenance_policy(MaintenancePolicy::LegacyResumable {
        segment_threshold: 5,
        rewrite_budget_bytes: 8 << 20,
        reclaim: ResumableReclaimBudget {
            mark_units: 16,
            unlink_files: 2,
            ..Default::default()
        },
    })
    .unwrap();
    // No prune, compact or manual reclaim call: ordinary automatic candidate
    // rebuilds supply authenticated old/new inventory differences themselves.
    for n in 2..=72 {
        update(&e, n);
    }
    let ready = e.maintenance_status().unwrap();
    assert!(ready.rebuilt_candidates > 0, "{ready:?}");
    assert!(ready.reclaimed_files > 0, "{ready:?}");
    assert!(ready.resumable_windows_completed > 0, "{ready:?}");
    assert_eq!(ready.resumable_nonforward_resets, 0, "{ready:?}");
    assert_eq!(value(&old), 1);
    e.set_durability(ulmdb::Durability::NoSync).unwrap();
    for n in 73..=80 {
        update(&e, n);
    }
    let weak = e.maintenance_status().unwrap();
    assert_eq!(weak.reclaimed_files, ready.reclaimed_files);
    assert!(weak.last_reclaim_error.is_some());
    e.set_durability(ulmdb::Durability::Full).unwrap();
    for n in 81..=100 {
        update(&e, n);
    }
    assert_eq!(value(&old), 1);
    assert_eq!(e.maintenance_status().unwrap().last_reclaim_error, None);
    assert_eq!(value(&e.begin_read()), 100);
}

#[test]
fn incremental_rewrite_exceeds_inventory_budget_and_survives_prepared_abort() {
    for mode in [PublicationMode::Legacy, PublicationMode::Cohort, PublicationMode::Indexed] {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"data", DatabaseKind::Kv).unwrap();
        w.create_database(b"dups", DatabaseKind::Duplicates { width: Some(32) }).unwrap();
        w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
        for n in 0..96u32 {
            w.put(b"data", &n.to_be_bytes(), &[n as u8; 4096]).unwrap();
            w.insert_duplicate(b"dups", b"key", &[n as u8; 32]).unwrap();
        }
        let prefix = ulmdb::Prefix::new(ulmdb::Address::V4(0), 0).unwrap();
        w.insert_prefix(b"ip", prefix, b"route").unwrap();
        w.commit().unwrap();
        let original = e.begin_read();
        let cutoff = ulmdb_storage::read_current(&d.0).unwrap().next_segment;
        assert!(original.stats().segment_bytes > 64 << 10);
        e.set_maintenance_policy(MaintenancePolicy::CrossFormatResumable {
            segment_threshold: 2,
            rewrite_budget_bytes: 64 << 10,
            reclaim: ResumableReclaimBudget { mark_units: 32, ..Default::default() },
            history_budget_bytes: 1 << 20,
        }).unwrap();
        for n in 0..128u32 {
            let mut w = e.begin_write().unwrap();
            w.put(b"data", b"tick", &n.to_le_bytes()).unwrap();
            if n == 5 {
                w.prepare().unwrap().abort();
            } else { w.commit().unwrap(); }
            if e.maintenance_status().unwrap().rebuilt_candidates > 0 { break; }
        }
        let status = e.maintenance_status().unwrap();
        assert!(status.rebuilt_candidates > 0, "{mode:?}: {status:?}");
        assert!(status.incremental_rewrite_steps > 3, "{status:?}");
        let manifest = ulmdb_storage::read_current(&d.0).unwrap();
        assert!(manifest.segments.iter().all(|entry| entry.id >= cutoff));
        for n in 0..96u32 {
            assert_eq!(original.get(b"data", &n.to_be_bytes()).unwrap().unwrap(), &[n as u8; 4096]);
            assert_eq!(e.begin_read().get(b"data", &n.to_be_bytes()).unwrap().unwrap(), &[n as u8; 4096]);
        }
        assert_eq!(e.begin_read().duplicate_count(b"dups", b"key").unwrap(), 96);
        assert_eq!(e.begin_read().exact_prefix(b"ip", prefix).unwrap().unwrap().1, b"route");
        backup::verify(&e).unwrap();
        drop(e);
        let cold = Environment::open(&d.0).unwrap();
        backup::verify(&cold).unwrap();
    }
}

#[test]
fn fallback_rewrite_keeps_large_cold_values_and_real_business_previous() {
    for mode in [PublicationMode::Legacy, PublicationMode::Cohort, PublicationMode::Indexed] {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"cold", DatabaseKind::Kv).unwrap();
        w.create_database(b"hot", DatabaseKind::Kv).unwrap();
        w.put(b"cold", b"k", &[7; 128 << 10]).unwrap();
        w.put(b"hot", b"k", &[0; 8192]).unwrap();
        w.commit().unwrap();
        let old = e.begin_read();
        e.set_maintenance_policy_with_fallback(MaintenancePolicy::CrossFormatResumable {
            segment_threshold: 2, rewrite_budget_bytes: 64 << 10,
            reclaim: ResumableReclaimBudget { mark_units: 64, ..Default::default() },
            history_budget_bytes: 1 << 20,
        }).unwrap();
        for n in 1..=40u8 {
            let before = ulmdb_storage::read_current(&d.0).unwrap();
            let mut w = e.begin_write().unwrap();
            w.put(b"hot", b"k", &[n; 8192]).unwrap();
            w.commit().unwrap();
            let (current, previous) = ulmdb_storage::read_roots(&d.0).unwrap();
            let previous = previous.unwrap();
            assert_eq!(current.sequence, before.sequence + 1);
            assert_eq!(current.previous, before.generation);
            assert_eq!(previous.generation, before.generation);
            assert_eq!(current.parent, previous.commit);
            assert_eq!(e.begin_read().get(b"cold", b"k").unwrap().unwrap(), &[7; 128 << 10]);
        }
        let status = e.maintenance_status().unwrap();
        assert!(status.fallback_rewrite_attempts > 0, "{mode:?}: {status:?}");
        assert!(status.rebuilt_candidates > 0 && status.reclaimed_files > 0, "{status:?}");
        assert!(status.last_rewrite_kept_segments > 0, "{status:?}");
        assert!(e.begin_read().stats().segments <= 4);
        assert_eq!(old.get(b"hot", b"k").unwrap().unwrap(), &[0; 8192]);
        backup::verify(&e).unwrap();
        drop(e);
        backup::verify(&Environment::open(&d.0).unwrap()).unwrap();
    }
}

#[test]
fn ordinary_file_owners_reuse_space_without_policy_or_manual_maintenance() {
    for mode in [PublicationMode::Legacy, PublicationMode::Cohort, PublicationMode::Indexed] {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        assert_eq!(e.maintenance_policy().unwrap(), MaintenancePolicy::AUTOMATIC);
        let mut w = e.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"k", &[0; 12000]).unwrap();
        w.commit().unwrap();
        let old = e.begin_read();
        for n in 1..=320u32 {
            let mut w = e.begin_write().unwrap();
            w.put(b"kv", b"k", &[n as u8; 12000]).unwrap();
            w.commit().unwrap();
        }
        let status = e.maintenance_status().unwrap();
        assert!(status.fallback_rewrite_attempts > 0 && status.rebuilt_candidates > 0
            && status.reclaimed_files > 0, "{mode:?}: {status:?}");
        assert!(e.begin_read().stats().segments < 128);
        assert_eq!(value(&old), 0);
        assert_eq!(value(&e.begin_read()), 320u32 as u8);
        backup::verify(&e).unwrap();
    }
}

#[cfg(feature = "fault-injection")]
#[test]
fn optional_rewrite_failure_worker() {
    if std::env::var_os("ULMDB_OPTIONAL_FAILURE_WORKER").is_none() { return; }
    let d = Dir::new();
    let e = Environment::create_with_publication(&d.0, PublicationMode::Indexed).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"data", DatabaseKind::Kv).unwrap();
    w.put(b"data", b"k", &[3; 32768]).unwrap();
    w.commit().unwrap();
    let old = e.begin_read();
    let before = e.file_checkpoint().unwrap();
    e.set_maintenance_policy_with_fallback(MaintenancePolicy::CrossFormatResumable {
        segment_threshold: 1, rewrite_budget_bytes: 64 << 10,
        reclaim: Default::default(), history_budget_bytes: 1 << 20,
    }).unwrap();
    for n in 4..=5u8 {
        let mut w = e.begin_write().unwrap();
        w.put(b"data", b"k", &[n; 32768]).unwrap();
        let ready = w.prepare().unwrap();
        if n == 4 { assert_eq!(before, e.file_checkpoint().unwrap()); }
        assert!(ready.will_publish());
        assert_eq!(ready.commit().unwrap(), u64::from(n - 2));
        assert_eq!(e.begin_read().get(b"data", b"k").unwrap().unwrap(), &[n; 32768]);
    }
    let status = e.maintenance_status().unwrap();
    assert!(status.fallback_rewrite_deferred >= 2, "{status:?}");
    assert!(status.last_fallback_rewrite_error.is_some());
    assert_eq!(old.get(b"data", b"k").unwrap().unwrap(), &[3; 32768]);
    backup::verify(&e).unwrap();
}

#[cfg(feature = "fault-injection")]
#[test]
fn optional_rewrite_io_failure_preserves_ready_business_write() {
    for point in ["maintenance.optional.before_seal", "maintenance.optional.before_descriptor"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "optional_rewrite_failure_worker", "--nocapture"])
            .env("ULMDB_OPTIONAL_FAILURE_WORKER", "1")
            .env("ULMDB_FAIL_AT", point)
            .output().unwrap();
        assert!(output.status.success(), "{point}: {}\n{}",
            String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    }
}
