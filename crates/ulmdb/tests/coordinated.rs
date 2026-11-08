#![cfg(target_os = "linux")]

use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, BufReader, Write},
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};
use ulmdb::{
    CoordinatedEnvironment, DatabaseKind, Environment, Error, PublicationMode, ReadTxn, backup,
};

const DEADLINE: Duration = Duration::from_secs(8);
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-coordinated-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn database(&self, mode: PublicationMode) -> PathBuf {
        let path = self.0.join(format!("{mode:?}"));
        let env = Environment::create_with_publication(&path, mode).unwrap();
        let mut write = env.begin_write().unwrap();
        write.create_database(b"records", DatabaseKind::Kv).unwrap();
        write.put(b"records", b"key", b"old").unwrap();
        write.put(b"records", b"counter", b"0").unwrap();
        write.commit().unwrap();
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Every worker execs a fresh copy of this test binary and opens its own handles.
/// Pipe deadlines make a locking regression fail without hanging the test suite.
struct Worker {
    child: Child,
    input: ChildStdin,
    output: Receiver<String>,
    transcript: Vec<String>,
}
impl Worker {
    fn spawn(path: &Path, role: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "coordinated_process_worker", "--nocapture"])
            .env("ULMDB_COORDINATED_TEST_PATH", path)
            .env("ULMDB_COORDINATED_TEST_ROLE", role)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, output) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            output,
            transcript: Vec::new(),
        }
    }
    fn send(&mut self, command: &str) {
        writeln!(self.input, "{command}").unwrap();
        self.input.flush().unwrap();
    }
    fn receive(&mut self) -> String {
        let start = Instant::now();
        loop {
            let remaining = DEADLINE.saturating_sub(start.elapsed());
            let line = self.output.recv_timeout(remaining).unwrap_or_else(|error| {
                panic!(
                    "worker blocked or exited: {error}; transcript: {:?}",
                    self.transcript
                )
            });
            self.transcript.push(line.clone());
            if let Some((_, message)) = line.split_once("M1 ") {
                return message.to_owned();
            }
        }
    }
    fn expect(&mut self, expected: &str) {
        assert_eq!(
            self.receive(),
            expected,
            "worker transcript: {:?}",
            self.transcript
        );
    }
    fn wait_success(&mut self) {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "worker failed: {:?}", self.transcript);
                return;
            }
            assert!(start.elapsed() < DEADLINE, "worker teardown blocked");
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn close(mut self) {
        self.send("close");
        self.expect("closed");
        self.wait_success();
    }
    fn kill(mut self) {
        self.child.kill().unwrap();
        let status = self.child.wait().unwrap();
        assert!(
            !status.success(),
            "dead-pin control unexpectedly exited normally"
        );
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

fn signal(message: &str) {
    println!("M1 {message}");
    std::io::stdout().flush().unwrap();
}
fn command(input: &mut impl BufRead) -> String {
    let mut line = String::new();
    assert_ne!(input.read_line(&mut line).unwrap(), 0, "parent pipe closed");
    line.trim().to_owned()
}
fn increment(env: &CoordinatedEnvironment) {
    env.update(|write| {
        let bytes = write.get(b"records", b"counter")?.unwrap();
        let number: u64 = std::str::from_utf8(bytes).unwrap().parse().unwrap();
        write.put(b"records", b"counter", (number + 1).to_string().as_bytes())
    })
    .unwrap();
}
fn counter(read: &ReadTxn) -> u64 {
    std::str::from_utf8(read.get(b"records", b"counter").unwrap().unwrap())
        .unwrap()
        .parse()
        .unwrap()
}
fn old_files(path: &Path) -> BTreeSet<PathBuf> {
    ulmdb_storage::read_current(path)
        .unwrap()
        .segments
        .iter()
        .map(|entry| ulmdb_storage::segment_path(path, entry))
        .collect()
}
fn maintain(env: &CoordinatedEnvironment) {
    for _ in 0..3 {
        env.update(|write| write.put(b"records", b"key", b"new"))
            .unwrap();
    }
    // Two new inventories rotate PREVIOUS beyond the old reader's generation.
    env.compact().unwrap();
    env.prune_inventory().unwrap();
    env.compact().unwrap();
    env.reclaim().unwrap();
}
fn case_pass(test: &str, mode: PublicationMode) {
    let backend = match mode {
        PublicationMode::Legacy => "file-legacy",
        PublicationMode::Cohort => "file-cohort",
        PublicationMode::Indexed => "file-indexed",
        _ => unreachable!("coordinated journey only qualifies file backends"),
    };
    println!("CASE_PASS test={test} backend={backend}");
}

#[test]
fn coordinated_process_worker() {
    let Ok(role) = std::env::var("ULMDB_COORDINATED_TEST_ROLE") else {
        return;
    };
    let path = PathBuf::from(std::env::var_os("ULMDB_COORDINATED_TEST_PATH").unwrap());
    let mut input = std::io::stdin().lock();
    if role == "cold-open" {
        signal("waiting-to-open");
        assert_eq!(command(&mut input), "open");
        let _env = CoordinatedEnvironment::open(&path).unwrap();
        signal("opened");
        assert_eq!(command(&mut input), "close");
        signal("closed");
        return;
    }
    let env = CoordinatedEnvironment::open(&path).unwrap();
    match role.as_str() {
        "capture" => {
            let old = env.begin_read().unwrap();
            let borrowed = old.get(b"records", b"key").unwrap().unwrap();
            assert_eq!(borrowed, b"old");
            signal("captured-old");
            assert_eq!(command(&mut input), "refresh");
            assert_eq!(borrowed, b"old");
            assert_eq!(
                env.begin_read().unwrap().get(b"records", b"key").unwrap(),
                Some(b"new".as_slice())
            );
            signal("refreshed");
        }
        "increment" => {
            signal("ready");
            assert_eq!(command(&mut input), "start");
            for _ in 0..12 {
                increment(&env);
            }
            signal("incremented");
        }
        "maintain" => {
            signal("ready");
            assert_eq!(command(&mut input), "maintain");
            maintain(&env);
            signal("maintained");
            assert_eq!(command(&mut input), "reclaim");
            let report = env.reclaim().unwrap();
            signal(&format!("reclaimed {}", report.removed_bytes));
        }
        "pin" => {
            let read = ReadTxn::open_coordinated_snapshot(&path).unwrap();
            let borrowed = read.get(b"records", b"key").unwrap().unwrap();
            assert_eq!(borrowed, b"old");
            drop(env);
            signal(&format!("pinned {}", read.generation()));
            // The parent kills this process; destructors must not release its pin.
            assert_eq!(command(&mut input), "unexpected-clean-close");
            panic!("dead-pin control must terminate the process");
        }
        "recursion" => {
            let other = CoordinatedEnvironment::open(&path).unwrap();
            env.update(|_| {
                assert!(matches!(other.update(|_| Ok(())), Err(Error::Busy)));
                Ok(())
            })
            .unwrap();
            signal("recursion-rejected");
        }
        "exclusive-owner" => {
            let exclusive = Environment::open(&path).unwrap();
            assert!(matches!(env.update(|_| Ok(())), Err(Error::Busy)));
            drop(exclusive);
            increment(&env);
            signal("exclusive-owner-rejected");
        }
        _ => panic!("unknown worker role {role}"),
    }
    assert_eq!(command(&mut input), "close");
    signal("closed");
}

#[test]
fn long_lived_wrappers_refresh_reads_and_alternate_updates() {
    let directory = Directory::new();
    for mode in MODES {
        let path = directory.database(mode);
        let first = CoordinatedEnvironment::open(&path).unwrap();
        let second = CoordinatedEnvironment::open(&path).unwrap();
        let old = first.begin_read().unwrap();
        let borrowed = old.get(b"records", b"counter").unwrap().unwrap();
        for expected in 1..=12 {
            let (writer, observer) = if expected % 2 == 0 {
                (&first, &second)
            } else {
                (&second, &first)
            };
            increment(writer);
            assert_eq!(counter(&observer.begin_read().unwrap()), expected);
            assert_eq!(
                counter(&ReadTxn::open_coordinated_snapshot(&path).unwrap()),
                expected
            );
            assert_eq!(borrowed, b"0");
        }
        assert!(matches!(
            first.update::<()>(|write| {
                write.put(b"records", b"counter", b"999")?;
                Err(Error::InvalidInput("deliberate coordinated abort"))
            }),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(counter(&second.begin_read().unwrap()), 12);
        increment(&second);
        assert_eq!(counter(&first.begin_read().unwrap()), 13);
        let interrupted = catch_unwind(AssertUnwindSafe(|| {
            first.update::<()>(|write| {
                write.put(b"records", b"counter", b"666")?;
                panic!("deliberate coordinated unwind")
            })
        }));
        assert!(interrupted.is_err());
        assert_eq!(counter(&second.begin_read().unwrap()), 13);
        increment(&second);
        assert_eq!(counter(&first.begin_read().unwrap()), 14);
        case_pass(
            "long_lived_wrappers_refresh_reads_and_alternate_updates",
            mode,
        );
    }
}

#[test]
fn independent_writers_serialize_without_lost_updates() {
    let directory = Directory::new();
    for mode in MODES {
        let path = directory.database(mode);
        let mut first = Worker::spawn(&path, "increment");
        first.expect("ready");
        // Capture APIs may return Busy during concurrent registration. Start
        // wrappers sequentially, then exercise simultaneous writer acquisition.
        let mut second = Worker::spawn(&path, "increment");
        second.expect("ready");
        first.send("start");
        second.send("start");
        first.expect("incremented");
        second.expect("incremented");
        assert_eq!(
            counter(&ReadTxn::open_coordinated_snapshot(&path).unwrap()),
            24
        );
        first.close();
        second.close();
        case_pass("independent_writers_serialize_without_lost_updates", mode);
    }
}

#[test]
fn simultaneous_independent_opens_succeed_without_maintenance() {
    let directory = Directory::new();
    for mode in MODES {
        let path = directory.database(mode);
        for _ in 0..8 {
            let mut first = Worker::spawn(&path, "cold-open");
            let mut second = Worker::spawn(&path, "cold-open");
            first.expect("waiting-to-open");
            second.expect("waiting-to-open");
            first.send("open");
            second.send("open");
            first.expect("opened");
            second.expect("opened");
            first.close();
            second.close();
        }
        case_pass(
            "simultaneous_independent_opens_succeed_without_maintenance",
            mode,
        );
    }
}

#[test]
fn independent_capture_does_not_wait_for_private_writer_and_pins_old_bytes() {
    let directory = Directory::new();
    for mode in MODES {
        let path = directory.database(mode);
        let env = CoordinatedEnvironment::open(&path).unwrap();
        let mut worker = env
            .update(|write| {
                write.put(b"records", b"key", b"new")?;
                let mut worker = Worker::spawn(&path, "capture");
                // The child must open and capture old bytes while this closure
                // still owns the live native writer and its unpublished changes.
                worker.expect("captured-old");
                Ok(worker)
            })
            .unwrap();
        worker.send("refresh");
        worker.expect("refreshed");
        assert_eq!(
            ReadTxn::open_coordinated_snapshot(&path)
                .unwrap()
                .get(b"records", b"key")
                .unwrap(),
            Some(b"new".as_slice())
        );
        worker.close();
        case_pass(
            "independent_capture_does_not_wait_for_private_writer_and_pins_old_bytes",
            mode,
        );
    }
}

#[test]
fn snapshot_outlives_native_or_coordinated_owner_and_exports_after_foreign_maintenance() {
    for mode in MODES {
        for coordinated_owner in [false, true] {
            let directory = Directory::new();
            let path = directory.database(mode);
            let files = old_files(&path);
            assert!(!files.is_empty());
            let read = if coordinated_owner {
                let env = CoordinatedEnvironment::open(&path).unwrap();
                let read = env.begin_read().unwrap();
                drop(env);
                read
            } else {
                let env = Environment::open(&path).unwrap();
                let read = env.begin_read();
                drop(env);
                read
            };
            let borrowed = read.get(b"records", b"key").unwrap().unwrap();
            let mut worker = Worker::spawn(&path, "maintain");
            worker.expect("ready");
            worker.send("maintain");
            worker.expect("maintained");
            assert!(files.iter().all(|file| file.is_file()));
            assert_eq!(borrowed, b"old");
            let destination = directory.0.join(format!("export-{mode:?}"));
            let summary = backup::export_independent_snapshot(&path, &read, &destination).unwrap();
            assert_eq!(summary.generation, read.generation());
            let copied = Environment::open(&destination).unwrap();
            assert_eq!(
                copied.begin_read().get(b"records", b"key").unwrap(),
                Some(b"old".as_slice())
            );
            backup::verify(&copied).unwrap();
            drop(copied);
            assert_eq!(borrowed, b"old");
            drop(read);
            worker.send("reclaim");
            let reclaimed = worker.receive();
            assert!(
                reclaimed
                    .strip_prefix("reclaimed ")
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
                    > 0
            );
            assert!(files.iter().all(|file| !file.exists()));
            worker.close();
        }
        case_pass(
            "snapshot_outlives_native_or_coordinated_owner_and_exports_after_foreign_maintenance",
            mode,
        );
    }
}

#[test]
fn process_death_releases_its_snapshot_pin_for_reclaim() {
    let directory = Directory::new();
    for mode in MODES {
        let path = directory.database(mode);
        let generation = ulmdb_storage::read_current(&path).unwrap().generation;
        let files = old_files(&path);
        let mut worker = Worker::spawn(&path, "pin");
        worker.expect(&format!("pinned {generation}"));
        let env = CoordinatedEnvironment::open(&path).unwrap();
        maintain(&env);
        assert!(files.iter().all(|file| file.is_file()));
        worker.kill();
        assert!(env.reclaim().unwrap().removed_bytes > 0);
        assert!(files.iter().all(|file| !file.exists()));
        assert_eq!(
            env.begin_read().unwrap().get(b"records", b"key").unwrap(),
            Some(b"new".as_slice())
        );
        case_pass("process_death_releases_its_snapshot_pin_for_reclaim", mode);
    }
}

#[test]
fn recursion_and_existing_exclusive_native_owner_fail_promptly() {
    let directory = Directory::new();
    for mode in MODES {
        let path = directory.database(mode);
        for (role, result) in [
            ("recursion", "recursion-rejected"),
            ("exclusive-owner", "exclusive-owner-rejected"),
        ] {
            let mut worker = Worker::spawn(&path, role);
            worker.expect(result);
            worker.close();
        }
        case_pass(
            "recursion_and_existing_exclusive_native_owner_fail_promptly",
            mode,
        );
    }
}
