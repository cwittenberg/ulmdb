//! Process exit/IO boundaries through the real hybrid facade, not power loss.
#![cfg(feature = "fault-injection")]
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use ulmdb::{
    Address, DatabaseKind, Error, Prefix,
    hybrid::{Environment, ImageMode, LiveRedoMode, Options},
};
const POINTS: [&str; 11] = [
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
fn options(image: ImageMode) -> Options {
    let mut o = Options {
        image,
        ..Options::default()
    };
    // Test-only selector; no production environment-variable mode switch.
    o.live_redo = if std::env::var("ULMDB_HYBRID_TEST_RECIPE").as_deref() == Ok("borrowed") {
        LiveRedoMode::Borrowed
    } else {
        LiveRedoMode::Owned
    };
    o.chunk.capacity = 2 * 1024 * 1024;
    o.chunk.reserve_quantum = 65536;
    o.chunk.max_burned_bytes = 1024 * 1024;
    o.max_image_bytes = 8 * 1024 * 1024;
    o
}
fn prefixes() -> (Prefix, Prefix) {
    (
        Prefix::new(Address::V4(0), 0).unwrap(),
        Prefix::new(Address::V6(0), 0).unwrap(),
    )
}
fn update(env: &Environment, value: &[u8], create: bool) -> ulmdb::Result<u64> {
    let mut w = env.begin_write()?;
    if create {
        w.create_database(b"kv", DatabaseKind::Kv)?;
        w.create_database(b"v4", DatabaseKind::Ipv4)?;
        w.create_database(b"v6", DatabaseKind::Ipv6)?;
    }
    let (v4, v6) = prefixes();
    w.put(b"kv", b"state", value)?;
    w.insert_prefix(b"v4", v4, value)?;
    w.insert_prefix(b"v6", v6, value)?;
    w.commit()
}
fn child() {
    let path =
        std::env::var_os("ULMDB_HYBRID_FACADE_CHILD").expect("fault child path selected by parent");
    let image = if std::env::var("ULMDB_HYBRID_TEST_IMAGE").as_deref() == Ok("anonymous") {
        ImageMode::Anonymous
    } else {
        ImageMode::File
    };
    let env = Environment::open(&path, options(image)).unwrap();
    let old = env.begin_read();
    let held = old.get(b"kv", b"state").unwrap().unwrap();
    let result = update(&env, b"new", false);
    if let Ok(point) = std::env::var("ULMDB_HYBRID_FAIL_AT") {
        let index = POINTS.iter().position(|p| *p == point).unwrap();
        if index < 5 {
            assert!(matches!(result, Err(Error::Io(_))));
            assert!(env.begin_write().is_ok());
        } else {
            assert!(matches!(result, Err(Error::OutcomeUnknown)));
            assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
        }
        assert_eq!(held, b"old");
        assert_eq!(env.sequence(), 1);
    } else {
        result.unwrap();
    }
}
#[test]
fn every_commit_cut_and_io_error_preserves_a_complete_authoritative_snapshot() {
    if std::env::var_os("ULMDB_HYBRID_FACADE_CHILD").is_some() {
        child();
        return;
    }
    run_matrix(ImageMode::File);
}
#[test]
fn anonymous_commit_cuts_and_io_errors_preserve_a_complete_authoritative_snapshot() {
    run_matrix(ImageMode::Anonymous);
}
fn run_matrix(image: ImageMode) {
    for base in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        if !base.is_dir() {
            continue;
        }
        for exit in [false, true] {
            for point in POINTS {
                let path = base.join(format!(
                    "ulmdb-hybrid-facade-cut-{:032x}",
                    ulmdb_storage::new_identity().unwrap()
                ));
                let env = Environment::create(&path, options(image)).unwrap();
                update(&env, b"old", true).unwrap();
                let old = env.begin_read();
                drop(env);
                let output = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "every_commit_cut_and_io_error_preserves_a_complete_authoritative_snapshot",
                        "--nocapture",
                    ])
                    .env("ULMDB_HYBRID_FACADE_CHILD", &path)
                    .env(
                        "ULMDB_HYBRID_TEST_IMAGE",
                        if image == ImageMode::Anonymous {
                            "anonymous"
                        } else {
                            "file"
                        },
                    )
                    .env(
                        if exit {
                            "ULMDB_HYBRID_EXIT_AT"
                        } else {
                            "ULMDB_HYBRID_FAIL_AT"
                        },
                        point,
                    )
                    .output()
                    .unwrap();
                assert_eq!(
                    output.status.code(),
                    Some(if exit { 86 } else { 0 }),
                    "{point}: stdout={} stderr={}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let env = Environment::open(&path, options(image)).unwrap();
                let r = env.begin_read();
                let value = r.get(b"kv", b"state").unwrap().unwrap();
                assert!(value == b"old" || value == b"new");
                let (v4, v6) = prefixes();
                assert_eq!(
                    r.prefix_index(b"v4").unwrap().exact(v4).unwrap().unwrap().1,
                    value
                );
                assert_eq!(
                    r.prefix_index(b"v6").unwrap().exact(v6).unwrap().unwrap().1,
                    value
                );
                assert_eq!(old.get(b"kv", b"state").unwrap(), Some(b"old".as_slice()));
                drop(env);
                drop(r);
                drop(old);
                std::fs::remove_dir_all(path).unwrap();
            }
        }
    }
}
#[test]
fn acknowledged_corruption_fails_closed_without_changing_journal() {
    corruption(ImageMode::File);
}
#[test]
fn anonymous_acknowledged_corruption_fails_closed_without_changing_journal() {
    corruption(ImageMode::Anonymous);
}
fn corruption(image: ImageMode) {
    use ulmdb_platform::FileExt;
    let path = std::env::temp_dir().join(format!(
        "ulmdb-hybrid-corrupt-{:032x}",
        ulmdb_storage::new_identity().unwrap()
    ));
    let env = Environment::create(&path, options(image)).unwrap();
    update(&env, b"old", true).unwrap();
    update(&env, b"new", false).unwrap();
    let old = env.begin_read();
    drop(env);
    let wal = path.join("HYBRID.wal");
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&wal)
        .unwrap();
    let mut byte = [0];
    f.read_exact_at(&mut byte, 196608 + 300).unwrap();
    byte[0] ^= 1;
    f.write_all_at(&byte, 196608 + 300).unwrap();
    f.sync_all().unwrap();
    let before = std::fs::read(&wal).unwrap();
    assert!(Environment::open(&path, options(image)).is_err());
    assert_eq!(std::fs::read(&wal).unwrap(), before);
    assert_eq!(old.get(b"kv", b"state").unwrap(), Some(b"new".as_slice()));
    drop(f);
    drop(old);
    std::fs::remove_dir_all(Path::new(&path)).unwrap();
}

#[test]
fn borrowed_strategy_runs_every_original_facade_fault_control() {
    for name in [
        "every_commit_cut_and_io_error_preserves_a_complete_authoritative_snapshot",
        "anonymous_commit_cuts_and_io_errors_preserve_a_complete_authoritative_snapshot",
        "acknowledged_corruption_fails_closed_without_changing_journal",
        "anonymous_acknowledged_corruption_fails_closed_without_changing_journal",
    ] {
        let output = Command::new("timeout")
            .args(["--signal=KILL", "90s"])
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("ULMDB_HYBRID_TEST_RECIPE", "borrowed")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"),
            "missing exact control completion: {name}"
        );
    }
}
