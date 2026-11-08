//! Fault-build-only, owned subprocess controls. EIO is not a power-loss oracle.
#![cfg(all(target_os = "linux", feature = "fault-injection"))]
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use ulmdb_storage::{
    Arena, Durability as D, Manifest, PreparedPublication, SegmentTable, WriteStore,
    coordination::{Coordination, Limits},
    create_layout, new_identity, prepare_publication_with_sealed_data, publish, read_current,
};
struct Fixture(PathBuf);
impl Fixture {
    fn new(format: u16) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-policy-fault-{:032x}",
            new_identity().unwrap()
        ));
        create_layout(&path).unwrap();
        let mut m = Manifest::empty().unwrap();
        m.format_minor = format;
        if format == 4 {
            m.prepare_indexed(None).unwrap();
        }
        publish(&path, &m).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn policy(s: &str) -> D {
    match s {
        "full" => D::Full,
        "meta" => D::NoMetaSync,
        "none" => D::NoSync,
        _ => panic!("bad policy"),
    }
}
fn next(path: &Path, data: D) -> PreparedPublication {
    let mut old = read_current(path).unwrap();
    let table = SegmentTable::load(path, &old.segments).unwrap();
    if old.format_minor == 4 {
        old.certify_indexed_table(&table).unwrap();
    }
    let mut m = old.clone();
    m.previous = old.generation;
    m.generation = new_identity().unwrap();
    m.sequence += 1;
    m.parent = old.commit;
    m.commit = m.generation;
    let mut arena = Arena::new(table, old.next_segment).unwrap();
    arena.allocate(b"fault policy bytes", 8).unwrap();
    match m.format_minor {
        2 => {
            let (t, e, r) = arena.seal_with_durability(path, data).unwrap();
            m.segments = t.entries().into();
            m.next_segment = e.unwrap().id + 1;
            prepare_publication_with_sealed_data(path, &m, r).unwrap()
        }
        3 => {
            arena
                .seal_cohort_with_durability(path, &mut m, data)
                .unwrap()
                .1
        }
        4 => {
            arena
                .seal_indexed_with_durability(path, &mut m, &old, data)
                .unwrap()
                .1
        }
        _ => panic!("bad format"),
    }
}
#[test]
fn policy_process_worker() {
    let Some(path) = std::env::var_os("POLICY_WORK_PATH") else {
        return;
    };
    let path = PathBuf::from(path);
    if std::env::var_os("POLICY_CAPTURE_ROLE").is_some() {
        println!("POLICY_CAPTURE_ATTEMPT");
        std::io::stdout().flush().unwrap();
        let (_, capture, selection, pin) =
            Coordination::capture_directory(&path, Limits::default())
                .unwrap()
                .capture_previous()
                .unwrap()
                .unwrap();
        assert_eq!(pin.root().checkpoint(), selection.previous_checkpoint());
        drop(pin);
        drop(capture);
        println!("POLICY_CAPTURED");
        std::io::stdout().flush().unwrap();
        return;
    }
    let data = policy(&std::env::var("POLICY_DATA").unwrap());
    if std::env::var("POLICY_METADATA").is_ok_and(|value| value == "rollback") {
        let (selected, previous) = ulmdb_storage::read_roots(&path).unwrap();
        let mut previous = previous.unwrap();
        previous.previous = 0;
        previous.generation = new_identity().unwrap();
        if previous.format_minor == 4 {
            previous.prepare_indexed(None).unwrap();
        }
        let ready = ulmdb_storage::prepare_rollback_with_durability(
            &path,
            &previous,
            &ulmdb_storage::checkpoint_bytes(&selected).unwrap(),
            data,
        )
        .unwrap()
        .prepare_ready(None)
        .unwrap();
        ready.publish(D::Full).unwrap();
        println!("POLICY_PASS");
        return;
    }
    let metadata = policy(&std::env::var("POLICY_METADATA").unwrap());
    let state = if std::env::var_os("POLICY_CACHED").is_some() {
        let (_, c, p, s) = Coordination::capture_directory(&path, Limits::default())
            .unwrap()
            .capture_current_with_durability()
            .unwrap();
        drop(p);
        drop(c);
        Some(s)
    } else {
        None
    };
    let p = next(&path, data);
    let ready = p.prepare_ready(state.as_ref()).unwrap();
    if let Some(trace) = std::env::var_os("ULMDB_TRACE_FILE") {
        writeln!(
            fs::OpenOptions::new().append(true).open(trace).unwrap(),
            "TEST_READY"
        )
        .unwrap();
    }
    ready.publish(metadata).unwrap();
    println!("POLICY_PASS");
}
fn run(
    path: &Path,
    data: &str,
    metadata: &str,
    fail: Option<&str>,
    trace: &Path,
    strace: Option<&Path>,
) -> std::process::Output {
    run_with_timeout(
        path,
        data,
        metadata,
        fail,
        trace,
        strace,
        Duration::from_secs(15),
    )
}
fn run_with_timeout(
    path: &Path,
    data: &str,
    metadata: &str,
    fail: Option<&str>,
    trace: &Path,
    strace: Option<&Path>,
    timeout: Duration,
) -> std::process::Output {
    let exe = std::env::current_exe().unwrap();
    let mut command = if let Some(output) = strace {
        let mut c = Command::new("strace");
        c.args([
            "-f",
            "-yy",
            "-e",
            "trace=fsync,fdatasync,rename,renameat,openat",
            "-o",
        ])
        .arg(output)
        .arg(exe);
        c
    } else {
        Command::new(exe)
    };
    command
        .args(["--exact", "policy_process_worker", "--nocapture"])
        .env("POLICY_WORK_PATH", path)
        .env("POLICY_DATA", data)
        .env("POLICY_METADATA", metadata)
        .env("ULMDB_TRACE_FILE", trace)
        .env_remove("ULMDB_FAIL_AT")
        .env_remove("ULMDB_CRASH_AT")
        .env_remove("ULMDB_PAUSE_AT");
    if let Some(point) = fail {
        if let Some(pause) = point.strip_prefix("pause:") {
            command.env("ULMDB_PAUSE_AT", pause);
        } else {
            command.env("ULMDB_FAIL_AT", point);
        }
    }
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if start.elapsed() > timeout {
            let _ = Command::new("kill")
                .args(["-KILL", "--", &format!("-{}", child.id())])
                .status()
                .expect("kill owned process group");
            let out = child.wait_with_output().unwrap();
            panic!("owned policy worker timeout: {:?}", out);
        }
        thread::sleep(Duration::from_millis(5));
    }
}
fn orphan(path: &Path) {
    let mut a = Arena::new(SegmentTable::default(), 1).unwrap();
    a.allocate(b"fault policy bytes", 8).unwrap();
    a.seal_with_durability(path, D::NoSync).unwrap();
}
#[test]
fn omitted_fences_are_actual_policy_choices_and_ready_has_no_data_io() {
    for format in [2, 3, 4] {
        for data in ["full", "meta", "none"] {
            for metadata in ["full", "meta", "none"] {
                let f = Fixture::new(format);
                let trace = f.0.join("trace");
                let out = run(&f.0, data, metadata, None, &trace, None);
                assert!(
                    out.status.success(),
                    "{}",
                    String::from_utf8_lossy(&out.stderr)
                );
                let trace = fs::read_to_string(trace).unwrap();
                let (before, after) = trace.split_once("TEST_READY\n").unwrap();
                assert_eq!(
                    before.contains(if format == 2 {
                        "segment.before_file_sync"
                    } else {
                        "cohort.before_file_sync"
                    }),
                    data != "none"
                );
                assert_eq!(
                    after.contains("pointer.CURRENT.before_file_sync"),
                    metadata != "none"
                );
                assert_eq!(
                    after.contains("pointer.CURRENT.before_directory_sync"),
                    metadata == "full"
                );
                assert!(!after.contains("sync.before_file_sync"));
                assert!(!after.contains("descriptor."));
                assert!(!after.contains("cohort."));
                assert!(!after.contains("segment."));
            }
        }
    }
}
#[test]
fn known_unpublished_eio_keeps_old_selection_and_retry_usable() {
    for format in [2, 3, 4] {
        let points = if format == 2 {
            vec![
                "segment.before_file_sync",
                "descriptor.before_file_sync",
                "descriptor.before_directory_sync",
            ]
        } else {
            vec!["cohort.before_file_sync", "cohort.before_directory_sync"]
        };
        for point in points {
            let f = Fixture::new(format);
            let old = fs::read(f.0.join("CURRENT")).unwrap();
            let trace = f.0.join("failed");
            let out = run(&f.0, "full", "full", Some(point), &trace, None);
            assert!(!out.status.success(), "injector did not fail {point}");
            assert!(fs::read_to_string(trace).unwrap().contains(point));
            assert_eq!(fs::read(f.0.join("CURRENT")).unwrap(), old);
            let out = run(&f.0, "full", "full", None, &f.0.join("retry"), None);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(read_current(&f.0).unwrap().sequence, 1);
        }
    }
}
#[test]
fn inherited_repair_eio_happens_before_ready_and_before_selection() {
    for format in [2, 3, 4] {
        let f = Fixture::new(format);
        assert!(
            run(&f.0, "none", "none", None, &f.0.join("weak"), None)
                .status
                .success()
        );
        let old = fs::read(f.0.join("CURRENT")).unwrap();
        let trace = f.0.join("repair");
        let out = run(
            &f.0,
            "full",
            "full",
            Some("sync.before_file_sync"),
            &trace,
            None,
        );
        assert!(!out.status.success());
        let log = fs::read_to_string(trace).unwrap();
        assert!(log.contains("sync.before_file_sync"));
        assert!(!log.contains("TEST_READY"));
        assert_eq!(fs::read(f.0.join("CURRENT")).unwrap(), old);
        assert!(
            run(&f.0, "full", "full", None, &f.0.join("retry"), None)
                .status
                .success()
        );
    }
}
#[test]
fn reused_unpublished_nosync_segment_is_fenced_on_the_actual_inode() {
    let f = Fixture::new(2);
    orphan(&f.0);
    let old = fs::read(f.0.join("CURRENT")).unwrap();
    let trace = f.0.join("reuse-failed");
    let out = run(
        &f.0,
        "full",
        "full",
        Some("segment.before_reused_file_sync"),
        &trace,
        None,
    );
    assert!(!out.status.success());
    assert_eq!(fs::read(f.0.join("CURRENT")).unwrap(), old);
    assert!(
        fs::read_to_string(trace)
            .unwrap()
            .contains("segment.before_reused_file_sync")
    );
    let trace = f.0.join("reuse-success");
    assert!(
        run(&f.0, "full", "full", None, &trace, None)
            .status
            .success()
    );
    assert!(
        fs::read_to_string(trace)
            .unwrap()
            .contains("segment.after_reused_file_sync")
    );
}
#[test]
fn weak_to_strong_legacy_transition_adds_its_predecessor_directory_fence() {
    let f = Fixture::new(2);
    assert!(
        run(&f.0, "none", "none", None, &f.0.join("weak"), None)
            .status
            .success()
    );
    let trace = f.0.join("transition");
    assert!(
        run(&f.0, "full", "full", None, &trace, None)
            .status
            .success()
    );
    let log = fs::read_to_string(trace).unwrap();
    let a = log.find("pointer.PREVIOUS.after_rename").unwrap();
    let b = log
        .find("pointer.PREVIOUS.before_transition_directory_sync")
        .unwrap();
    let c = log.find("pointer.CURRENT.before_rename").unwrap();
    assert!(a < b && b < c);
}
#[test]
#[ignore = "explicit actual syscall evidence; requires strace"]
fn actual_syscalls_match_policy_matrix() {
    let root = PathBuf::from(
        std::env::var_os("ULMDB_POLICY_EVIDENCE").expect("retained evidence directory"),
    );
    fs::create_dir_all(&root).unwrap();
    for format in [2, 3, 4] {
        for data in ["full", "meta", "none"] {
            for metadata in ["full", "meta", "none"] {
                let f = Fixture::new(format);
                let name = format!("format{format}-{data}-{metadata}");
                let trace = root.join(format!("{name}.hooks"));
                let syscall = root.join(format!("{name}.syscalls"));
                let out = run(&f.0, data, metadata, None, &trace, Some(&syscall));
                fs::write(root.join(format!("{name}.stdout")), &out.stdout).unwrap();
                fs::write(root.join(format!("{name}.stderr")), &out.stderr).unwrap();
                assert!(out.status.success());
                let log = fs::read_to_string(syscall).unwrap();
                let data_fsync = log.lines().any(|line| {
                    line.contains("fsync(")
                        && (line.contains("-segment.tmp>") || line.contains(".cohort>"))
                });
                let descriptor_fsync = log
                    .lines()
                    .any(|line| line.contains("fsync(") && line.contains(".root>"));
                let current_fsync = log
                    .lines()
                    .any(|line| line.contains("fsync(") && line.contains("-CURRENT.tmp>"));
                assert_eq!(
                    data_fsync || descriptor_fsync,
                    data != "none",
                    "{name}: {log}"
                );
                assert_eq!(current_fsync, metadata != "none", "{name}: {log}");
            }
        }
    }
}

