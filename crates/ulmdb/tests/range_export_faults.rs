#![cfg(all(
    target_os = "linux",
    feature = "range-lifecycle",
    feature = "fault-injection"
))]
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix, PublicationMode, ReadTxn};

fn evidence() -> PathBuf {
    std::env::var_os("ULMDB_RANGE_EXPORT_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "ulmdb-range-export-evidence-{}",
                std::process::id()
            ))
        })
}
fn eio_library() -> PathBuf {
    if let Some(path) = std::env::var_os("RANGE_EXPORT_EIO_LIBRARY") {
        return PathBuf::from(path);
    }
    static LIBRARY: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    LIBRARY
        .get_or_init(|| {
            let root = evidence().join("syscall-shim");
            fs::create_dir_all(&root).unwrap();
            let library = root.join("fsync_eio.so");
            let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/range_export_support/fsync_eio.c");
            let mut command = Command::new("cc");
            command.args(["-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-O2"]);
            if std::mem::size_of::<usize>() == 4 {
                command.arg("-m32");
            }
            let output = command
                .arg(source)
                .arg("-ldl")
                .arg("-o")
                .arg(&library)
                .output()
                .unwrap();
            fs::write(root.join("build.stdout"), &output.stdout).unwrap();
            fs::write(root.join("build.stderr"), &output.stderr).unwrap();
            assert!(output.status.success(), "syscall shim build failed");
            library
        })
        .clone()
}
fn files(path: &Path) -> BTreeMap<PathBuf, [u8; 32]> {
    fs::read_dir(path)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .map(|p| {
            (
                p.file_name().unwrap().into(),
                Sha256::digest(fs::read(p).unwrap()).into(),
            )
        })
        .collect()
}
fn fixture(name: &str) -> (PathBuf, PathBuf) {
    let root = evidence().join(name);
    fs::create_dir_all(&root).unwrap();
    let source = root.join("source");
    let env = Environment::create_with_publication(&source, PublicationMode::Range).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
    w.put(b"kv", b"key", b"value").unwrap();
    w.insert_prefix(b"v4", Prefix::new(Address::V4(0), 0).unwrap(), b"route")
        .unwrap();
    w.commit().unwrap();
    drop(env);
    (source, root.join("target"))
}
fn check_read(r: &ReadTxn) {
    assert_eq!(r.get(b"kv", b"key").unwrap(), Some(&b"value"[..]));
    assert_eq!(
        r.longest_match(b"v4", Address::V4(11)).unwrap().unwrap().1,
        b"route"
    );
}
fn invoke(source: &Path, target: &Path, kind: &str, point: &str, expected: &str) {
    let root = target.parent().unwrap();
    let original = files(source);
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "export_worker", "--nocapture"])
        .env("RANGE_EXPORT_SOURCE", source)
        .env("RANGE_EXPORT_TARGET", target)
        .env("RANGE_EXPORT_EXPECTED", expected);
    if kind == "cut" {
        cmd.env("ULMDB_PACKED_CRASH_AT", point);
    }
    if kind == "error" {
        cmd.env("ULMDB_PACKED_FAIL_AT", point);
    }
    if kind == "eio" {
        cmd.env("LD_PRELOAD", eio_library())
            .env("RANGE_EXPORT_EIO_TARGET", target)
            .env("RANGE_EXPORT_EIO_KIND", point)
            .env("RANGE_EXPORT_EIO_LOG", root.join("syscall.log"));
    }
    fs::write(root.join("case.txt"), format!("kind={kind}\npoint={point}\nexpected={expected}\nsource={source:?}\ntarget={target:?}\ncommand={cmd:?}\n")).unwrap();
    let out = cmd.output().unwrap();
    fs::write(root.join("stdout.log"), &out.stdout).unwrap();
    fs::write(root.join("stderr.log"), &out.stderr).unwrap();
    assert_eq!(files(source), original, "export must not mutate source");
    assert_eq!(
        out.status.code(),
        Some(if kind == "cut" { 86 } else { 0 }),
        "{kind}/{point}: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let published = target.join("BACKEND").exists();
    if published {
        let read = ReadTxn::open_snapshot(target).unwrap();
        check_read(&read);
        let writer = Environment::open(target).unwrap();
        check_read(&writer.begin_read());
    } else {
        assert!(ReadTxn::open_snapshot(target).is_err());
        assert!(Environment::open(target).is_err());
    }
    if kind == "eio" {
        let trace = fs::read_to_string(root.join("syscall.log")).unwrap();
        assert_eq!(trace.lines().count(), 1);
        assert!(trace.contains("errno=5") && trace.contains(&format!("path={}", target.display())));
        assert!(trace.contains(if point == "directory" {
            "matched_ordinal=2"
        } else {
            "matched_ordinal=1"
        }));
        assert_eq!(published, point == "directory");
    } else {
        assert_eq!(
            published,
            matches!(
                point,
                "range.export.after_marker_publish"
                    | "range.export.before_directory_sync"
                    | "range.export.after_directory_sync"
            )
        );
    }
}
#[test]
fn export_worker() {
    let Some(source) = std::env::var_os("RANGE_EXPORT_SOURCE") else {
        return;
    };
    let target = std::env::var_os("RANGE_EXPORT_TARGET").unwrap();
    let read = ReadTxn::open_snapshot(&source).unwrap();
    check_read(&read);
    let result = ulmdb::backup::export_independent_snapshot(&source, &read, &target);
    let actual = match result {
        Ok(_) => "ok",
        Err(Error::OutcomeUnknown) => "unknown",
        Err(Error::Io(e)) => {
            if std::env::var_os("RANGE_EXPORT_EIO_KIND").is_some() {
                assert_eq!(e.raw_os_error(), Some(5));
            }
            "io"
        }
        Err(e) => panic!("unexpected {e:?}"),
    };
    println!("export outcome={actual}");
    assert_eq!(actual, std::env::var("RANGE_EXPORT_EXPECTED").unwrap());
}
#[test]
fn actual_directory_fsync_eio_classifies_visible_publication() {
    // The failed-before-fix run retained a published valid target. Confirm
    // that plain Io was returned despite a fully openable snapshot.
    if let Some(old) = std::env::var_os("RANGE_EXPORT_BEFORE_TARGET") {
        check_read(&ReadTxn::open_snapshot(&old).unwrap());
        check_read(&Environment::open(old).unwrap().begin_read());
    }
    let (source, target) = fixture("directory-eio");
    invoke(&source, &target, "eio", "directory", "unknown");
}
#[test]
fn actual_data_and_marker_fsync_eio_leave_unpublished_targets() {
    for kind in ["data", "marker"] {
        let (source, target) = fixture(&format!("{kind}-eio"));
        invoke(&source, &target, "eio", kind, "io");
    }
}
#[test]
fn export_process_cuts_and_errors_preserve_source_and_publication_boundary() {
    for point in [
        "source_validated",
        "copy_chunk",
        "before_data_sync",
        "after_data_sync",
        "destination_validated",
        "before_marker_sync",
        "after_marker_sync",
        "before_marker_publish",
        "after_marker_publish",
        "before_directory_sync",
        "after_directory_sync",
    ] {
        for kind in ["cut", "error"] {
            let (source, target) = fixture(&format!("{kind}-{point}"));
            let unknown = matches!(
                point,
                "before_marker_publish"
                    | "after_marker_publish"
                    | "before_directory_sync"
                    | "after_directory_sync"
            );
            invoke(
                &source,
                &target,
                kind,
                &format!("range.export.{point}"),
                if unknown { "unknown" } else { "io" },
            );
        }
    }
}
#[test]
fn failed_ack_does_not_replace_warm_checkpoint_authority() {
    // Environment changes are process-local, so isolate commit fault injection.
    if std::env::var_os("RANGE_ACK_WORKER").is_none() {
        let (source, target) = fixture("ack-candidate");
        let root = evidence().join("ack-owner");
        fs::create_dir_all(&root).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "failed_ack_does_not_replace_warm_checkpoint_authority",
                "--nocapture",
            ])
            .env("RANGE_ACK_WORKER", "1")
            .env("RANGE_ACK_SOURCE", source)
            .env("RANGE_ACK_TARGET", target)
            .env("ULMDB_PACKED_FAIL_AT", "packed.commit.before_ack")
            .output()
            .unwrap();
        fs::write(root.join("stdout.log"), &output.stdout).unwrap();
        fs::write(root.join("stderr.log"), &output.stderr).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        return;
    }
    let source = PathBuf::from(std::env::var_os("RANGE_ACK_SOURCE").unwrap());
    let target = PathBuf::from(std::env::var_os("RANGE_ACK_TARGET").unwrap());
    let env = Environment::open(&source).unwrap();
    let old_sequence = env.stats().sequence;
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"key", b"unacknowledged").unwrap();
    assert!(matches!(w.commit(), Err(Error::OutcomeUnknown)));
    let warm = env.begin_read();
    assert_eq!(warm.stats().sequence, old_sequence);
    check_read(&warm);
    assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
    ulmdb::backup::export_snapshot(&env, &warm, &target).unwrap();
    check_read(&Environment::open(target).unwrap().begin_read());
}

