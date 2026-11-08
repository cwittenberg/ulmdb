#![cfg(target_os = "linux")]
use std::{fs, path::PathBuf};
use ulmdb::{
    DatabaseKind, Environment, Error, MaintenancePolicy, PublicationMode, ReadTxn,
    ReclaimPlanLimits, ReclaimStepBudget, backup,
};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "resumable-reclaim-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn seed(path: &std::path::Path, mode: PublicationMode) -> Environment {
    let e = Environment::create_with_publication(path, mode).unwrap();
    // This fixture owns bounded reclamation; preserve the obsolete files for
    // its explicit plan rather than letting commit-time cleanup consume them.
    e.set_maintenance_policy(MaintenancePolicy::Disabled).unwrap();
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
fn orphan(d: &Dir) -> PathBuf {
    let p = d.0.join("segments").join(format!("{}.seg", "f".repeat(64)));
    fs::write(&p, b"unselected").unwrap();
    p
}
fn value(r: &ReadTxn) -> u8 {
    r.get(b"kv", b"k").unwrap().unwrap()[0]
}
#[test]
fn tiny_mark_steps_preserve_readers_retention_and_exact_incremental_history() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new();
        let target = Dir::new();
        let e = seed(&d.0, mode);
        let first = e.begin_read();
        backup::export(&e, &target.0).unwrap();
        e.retain_snapshot("base", &first).unwrap();
        drop(first);
        update(&e, 2);
        let old = e.begin_read();
        update(&e, 3);
        let remote = ReadTxn::open_coordinated_snapshot(&d.0).unwrap();
        for n in 4..=6 {
            update(&e, n);
        }
        e.prune_inventory().unwrap();
        update(&e, 7);
        let garbage = orphan(&d);
        let coordination = ulmdb_storage::coordination::Coordination::open(
            &d.0,
            e.begin_read().file_manifest().unwrap().uuid,
            Default::default(),
        )
        .unwrap();
        let expected = e
            .begin_read()
            .file_manifest()
            .unwrap()
            .segments
            .iter()
            .map(|x| x.length)
            .sum::<u64>();
        let mut plan = e.begin_reclaim_plan(Default::default()).unwrap();
        assert!(matches!(coordination.try_capture(), Err(Error::Busy)));
        assert!(matches!(
            e.set_maintenance_policy(MaintenancePolicy::Disabled),
            Err(Error::Busy)
        ));
        let mut total = 0;
        let mut last_scanned = 0;
        loop {
            let p = plan
                .step(ReclaimStepBudget {
                    mark_units: 1,
                    directory_entries: 2,
                    unlink_files: 1,
                })
                .unwrap();
            assert!(p.last_mark_units <= 1);
            assert!(p.report.scanned_directory_entries - last_scanned <= 2);
            last_scanned = p.report.scanned_directory_entries;
            total += 1;
            if !p.marking_complete {
                assert!(garbage.exists());
            }
            assert_eq!((value(&old), value(&remote)), (2, 3));
            if p.complete {
                assert_eq!(p.rewrite_source_bytes, expected);
                assert!(p.rewrite_estimate_complete);
                assert!(p.preserved_history_files > 0);
                break;
            }
            assert!(total < 1000);
        }
        assert!(total > 10);
        assert!(!garbage.exists());
        drop(plan);
        drop(coordination.try_capture().unwrap());
        assert_eq!(value(&e.open_retained("base").unwrap()), 1);
        let mut delta = Vec::new();
        e.dump_incremental(1, &mut delta).unwrap();
        let dest = Environment::open(&target.0).unwrap();
        dest.load_incremental(&mut &delta[..]).unwrap();
        assert_eq!(value(&dest.begin_read()), 7);
        backup::verify(&dest).unwrap();
        if mode == PublicationMode::Legacy {
            assert!(last_scanned > 0);
        }
    }
}
#[test]
fn mark_limits_and_cancel_never_delete_assets_or_poison_the_owner() {
    let d = Dir::new();
    let e = seed(&d.0, PublicationMode::Indexed);
    let garbage = orphan(&d);
    let mut limits = ReclaimPlanLimits::default();
    limits.max_marks = 1;
    let mut plan = e.begin_reclaim_plan(limits).unwrap();
    assert!(matches!(
        plan.step(ReclaimStepBudget {
            mark_units: 1,
            directory_entries: 10,
            unlink_files: 10
        }),
        Err(Error::LimitExceeded)
    ));
    assert!(garbage.exists());
    drop(plan);
    let mut plan = e.begin_reclaim_plan(Default::default()).unwrap();
    assert!(matches!(
        plan.step(ReclaimStepBudget {
            mark_units: 0,
            directory_entries: 0,
            unlink_files: 0
        }),
        Err(Error::InvalidInput(_))
    ));
    assert!(
        plan.step(ReclaimStepBudget {
            mark_units: 1,
            directory_entries: 0,
            unlink_files: 0
        })
        .is_ok()
    );
    drop(plan);
    assert!(garbage.exists());
    update(&e, 2);
    e.set_maintenance_policy(MaintenancePolicy::Disabled)
        .unwrap();
    assert_eq!(value(&e.begin_read()), 2);
}
#[test]
fn changed_capture_gate_and_head_refuse_before_any_asset_delete() {
    for change_head in [false, true] {
        let d = Dir::new();
        let e = seed(&d.0, PublicationMode::Legacy);
        let garbage = orphan(&d);
        let current = fs::read(d.0.join("CURRENT")).unwrap();
        let mut plan = e.begin_reclaim_plan(Default::default()).unwrap();
        if change_head {
            fs::write(d.0.join("CURRENT"), b"changed").unwrap();
        } else {
            let gate = d.0.join(".ulmdb-coordination/capture");
            fs::rename(&gate, gate.with_extension("saved")).unwrap();
            fs::write(&gate, []).unwrap();
        }
        let result = plan.step(ReclaimStepBudget {
            mark_units: 65536,
            directory_entries: 65536,
            unlink_files: 4096,
        });
        if change_head {
            assert!(matches!(result, Err(Error::OutcomeUnknown)));
        } else {
            assert!(matches!(result, Err(Error::Corrupt(_))));
        }
        assert!(garbage.exists());
        drop(plan);
        if change_head {
            fs::write(d.0.join("CURRENT"), current).unwrap();
        } else {
            let gate = d.0.join(".ulmdb-coordination/capture");
            fs::remove_file(&gate).unwrap();
            fs::rename(gate.with_extension("saved"), gate).unwrap();
        }
        drop(e);
        assert_eq!(value(&Environment::open(&d.0).unwrap().begin_read()), 1);
    }
}
