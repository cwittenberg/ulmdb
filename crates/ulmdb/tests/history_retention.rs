#![cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::{fs, path::PathBuf};
use ulmdb::{
    DatabaseKind, Environment, Error, HistoryLifecycleBudget, HistoryPolicy, MaintenancePolicy,
    ResumableReclaimBudget,
};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "history-floor-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn put(e: &Environment, n: u8) {
    let mut w = e.begin_write().unwrap();
    w.put(b"db", b"key", &[n; 1024]).unwrap();
    w.commit().unwrap();
}
#[test]
fn explicit_floor_preserves_named_copy_base_then_reports_expiry() {
    let d = Dir::new();
    let e = Environment::create(&d.0).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"db", DatabaseKind::Kv).unwrap();
    w.put(b"db", b"key", &[1; 1024]).unwrap();
    w.commit().unwrap();
    assert_eq!(e.history_availability().unwrap().floor_sequence, 0);
    let base = e.begin_read();
    e.retain_snapshot("copy-base", &base).unwrap();
    e.set_maintenance_policy(MaintenancePolicy::LegacyResumable {
        segment_threshold: usize::MAX,
        rewrite_budget_bytes: 1 << 20,
        reclaim: ResumableReclaimBudget {
            mark_units: 256,
            unlink_files: 8,
            ..Default::default()
        },
    })
    .unwrap();
    e.set_history_policy(HistoryPolicy::Retain {
        keep_last_commits: 3,
        budget: HistoryLifecycleBudget {
            directory_entries: 64,
            ..Default::default()
        },
    })
    .unwrap();
    for n in 2..=24 {
        put(&e, n);
        assert_eq!(base.get(b"db", b"key").unwrap().unwrap()[0], 1);
    }
    assert!(e.history_availability().unwrap().floor_sequence <= 1);
    let mut delta = Vec::new();
    e.dump_incremental(1, &mut delta).unwrap();
    e.release_snapshot("copy-base").unwrap();
    drop(base);
    for n in 25..=50 {
        put(&e, n);
    }
    let info = e.history_availability().unwrap();
    assert!(info.floor_sequence > 1, "{info:?}");
    assert!(matches!(
        e.dump_incremental(1, &mut Vec::new()),
        Err(Error::InvalidInput(
            "incremental base expired by explicit history retention policy"
        ))
    ));
    assert_eq!(e.begin_read().get(b"db", b"key").unwrap().unwrap()[0], 50);
    drop(e);
    let e = Environment::open(&d.0).unwrap();
    assert!(e.history_availability().unwrap().floor_sequence > 1);
}

#[test]
fn idle_steps_resume_bounded_orphan_discovery_without_fake_commits() {
    let d = Dir::new();
    let e = Environment::create(&d.0).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"db", DatabaseKind::Kv).unwrap();
    w.put(b"db", b"key", b"stable").unwrap();
    w.commit().unwrap();
    e.set_maintenance_policy(MaintenancePolicy::LegacyResumable {
        segment_threshold: usize::MAX,
        rewrite_budget_bytes: 1 << 20,
        reclaim: ResumableReclaimBudget {
            mark_units: 256,
            unlink_files: 8,
            ..Default::default()
        },
    })
    .unwrap();
    e.set_history_policy(HistoryPolicy::KeepAll).unwrap();
    for n in 0..130 {
        fs::write(
            d.0.join("segments")
                .join(format!(".prepare-history-{n:08x}.tmp")),
            b"orphan",
        )
        .unwrap();
    }
    let unknown = d.0.join("segments/operator.keep");
    fs::write(&unknown, b"keep").unwrap();
    let checkpoint = e.file_checkpoint().unwrap();
    let sequence = e.begin_read().sequence();
    let mut previous = e.maintenance_status().unwrap();
    for _ in 0..40 {
        let status = e.step_history_maintenance().unwrap();
        assert!(
            status.reclaimed_files - previous.reclaimed_files <= 8,
            "{status:?}"
        );
        assert!(
            status.scanned_directory_entries - previous.scanned_directory_entries <= 64,
            "{status:?}"
        );
        assert_eq!(status.last_reclaim_error, None, "{status:?}");
        previous = status;
    }
    assert!(previous.reclaimed_files >= 130, "{previous:?}");
    assert!(unknown.exists());
    assert_eq!(e.file_checkpoint().unwrap(), checkpoint);
    assert_eq!(e.begin_read().sequence(), sequence);
    assert_eq!(
        e.begin_read().get(b"db", b"key").unwrap(),
        Some(b"stable".as_slice())
    );
}

#[test]
fn finite_retention_rotates_data_windows_and_idle_orphans_after_forward_churn() {
    let d=Dir::new();let e=Environment::create(&d.0).unwrap();
    let mut w=e.begin_write().unwrap();w.create_database(b"db",DatabaseKind::Kv).unwrap();
    w.put(b"db",b"key",&[1;12000]).unwrap();w.commit().unwrap();
    let old=e.begin_read();
    e.set_maintenance_policy(MaintenancePolicy::LegacyResumable {
        segment_threshold:4,rewrite_budget_bytes:128<<10,
        reclaim:ResumableReclaimBudget{mark_units:256,unlink_files:4,..Default::default()}
    }).unwrap();
    e.set_history_policy(HistoryPolicy::Retain {keep_last_commits:3,
        budget:HistoryLifecycleBudget{generations:1,mark_units:1,directory_entries:2,unlink_files:1}
    }).unwrap();
    for n in 2..=32u8 {let mut w=e.begin_write().unwrap();w.put(b"db",b"key",&[n;12000]).unwrap();w.commit().unwrap();}
    // This exact real asset was created long AFTER the first cutoff; retaining
    // that cutoff forever leaves it permanently ineligible for retirement.
    let m=ulmdb_storage::read_current(&d.0).unwrap();
    let late=ulmdb_storage::segment_path(&d.0,m.segments.last().unwrap());assert!(late.exists());drop(m);
    for n in 33..=96u8 {let mut w=e.begin_write().unwrap();w.put(b"db",b"key",&[n;12000]).unwrap();w.commit().unwrap();
        assert_eq!(old.get(b"db",b"key").unwrap().unwrap()[0],1);}
    let status=e.maintenance_status().unwrap();
    assert!(status.resumable_windows_completed>=3,"data windows must finish independently: {status:?}");
    assert!(status.reclaimed_files>0,"{status:?}");
    assert!(!late.exists(),"post-first-cutoff segment must eventually reclaim: {status:?}");
    let mut temps=Vec::new();for n in 0..32 {let p=d.0.join("segments").join(format!(".prepare-after-churn-{n:08x}.tmp"));fs::write(&p,b"orphan").unwrap();temps.push(p);}
    let cp=e.file_checkpoint().unwrap();let sequence=e.begin_read().sequence();
    for _ in 0..400 {e.step_history_maintenance().unwrap();if temps.iter().all(|p|!p.exists()){break;}}
    assert!(temps.iter().all(|p|!p.exists()),"idle fresh windows must drain post-forward orphans: {:?}",e.maintenance_status().unwrap());
    assert_eq!(e.file_checkpoint().unwrap(),cp);assert_eq!(e.begin_read().sequence(),sequence);
    assert_eq!(e.begin_read().get(b"db",b"key").unwrap().unwrap()[0],96);
}
