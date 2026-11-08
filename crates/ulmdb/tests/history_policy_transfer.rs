#![cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::{fs, path::PathBuf};
use ulmdb::{
    DatabaseKind, Environment, HistoryLifecycleBudget, HistoryPolicy, MaintenancePolicy,
    MaintenanceTransfer, PublicationMode,
};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "history-policy-transfer-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn put(e: &Environment, byte: u8) {
    let mut w = e.begin_write().unwrap();
    w.put(b"db", b"key", &[byte; 1024]).unwrap();
    w.commit().unwrap();
    assert_eq!(e.maintenance_status().unwrap().last_reclaim_error, None);
}
fn resume(d: &Dir, transfer: MaintenanceTransfer) -> Environment {
    let e = Environment::open(&d.0).unwrap();
    e.set_maintenance_policy(MaintenancePolicy::Disabled)
        .unwrap();
    e.restore_maintenance(transfer).unwrap();
    e
}

#[test]
fn same_checkpoint_transfer_observes_external_policy_changes() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new();
        let e = Environment::create_with_publication(&d.0, mode).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"db", DatabaseKind::Kv).unwrap();
        w.put(b"db", b"key", &[1; 1024]).unwrap();
        w.commit().unwrap();
        for byte in 2..=24 {
            put(&e, byte);
        }
        let checkpoint = e.file_checkpoint().unwrap();
        let transfer = e.take_maintenance().unwrap();
        assert!(!transfer.has_pending_work().unwrap());
        drop(e);

        // This owner changes only the typed sidecar. The old transfer cannot
        // observe its in-memory notification, nor rely on a changed D.
        let external = Environment::open(&d.0).unwrap();
        external
            .set_history_policy(HistoryPolicy::Retain {
                keep_last_commits: 3,
                budget: HistoryLifecycleBudget {
                    generations: 8,
                    ..Default::default()
                },
            })
            .unwrap();
        assert_eq!(external.file_checkpoint().unwrap(), checkpoint);
        drop(external);

        let e = resume(&d, transfer);
        for byte in 25..=36 {
            put(&e, byte);
        }
        let finite = e.history_availability().unwrap();
        assert!(finite.floor_sequence > 0, "{finite:?}");
        let checkpoint = e.file_checkpoint().unwrap();
        let transfer = e.take_maintenance().unwrap();
        drop(e);

        let external = Environment::open(&d.0).unwrap();
        let keep_all = external.set_history_policy(HistoryPolicy::KeepAll).unwrap();
        assert_eq!(external.file_checkpoint().unwrap(), checkpoint);
        assert_eq!(keep_all.floor_sequence, finite.floor_sequence);
        drop(external);

        let e = resume(&d, transfer);
        for byte in 37..=48 {
            put(&e, byte);
            let info = e.history_availability().unwrap();
            assert_eq!(info.policy, HistoryPolicy::KeepAll);
            assert_eq!(info.floor_sequence, finite.floor_sequence);
        }
        assert_eq!(e.begin_read().get(b"db", b"key").unwrap().unwrap()[0], 48);
    }
}
