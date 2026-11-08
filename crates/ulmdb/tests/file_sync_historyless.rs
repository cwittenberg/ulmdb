#![cfg(target_os = "linux")]
//! Valid full exports deliberately retain commit metadata without all history.
use std::{fs, path::PathBuf};
use ulmdb::{DatabaseKind, Environment, PublicationMode};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "sync-historyless-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn full_native_and_client_copy_exports_remain_verifiable_and_syncable_without_history() {
    let mut failures = vec![];
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let source = Directory::new();
        let env = Environment::create_with_publication(&source.0, mode).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"key", b"A").unwrap();
        w.commit().unwrap();
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", b"B").unwrap();
        w.commit().unwrap();
        let read = env.begin_read();
        assert_ne!(read.file_manifest().unwrap().previous, 0);
        for client_copy in [false, true] {
            let target = Directory::new();
            if client_copy {
                ulmdb::backup::export_independent_snapshot(&source.0, &read, &target.0).unwrap();
            } else {
                ulmdb::backup::export(&env, &target.0).unwrap();
            }
            let exported = Environment::open(&target.0).unwrap();
            ulmdb::backup::verify(&exported).unwrap();
            let manifest = ulmdb_storage::read_current(&target.0).unwrap();
            assert_ne!(manifest.previous, 0);
            assert!(ulmdb_storage::read_roots(&target.0).unwrap().1.is_none());
            assert!(!target.0.join("PREVIOUS").exists());
            assert_eq!(
                exported.begin_read().get(b"kv", b"key").unwrap(),
                Some(b"B".as_slice())
            );
            let before = fs::read(target.0.join("CURRENT")).unwrap();
            match exported.sync() {
                Ok(report) => {
                    assert!(report.previous_checkpoint.is_none());
                    assert_eq!(
                        report.selected_checkpoint,
                        exported.file_checkpoint().unwrap()
                    );
                    println!("HISTORYLESS_SYNC mode={mode:?} client_copy={client_copy} result=ok");
                }
                Err(error) => {
                    println!(
                        "HISTORYLESS_SYNC mode={mode:?} client_copy={client_copy} result={error:?}"
                    );
                    failures.push(format!("{mode:?}/client_copy={client_copy}: {error}"));
                }
            }
            assert_eq!(fs::read(target.0.join("CURRENT")).unwrap(), before);
            assert!(!target.0.join("PREVIOUS").exists());
            exported.begin_write().unwrap().abort();
        }
    }
    assert!(
        failures.is_empty(),
        "valid exported snapshots rejected by force sync: {failures:#?}"
    );
}