struct Paused {
    child: std::process::Child,
    lines: std::sync::mpsc::Receiver<String>,
}
impl Paused {
    fn spawn(path: &Path, pause: Option<&str>, capture: bool) -> Self {
        use std::io::{BufRead, BufReader};
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "policy_process_worker", "--nocapture"])
            .env("POLICY_WORK_PATH", path)
            .env("POLICY_DATA", "full")
            .env("POLICY_METADATA", "full")
            .env_remove("ULMDB_TRACE_FILE")
            .env_remove("ULMDB_FAIL_AT")
            .env_remove("ULMDB_CRASH_AT")
            .env_remove("ULMDB_PAUSE_AT");
        if let Some(p) = pause {
            cmd.env("ULMDB_PAUSE_AT", p);
        }
        if capture {
            cmd.env("POLICY_CAPTURE_ROLE", "1");
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let out = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(out).lines() {
                if tx.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        Self { child, lines: rx }
    }
    fn expect(&self, marker: &str) {
        let start = Instant::now();
        loop {
            let line = self.lines.recv_timeout(Duration::from_secs(10)).unwrap();
            if line.contains(marker) {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(10));
        }
    }
    fn finish(&mut self) {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(10));
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Paused {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
fn kernel_history_wait(path: &Path) {
    use std::os::unix::fs::MetadataExt;
    let inode = fs::metadata(path.join(".ulmdb-coordination/history"))
        .unwrap()
        .ino();
    let suffix = format!(":{inode} ");
    let start = Instant::now();
    loop {
        let locks = fs::read_to_string("/proc/locks").unwrap();
        if locks
            .lines()
            .any(|line| line.contains("->") && line.contains(&suffix))
        {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no actual kernel History waiter: {locks}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn actual_history_waiter_observes_complete_pair_after_selection_or_publisher_death() {
    for format in [2, 3, 4] {
        for die in [false, true] {
            let f = Fixture::new(format);
            assert!(
                run(&f.0, "none", "none", None, &f.0.join("weak"), None)
                    .status
                    .success()
            );
            let old = read_current(&f.0).unwrap().generation;
            let point = if format == 2 {
                "pointer.PREVIOUS.after_rename"
            } else {
                "pointer.CURRENT.before_rename"
            };
            let mut writer = Paused::spawn(&f.0, Some(point), false);
            writer.expect(&format!("PAUSED point={point}"));
            let mut capture = Paused::spawn(&f.0, None, true);
            capture.expect("POLICY_CAPTURE_ATTEMPT");
            kernel_history_wait(&f.0);
            assert!(
                capture
                    .lines
                    .recv_timeout(Duration::from_millis(30))
                    .is_err()
            );
            if die {
                writer.child.kill().unwrap();
                assert!(!writer.child.wait().unwrap().success());
            } else {
                writer
                    .child
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(b"x")
                    .unwrap();
                writer.expect("POLICY_PASS");
                writer.finish();
            }
            capture.expect("POLICY_CAPTURED");
            capture.finish();
            let roots = ulmdb_storage::read_roots(&f.0).unwrap();
            if die {
                assert_eq!(roots.0.generation, old);
            } else {
                assert_eq!(roots.1.unwrap().generation, old);
            }
        }
    }
}

#[test]
fn rollback_does_not_refence_user_assets_even_with_full_environment_metadata() {
    for format in [2, 3, 4] {
        let f = Fixture::new(format);
        assert!(
            run(&f.0, "full", "full", None, &f.0.join("A"), None)
                .status
                .success()
        );
        assert!(
            run(&f.0, "none", "none", None, &f.0.join("B"), None)
                .status
                .success()
        );
        let trace = f.0.join("rollback");
        let out = run(
            &f.0,
            "full",
            "rollback",
            Some("sync.before_file_sync"),
            &trace,
            None,
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let log = fs::read_to_string(trace).unwrap();
        assert!(!log.contains("sync.before_file_sync"));
        let (_, capture, pin, state) = Coordination::capture_directory(&f.0, Limits::default())
            .unwrap()
            .capture_current_with_durability()
            .unwrap();
        assert!(state.has_data_debt());
        assert!(state.is_weak());
        drop(pin);
        drop(capture);
    }
}

#[test]
fn intermediate_legacy_transition_fence_failure_retains_authoritative_weak_pair() {
    for point in [
        "pointer.PREVIOUS.before_transition_directory_sync",
        "pointer.PREVIOUS.after_transition_directory_sync",
    ] {
        let f = Fixture::new(2);
        assert!(
            run(&f.0, "none", "none", None, &f.0.join("weak"), None)
                .status
                .success()
        );
        let before = fs::read(f.0.join("CURRENT")).unwrap();
        let old = ulmdb_storage::read_roots(&f.0).unwrap();
        let out = run(
            &f.0,
            "full",
            "full",
            Some(point),
            &f.0.join("failed-transition"),
            None,
        );
        assert!(!out.status.success());
        assert_eq!(fs::read(f.0.join("CURRENT")).unwrap(), before);
        let still = ulmdb_storage::read_roots(&f.0).unwrap();
        assert_eq!(still.0.generation, old.0.generation);
        assert_eq!(still.1.unwrap().generation, old.1.unwrap().generation);
        assert!(
            run(
                &f.0,
                "full",
                "full",
                None,
                &f.0.join("retry-transition"),
                None
            )
            .status
            .success()
        );
    }
}

#[test]
#[ignore = "explicit owned trace-worker timeout control; requires strace"]
fn timed_out_tracer_cannot_leave_its_owned_worker_running() {
    let f = Fixture::new(2);
    let log = f.0.join("timed-out.syscalls");
    let failure = std::panic::catch_unwind(|| {
        run_with_timeout(
            &f.0,
            "full",
            "full",
            Some("pause:history.publish.acquired"),
            &f.0.join("timed-out.hooks"),
            Some(&log),
            Duration::from_millis(400),
        )
    })
    .expect_err("paused tracer must time out");
    let message = failure.downcast_ref::<String>().unwrap();
    assert!(
        message.contains("PAUSED point=history.publish.acquired"),
        "worker must reach the real held History gate: {message}"
    );
    let text = fs::read_to_string(&log).unwrap();
    let ids = text
        .lines()
        .filter_map(|line| line.split_whitespace().next()?.parse::<u32>().ok())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!ids.is_empty());
    for id in ids {
        let stat = PathBuf::from(format!("/proc/{id}/stat"));
        let start = Instant::now();
        loop {
            match fs::read_to_string(&stat) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Ok(value)
                    if value
                        .rsplit_once(") ")
                        .is_some_and(|(_, s)| s.starts_with('Z')) =>
                {
                    break;
                }
                Ok(_) => {
                    assert!(
                        start.elapsed() < Duration::from_secs(5),
                        "timed-out owned worker {id} still running"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("stat control {e}"),
            }
        }
    }
    assert_eq!(read_current(&f.0).unwrap().sequence, 0);
    assert!(
        run(&f.0, "full", "full", None, &f.0.join("retry"), None)
            .status
            .success()
    );
}
