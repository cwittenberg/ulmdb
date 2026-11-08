//! Process-exit/returned-IO checks, not hardware power-loss simulation.
use ulmdb_platform::FileExt;
use super::support::*;
use crate::Error;
use crate::hybrid::journal::{Journal, WalLayout, wire};
use std::{path::Path, process::Command};
const COMMIT: [&str; 11] = [
    "hybrid.commit.before_append",
    "hybrid.commit.half_record",
    "hybrid.commit.after_append",
    "hybrid.commit.before_wal_sync",
    "hybrid.commit.after_wal_sync",
    "hybrid.commit.before_witness",
    "hybrid.commit.half_witness",
    "hybrid.commit.after_witness",
    "hybrid.commit.before_witness_sync",
    "hybrid.commit.after_witness_sync",
    "hybrid.commit.before_ack",
];
const HEAL: [&str; 3] = [
    "hybrid.recovery.before_heal",
    "hybrid.recovery.after_heal",
    "hybrid.recovery.after_heal_sync",
];
const TRIM: [&str; 3] = [
    "hybrid.recovery.before_clear",
    "hybrid.recovery.after_clear",
    "hybrid.recovery.after_clear_sync",
];
const READY: [&str; 3] = [
    "hybrid.recovery.before_ready_file_sync",
    "hybrid.recovery.after_ready_file_sync",
    "hybrid.recovery.after_ready_parent_sync",
];
fn child() {
    let wal = std::env::var_os("HYBRID_CHILD_WAL").expect("fault child path selected by parent");
    let action = std::env::var("HYBRID_CHILD_ACTION").unwrap();
    if action == "create" {
        let result = Journal::create_with_layout(
            Path::new(&wal),
            HEADER,
            WalLayout::PreSized {
                capacity: 16 * 1024 * 1024,
            },
        );
        if std::env::var_os("ULMDB_HYBRID_FAIL_AT").is_some() {
            assert!(matches!(result, Err(Error::Io(_))));
        } else {
            result.unwrap();
        }
    } else if action == "commit" {
        let (mut j, _) = Journal::open_checked(Path::new(&wal), |_| Ok(())).unwrap();
        let before = j.selected();
        let result = j.commit_checked(&small_record(&j), &Registry::default(), |_| Ok(()));
        if let Ok(point) = std::env::var("ULMDB_HYBRID_FAIL_AT") {
            let index = COMMIT.iter().position(|p| *p == point).unwrap();
            if index < 5 {
                assert!(matches!(result, Err(Error::Io(_))));
                assert!(!j.is_poisoned());
                assert_eq!(j.selected(), before);
            } else {
                assert!(matches!(result, Err(Error::OutcomeUnknown)));
                assert!(j.is_poisoned());
            }
        } else {
            result.unwrap();
        }
    } else {
        let result = Journal::open_checked(Path::new(&wal), |_| Ok(()));
        if std::env::var_os("ULMDB_HYBRID_FAIL_AT").is_some() {
            assert!(matches!(result, Err(Error::Io(_))));
        } else {
            result.unwrap();
        }
    }
}
fn child_run(wal: &Path, action: &str, point: Option<&str>, exit: bool, trace: &Path) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "hybrid::tests::presized_fault_cases::all_commit_cuts_and_io_errors_keep_exact_authoritative_cut",
            "--nocapture",
        ])
        .env("HYBRID_CHILD_WAL", wal)
        .env("HYBRID_CHILD_ACTION", action)
        .env("ULMDB_HYBRID_TRACE", trace);
    if let Some(point) = point {
        command.env(
            if exit {
                "ULMDB_HYBRID_EXIT_AT"
            } else {
                "ULMDB_HYBRID_FAIL_AT"
            },
            point,
        );
    }
    let output = command.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(if exit { 86 } else { 0 }),
        "{point:?} stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn initial(wal: &Path) {
    let mut j = Journal::create_with_layout(
        wal,
        HEADER,
        WalLayout::PreSized {
            capacity: 16 * 1024 * 1024,
        },
    )
    .unwrap();
    j.commit_checked(&small_record(&j), &Registry::default(), |_| Ok(()))
        .unwrap();
}
#[test]
fn all_commit_cuts_and_io_errors_keep_exact_authoritative_cut() {
    if std::env::var_os("HYBRID_CHILD_WAL").is_some() {
        child();
        return;
    }
    for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
        for exit in [false, true] {
            for (index, point) in COMMIT.iter().enumerate() {
                let t = Temp::new(base, "commit-cut");
                initial(&t.wal());
                child_run(&t.wal(), "commit", Some(point), exit, &t.0.join("trace"));
                let (j, _) = Journal::open_checked(&t.wal(), |p| {
                    assert_eq!(p.root().next_chunk, 2);
                    Ok(())
                })
                .unwrap();
                assert_eq!(
                    j.selected().lsn,
                    if index >= 6 { 2 } else { 1 },
                    "{point} exit{exit}"
                );
            }
        }
    }
}
#[test]
fn complete_trace_is_eleven_ordered_points() {
    let t = Temp::new(Path::new("/tmp"), "trace");
    initial(&t.wal());
    let trace = t.0.join("trace");
    child_run(&t.wal(), "commit", None, false, &trace);
    let lines = std::fs::read_to_string(trace).unwrap();
    let actual: Vec<_> = lines
        .lines()
        .filter(|s| s.starts_with("hybrid.commit."))
        .collect();
    assert_eq!(actual, COMMIT);
}
#[test]
fn recovery_cuts_never_mutate_before_image_validation() {
    for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
        for exit in [false, true] {
            for mode in [0, 1, 2] {
                let points: &[&str] = match mode {
                    0 => &HEAL,
                    1 => &TRIM,
                    _ => &READY,
                };
                for point in points {
                    let t = Temp::new(base, "recover-cut");
                    initial(&t.wal());
                    if mode == 0 {
                        child_run(
                            &t.wal(),
                            "commit",
                            Some(COMMIT[6]),
                            true,
                            &t.0.join("prepare-trace"),
                        );
                    } else if mode == 1 {
                        let f = std::fs::OpenOptions::new()
                            .read(true)
                            .write(true)
                            .open(t.wal())
                            .unwrap();
                        let at = crate::hybrid::recovery::inspect(&f)
                            .unwrap()
                            .selected
                            .receipt
                            .end()
                            .unwrap();
                        f.write_all_at(b"private orphan", at).unwrap();
                    }
                    child_run(&t.wal(), "open", Some(point), exit, &t.0.join("trace"));
                    let (j, _) = Journal::open_checked(&t.wal(), |_| Ok(())).unwrap();
                    assert_eq!(j.selected().lsn, if mode == 0 { 2 } else { 1 });
                }
            }
        }
    }
}
#[test]
fn torn_genesis_without_successor_is_corruption() {
    let t = Temp::new(Path::new("/tmp"), "torn-genesis");
    drop(
        Journal::create_with_layout(
            &t.wal(),
            HEADER,
            WalLayout::PreSized {
                capacity: 16 * 1024 * 1024,
            },
        )
        .unwrap(),
    );
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(t.wal())
        .unwrap();
    f.write_all_at(&[1], wire::WITNESS_OFFSETS[0]).unwrap();
    drop(f);
    assert!(Journal::open_checked(&t.wal(), |_| Ok(())).is_err());
}

#[test]
fn creation_cuts_pay_zero_fill_and_never_publish_nonempty_state() {
    const CREATE: [&str; 7] = [
        "hybrid.create.before_zero_fill",
        "hybrid.create.after_zero_fill",
        "hybrid.create.before_header",
        "hybrid.create.after_header",
        "hybrid.create.after_witnesses",
        "hybrid.create.after_file_sync",
        "hybrid.create.after_parent_sync",
    ];
    for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
        for exit in [false, true] {
            for point in CREATE {
                let t = Temp::new(base, "presized-create-cut");
                child_run(&t.wal(), "create", Some(point), exit, &t.0.join("trace"));
                let before = std::fs::read(t.wal()).unwrap();
                match Journal::open_checked(&t.wal(), |p| {
                    assert_eq!(p.selected.receipt.lsn, 0);
                    assert!(p.records.is_empty());
                    Ok(())
                }) {
                    Ok((j, _)) => {
                        assert_eq!(j.selected().lsn, 0);
                        assert_eq!(j.physical_bytes().unwrap(), 16 * 1024 * 1024);
                    }
                    Err(_) => assert_eq!(std::fs::read(t.wal()).unwrap(), before),
                }
            }
        }
    }
}
