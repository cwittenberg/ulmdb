#![cfg(all(target_os = "linux", feature = "fault-injection"))]
//! Real process schedules/interruption, never a power-loss durability claim.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use ulmdb::{
    DatabaseKind, Environment, Error, OwnedFileCapture, PublicationMode, ResidencyPolicy, backup,
};
use ulmdb_storage::coordination::Coordination;
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "copy-process-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn seed(path: &Path, mode: PublicationMode) -> Environment {
    let e = Environment::create_with_publication(path, mode).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"k", b"A").unwrap();
    w.put(b"kv", b"large", &vec![37; 8192]).unwrap();
    w.commit().unwrap();
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", b"k", b"B").unwrap();
    w.commit().unwrap();
    e
}
fn owned(path: &Path) -> OwnedFileCapture {
    OwnedFileCapture::current_for_copy(
        path,
        Coordination::capture_directory(path, Default::default()).unwrap(),
    )
    .unwrap()
}
#[test]
fn copy_worker() {
    let Some(source) = std::env::var_os("ULMDB_COPY_SOURCE") else {
        return;
    };
    let source = PathBuf::from(source);
    let target = PathBuf::from(std::env::var_os("ULMDB_COPY_TARGET").unwrap());
    let role = std::env::var("ULMDB_COPY_ROLE").unwrap();
    let result = match role.as_str() {
        "pair" | "historyless" => {
            let mut pair = None;
            for _ in 0..4 {
                match owned(&source).into_copy_pair() {
                    Ok(c) => {
                        pair = Some(c.admit(ResidencyPolicy::Demand).unwrap());
                        break;
                    }
                    Err(Error::Busy) => continue,
                    Err(e) => panic!("pair capture: {e:?}"),
                }
            }
            backup::export_file_pair(&pair.expect("bounded current retry"), &target).map(|r| {
                println!("COPY_REPORT normalization={:?} source_generation={} target_generation={} source_sequence={} target_sequence={} physical_files={} physical_bytes={}",
                    r.normalization,r.source_selected.generation,r.selected.generation,r.source_selected.sequence,r.selected.sequence,r.physical_files,r.physical_bytes);
            })
        }
        "compact" => backup::compact_file_copy(
            &owned(&source)
                .admit_copy_view(ResidencyPolicy::Demand)
                .unwrap(),
            &target,
        )
        .map(|_| ()),
        "empty" => {
            let mode = match ulmdb_storage::read_current(&source).unwrap().format_minor {
                2 => PublicationMode::Legacy,
                3 => PublicationMode::Cohort,
                _ => PublicationMode::Indexed,
            };
            backup::create_empty_file_copy(&target, mode).map(|_| ())
        }
        _ => panic!("role"),
    };
    match result {
        Ok(()) => println!("COPY_RESULT OK role={role}"),
        Err(Error::OutcomeUnknown) => println!("COPY_RESULT UNKNOWN role={role}"),
        Err(Error::Io(_)) => println!("COPY_RESULT IO role={role}"),
        Err(Error::Corrupt(_)) => println!("COPY_RESULT CORRUPT role={role}"),
        Err(e) => panic!("unexpected copy error: {e:?}"),
    }
}
struct Worker {
    child: Child,
    out: PathBuf,
    err: PathBuf,
}
impl Worker {
    fn spawn(
        dir: &Path,
        source: &Path,
        target: &Path,
        role: &str,
        setting: Option<(&str, &str)>,
    ) -> Self {
        let out = dir.join("stdout");
        let err = dir.join("stderr");
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "copy_worker", "--nocapture"])
            .env_remove("ULMDB_FAIL_AT")
            .env_remove("ULMDB_PAUSE_AT")
            .env_remove("ULMDB_CRASH_AT")
            .env_remove("ULMDB_TRACE_FILE")
            .env("ULMDB_COPY_SOURCE", source)
            .env("ULMDB_COPY_TARGET", target)
            .env("ULMDB_COPY_ROLE", role)
            .stdin(Stdio::piped())
            .stdout(fs::File::create(&out).unwrap())
            .stderr(fs::File::create(&err).unwrap());
        if let Some((k, v)) = setting {
            cmd.env(k, v);
        }
        Self {
            child: cmd.spawn().unwrap(),
            out,
            err,
        }
    }
    fn ready(&mut self, point: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if fs::read_to_string(&self.out)
                .unwrap()
                .contains(&format!("PAUSED point={point}"))
            {
                return;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "worker ended before ready: {}",
                self.logs()
            );
            assert!(Instant::now() < deadline, "ready timeout: {}", self.logs());
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn release(&mut self) {
        self.child.stdin.as_mut().unwrap().write_all(b"x").unwrap();
    }
    fn logs(&self) -> String {
        format!(
            "{}\n{}",
            fs::read_to_string(&self.out).unwrap(),
            fs::read_to_string(&self.err).unwrap()
        )
    }
    fn terminal(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                return s;
            }
            assert!(Instant::now() < deadline, "worker deadline {}", self.logs());
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
fn verify_output(path: &Path, role: &str) {
    if !path.join("CURRENT").exists() {
        return;
    }
    let e = Environment::open(path).unwrap();
    backup::verify(&e).unwrap();
    let expected = match role {
        "pair" | "historyless" => 2,
        "compact" => 1,
        _ => 0,
    };
    assert_eq!(e.begin_read().sequence(), expected);
    if role != "empty" {
        assert_eq!(
            e.begin_read().get(b"kv", b"k").unwrap(),
            Some(b"B".as_slice())
        );
    }
    if role == "historyless" {
        assert_eq!(e.begin_read().file_manifest().unwrap().previous, 0);
        assert!(matches!(
            ulmdb::ReadTxn::open_previous_coordinated_snapshot(path, ResidencyPolicy::Demand),
            Err(ulmdb::PreviousError::NoPrevious)
        ));
    } else {
        let p = ulmdb::ReadTxn::open_previous_coordinated_snapshot(path, ResidencyPolicy::Demand)
            .unwrap();
        assert_eq!(p.sequence(), if role == "pair" { 1 } else { 0 });
    }
    assert!(fs::metadata(path.join("CURRENT")).unwrap().len() <= 144);
}
#[test]
fn io_failures_and_process_interruptions_leave_absent_or_complete_strong_selection() {
    for mode in MODES {
        let d = Dir::new();
        let source = d.0.join("source");
        let e = seed(&source, mode);
        for role in ["pair", "compact", "empty"] {
            let mut points = vec![
                "copy.pair.before_previous",
                "copy.pair.after_previous",
                "copy.pair.before_current",
                "copy.pair.after_current",
                "pointer.CURRENT.before_write",
                "pointer.CURRENT.after_write",
                "pointer.CURRENT.before_file_sync",
                "pointer.CURRENT.after_file_sync",
                "pointer.CURRENT.before_rename",
                "pointer.CURRENT.after_rename",
                "pointer.CURRENT.before_directory_sync",
                "pointer.CURRENT.after_directory_sync",
            ];
            if mode == PublicationMode::Legacy && role != "empty" {
                points.extend([
                    "pointer.PREVIOUS.before_write",
                    "pointer.PREVIOUS.after_write",
                    "pointer.PREVIOUS.before_file_sync",
                    "pointer.PREVIOUS.after_file_sync",
                    "pointer.PREVIOUS.before_rename",
                    "pointer.PREVIOUS.after_rename",
                ]);
            }
            if role == "pair" {
                points.extend([
                    "copy.asset.before_open",
                    "copy.asset.before_sync",
                    "copy.asset.after_sync",
                    "copy.assets.before_directory_sync",
                    "copy.assets.after_directory_sync",
                    "copy.before_target_audit",
                    "copy.after_target_audit",
                ]);
            } else {
                points.extend(if mode == PublicationMode::Legacy {
                    vec![
                        "descriptor.before_write",
                        "descriptor.after_write",
                        "descriptor.before_file_sync",
                        "descriptor.after_file_sync",
                        "descriptor.before_directory_sync",
                        "descriptor.after_directory_sync",
                    ]
                } else {
                    vec![
                        "cohort.before_write",
                        "cohort.after_write",
                        "cohort.before_file_sync",
                        "cohort.after_file_sync",
                        "cohort.before_directory_sync",
                        "cohort.after_directory_sync",
                    ]
                });
                if role == "compact" {
                    points.extend([
                        "segment.before_write",
                        "segment.after_write",
                        "segment.before_file_sync",
                        "segment.after_file_sync",
                        "segment.before_rename",
                        "segment.after_rename",
                        "segment.before_segments_dir_sync",
                        "segment.after_segments_dir_sync",
                    ]);
                }
            }
            for (i, point) in points.into_iter().enumerate() {
                let case = d.0.join(format!("{role}-io-{i}"));
                fs::create_dir(&case).unwrap();
                let target = case.join("output");
                let before = fs::read(source.join("CURRENT")).unwrap();
                let mut worker = Worker::spawn(
                    &case,
                    &source,
                    &target,
                    role,
                    Some(("ULMDB_FAIL_AT", point)),
                );
                assert!(
                    worker.terminal().success(),
                    "{mode:?}/{role}/{point}: {}",
                    worker.logs()
                );
                let expected = if point.starts_with("pointer.CURRENT.")
                    || point == "copy.pair.after_current"
                {
                    "UNKNOWN"
                } else {
                    "IO"
                };
                assert!(
                    worker.logs().contains(&format!("COPY_RESULT {expected}")),
                    "{mode:?}/{role}/{point}: {}",
                    worker.logs()
                );
                assert_eq!(fs::read(source.join("CURRENT")).unwrap(), before);
                if !point.starts_with("pointer.CURRENT.") && point != "copy.pair.after_current" {
                    assert!(!target.join("CURRENT").exists());
                }
                if matches!(
                    point,
                    "pointer.CURRENT.after_rename"
                        | "pointer.CURRENT.before_directory_sync"
                        | "pointer.CURRENT.after_directory_sync"
                        | "copy.pair.after_current"
                ) {
                    assert!(target.join("CURRENT").exists());
                }
                verify_output(&target, role);
                println!(
                    "CASE_PASS io backend={mode:?} role={role} point={point} result={expected}"
                );
            }
            for (i, point) in [
                "copy.pair.after_previous",
                "pointer.CURRENT.after_rename",
                "pointer.CURRENT.after_directory_sync",
            ]
            .into_iter()
            .enumerate()
            {
                let case = d.0.join(format!("{role}-crash-{i}"));
                fs::create_dir(&case).unwrap();
                let target = case.join("output");
                let mut worker = Worker::spawn(
                    &case,
                    &source,
                    &target,
                    role,
                    Some(("ULMDB_CRASH_AT", point)),
                );
                assert_eq!(worker.terminal().code(), Some(86), "{}", worker.logs());
                if point == "copy.pair.after_previous" {
                    assert!(!target.join("CURRENT").exists());
                } else {
                    assert!(target.join("CURRENT").exists());
                }
                verify_output(&target, role);
                println!("CASE_PASS crash backend={mode:?} role={role} point={point}");
            }
        }
        // Neither failed copy nor uncertain target publication poisons source.
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"after", b"kept").unwrap();
        assert_eq!(w.commit().unwrap(), 3);
    }
}
#[test]
fn target_never_selects_p_and_source_writer_reclaim_continue_after_admission() {
    for mode in MODES {
        let d = Dir::new();
        let source = d.0.join("source");
        let target = d.0.join("output");
        let e = seed(&source, mode);
        let old = e.begin_read();
        let borrowed = old.get(b"kv", b"large").unwrap().unwrap();
        let (b, a) = ulmdb_storage::read_roots(&source).unwrap();
        let a = a.unwrap();
        let mut worker = Worker::spawn(
            &d.0,
            &source,
            &target,
            "pair",
            Some(("ULMDB_PAUSE_AT", "copy.pair.after_previous")),
        );
        worker.ready("copy.pair.after_previous");
        assert!(!target.join("CURRENT").exists());
        assert!(ulmdb::ReadTxn::open_coordinated_snapshot(&target).is_err());
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"k", b"C").unwrap();
        assert_eq!(w.commit().unwrap(), 3);
        e.compact().unwrap();
        e.prune_inventory().unwrap();
        e.reclaim().unwrap();
        assert!(ulmdb_storage::descriptor_path(&source, &a).exists());
        assert!(ulmdb_storage::descriptor_path(&source, &b).exists());
        assert_eq!(borrowed, vec![37; 8192]);
        assert_eq!(old.get(b"kv", b"k").unwrap(), Some(b"B".as_slice()));
        worker.release();
        assert!(worker.terminal().success(), "{}", worker.logs());
        verify_output(&target, "pair");
        assert_eq!(
            e.begin_read().get(b"kv", b"k").unwrap(),
            Some(b"C".as_slice())
        );
        println!("CASE_PASS paused-target backend={mode:?}");
    }
}
#[test]
fn history_capture_blocks_only_final_source_selection_and_maintenance() {
    use std::os::unix::fs::MetadataExt;
    for mode in MODES {
        let d = Dir::new();
        let source = d.0.join("source");
        let target = d.0.join("output");
        let e = std::sync::Arc::new(seed(&source, mode));
        let mut worker = Worker::spawn(
            &d.0,
            &source,
            &target,
            "pair",
            Some(("ULMDB_PAUSE_AT", "history.capture.acquired")),
        );
        worker.ready("history.capture.acquired");
        let coord = Coordination::open(
            &source,
            e.begin_read().file_manifest().unwrap().uuid,
            Default::default(),
        )
        .unwrap();
        assert!(matches!(coord.try_maintenance(), Err(Error::Busy)));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let writer_env = e.clone();
        let writer = std::thread::spawn(move || {
            let mut w = writer_env.begin_write().unwrap();
            w.put(b"kv", b"k", b"C").unwrap();
            let p = w.prepare().unwrap();
            ready_tx.send(()).unwrap();
            let r = p.commit();
            done_tx.send(r).unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let ino = fs::metadata(source.join(".ulmdb-coordination/history"))
            .unwrap()
            .ino();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let blocked = fs::read_to_string("/proc/locks").unwrap().lines().any(|l| {
                l.contains("->")
                    && l.split_whitespace()
                        .any(|w| w.ends_with(&format!(":{ino}")))
            });
            if blocked {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "no real blocked history publisher"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(done_rx.try_recv().is_err());
        // The pause hook would match a retry too; remove it by releasing each
        // actual rendezvous until this one publication/copy schedule completes.
        worker.release();
        assert_eq!(
            done_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap(),
            3
        );
        writer.join().unwrap();
        for _ in 0..8 {
            if worker.child.try_wait().unwrap().is_some() {
                break;
            }
            if let Some(input) = worker.child.stdin.as_mut() {
                let _ = input.write_all(b"xxxxxxxx");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(worker.terminal().success(), "{}", worker.logs());
        let out = Environment::open(&target).unwrap();
        let r = out.begin_read();
        assert!([2, 3].contains(&r.sequence()));
        assert_eq!(
            r.get(b"kv", b"k").unwrap(),
            Some(if r.sequence() == 2 {
                b"B".as_slice()
            } else {
                b"C".as_slice()
            })
        );
        let p =
            ulmdb::ReadTxn::open_previous_coordinated_snapshot(&target, ResidencyPolicy::Demand)
                .unwrap();
        assert_eq!(p.sequence() + 1, r.sequence());
        println!("CASE_PASS paused-capture backend={mode:?}");
    }
}
#[test]
fn target_namespace_replacement_before_and_after_current_has_exact_error_boundary() {
    for mode in MODES {
        let d = Dir::new();
        let source = d.0.join("source");
        let e = seed(&source, mode);
        for (i, point) in [
            "copy.pair.before_previous",
            "copy.pair.before_current",
            "pointer.CURRENT.after_rename",
            "copy.pair.after_current",
        ]
        .into_iter()
        .enumerate()
        {
            let case = d.0.join(format!("swap-{i}"));
            fs::create_dir(&case).unwrap();
            let target = case.join("output");
            let moved = case.join("moved");
            let before = fs::read(source.join("CURRENT")).unwrap();
            let mut worker = Worker::spawn(
                &case,
                &source,
                &target,
                "pair",
                Some(("ULMDB_PAUSE_AT", point)),
            );
            worker.ready(point);
            fs::rename(&target, &moved).unwrap();
            // A complete replacement layout makes accidental selector writes
            // succeed; merely leaving the path absent would mask the defect.
            ulmdb_storage::create_layout(&target).unwrap();
            worker.release();
            assert!(worker.terminal().success(), "{}", worker.logs());
            let pre = point == "copy.pair.before_previous" || point == "copy.pair.before_current";
            let expected = if pre { "CORRUPT" } else { "UNKNOWN" };
            assert!(
                worker.logs().contains(&format!("COPY_RESULT {expected}")),
                "{mode:?}/{point}: {}",
                worker.logs()
            );
            assert!(!target.join("CURRENT").exists());
            assert!(!target.join("PREVIOUS").exists());
            if pre {
                assert!(!moved.join("CURRENT").exists());
            } else {
                assert!(moved.join("CURRENT").exists());
                verify_output(&moved, "pair");
            }
            assert_eq!(fs::read(source.join("CURRENT")).unwrap(), before);
            println!("CASE_PASS namespace-swap backend={mode:?} point={point} result={expected}");
        }
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"after", b"kept").unwrap();
        assert_eq!(w.commit().unwrap(), 3);
    }
}
#[test]
fn genuine_historyless_normalization_has_known_preselection_errors_and_uncertain_current_errors() {
    for mode in MODES {
        let d = Dir::new();
        let original = d.0.join("original");
        let source = d.0.join("source");
        let e = seed(&original, mode);
        backup::export(&e, &source).unwrap();
        let m = ulmdb_storage::read_current(&source).unwrap();
        assert_ne!(m.previous, 0);
        assert!(ulmdb_storage::read_previous(&source).unwrap().is_none());
        let before = fs::read(source.join("CURRENT")).unwrap();
        let mut points = vec![
            "copy.asset.before_open",
            "copy.asset.before_sync",
            "copy.asset.after_sync",
            "copy.assets.before_directory_sync",
            "copy.assets.after_directory_sync",
            "copy.before_target_audit",
            "copy.after_target_audit",
            "copy.normalize.before_prepare",
            "copy.normalize.after_prepare",
            "copy.pair.before_previous",
            "copy.pair.after_previous",
            "copy.pair.before_current",
            "copy.pair.after_current",
            "pointer.CURRENT.before_write",
            "pointer.CURRENT.after_write",
            "pointer.CURRENT.before_file_sync",
            "pointer.CURRENT.after_file_sync",
            "pointer.CURRENT.before_rename",
            "pointer.CURRENT.after_rename",
            "pointer.CURRENT.before_directory_sync",
            "pointer.CURRENT.after_directory_sync",
        ];
        points.extend(if mode == PublicationMode::Legacy {
            vec![
                "descriptor.before_write",
                "descriptor.after_write",
                "descriptor.before_file_sync",
                "descriptor.after_file_sync",
                "descriptor.before_directory_sync",
                "descriptor.after_directory_sync",
            ]
        } else {
            vec![
                "cohort.before_write",
                "cohort.after_write",
                "cohort.before_file_sync",
                "cohort.after_file_sync",
                "cohort.before_directory_sync",
                "cohort.after_directory_sync",
            ]
        });
        for (i, point) in points.into_iter().enumerate() {
            let case = d.0.join(format!("historyless-io-{i}"));
            fs::create_dir(&case).unwrap();
            let target = case.join("output");
            let mut worker = Worker::spawn(
                &case,
                &source,
                &target,
                "historyless",
                Some(("ULMDB_FAIL_AT", point)),
            );
            assert!(
                worker.terminal().success(),
                "{mode:?}/{point}: {}",
                worker.logs()
            );
            let uncertain =
                point.starts_with("pointer.CURRENT.") || point == "copy.pair.after_current";
            let expected = if uncertain { "UNKNOWN" } else { "IO" };
            assert!(
                worker.logs().contains(&format!("COPY_RESULT {expected}")),
                "{mode:?}/{point}: {}",
                worker.logs()
            );
            if !uncertain {
                assert!(!target.join("CURRENT").exists());
            }
            if matches!(
                point,
                "pointer.CURRENT.after_rename"
                    | "pointer.CURRENT.before_directory_sync"
                    | "pointer.CURRENT.after_directory_sync"
                    | "copy.pair.after_current"
            ) {
                assert!(target.join("CURRENT").exists());
            }
            verify_output(&target, "historyless");
            assert_eq!(fs::read(source.join("CURRENT")).unwrap(), before);
            println!("CASE_PASS historyless-io backend={mode:?} point={point} result={expected}");
        }
        for (i, point) in [
            "copy.normalize.after_prepare",
            "copy.after_target_audit",
            "copy.pair.after_previous",
            "pointer.CURRENT.after_rename",
            "pointer.CURRENT.after_directory_sync",
        ]
        .into_iter()
        .enumerate()
        {
            let case = d.0.join(format!("historyless-crash-{i}"));
            fs::create_dir(&case).unwrap();
            let target = case.join("output");
            let mut worker = Worker::spawn(
                &case,
                &source,
                &target,
                "historyless",
                Some(("ULMDB_CRASH_AT", point)),
            );
            assert_eq!(worker.terminal().code(), Some(86), "{}", worker.logs());
            if point.starts_with("pointer.CURRENT.") {
                assert!(target.join("CURRENT").exists());
            } else {
                assert!(!target.join("CURRENT").exists());
            }
            verify_output(&target, "historyless");
            assert_eq!(fs::read(source.join("CURRENT")).unwrap(), before);
            println!("CASE_PASS historyless-crash backend={mode:?} point={point}");
        }
        // Actual report plus concurrent source writer/maintenance on the branch.
        let case = d.0.join("historyless-report");
        fs::create_dir(&case).unwrap();
        let target = case.join("output");
        let mut worker = Worker::spawn(
            &case,
            &source,
            &target,
            "historyless",
            Some(("ULMDB_PAUSE_AT", "copy.pair.before_current")),
        );
        worker.ready("copy.pair.before_current");
        assert!(!target.join("CURRENT").exists());
        let source_owner = Environment::open(&source).unwrap();
        let held = source_owner.begin_read();
        let borrowed = held.get(b"kv", b"large").unwrap().unwrap();
        let mut w = source_owner.begin_write().unwrap();
        w.put(b"kv", b"k", b"C").unwrap();
        assert_eq!(w.commit().unwrap(), 3);
        source_owner.compact().unwrap();
        source_owner.prune_inventory().unwrap();
        source_owner.reclaim().unwrap();
        worker.release();
        assert!(worker.terminal().success(), "{}", worker.logs());
        assert!(
            worker
                .logs()
                .contains("normalization=Some(UnavailablePredecessor)")
        );
        assert!(
            worker
                .logs()
                .contains(&format!("source_generation={}", m.generation))
        );
        verify_output(&target, "historyless");
        let out = Environment::open(&target).unwrap();
        let n = out.begin_read();
        let n = n.file_manifest().unwrap();
        assert_ne!(n.generation, m.generation);
        assert_eq!(
            (
                n.uuid,
                n.epoch,
                n.sequence,
                n.commit,
                n.parent,
                n.next_database
            ),
            (
                m.uuid,
                m.epoch,
                m.sequence,
                m.commit,
                m.parent,
                m.next_database
            )
        );
        assert_eq!(borrowed, vec![37; 8192]);
        assert_eq!(held.get(b"kv", b"k").unwrap(), Some(b"B".as_slice()));
        println!("CASE_PASS historyless-report backend={mode:?}");
    }
}
