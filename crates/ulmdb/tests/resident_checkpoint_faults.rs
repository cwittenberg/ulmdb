//! Owned subprocess cuts at checkpoint creation/selection. No power-loss claim.
#![cfg(feature = "fault-injection")]
use std::{
    fs,
    io::Read,
    path::PathBuf,
    process::{Child, Command, Output},
    time::{Duration, Instant},
};
use ulmdb::{
    DatabaseKind, Error,
    resident::{Environment, Options, RedoFormat},
};
use ulmdb_storage::resident::Config;
fn options() -> Options {
    Options {
        pool: Config {
            kv_pages: 32,
            tiny_pages: 8,
            max_reserved_bytes: 8 << 20,
            max_metadata_bytes: 16 << 20,
            max_views: 32,
        },
        wal_capacity: 256 << 10,
        graph_read_budget: 1_000_000,
        auto_collect_below: 8,
    }
}
const POINTS: [&str; 6] = [
    "resident.checkpoint.before_image",
    "resident.checkpoint.after_image",
    "resident.checkpoint.after_wal_sync",
    "resident.checkpoint.before_select",
    "resident.checkpoint.after_select",
    "resident.checkpoint.after_directory_sync",
];
// Each failpoint worker is one libtest process with no child processes. Keep
// its live OS handle until reaping; logs cannot fill a pipe and stall exit.
fn checkpoint_child(mut command: Command) -> Output {
    struct Logs(PathBuf);
    impl Drop for Logs {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    struct Worker(Option<Child>);
    impl Drop for Worker {
        fn drop(&mut self) {
            if let Some(mut child) = self.0.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let log_directory = std::env::temp_dir().join(format!(
        "resident-cut-worker-{:032x}",
        ulmdb_storage::new_identity().unwrap()
    ));
    ulmdb_platform::create_private_directory(&log_directory).unwrap();
    let logs = Logs(log_directory);
    let stdout = logs.0.join("stdout");
    let stderr = logs.0.join("stderr");
    command
        .stdout(ulmdb_platform::create_private_file(&stdout, false).unwrap())
        .stderr(ulmdb_platform::create_private_file(&stderr, false).unwrap());
    let mut worker = Worker(Some(command.spawn().unwrap()));
    drop(command);
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = worker.0.as_mut().unwrap().try_wait().unwrap() {
            worker.0.take();
            break status;
        }
        assert!(Instant::now() < deadline, "checkpoint worker exceeded 15s");
        std::thread::sleep(Duration::from_millis(5));
    };
    let read_log = |path: &PathBuf| {
        let file = fs::File::open(path).unwrap();
        assert!(file.metadata().unwrap().len() <= 1 << 20);
        let mut bytes = Vec::new();
        file.take(1 << 20).read_to_end(&mut bytes).unwrap();
        bytes
    };
    Output {
        status,
        stdout: read_log(&stdout),
        stderr: read_log(&stderr),
    }
}
#[test]
fn checkpoint_errors_and_death_preserve_exact_business_head_and_writer_progress() {
    if let Some(path) = std::env::var_os("CHECKPOINT_CHILD") {
        let env = Environment::open(&path, options()).unwrap();
        let old = env.begin_read();
        let bytes = old.get(b"kv", b"key").unwrap().unwrap();
        let id = old.commit_identity();
        let result = env.checkpoint();
        let point = std::env::var("ULMDB_RESIDENT_FAIL_AT").unwrap_or_default();
        let index = if std::env::var("CHECKPOINT_OS_UNKNOWN").is_ok() {
            4
        } else {
            POINTS.iter().position(|p| *p == point).unwrap_or(0)
        };
        assert_eq!(bytes, b"stable");
        assert_eq!(old.commit_identity(), id);
        if index >= 4 {
            assert!(matches!(result, Err(Error::OutcomeUnknown)), "{result:?}");
            assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
        } else {
            assert!(matches!(result, Err(Error::Io(_))), "{result:?}");
            assert!(env.begin_write().is_ok());
        }
        return;
    }
    for format in [
        RedoFormat::LiteralV1,
        RedoFormat::CompactV2,
        RedoFormat::FullPostimageV2,
    ] {
        for point in POINTS {
            for exit in [false, true] {
                let d = std::env::temp_dir().join(format!(
                    "resident-cp-fault-{:032x}",
                    ulmdb_storage::new_identity().unwrap()
                ));
                let env = Environment::create_with_redo(&d, options(), format).unwrap();
                {
                    let mut w = env.begin_write().unwrap();
                    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
                    w.put(b"kv", b"key", b"stable").unwrap();
                    w.commit().unwrap();
                }
                let id = env.begin_read().commit_identity();
                drop(env);
                let mut command = Command::new(std::env::current_exe().unwrap());
                command
                    .args([
                        "--exact",
                        "checkpoint_errors_and_death_preserve_exact_business_head_and_writer_progress",
                        "--nocapture",
                    ])
                    .env("CHECKPOINT_CHILD", &d)
                    .env(
                        if exit {
                            "ULMDB_RESIDENT_EXIT_AT"
                        } else {
                            "ULMDB_RESIDENT_FAIL_AT"
                        },
                        point,
                    );
                let output = checkpoint_child(command);
                assert_eq!(
                    output.status.code(),
                    Some(if exit { 86 } else { 0 }),
                    "{format:?}/{point}/{exit}: {} {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let env = Environment::open_with_redo(&d, options(), format).unwrap();
                assert_eq!(env.sequence(), 1);
                assert_eq!(env.begin_read().commit_identity(), id);
                assert_eq!(
                    env.begin_read().get(b"kv", b"key").unwrap(),
                    Some(b"stable".as_slice())
                );
                {
                    let mut w = env.begin_write().unwrap();
                    w.put(b"kv", b"key", b"next").unwrap();
                    assert_eq!(w.commit().unwrap(), 2);
                }
                drop(env);
                let env = Environment::open_with_redo(&d, options(), format).unwrap();
                assert_eq!(
                    env.begin_read().get(b"kv", b"key").unwrap(),
                    Some(b"next".as_slice())
                );
                drop(env);
                fs::remove_dir_all(d).unwrap();
            }
        }
    }
}
#[test]
#[cfg(target_os = "linux")]
fn real_checkpoint_fsync_eio_and_image_or_wal_enospc_keep_reopen_safe() {
    let evidence = std::env::var_os("ULMDB_RESIDENT_CP_EVIDENCE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "resident-cp-io-{:032x}",
                ulmdb_storage::new_identity().unwrap()
            ))
        });
    fs::create_dir_all(&evidence).unwrap();
    let shim = evidence.join("enospc.so");
    let status = Command::new("cc")
        .args(["-shared", "-fPIC", "-O2"])
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/resident_checkpoint_support/enospc.c"),
        )
        .args(["-ldl", "-o"])
        .arg(&shim)
        .status()
        .unwrap();
    assert!(status.success());
    for (format, label) in [
        (RedoFormat::LiteralV1, "literal"),
        (RedoFormat::CompactV2, "compact"),
        (RedoFormat::FullPostimageV2, "full-postimage"),
    ] {
        for (case, ordinal, space, unknown) in [
            ("image-eio", 3, None, false),
            ("image-directory-eio", 4, None, false),
            ("new-wal-eio", 5, None, false),
            ("selection-directory-eio", 6, None, true),
            ("image-enospc", 0, Some("image"), false),
            ("wal-enospc", 0, Some("wal"), false),
        ] {
            let d = evidence.join(format!("{label}-{case}"));
            let env = Environment::create_with_redo(&d, options(), format).unwrap();
            {
                let mut w = env.begin_write().unwrap();
                w.create_database(b"kv", DatabaseKind::Kv).unwrap();
                w.put(b"kv", b"key", b"stable").unwrap();
                w.commit().unwrap();
            }
            let id = env.begin_read().commit_identity();
            drop(env);
            let trace = evidence.join(format!("{label}-{case}.strace"));
            let mut command = Command::new("timeout");
            command.args(["--signal=KILL", "15s"]);
            if space.is_none() {
                command
                    .args([
                        "strace",
                        "-f",
                        "-yy",
                        "-e",
                        "trace=fsync",
                        "-e",
                        &format!("inject=fsync:error=EIO:when={ordinal}"),
                        "-o",
                    ])
                    .arg(&trace);
            }
            command
                .arg(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "checkpoint_errors_and_death_preserve_exact_business_head_and_writer_progress",
                    "--nocapture",
                ])
                .env("CHECKPOINT_CHILD", &d);
            if let Some(space) = space {
                command
                    .env("LD_PRELOAD", &shim)
                    .env("RESIDENT_CHECKPOINT_ENOSPC", space);
            }
            if unknown {
                command.env("CHECKPOINT_OS_UNKNOWN", "1");
            }
            let output = command.output().unwrap();
            fs::write(
                evidence.join(format!("{label}-{case}.stdout")),
                &output.stdout,
            )
            .unwrap();
            fs::write(
                evidence.join(format!("{label}-{case}.stderr")),
                &output.stderr,
            )
            .unwrap();
            assert!(
                output.status.success(),
                "{label}/{case}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            if space.is_some() {
                assert!(
                    String::from_utf8_lossy(&output.stderr)
                        .contains("RESIDENT_CHECKPOINT_REAL_ENOSPC")
                );
            } else {
                let raw = fs::read_to_string(trace).unwrap();
                assert!(raw.contains("EIO") && raw.contains("INJECTED"));
            }
            let env = Environment::open_with_redo(&d, options(), format).unwrap();
            assert_eq!(env.sequence(), 1);
            assert_eq!(env.begin_read().commit_identity(), id);
            {
                let mut w = env.begin_write().unwrap();
                w.put(b"kv", b"key", b"after-io").unwrap();
                assert_eq!(w.commit().unwrap(), 2);
            }
            drop(env);
            let env = Environment::open_with_redo(&d, options(), format).unwrap();
            assert_eq!(
                env.begin_read().get(b"kv", b"key").unwrap(),
                Some(b"after-io".as_slice())
            );
        }
    }
    println!("CHECKPOINT_IO_EVIDENCE={}", evidence.display());
}
