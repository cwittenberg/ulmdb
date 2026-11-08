//! Bounded process exits/returned IO during first installation and cold recovery.
#![cfg(feature = "fault-injection")]
use std::{fs, path::PathBuf, process::Command};
use ulmdb::{
    DatabaseKind, Error,
    resident::{Options, RedoFormat},
};
use ulmdb_storage::resident::Config;
fn options() -> Options {
    Options {
        pool: Config {
            kv_pages: 16,
            tiny_pages: 16,
            max_reserved_bytes: 64 << 20,
            max_metadata_bytes: 64 << 20,
            max_views: 32,
        },
        wal_capacity: 1 << 20,
        graph_read_budget: 1_000_000,
        auto_collect_below: 0,
    }
}
const CREATE: [&str; 7] = [
    "resident2.create.before_zero",
    "resident2.create.after_zero",
    "resident2.create.after_header",
    "resident2.create.after_head0",
    "resident2.create.after_head1",
    "resident2.create.after_file_sync",
    "resident2.create.after_parent_sync",
];
const RECOVER: [&str; 9] = [
    "resident2.recovery.before_heal",
    "resident2.recovery.after_heal",
    "resident2.recovery.after_heal_sync",
    "resident2.recovery.before_clear",
    "resident2.recovery.after_clear",
    "resident2.recovery.after_clear_sync",
    "resident2.recovery.before_ready_file_sync",
    "resident2.recovery.after_ready_file_sync",
    "resident2.recovery.after_ready_parent_sync",
];
#[test]
fn installation_and_healing_cuts_keep_empty_or_complete_selected_state() {
    if let Some(path) = std::env::var_os("RESIDENT_LIFECYCLE_CHILD") {
        let create = std::env::var_os("RESIDENT_CREATE_CHILD").is_some();
        let r = if create {
            Environment::create_with_redo(path, options(), RedoFormat::CompactV2)
        } else {
            Environment::open(path, options())
        };
        assert!(matches!(r, Err(Error::Io(_))));
        println!("RESIDENT_LIFECYCLE_DONE");
        return;
    }
    for base in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for create in [true, false] {
            for exit in [false, true] {
                for point in if create {
                    CREATE.as_slice()
                } else {
                    RECOVER.as_slice()
                } {
                    let path = base.join(format!(
                        "resident-lifecycle-{:032x}",
                        ulmdb_storage::new_identity().unwrap()
                    ));
                    if !create {
                        let e =
                            Environment::create_with_redo(&path, options(), RedoFormat::CompactV2)
                                .unwrap();
                        let mut w = e.begin_write().unwrap();
                        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
                        w.put(b"kv", b"key", b"selected").unwrap();
                        w.commit().unwrap();
                        drop(e);
                        let p = path.join("RESIDENT.wal");
                        let mut b = fs::read(&p).unwrap();
                        b[131072] ^= 1;
                        fs::write(p, b).unwrap();
                    }
                    let mut c = Command::new("timeout");
                    c.args(["--signal=KILL", "30s"])
                        .arg(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "installation_and_healing_cuts_keep_empty_or_complete_selected_state",
                            "--nocapture",
                        ])
                        .env("RESIDENT_LIFECYCLE_CHILD", &path)
                        .env(
                            if exit {
                                "ULMDB_RESIDENT_EXIT_AT"
                            } else {
                                "ULMDB_RESIDENT_FAIL_AT"
                            },
                            point,
                        );
                    if create {
                        c.env("RESIDENT_CREATE_CHILD", "1");
                    }
                    let o = c.output().unwrap();
                    assert_eq!(
                        o.status.code(),
                        Some(if exit { 86 } else { 0 }),
                        "{point}: {} {}",
                        String::from_utf8_lossy(&o.stdout),
                        String::from_utf8_lossy(&o.stderr)
                    );
                    if !exit {
                        assert!(
                            String::from_utf8_lossy(&o.stdout).contains("RESIDENT_LIFECYCLE_DONE")
                        );
                    }
                    match Environment::open(&path, options()) {
                        Ok(e) => {
                            if create {
                                assert_eq!(e.sequence(), 0);
                                assert_eq!(e.begin_read().database_count(), 0);
                            } else {
                                assert_eq!(e.sequence(), 1);
                                assert_eq!(
                                    e.begin_read().get(b"kv", b"key").unwrap(),
                                    Some(b"selected".as_slice())
                                );
                            }
                        }
                        Err(Error::Corrupt(_) | Error::Io(_)) if create => {}
                        Err(e) => panic!("{point}: {e}"),
                    }
                    fs::remove_dir_all(path).unwrap();
                }
            }
        }
    }
}

#[path = "support/resident_borrowed.rs"]
mod borrowed_policy;
use borrowed_policy::Environment;