#[test]
fn validation_rejection_and_prefault_budget_precede_publication() {
    use ulmdb_storage::{ResidencyController, ResidencyPolicy};
    let (source, target) = fixture("validation-budget");
    let original = files(&source);
    let demand = ResidencyController::new(ResidencyPolicy::Demand).unwrap();
    let state =
        ulmdb_storage::range::open_snapshot_checked(&source, &demand, |_, _| Ok(())).unwrap();
    let checkpoint = state.checkpoint.unwrap();
    let zero = ResidencyController::new(ResidencyPolicy::Prefault { max_bytes: 0 }).unwrap();
    assert!(matches!(
        ulmdb_storage::range::open_snapshot_checked(&source, &zero, |_, _| Ok(())),
        Err(Error::LimitExceeded)
    ));
    assert!(matches!(
        checkpoint.export_checked(&target, &zero, |_, _| Ok(())),
        Err(Error::LimitExceeded)
    ));
    assert!(!target.exists());
    assert!(
        checkpoint
            .export_checked(&target, &demand, |_, _| Err(Error::Corrupt(
                "reject source graph"
            )))
            .is_err()
    );
    assert!(!target.exists());
    let mut calls = 0;
    assert!(
        checkpoint
            .export_checked(&target, &demand, |_, _| {
                calls += 1;
                if calls == 2 {
                    Err(Error::Corrupt("reject copied graph"))
                } else {
                    Ok(())
                }
            })
            .is_err()
    );
    assert_eq!(calls, 2);
    assert!(!target.join("BACKEND").exists());
    assert!(Environment::open(&target).is_err());
    assert_eq!(files(&source), original);
}
#[test]
fn virtual_catalog_is_read_only_projection_and_not_exported() {
    let (source, target) = fixture("virtual-catalog");
    let before = files(&source);
    let read = ReadTxn::open_snapshot_with_empty_kv(&source, b"virtual").unwrap();
    assert!(
        read.database_names()
            .unwrap()
            .contains(&b"virtual".to_vec())
    );
    assert_eq!(read.get(b"virtual", b"anything").unwrap(), None);
    ulmdb::backup::export_independent_snapshot(&source, &read, &target).unwrap();
    let copy = Environment::open(target).unwrap();
    check_read(&copy.begin_read());
    assert!(
        !copy
            .begin_read()
            .database_names()
            .unwrap()
            .contains(&b"virtual".to_vec())
    );
    assert_eq!(files(&source), before);
}
#[test]
fn concurrent_append_never_returns_mixed_snapshot_and_preserves_old_borrow() {
    use std::sync::{Arc, Barrier};
    let (source, _) = fixture("concurrent-append");
    let env = Arc::new(Environment::open(&source).unwrap());
    let old = ReadTxn::open_snapshot(&source).unwrap();
    let escaped = old.get(b"kv", b"key").unwrap().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let other_env = Arc::clone(&env);
    let other_barrier = Arc::clone(&barrier);
    let writer = std::thread::spawn(move || {
        other_barrier.wait();
        for n in 0u64..16 {
            let mut w = other_env.begin_write().unwrap();
            w.put(b"kv", b"key", &n.to_le_bytes()).unwrap();
            w.put(b"kv", b"epoch", &n.to_le_bytes()).unwrap();
            w.commit().unwrap();
        }
    });
    barrier.wait();
    let mut successful = 0;
    for _ in 0..24 {
        match ReadTxn::open_snapshot(&source) {
            Ok(read) => {
                let key = read.get(b"kv", b"key").unwrap().unwrap();
                if key != b"value" {
                    assert_eq!(Some(key), read.get(b"kv", b"epoch").unwrap());
                }
                successful += 1;
            }
            Err(Error::Busy | Error::Corrupt(_)) => (), // moving/torn heads fail closed
            Err(e) => panic!("unexpected concurrent capture result {e:?}"),
        }
    }
    writer.join().unwrap();
    let current = ReadTxn::open_snapshot(source).unwrap();
    assert_eq!(
        current.get(b"kv", b"key").unwrap(),
        Some(&15u64.to_le_bytes()[..])
    );
    assert_eq!(escaped, b"value");
    println!("concurrent captures accepted={successful}/24 plus valid final capture");
}
