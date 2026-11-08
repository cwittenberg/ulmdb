//! Actual pre-append native capacity-query EIO; process-local opt-in shim, never timing.
#![cfg(all(
    target_os = "linux",
    target_env = "gnu",
    target_pointer_width = "64",
    feature = "fault-injection"
))]
use sha2::{Digest, Sha256};
use std::{
    fs,
    ops::Bound,
    path::{Path, PathBuf},
    process::Command,
};
use ulmdb::{
    Address, DatabaseKind, Error, Prefix,
    resident::{Environment, Options, ReadTxn, RedoEncoding, RedoFormat, WriteTxn},
};
use ulmdb_storage::resident::Config;

fn options() -> Options {
    Options {
        pool: Config {
            kv_pages: 32,
            tiny_pages: 32,
            max_reserved_bytes: 64 << 20,
            max_metadata_bytes: 64 << 20,
            max_views: 32,
        },
        wal_capacity: 4 << 20,
        graph_read_budget: 1_000_000,
        auto_collect_below: 0,
    }
}
fn evidence() -> PathBuf {
    std::env::var_os("ULMDB_RESIDENT_CAPACITY_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "ulmdb-resident-capacity-evidence-{}",
                std::process::id()
            ))
        })
}
fn eio_library() -> PathBuf {
    if let Some(path) = std::env::var_os("ULMDB_RESIDENT_CAPACITY_EIO_LIBRARY") {
        return PathBuf::from(path);
    }
    static LIBRARY: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    LIBRARY
        .get_or_init(|| {
            // Same opt-in builder pattern as range_export_faults. This entire
            // test binary is cfg-disabled without fault-injection, so ordinary
            // tests gain no C compiler or dynamic-loader dependency.
            let root = evidence().join("syscall-shim");
            fs::create_dir_all(&root).unwrap();
            let library = root.join("capacity_fstat_eio.so");
            let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/resident_capacity_support/fstat_eio.c");
            let output = Command::new("cc")
                .args(["-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-O2"])
                .arg(source)
                .arg("-ldl")
                .arg("-o")
                .arg(&library)
                .output()
                .unwrap();
            fs::write(root.join("build.stdout"), &output.stdout).unwrap();
            fs::write(root.join("build.stderr"), &output.stderr).unwrap();
            assert!(output.status.success(), "fstat EIO shim build failed");
            library
        })
        .clone()
}
fn policy(case: &str) -> (RedoFormat, RedoEncoding) {
    match case {
        "literal" => (RedoFormat::LiteralV1, RedoEncoding::Owned),
        "compact-owned" => (RedoFormat::CompactV2, RedoEncoding::Owned),
        "compact-borrowed" => (RedoFormat::CompactV2, RedoEncoding::Borrowed),
        _ => panic!("unknown test policy"),
    }
}
fn prepare_update<'a>(
    env: &'a Environment,
    value: &[u8],
    create: bool,
    extra: bool,
) -> ulmdb::Result<WriteTxn<'a>> {
    let mut w = env.begin_write()?;
    if create {
        w.create_database(b"kv", DatabaseKind::Kv)?;
        w.create_database(b"v4", DatabaseKind::Ipv4)?;
        w.create_database(b"v6", DatabaseKind::Ipv6)?;
        w.put(b"kv", &1u64.to_be_bytes(), b"sentinel")?;
    }
    w.put(b"kv", &0u64.to_be_bytes(), value)?;
    if extra {
        w.put(b"kv", &2u64.to_be_bytes(), b"private!")?;
    }
    w.insert_prefix(b"v4", Prefix::new(Address::V4(0), 0).unwrap(), value)?;
    w.insert_prefix(b"v6", Prefix::new(Address::V6(0), 0).unwrap(), value)?;
    Ok(w)
}
fn update(env: &Environment, value: &[u8], create: bool, extra: bool) -> ulmdb::Result<u64> {
    prepare_update(env, value, create, extra)?.commit()
}
fn check_read(read: &ReadTxn, value: &[u8]) {
    assert_eq!(read.database_count(), 3);
    assert_eq!(read.get(b"kv", &0u64.to_be_bytes()).unwrap(), Some(value));
    assert_eq!(
        read.get(b"kv", &1u64.to_be_bytes()).unwrap(),
        Some(b"sentinel".as_slice())
    );
    assert_eq!(read.get(b"kv", &2u64.to_be_bytes()).unwrap(), None);
    assert_eq!(read.get(b"kv", &u64::MAX.to_be_bytes()).unwrap(), None);
    let kv = read.kv(b"kv").unwrap();
    for reverse in [false, true] {
        let got = kv
            .cursor(Bound::Unbounded, Bound::Unbounded, reverse)
            .unwrap()
            .map(|row| row.map(|(key, value)| (key.to_vec(), value.to_vec())))
            .collect::<ulmdb::Result<Vec<_>>>()
            .unwrap();
        let mut expected = vec![
            (0u64.to_be_bytes().to_vec(), value.to_vec()),
            (1u64.to_be_bytes().to_vec(), b"sentinel".to_vec()),
        ];
        if reverse {
            expected.reverse();
        }
        assert_eq!(got, expected);
    }
    for (name, address) in [
        (b"v4".as_slice(), Address::V4(11)),
        (b"v6", Address::V6(11)),
    ] {
        assert_eq!(read.longest_match(name, address).unwrap().unwrap().1, value);
        let zero = Prefix::new(
            if name == b"v4" {
                Address::V4(0)
            } else {
                Address::V6(0)
            },
            0,
        )
        .unwrap();
        assert_eq!(
            read.prefix_index(name)
                .unwrap()
                .exact(zero)
                .unwrap()
                .unwrap()
                .1,
            value
        );
    }
}
fn check_guard_trace(trace: &str) {
    let lines: Vec<_> = trace.lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "both witnesses then one capacity query only: {trace}"
    );
    for (line, offset) in lines[..2].iter().zip([65536, 131072]) {
        assert!(line.starts_with("pread"), "{trace}");
        assert!(
            line.contains(&format!("offset={offset} length=256 result=256 errno=0")),
            "{trace}"
        );
    }
    assert!(
        lines[2].starts_with("fstat") || lines[2].starts_with("statx"),
        "{trace}"
    );
    assert!(lines[2].ends_with("result=-1 errno=5"), "{trace}");
}
fn worker(root: &Path, case: &str) {
    let (format, encoding) = policy(case);
    let detection_mode = std::env::var("ULMDB_RESIDENT_CAPACITY_DETECT_MODE").unwrap();
    let path = root.join("db");
    let env = Environment::create_with_encoding(&path, options(), format, encoding).unwrap();
    let detection = fs::read_to_string(root.join("detection.log")).unwrap();
    assert_eq!(detection.lines().count(), 1, "choose once at create");
    if detection_mode != "normal" {
        assert!(detection.contains("strategy=metadata"), "{detection}");
    }
    let ram_probe = detection.contains("strategy=ram");
    assert_eq!(update(&env, b"oldvalue", true, false).unwrap(), 1);
    let old = env.begin_read();
    let held = old.get(b"kv", &0u64.to_be_bytes()).unwrap().unwrap();
    let address = held.as_ptr();
    check_read(&old, b"oldvalue");
    let original = fs::read(path.join("RESIDENT.wal")).unwrap();
    let metadata_file = fs::File::open(path.join("RESIDENT.wal")).unwrap();
    fs::write(
        root.join("metadata-sample"),
        b"unarmed standard metadata call\n",
    )
    .unwrap();
    assert_eq!(
        metadata_file.metadata().unwrap().len(),
        original.len() as u64
    );
    fs::remove_file(root.join("metadata-sample")).unwrap();
    drop(metadata_file);
    let metadata_methods = fs::read_to_string(root.join("metadata-method.log")).unwrap();
    let metadata_method = metadata_methods.lines().last().unwrap();
    assert!(matches!(metadata_method, "fstat" | "fstat64" | "statx"));
    let sequence = env.sequence();
    let lsn = env.physical_lsn();
    let wal_end = env.wal_bytes();
    let identity = old.commit_identity();
    let before = env.pool_stats().unwrap();

    let candidate = prepare_update(&env, b"rejected", false, true).unwrap();
    assert!(
        env.pool_stats().unwrap().held_maps > before.held_maps,
        "real private candidate map owners must exist before commit"
    );
    // The preload library starts with an absent marker. Arming is filesystem
    // state, not unsafe global environment mutation, and occurs after the exact
    // selected bytes/snapshot were captured and candidate mutation completed.
    // No WAL metadata calls intervene before the publication guard.
    fs::write(root.join("observe"), b"observe through failed return\n").unwrap();
    fs::write(root.join("arm"), b"one pre-append capacity call\n").unwrap();
    let failed = candidate.commit();
    fs::remove_file(root.join("observe")).unwrap();
    match failed {
        Err(Error::Io(error)) => assert_eq!(error.raw_os_error(), Some(libc::EIO)),
        other => panic!("capacity guard must return raw EIO: {other:?}"),
    }
    assert!(
        !root.join("arm").exists(),
        "shim must consume the marker exactly once"
    );
    let trace = fs::read_to_string(root.join("syscall.log")).unwrap();
    check_guard_trace(&trace);
    if ram_probe {
        assert!(trace.lines().last().unwrap().starts_with("fstat"));
    } else {
        assert_eq!(
            trace
                .lines()
                .last()
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap(),
            metadata_method,
            "Metadata selection must use the independently observed standard-library method"
        );
    }
    assert_eq!(
        fs::read_to_string(root.join("detection.log")).unwrap(),
        detection,
        "never detect in commit"
    );
    let after_failure = fs::read(path.join("RESIDENT.wal")).unwrap();
    assert_eq!(
        after_failure, original,
        "pre-append EIO must not change any WAL byte"
    );
    assert_eq!(
        (env.sequence(), env.physical_lsn(), env.wal_bytes()),
        (sequence, lsn, wal_end)
    );
    assert_eq!(env.begin_read().commit_identity(), identity);
    check_read(&env.begin_read(), b"oldvalue");
    let after = env.pool_stats().unwrap();
    assert!(
        after.allocations > before.allocations,
        "real private candidate was allocated"
    );
    assert!(!after.poisoned);
    assert_eq!(
        after.held_maps, before.held_maps,
        "private map owners must drop"
    );
    assert_eq!(after.free_pages, before.free_pages);
    assert_eq!(after.unused_tiny_pages, before.unused_tiny_pages);
    assert_eq!(
        after.view_roots, before.view_roots,
        "private view owners must drop"
    );
    assert_eq!(after.quarantined_maps, before.quarantined_maps);
    assert!(!env.private_workspace_stats().unwrap().leased);
    assert_eq!(held, b"oldvalue");
    assert_eq!(held.as_ptr(), address);
    check_read(&old, b"oldvalue");

    // Actual previous metadata-error semantics are retryable. A one-shot error
    // must not poison the live writer or transfer the rejected candidate owner.
    assert_eq!(
        update(&env, b"retryval", false, false).unwrap(),
        sequence + 1
    );
    let current = env.begin_read();
    check_read(&current, b"retryval");
    assert_eq!(current.sequence(), sequence + 1);
    assert_ne!(current.commit_identity(), identity);
    assert_eq!(fs::read_to_string(root.join("syscall.log")).unwrap(), trace);
    assert_eq!(held, b"oldvalue");
    assert_eq!(held.as_ptr(), address);
    check_read(&old, b"oldvalue");
    assert_eq!(
        fs::read_to_string(root.join("detection.log")).unwrap(),
        detection,
        "method remains immutable through retry"
    );
    let selected = (
        env.sequence(),
        env.physical_lsn(),
        current.commit_identity(),
    );
    let owner = env.pool_stats().unwrap().physical_incarnation;
    drop(current);
    drop(env);
    let reopened = Environment::open_with_encoding(&path, options(), format, encoding).unwrap();
    let fresh = reopened.begin_read();
    let final_detection = fs::read_to_string(root.join("detection.log")).unwrap();
    assert_eq!(
        final_detection.lines().count(),
        2,
        "one fresh detector at open"
    );
    assert_eq!(
        final_detection.lines().next(),
        final_detection.lines().last(),
        "same pinned filesystem and mode on reopen"
    );
    assert_eq!(
        (
            fresh.sequence(),
            reopened.physical_lsn(),
            fresh.commit_identity()
        ),
        selected
    );
    assert_ne!(reopened.pool_stats().unwrap().physical_incarnation, owner);
    check_read(&fresh, b"retryval");
    assert_eq!(held, b"oldvalue");
    assert_eq!(held.as_ptr(), address);
    check_read(&old, b"oldvalue");
    fs::write(root.join("oracle.txt"), format!(
        "case={case}\ndetection_mode={detection_mode}\nselected_query={}\nram_probe={ram_probe}\nerrno={}\nwal_before_sha256={:x}\nwal_after_failure_sha256={:x}\nsequence_before={sequence}\nsequence_after_retry={}\nheld_maps_before={}\nheld_maps_after_failure={}\nview_roots_before={}\nview_roots_after_failure={}\nall_point_miss_cursor_prefix_reopen_oracles=passed\n",
        trace.lines().last().unwrap().split_whitespace().next().unwrap(), libc::EIO, Sha256::digest(&original), Sha256::digest(&after_failure), selected.0,
        before.held_maps, after.held_maps, before.view_roots, after.view_roots
    )).unwrap();
    println!("RESIDENT_CAPACITY_EIO_DONE {case}");
}
#[test]
fn actual_capacity_query_eio_preserves_selected_snapshot_and_retry() {
    if let Some(root) = std::env::var_os("ULMDB_RESIDENT_CAPACITY_WORKER") {
        worker(
            Path::new(&root),
            &std::env::var("ULMDB_RESIDENT_CAPACITY_CASE").unwrap(),
        );
        return;
    }
    let library = eio_library();
    for (base_name, base) in [
        ("disk", std::env::temp_dir()),
        ("ram", PathBuf::from("/dev/shm")),
    ] {
        if !base.is_dir() {
            continue;
        }
        for case in ["literal", "compact-owned", "compact-borrowed"] {
            for detection_mode in ["normal", "eio", "unknown"] {
                let root = base.join(format!(
                    "ulmdb-capacity-eio-{:032x}",
                    ulmdb_storage::new_identity().unwrap()
                ));
                fs::create_dir(&root).unwrap();
                let root = fs::canonicalize(root).unwrap();
                let proof = evidence().join(format!("{base_name}-{case}-{detection_mode}"));
                fs::create_dir_all(&proof).unwrap();
                let mut command = Command::new("timeout");
                command
                    .args(["--signal=KILL", "30s"])
                    .arg(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "actual_capacity_query_eio_preserves_selected_snapshot_and_retry",
                        "--nocapture",
                        "--test-threads=1",
                    ])
                    .env("LD_PRELOAD", &library)
                    .env("ULMDB_RESIDENT_CAPACITY_WORKER", &root)
                    .env("ULMDB_RESIDENT_CAPACITY_CASE", case)
                    .env("ULMDB_RESIDENT_CAPACITY_DETECT_MODE", detection_mode)
                    .env(
                        "ULMDB_RESIDENT_CAPACITY_DETECT_LOG",
                        root.join("detection.log"),
                    )
                    .env("ULMDB_RESIDENT_CAPACITY_EIO_OBSERVE", root.join("observe"))
                    .env(
                        "ULMDB_RESIDENT_CAPACITY_METADATA_SAMPLE",
                        root.join("metadata-sample"),
                    )
                    .env(
                        "ULMDB_RESIDENT_CAPACITY_METADATA_LOG",
                        root.join("metadata-method.log"),
                    )
                    .env(
                        "ULMDB_RESIDENT_CAPACITY_EIO_WAL",
                        root.join("db/RESIDENT.wal"),
                    )
                    .env("ULMDB_RESIDENT_CAPACITY_EIO_ARM", root.join("arm"))
                    .env("ULMDB_RESIDENT_CAPACITY_EIO_LOG", root.join("syscall.log"))
                    .env_remove("ULMDB_RESIDENT_FAIL_AT")
                    .env_remove("ULMDB_RESIDENT_EXIT_AT");
                fs::write(
                    proof.join("case.txt"),
                    format!("root={root:?}\ncommand={command:?}\n"),
                )
                .unwrap();
                let output = command.output().unwrap();
                fs::write(proof.join("stdout.log"), &output.stdout).unwrap();
                fs::write(proof.join("stderr.log"), &output.stderr).unwrap();
                for file in [
                    "syscall.log",
                    "detection.log",
                    "metadata-method.log",
                    "oracle.txt",
                ] {
                    if root.join(file).is_file() {
                        fs::copy(root.join(file), proof.join(file)).unwrap();
                    }
                }
                assert!(
                    output.status.success(),
                    "{base_name}/{case}: {} {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    String::from_utf8_lossy(&output.stdout).contains("RESIDENT_CAPACITY_EIO_DONE")
                );
                check_guard_trace(&fs::read_to_string(proof.join("syscall.log")).unwrap());
                fs::remove_dir_all(root).unwrap();
            }
        }
    }
}
