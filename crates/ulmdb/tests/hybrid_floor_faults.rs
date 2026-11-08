#![cfg(feature = "fault-injection")]
//! Process exits/IO injection, not a power-loss simulation. No parent owns Env
//! while spawning a worker; original writer OFDs cannot leak across sibling fork.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use ulmdb::{
    DatabaseKind, Error,
    hybrid::{Environment, ImageMode, Options, WalLayout},
};
use ulmdb_storage::hybrid::{
    checkpoint::CheckpointLimits,
    retirement::selection::{RecoveryBase, read_typed},
};
const POINTS: [&str; 12] = [
    "h5.retire.before_selector_write",
    "h5.retire.after_selector_write",
    "h5.retire.after_selector_file_sync",
    "h5.retire.before_required_write",
    "h5.retire.after_required_write",
    "h5.retire.after_required_file_sync",
    "h5.retire.before_selector_rename",
    "h5.retire.after_selector_rename",
    "h5.retire.after_selector_directory_sync",
    "h5.retire.before_required_rename",
    "h5.retire.after_required_rename",
    "h5.retire.after_required_directory_sync",
];
struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn options(image: ImageMode, layout: WalLayout) -> Options {
    Options {
        image,
        wal_layout: layout,
        chunk: ulmdb_storage::hybrid::ChunkLimits {
            capacity: 1024 * 1024,
            reserve_quantum: 65536,
            max_burned_bytes: 512 * 1024,
        },
        max_image_bytes: 8 * 1024 * 1024,
        max_wal_bytes: 4 * 1024 * 1024,
        ..Options::default()
    }
}
fn set(e: &Environment, n: u64) {
    let mut w = e.begin_write().unwrap();
    if n == 0 {
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    }
    w.put(b"kv", b"state", &n.to_le_bytes()).unwrap();
    w.commit().unwrap();
}
fn worker(path: &Path, image: ImageMode, layout: WalLayout, phase: &str) {
    let e = Environment::create(path, options(image, layout)).unwrap();
    for n in 0..3 {
        set(&e, n);
    }
    let mut cp = e.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    e.activate_checkpoint(&mut cp, CheckpointLimits::default())
        .unwrap();
    drop(cp);
    set(&e, 3);
    let old = e.begin_read();
    let bytes = old.get(b"kv", b"state").unwrap().unwrap();
    let mut p = e.prepare_retirement(CheckpointLimits::default()).unwrap();
    if phase != "first" {
        e.activate_retirement(&mut p).unwrap();
    }
    if phase == "repair" {
        let wal = fs::read(path.join("HYBRID.wal")).unwrap();
        drop(p);
        drop(e);
        fs::remove_file(path.join("HYBRID.required")).unwrap();
        let result = Environment::open(path, options(image, layout));
        assert!(matches!(
            result,
            Err(Error::Io(_)) | Err(Error::OutcomeUnknown)
        ));
        assert_eq!(bytes, 3u64.to_le_bytes());
        assert_eq!(wal, fs::read(path.join("HYBRID.wal")).unwrap());
        println!("H5_FLOOR_WORKER_DONE");
        return;
    }
    let mut cp = if phase == "carry" {
        set(&e, 4);
        Some(e.prepare_checkpoint(CheckpointLimits::default()).unwrap())
    } else {
        None
    };
    let wal = fs::read(path.join("HYBRID.wal")).unwrap();
    let result = if let Some(cp) = cp.as_mut() {
        e.activate_checkpoint(cp, CheckpointLimits::default())
            .map(|_| ())
    } else {
        e.activate_retirement(&mut p).map(|_| ())
    };
    if let Ok(point) = std::env::var("ULMDB_H5_ACT_FAIL_AT") {
        if point.contains("rename")
            || point.ends_with("selector_directory_sync")
            || point.ends_with("required_directory_sync")
        {
            assert!(matches!(result, Err(Error::OutcomeUnknown)));
            assert!(matches!(e.begin_write(), Err(Error::OutcomeUnknown)));
        } else {
            assert!(matches!(result, Err(Error::Io(_))));
            drop(e.begin_write().unwrap());
        }
    } else {
        result.unwrap();
    }
    assert_eq!(e.sequence(), if phase == "carry" { 5 } else { 4 });
    assert_eq!(bytes, 3u64.to_le_bytes());
    assert_eq!(wal, fs::read(path.join("HYBRID.wal")).unwrap());
    println!("H5_FLOOR_WORKER_DONE");
}
fn child(mut command: Command) -> std::process::Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let out = child.wait_with_output().unwrap();
            panic!("floor worker timed out: {out:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.wait_with_output().unwrap()
}
#[test]
fn compound_and_required_marker_cuts_preserve_h_and_never_fallback_from_required() {
    if let Some(path) = std::env::var_os("ULMDB_H5_FLOOR_WORKER") {
        let image = if std::env::var("ULMDB_H5_FLOOR_IMAGE").as_deref() == Ok("anon") {
            ImageMode::Anonymous
        } else {
            ImageMode::File
        };
        let layout = if std::env::var("ULMDB_H5_FLOOR_LAYOUT").as_deref() == Ok("fixed") {
            WalLayout::PreSized {
                capacity: 4 * 1024 * 1024,
            }
        } else {
            WalLayout::Dense
        };
        worker(
            Path::new(&path),
            image,
            layout,
            &std::env::var("ULMDB_H5_FLOOR_PHASE").unwrap(),
        );
        return;
    }
    let mut scenarios = 0;
    for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
        for image in [ImageMode::File, ImageMode::Anonymous] {
            for layout in [
                WalLayout::Dense,
                WalLayout::PreSized {
                    capacity: 4 * 1024 * 1024,
                },
            ] {
                for phase in ["first", "carry", "repair"] {
                    for point in POINTS {
                        if (phase == "carry" && point.contains("required"))
                            || (phase == "repair" && !point.contains("required"))
                        {
                            continue;
                        }
                        for crash in [false, true] {
                            let root = Dir(base.join(format!(
                                "ulmdb-floor-cut-{}",
                                ulmdb_storage::new_identity().unwrap()
                            )));
                            let mut c = Command::new(std::env::current_exe().unwrap());
                            c.args(["--exact","compound_and_required_marker_cuts_preserve_h_and_never_fallback_from_required","--nocapture"]).env("ULMDB_H5_FLOOR_WORKER",&root.0).env("ULMDB_H5_FLOOR_IMAGE",if image==ImageMode::Anonymous{"anon"}else{"file"}).env("ULMDB_H5_FLOOR_LAYOUT",if layout==WalLayout::Dense{"dense"}else{"fixed"}).env(if crash{"ULMDB_H5_ACT_EXIT_AT"}else{"ULMDB_H5_ACT_FAIL_AT"},point).env_remove(if crash{"ULMDB_H5_ACT_FAIL_AT"}else{"ULMDB_H5_ACT_EXIT_AT"}).env("ULMDB_H5_ACT_SKIP",if phase=="first"{"0"}else{"1"}).env("ULMDB_H5_FLOOR_PHASE",phase);
                            let out = child(c);
                            if crash {
                                assert_eq!(out.status.code(), Some(86), "{point}: {out:?}");
                            } else {
                                assert!(
                                    out.status.success()
                                        && String::from_utf8_lossy(&out.stdout)
                                            .contains("H5_FLOOR_WORKER_DONE"),
                                    "{point}: {out:?}"
                                );
                            }
                            let wal = fs::read(root.0.join("HYBRID.wal")).unwrap();
                            let e = Environment::open(&root.0, options(image, layout)).unwrap();
                            assert_eq!(e.sequence(), if phase == "carry" { 5 } else { 4 });
                            assert_eq!(
                                e.begin_read().get(b"kv", b"state").unwrap(),
                                Some(
                                    (if phase == "carry" { 4u64 } else { 3u64 })
                                        .to_le_bytes()
                                        .as_slice()
                                )
                            );
                            assert_eq!(wal, fs::read(root.0.join("HYBRID.wal")).unwrap());
                            if root.0.join("HYBRID.required").exists() {
                                assert!(matches!(
                                    read_typed(&root.0).unwrap(),
                                    Some(RecoveryBase::Compound(_))
                                ));
                            }
                            set(&e, 5);
                            drop(e);
                            let e = Environment::open(&root.0, options(image, layout)).unwrap();
                            assert_eq!(e.sequence(), if phase == "carry" { 6 } else { 5 });
                            assert_eq!(
                                e.begin_read().get(b"kv", b"state").unwrap(),
                                Some(5u64.to_le_bytes().as_slice())
                            );
                            scenarios += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(scenarios, 384);
    eprintln!("H5_FLOOR_FAULT_SCENARIOS {scenarios}");
}
