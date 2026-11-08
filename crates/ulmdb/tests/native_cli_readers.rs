#![cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};
use ulmdb::{
    Address, CoordinatedEnvironment, DatabaseKind, Environment, Error, Prefix, PublicationMode,
    ReadTxn, ResidencyPolicy, backup,
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-cli-readers-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct OwnedChild(Option<Child>);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn run(command: &str, source: &Path, arguments: &[OsString]) -> Output {
    // The CLI does not create subprocesses. Own and reap precisely this child,
    // including timeout/panic paths, without affecting any other libtest worker.
    let mut child = OwnedChild(Some(
        Command::new(env!("CARGO_BIN_EXE_ulmdb"))
            .arg(command)
            .arg(source)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.0.as_mut().unwrap().try_wait().unwrap().is_some() {
            // Fixture outputs are bounded: scan has one row, duplicates two,
            // stats/check/export are fixed-size summaries, below pipe capacity.
            return child.0.take().unwrap().wait_with_output().unwrap();
        }
        assert!(Instant::now() < deadline, "CLI {command} exceeded deadline");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn success(command: &str, source: &Path, arguments: &[&str]) -> String {
    let arguments: Vec<_> = arguments.iter().map(OsString::from).collect();
    let output = run(command, source, &arguments);
    assert!(
        output.status.success(),
        "CLI {command}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn writer_busy(source: &Path) {
    assert!(matches!(Environment::open(source), Err(Error::Busy)));
    let output = run(
        "put",
        source,
        &["data".into(), "k".into(), "must-not-publish".into()],
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("database writer is busy"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn network() -> Prefix {
    Prefix::new(Address::V4(0xc000_0200), 24).unwrap()
}
fn committed_values(read: &ReadTxn) {
    assert_eq!(read.sequence(), 1);
    assert_eq!(read.get(b"data", b"k").unwrap(), Some(b"old".as_slice()));
    assert_eq!(
        read.exact_prefix(b"routes", network()).unwrap().unwrap().1,
        b"old-route"
    );
    assert_eq!(read.duplicate_count(b"dups", b"k").unwrap(), 2);
    assert!(matches!(
        read.get(b"unpublished", b"private"),
        Err(Error::MissingDatabase)
    ));
}

#[test]
fn file_cli_reads_and_copies_keep_the_real_writer_fenced() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let root = Directory::new();
        let source = root.0.join("source");
        let env = Environment::create_with_publication(&source, mode).unwrap();
        let mut seed = env.begin_write().unwrap();
        seed.create_database(b"data", DatabaseKind::Kv).unwrap();
        seed.create_database(b"routes", DatabaseKind::Ipv4).unwrap();
        seed.create_database(b"dups", DatabaseKind::Duplicates { width: None })
            .unwrap();
        seed.put(b"data", b"k", b"old").unwrap();
        seed.insert_prefix(b"routes", network(), b"old-route")
            .unwrap();
        seed.insert_duplicate(b"dups", b"k", b"a").unwrap();
        seed.insert_duplicate(b"dups", b"k", b"b").unwrap();
        assert_eq!(seed.commit().unwrap(), 1);
        let checkpoint = env.file_checkpoint().unwrap();
        let old = env.begin_read();
        let borrowed = old.get(b"data", b"k").unwrap().unwrap();
        let mut writer = env.begin_write().unwrap();
        writer.put(b"data", b"k", b"private").unwrap();
        writer
            .insert_prefix(b"routes", network(), b"private-route")
            .unwrap();
        writer.insert_duplicate(b"dups", b"k", b"private").unwrap();
        writer
            .create_database(b"unpublished", DatabaseKind::Kv)
            .unwrap();
        writer.put(b"unpublished", b"private", b"private").unwrap();

        writer_busy(&source);
        assert_eq!(success("get", &source, &["data", "k"]), "old\n");
        assert!(success("stats", &source, &[]).contains("sequence: 1"));
        assert!(success("check", &source, &[]).contains("sequence: 1"));
        for (command, query) in [
            ("lpm", "192.0.2.1"),
            ("exact", "192.0.2.0/24"),
            ("cover", "192.0.2.128/25"),
        ] {
            assert_eq!(
                success(command, &source, &["routes", query]),
                "192.0.2.0/24\told-route\n"
            );
        }
        assert_eq!(success("scan", &source, &["data"]), "k\told\n");
        assert_eq!(success("dup-count", &source, &["dups", "k"]), "2\n");
        assert_eq!(success("dup-scan", &source, &["dups", "k"]), "a\nb\n");
        for command in ["export", "restore"] {
            let target = root.0.join(command);
            let output = run(command, &source, &[target.as_os_str().to_owned()]);
            assert!(
                output.status.success(),
                "CLI {command}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let copy = Environment::open(&target).unwrap();
            committed_values(&copy.begin_read());
        }
        // Verification accepts genuine coordinated pin authority only. The
        // same checkpoint/realm copied into another directory is insufficient.
        let (_reader_owner, pinned) = CoordinatedEnvironment::open_with_initial_read_and_residency(
            &source,
            ResidencyPolicy::Demand,
        )
        .unwrap();
        assert_eq!(
            backup::verify_independent_snapshot(&source, &pinned)
                .unwrap()
                .sequence,
            1
        );
        let unpinned = ReadTxn::open_snapshot(&source).unwrap();
        assert!(matches!(
            backup::verify_independent_snapshot(&source, &unpinned),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            backup::verify_independent_snapshot(root.0.join("export"), &pinned),
            Err(Error::InvalidInput(_))
        ));
        writer_busy(&source);
        assert_eq!(env.file_checkpoint().unwrap(), checkpoint);
        assert_eq!(borrowed, b"old");
        committed_values(&env.begin_read());
        writer.abort();
    }
}
