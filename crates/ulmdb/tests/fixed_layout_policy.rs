#![cfg(target_os = "linux")]
//! Worker for focused owned-process strace evidence. No power-loss claim.
use std::{fs, path::PathBuf};
use ulmdb::{CommitIntent, DatabaseKind, Durability, Environment, MappedFlush, PublicationMode};

#[test]
fn fixed_layout_policies_keep_borrows_reopen_and_sync_with_caller_owned_gates() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    struct Directory(PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let directory = Directory(std::env::temp_dir().join(format!(
        "ulmdb-fixed-policies-{:032x}",
        ulmdb_storage::new_identity().unwrap()
    )));
    fs::create_dir(&directory.0).unwrap();
    let executable = std::env::current_exe().unwrap();
    for format in ["2", "3", "4"] {
        for external in ["0", "1"] {
            for (policy, mapped) in [("full", "0"), ("meta", "0"), ("none", "0"), ("none", "1")] {
                let root = directory
                    .0
                    .join(format!("{format}-{external}-{policy}-{mapped}"));
                let mut child = Command::new(&executable)
                    .args([
                        "--exact",
                        "fixed_layout_policy_worker",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("ULMDB_FIXED_LAYOUT_WORKER", root)
                    .env("ULMDB_FIXED_LAYOUT_FORMAT", format)
                    .env("ULMDB_FIXED_LAYOUT_EXTERNAL", external)
                    .env("ULMDB_FIXED_LAYOUT_POLICY", policy)
                    .env("ULMDB_FIXED_LAYOUT_MAPPED", mapped)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let deadline = Instant::now() + Duration::from_secs(20);
                loop {
                    if child.try_wait().unwrap().is_some() {
                        break;
                    }
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let output = child.wait_with_output().unwrap();
                        panic!(
                            "fixed policy child timed out: {}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                let output = child.wait_with_output().unwrap();
                assert!(
                    output.status.success(),
                    "{format}/{external}/{policy}/{mapped}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(String::from_utf8_lossy(&output.stdout).contains("LAYOUT DONE"));
            }
        }
    }
}

#[test]
#[ignore]
fn fixed_layout_policy_worker() {
    let Ok(root) = std::env::var("ULMDB_FIXED_LAYOUT_WORKER") else {
        return;
    };
    let root = PathBuf::from(root);
    let mode = match std::env::var("ULMDB_FIXED_LAYOUT_FORMAT").unwrap().as_str() {
        "2" => PublicationMode::Legacy,
        "3" => PublicationMode::Cohort,
        "4" => PublicationMode::Indexed,
        _ => panic!(),
    };
    let policy = match std::env::var("ULMDB_FIXED_LAYOUT_POLICY").unwrap().as_str() {
        "full" => Durability::Full,
        "meta" => Durability::NoMetaSync,
        "none" => Durability::NoSync,
        _ => panic!(),
    };
    let external = std::env::var("ULMDB_FIXED_LAYOUT_EXTERNAL").unwrap() == "1";
    let mapped = std::env::var("ULMDB_FIXED_LAYOUT_MAPPED").unwrap() == "1";
    let mut e = Environment::create_fixed_map(&root, mode, 4 << 20, None).unwrap();
    if external {
        drop(e);
        e = Environment::open_externally_synchronized(&root).unwrap();
        e.bind_fixed_map_layout(ulmdb::FixedMap::open_namespace(&root).unwrap())
            .unwrap();
    }
    let mut w = e.begin_write().unwrap();
    w.create_database(b"data", DatabaseKind::Kv).unwrap();
    w.put(b"data", b"k", &vec![7; 48_000]).unwrap();
    w.commit().unwrap();
    let old = e.begin_read();
    let a = old.get(b"data", b"k").unwrap().unwrap();
    // Genuine NOLOCK must progress with all standard kernel gates occupied.
    let _locks: Vec<fs::File> = if external {
        [
            "LOCK",
            ".ulmdb-coordination/capture",
            ".ulmdb-coordination/history",
        ]
        .into_iter()
        .map(|name| {
            let f = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(root.join(name))
                .unwrap();
            f.lock().unwrap();
            f
        })
        .collect()
    } else {
        Vec::new()
    };
    e.set_durability(policy).unwrap();
    let mut w = e.begin_write().unwrap();
    if mapped {
        w.enable_write_map(
            1 << 20,
            if policy == Durability::NoSync {
                MappedFlush::None
            } else {
                MappedFlush::Synchronous
            },
        )
        .unwrap();
    }
    w.put(b"data", b"k", &vec![9; 48_000]).unwrap();
    eprintln!("LAYOUT prepare.begin");
    let p = w
        .prepare_with_data_policy(CommitIntent::ForcePublication, policy)
        .unwrap();
    eprintln!("LAYOUT prepare.end");
    assert_eq!(p.data_policy(), policy);
    eprintln!("LAYOUT final.begin");
    p.commit_with_metadata_policy(policy).unwrap();
    eprintln!("LAYOUT final.end");
    assert_eq!(a, vec![7; 48_000]);
    assert_eq!(
        e.begin_read().get(b"data", b"k").unwrap().unwrap(),
        vec![9; 48_000]
    );
    let current = fs::read(root.join("CURRENT")).unwrap();
    let layout = fs::read(root.join("FIXEDMAP")).unwrap();
    e.set_durability(Durability::NoSync).unwrap();
    eprintln!("LAYOUT skip.begin");
    assert!(e.sync_with_force(false).unwrap().is_none());
    eprintln!("LAYOUT skip.end");
    eprintln!("LAYOUT async.begin");
    let async_report = if external {
        e.begin_read()
            .external_synchronization()
            .unwrap()
            .flush_async(None)
            .unwrap()
    } else {
        ulmdb_storage::flush_async_file_environment(&root, None).unwrap()
    };
    eprintln!("LAYOUT async.end");
    assert!(async_report.files_scheduled >= 3);
    eprintln!("LAYOUT force.begin");
    let report = e.sync().unwrap();
    eprintln!("LAYOUT force.end");
    assert!(report.files_synced >= 3);
    assert_eq!(fs::read(root.join("CURRENT")).unwrap(), current);
    assert_eq!(fs::read(root.join("FIXEDMAP")).unwrap(), layout);
    assert_eq!(a, vec![7; 48_000]);
    assert_eq!(
        e.begin_read().get(b"data", b"k").unwrap().unwrap(),
        vec![9; 48_000]
    );
    drop(_locks);
    drop(old);
    drop(e);
    let reopened = Environment::open_fixed_map(&root).unwrap();
    assert_eq!(
        reopened.begin_read().get(b"data", b"k").unwrap().unwrap(),
        vec![9; 48_000]
    );
    println!("LAYOUT DONE");
}

#[test]
#[ignore]
fn fixed_layout_failure_worker() {
    let Ok(root) = std::env::var("ULMDB_FIXED_LAYOUT_FAILURE_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let role = std::env::var("ULMDB_FIXED_LAYOUT_ROLE").unwrap();
    if role == "force" {
        eprintln!("FORCER START pid={}", std::process::id());
        ulmdb_storage::sync_file_environment(&root).unwrap();
        println!("FORCER DONE");
        return;
    }
    if role == "seed" {
        let mode = match std::env::var("ULMDB_FIXED_LAYOUT_FORMAT").unwrap().as_str() {
            "2" => PublicationMode::Legacy,
            "3" => PublicationMode::Cohort,
            "4" => PublicationMode::Indexed,
            _ => panic!(),
        };
        let e = Environment::create_fixed_map(&root, mode, 4 << 20, None).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"data", DatabaseKind::Kv).unwrap();
        w.put(b"data", b"k", b"A").unwrap();
        w.commit().unwrap();
        println!("SEEDED");
        return;
    }
    let e = Environment::open_fixed_map(&root).unwrap();
    let before = fs::read(root.join("CURRENT")).unwrap();
    let mut w = e.begin_write().unwrap();
    w.put(b"data", b"k", b"B").unwrap();
    if role == "failure" {
        assert!(w.prepare().is_err());
        assert_eq!(fs::read(root.join("CURRENT")).unwrap(), before);
        assert_eq!(
            e.begin_read().get(b"data", b"k").unwrap(),
            Some(b"A".as_slice())
        );
        drop(e.begin_write().unwrap());
        println!("FAILED SAFELY");
    } else {
        w.commit().unwrap();
        assert_eq!(
            e.begin_read().get(b"data", b"k").unwrap(),
            Some(b"B".as_slice())
        );
        println!("PUBLISHED");
    }
}

#[test]
#[ignore]
fn fixed_layout_reentrant_checksum_worker() {
    use sha2::{Digest, Sha256};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::{Duration, Instant};
    use ulmdb_storage::transform::{Checksum, ChecksumConfig, TransformContext};
    let Ok(root) = std::env::var("ULMDB_FIXED_LAYOUT_FAILURE_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    struct Sum {
        fixed: Mutex<Option<Arc<ulmdb::FixedMap>>>,
        pause: AtomicBool,
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl Checksum for Sum {
        fn compute(&self, src: &[u8], dst: &mut [u8], _: Option<&[u8]>) -> ulmdb::Result<()> {
            if self.pause.swap(false, Ordering::AcqRel) {
                self.entered.send(()).unwrap();
                self.release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(8))
                    .unwrap();
            }
            let fixed = self.fixed.lock().unwrap().clone();
            if let Some(fixed) = fixed {
                fixed.info()?;
            }
            dst.copy_from_slice(&Sha256::digest(src));
            Ok(())
        }
    }
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, receiver) = mpsc::sync_channel(1);
    let sum = Arc::new(Sum {
        fixed: Mutex::new(None),
        pause: AtomicBool::new(false),
        entered,
        release: Mutex::new(receiver),
    });
    let ctx = Arc::new(
        TransformContext::new(
            Some(ChecksumConfig {
                callback: sum.clone(),
                bytes: 32,
            }),
            None,
            16 << 20,
        )
        .unwrap(),
    );
    let mode = match std::env::var("ULMDB_FIXED_LAYOUT_FORMAT").unwrap().as_str() {
        "2" => PublicationMode::Legacy,
        "3" => PublicationMode::Cohort,
        "4" => PublicationMode::Indexed,
        _ => panic!(),
    };
    let mut e = Environment::create_with_transform(&root, mode, ctx).unwrap();
    e.enable_fixed_map(4 << 20, None).unwrap();
    *sum.fixed.lock().unwrap() = Some(ulmdb::FixedMap::open_namespace(&root).unwrap());
    let mut w = e.begin_write().unwrap();
    w.create_database(b"data", DatabaseKind::Kv).unwrap();
    w.put(b"data", b"k", b"A").unwrap();
    w.commit().unwrap();
    let e = Arc::new(e);
    let old = e.begin_read();
    let trace = PathBuf::from(std::env::var("ULMDB_TRACE_FILE").unwrap());
    let before = fs::metadata(&trace).unwrap().len() as usize;
    sum.pause.store(true, Ordering::Release);
    let force = {
        let e = e.clone();
        std::thread::spawn(move || e.sync().unwrap())
    };
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let writer = {
        let e = e.clone();
        std::thread::spawn(move || {
            let mut w = e.begin_write().unwrap();
            w.put(b"data", b"k", b"B").unwrap();
            w.commit().unwrap()
        })
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut blocked = false;
    while Instant::now() < deadline {
        let points = fs::read_to_string(&trace).unwrap();
        let gated = points[before..].contains("fixed.layout.before_gate");
        let waiting = fs::read_dir("/proc/self/task").unwrap().flatten().any(|t| {
            fs::read_to_string(t.path().join("wchan"))
                .is_ok_and(|w| w.contains("locks_lock") || w.contains("flock"))
        });
        if gated && waiting {
            blocked = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    release.send(()).unwrap();
    assert!(blocked, "writer did not wait behind force History");
    force.join().unwrap();
    assert_eq!(writer.join().unwrap(), 2);
    assert_eq!(old.get(b"data", b"k").unwrap(), Some(b"A".as_slice()));
    assert_eq!(
        e.begin_read().get(b"data", b"k").unwrap(),
        Some(b"B".as_slice())
    );
    println!("REENTRANT DONE");
}
