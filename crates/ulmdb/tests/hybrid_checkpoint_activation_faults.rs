#![cfg(feature = "fault-injection")]
use std::{fs, path::Path, process::Command};
use ulmdb::{
    Address, DatabaseKind, Error, Prefix,
    hybrid::{Environment, ImageMode, Options},
};
use ulmdb_storage::hybrid::checkpoint::{CheckpointLimits, selection};
const POINTS: [&str; 6] = [
    "h5.activate.before_selector_write",
    "h5.activate.after_selector_write",
    "h5.activate.after_selector_file_sync",
    "h5.activate.before_selector_rename",
    "h5.activate.after_selector_rename",
    "h5.activate.after_selector_directory_sync",
];
fn options(image: ImageMode) -> Options {
    Options {
        image,
        chunk: ulmdb_storage::hybrid::ChunkLimits {
            capacity: 1024 * 1024,
            reserve_quantum: 65536,
            max_burned_bytes: 512 * 1024,
        },
        max_image_bytes: 8 * 1024 * 1024,
        ..Options::default()
    }
}
fn set(env: &Environment, value: u64, create: bool) {
    let mut w = env.begin_write().unwrap();
    if create {
        for (n, k) in [
            (b"kv".as_slice(), DatabaseKind::Kv),
            (b"v4", DatabaseKind::Ipv4),
            (b"v6", DatabaseKind::Ipv6),
        ] {
            w.create_database(n, k).unwrap();
        }
    }
    w.put(b"kv", b"state", &value.to_le_bytes()).unwrap();
    w.insert_prefix(
        b"v4",
        Prefix::new(Address::V4(0x0a000000), 8).unwrap(),
        &value.to_le_bytes(),
    )
    .unwrap();
    w.insert_prefix(
        b"v6",
        Prefix::new(Address::V6(0x20010db8000000000000000000000000), 32).unwrap(),
        &value.to_le_bytes(),
    )
    .unwrap();
    w.commit().unwrap();
}
fn check(env: &Environment) {
    let r = env.begin_read();
    assert_eq!(
        r.get(b"kv", b"state").unwrap(),
        Some(2u64.to_le_bytes().as_slice())
    );
    assert_eq!(
        r.longest_match(b"v4", Address::V4(0x0a000001))
            .unwrap()
            .unwrap()
            .1,
        2u64.to_le_bytes()
    );
    assert_eq!(
        r.longest_match(b"v6", Address::V6(0x20010db8000000000000000000000001))
            .unwrap()
            .unwrap()
            .1,
        2u64.to_le_bytes()
    );
}
fn worker(root: &Path) {
    let image = if std::env::var("ULMDB_H5_ACT_IMAGE").as_deref() == Ok("anon") {
        ImageMode::Anonymous
    } else {
        ImageMode::File
    };
    let env = Environment::create(root.join("engine"), options(image)).unwrap();
    set(&env, 0, true);
    let old = env.begin_read();
    let held = old.get(b"kv", b"state").unwrap().unwrap();
    let mut first = env.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    env.activate_checkpoint(&mut first, CheckpointLimits::default())
        .unwrap();
    set(&env, 1, false);
    let mut second = env.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    set(&env, 2, false);
    fs::write(
        root.join("wal.before"),
        fs::read(root.join("engine/HYBRID.wal")).unwrap(),
    )
    .unwrap();
    let result = env.activate_checkpoint(&mut second, CheckpointLimits::default());
    if let Ok(point) = std::env::var("ULMDB_H5_ACT_FAIL_AT") {
        assert!(result.is_err());
        let uncertain = POINTS[3..].contains(&point.as_str());
        assert_eq!(matches!(result, Err(Error::OutcomeUnknown)), uncertain);
        assert_eq!(
            matches!(env.begin_write(), Err(Error::OutcomeUnknown)),
            uncertain
        );
    } else {
        result.unwrap();
    }
    check(&env);
    assert_eq!(held, 0u64.to_le_bytes());
    println!("H5_ACT_WORKER_DONE");
}
#[test]
fn selector_cuts_and_io_errors_keep_live_h_and_full_wal_recovery() {
    if let Some(path) = std::env::var_os("ULMDB_H5_ACT_WORKER") {
        worker(Path::new(&path));
        return;
    }
    for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
        for image in [ImageMode::File, ImageMode::Anonymous] {
            for point in POINTS {
                for exit in [false, true] {
                    let root = base.join(format!(
                        "ulmdb-h5-selector-cut-{}",
                        ulmdb_storage::new_identity().unwrap()
                    ));
                    fs::create_dir(&root).unwrap();
                    let output = Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "selector_cuts_and_io_errors_keep_live_h_and_full_wal_recovery",
                            "--nocapture",
                        ])
                        .env("ULMDB_H5_ACT_WORKER", &root)
                        .env(
                            "ULMDB_H5_ACT_IMAGE",
                            if image == ImageMode::Anonymous {
                                "anon"
                            } else {
                                "file"
                            },
                        )
                        .env("ULMDB_H5_ACT_SKIP", "1")
                        .env(
                            if exit {
                                "ULMDB_H5_ACT_EXIT_AT"
                            } else {
                                "ULMDB_H5_ACT_FAIL_AT"
                            },
                            point,
                        )
                        .env_remove(if exit {
                            "ULMDB_H5_ACT_FAIL_AT"
                        } else {
                            "ULMDB_H5_ACT_EXIT_AT"
                        })
                        .output()
                        .unwrap();
                    assert_eq!(
                        output.status.code(),
                        Some(if exit { 86 } else { 0 }),
                        "{point}: {} {}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                    if !exit {
                        assert!(
                            String::from_utf8_lossy(&output.stdout).contains("H5_ACT_WORKER_DONE")
                        );
                    }
                    let path = root.join("engine");
                    assert_eq!(
                        fs::read(path.join("HYBRID.wal")).unwrap(),
                        fs::read(root.join("wal.before")).unwrap()
                    );
                    let selected = selection::read(&path).unwrap().unwrap();
                    assert!([1, 2].contains(&selected.cut.receipt.sequence));
                    let replay = if image == ImageMode::File {
                        ImageMode::Anonymous
                    } else {
                        ImageMode::File
                    };
                    let env = Environment::open(&path, options(replay)).unwrap();
                    assert_eq!(env.sequence(), 3);
                    check(&env);
                    drop(env);
                    fs::remove_dir_all(root).unwrap();
                }
            }
        }
    }
}
