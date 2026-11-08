#![cfg(feature = "fault-injection")]
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use ulmdb::{DatabaseKind, Environment, Error, OwnedFileCapture, PreviousError, PublicationMode};
use ulmdb_storage::coordination::Coordination;
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "file-capture-fault-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn seed(p: &Path, mode: PublicationMode) {
    let e = Environment::create_with_publication(p, mode).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"old").unwrap();
    w.commit().unwrap();
}
fn worker(p: &Path, role: &str, point: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "file_capture_fault_worker", "--nocapture"])
        .env("ULMDB_CAPTURE_PATH", p)
        .env("ULMDB_CAPTURE_ROLE", role)
        .env("ULMDB_FAIL_AT", point)
        .env_remove("ULMDB_CRASH_AT")
        .env_remove("ULMDB_PAUSE_AT")
        .env_remove("ULMDB_TRACE_FILE");
    c
}
fn success(out: Output) {
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
#[test]
fn file_capture_fault_worker() {
    let Some(p) = std::env::var_os("ULMDB_CAPTURE_PATH") else {
        return;
    };
    let p = PathBuf::from(p);
    let role = std::env::var("ULMDB_CAPTURE_ROLE").unwrap();
    let env = Environment::open(&p).unwrap();
    let head = fs::read(p.join("CURRENT")).unwrap();
    let checkpoint = env.file_checkpoint().unwrap();
    if role == "capture" {
        let count = fs::read_dir(p.join(".ulmdb-coordination/pins"))
            .unwrap()
            .count();
        let cap = Coordination::capture_directory(&p, Default::default()).unwrap();
        let current = OwnedFileCapture::current(&p, cap).unwrap();
        assert!(matches!(
            current.select_previous(),
            Err(PreviousError::Engine(Error::Io(_)))
        ));
        assert_eq!(
            fs::read_dir(p.join(".ulmdb-coordination/pins"))
                .unwrap()
                .count(),
            count
        );
        env.begin_write().unwrap().abort();
        return;
    }
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"key", b"new").unwrap();
    let point = std::env::var("ULMDB_FAIL_AT").unwrap();
    let legacy_descriptor =
        env.publication_mode() == PublicationMode::Legacy && point.starts_with("descriptor.");
    let ordinary = role != "explicit";
    let result = match role.as_str() {
        "commit" => w.commit().map(|_| ()),
        "ordinary" => w.prepare_for_commit().map(drop),
        "explicit" => w.prepare().map(drop),
        "final" => w.prepare_for_commit().unwrap().commit().map(|_| ()),
        _ => panic!("bad role"),
    };
    if result.is_ok() {
        println!("UNEXPECTED_PREPARATION_SUCCESS");
        panic!("requested failure was not exercised");
    }
    let poison = role == "final" || (legacy_descriptor && ordinary);
    assert!(
        if poison {
            matches!(result, Err(Error::OutcomeUnknown))
        } else {
            matches!(result, Err(Error::Io(_)))
        },
        "role={role} result={result:?}"
    );
    if poison {
        assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
        assert!(matches!(env.file_checkpoint(), Err(Error::OutcomeUnknown)));
    } else {
        env.begin_write().unwrap().abort();
        assert_eq!(env.file_checkpoint().unwrap(), checkpoint);
    }
    assert_eq!(fs::read(p.join("CURRENT")).unwrap(), head);
    assert_eq!(
        env.begin_read().get(b"kv", b"key").unwrap(),
        Some(b"old".as_slice())
    );
}
#[test]
fn ordinary_split_preparation_matches_commit_errors_and_keeps_explicit_prepare_distinct() {
    for mode in MODES {
        let points = if mode == PublicationMode::Legacy {
            vec!["segment.before_write", "descriptor.before_write"]
        } else {
            vec!["cohort.before_write"]
        };
        for point in points {
            for role in ["commit", "ordinary", "explicit"] {
                let d = Directory::new();
                seed(&d.0, mode);
                success(worker(&d.0, role, point).output().unwrap());
                let env = Environment::open(&d.0).unwrap();
                assert_eq!(env.begin_read().sequence(), 1);
            }
        }
    }
}
#[test]
fn ordinary_prepared_final_selection_retains_uncertain_poison_boundary() {
    for mode in MODES {
        let d = Directory::new();
        seed(&d.0, mode);
        success(
            worker(&d.0, "final", "pointer.CURRENT.before_write")
                .output()
                .unwrap(),
        );
    }
}
#[test]
fn failed_previous_capture_drops_pin_and_capture_without_poisoning_native_owner() {
    for mode in MODES {
        let d = Directory::new();
        seed(&d.0, mode);
        success(
            worker(&d.0, "capture", "history.capture.acquired")
                .output()
                .unwrap(),
        );
    }
}
#[test]
fn nonexistent_injector_does_not_falsely_pass_the_error_equivalence_oracle() {
    let d = Directory::new();
    seed(&d.0, PublicationMode::Legacy);
    let out = worker(&d.0, "ordinary", "does.not.exist").output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("UNEXPECTED_PREPARATION_SUCCESS"));
}
