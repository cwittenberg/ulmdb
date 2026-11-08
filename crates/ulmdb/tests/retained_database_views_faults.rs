#![cfg(feature = "fault-injection")]
//! Actual subprocess I/O boundaries; process failure is not power-loss proof.
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use ulmdb::{
    DatabaseKind as K, DatabaseViewTransfer as T, Environment, Error, PublicationMode as M,
};
fn wait(mut child: std::process::Child) {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "worker failed: {status}");
            return;
        }
        if start.elapsed() > Duration::from_secs(15) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("owned retained-view worker timeout");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn retained_boundary_worker() {
    let Some(path) = std::env::var_os("ULMDB_RETAINED_DATABASE") else {
        return;
    };
    let e = Environment::open(&path).unwrap();
    let mut w = e.begin_write().unwrap();
    let a = w.open_database(b"A").unwrap();
    let v = w.open_database_view(&a).unwrap();
    let g = w.retain_database_views(&a, &[&v]).unwrap();
    let r = w.retained_database_view(&g, &v).unwrap();
    w.retained_view_put(&r, b"k", b"NEW").unwrap();
    w.create_database(b"B", K::Kv).unwrap();
    let b = w.open_database(b"B").unwrap();
    w.finalize_database_view_groups(
        &[],
        &[T {
            source: &r,
            destination: &b,
        }],
        &[],
        &[&b],
    )
    .unwrap();
    let role = std::env::var("ULMDB_RETAINED_ROLE").unwrap();
    if role == "explicit" {
        assert!(matches!(w.prepare(), Err(Error::Io(_))));
        e.begin_write().unwrap().abort();
    } else if role == "ordinary" {
        // Only Legacy descriptor preparation has the old conservative
        // OutcomeUnknown boundary. Cohort sealing is known unpublished I/O.
        if std::env::var("ULMDB_RETAINED_CONSERVATIVE").unwrap() == "1" {
            assert!(matches!(w.commit(), Err(Error::OutcomeUnknown)));
            assert!(matches!(e.begin_write(), Err(Error::OutcomeUnknown)));
        } else {
            assert!(matches!(w.commit(), Err(Error::Io(_))));
            e.begin_write().unwrap().abort();
        }
    } else {
        let p = w.prepare().unwrap();
        assert!(matches!(p.commit(), Err(Error::OutcomeUnknown)));
        assert!(matches!(e.begin_write(), Err(Error::OutcomeUnknown)));
    }
    assert_eq!(
        e.begin_read().get(b"A", b"k").unwrap(),
        Some(b"OLD".as_slice())
    );
    assert!(e.begin_read().open_database(b"B").is_err());
}
fn seed(p: &Path, m: M) {
    let e = Environment::create_with_publication(p, m).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"A", K::Kv).unwrap();
    w.put(b"A", b"k", b"OLD").unwrap();
    w.commit().unwrap();
}
#[test]
fn retained_root_transfer_preserves_existing_preparation_and_publication_error_boundaries() {
    let root = std::env::temp_dir().join(format!(
        "retained-fault-{}",
        ulmdb_storage::new_identity().unwrap()
    ));
    fs::create_dir(&root).unwrap();
    for (i, m) in [M::Legacy, M::Cohort, M::Indexed].into_iter().enumerate() {
        let prepare = if m == M::Legacy {
            "descriptor.before_write"
        } else {
            "cohort.before_write"
        };
        for (j, (role, point)) in [
            ("explicit", prepare),
            ("ordinary", prepare),
            ("selection", "pointer.CURRENT.before_rename"),
            ("selection", "pointer.CURRENT.after_rename"),
        ]
        .into_iter()
        .enumerate()
        {
            let p = root.join(format!("{i}-{j}"));
            seed(&p, m);
            let current = fs::read(p.join("CURRENT")).unwrap();
            let trace = root.join(format!("{i}-{j}.trace"));
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "retained_boundary_worker", "--nocapture"])
                .env("ULMDB_RETAINED_DATABASE", &p)
                .env("ULMDB_RETAINED_ROLE", role)
                .env(
                    "ULMDB_RETAINED_CONSERVATIVE",
                    if m == M::Legacy { "1" } else { "0" },
                )
                .env("ULMDB_FAIL_AT", point)
                .env("ULMDB_TRACE_FILE", &trace)
                .env_remove("ULMDB_CRASH_AT")
                .env_remove("ULMDB_PAUSE_AT")
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            wait(child);
            assert!(
                fs::read_to_string(&trace)
                    .unwrap()
                    .lines()
                    .any(|s| s == point)
            );
            let e = Environment::open(&p).unwrap();
            if point == "pointer.CURRENT.after_rename" {
                assert_ne!(fs::read(p.join("CURRENT")).unwrap(), current);
                assert_eq!(
                    e.begin_read().get(b"B", b"k").unwrap(),
                    Some(b"NEW".as_slice())
                );
            } else {
                assert_eq!(fs::read(p.join("CURRENT")).unwrap(), current);
                assert_eq!(
                    e.begin_read().get(b"A", b"k").unwrap(),
                    Some(b"OLD".as_slice())
                );
            }
            e.begin_write().unwrap().abort();
        }
    }
    fs::remove_dir_all(root).unwrap();
}
