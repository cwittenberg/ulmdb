#![cfg(target_os = "linux")]
//! Focused actual-syscall worker for finalized encoded MAPASYNC output.
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::MetadataExt, path::PathBuf, sync::Arc};
use ulmdb::{
    DatabaseKind, Durability, Environment, MappedFlush, PublicationMode, ReadAhead, ResidencyPolicy,
};
use ulmdb_storage::transform::{Checksum, ChecksumConfig, TransformContext};
struct Sum;
impl Checksum for Sum {
    fn compute(&self, src: &[u8], dst: &mut [u8], _: Option<&[u8]>) -> ulmdb::Result<()> {
        dst.copy_from_slice(&Sha256::digest(src));
        Ok(())
    }
}
#[test]
#[ignore]
fn checksum_async_worker() {
    let Ok(root) = std::env::var("ULMDB_CHECKSUM_ASYNC_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let mode = match std::env::var("ULMDB_CHECKSUM_ASYNC_FORMAT")
        .unwrap()
        .as_str()
    {
        "2" => PublicationMode::Legacy,
        "3" => PublicationMode::Cohort,
        "4" => PublicationMode::Indexed,
        _ => panic!(),
    };
    let fixed = std::env::var("ULMDB_CHECKSUM_ASYNC_FIXED").unwrap() == "1";
    let external = std::env::var("ULMDB_CHECKSUM_ASYNC_EXTERNAL").unwrap() == "1";
    let ctx = Arc::new(
        TransformContext::new(
            Some(ChecksumConfig {
                callback: Arc::new(Sum),
                bytes: 32,
            }),
            None,
            32 << 20,
        )
        .unwrap(),
    );
    let mut e = Environment::create_with_transform(&root, mode, ctx.clone()).unwrap();
    if external {
        drop(e);
        e = Environment::open_externally_synchronized_with_transform_options(
            &root,
            ResidencyPolicy::Demand,
            ReadAhead::Normal,
            Some(ctx.clone()),
        )
        .unwrap();
    }
    if fixed {
        e.enable_fixed_map(4 << 20, None).unwrap();
    }
    let mut w = e.begin_write().unwrap();
    w.create_database(b"data", DatabaseKind::Kv).unwrap();
    w.put(b"data", b"k", b"A").unwrap();
    w.commit().unwrap();
    let old = e.begin_read();
    let a = old.get(b"data", b"k").unwrap().unwrap();
    let before = fs::read(root.join("CURRENT")).unwrap();
    let mut w = e.begin_write().unwrap();
    w.enable_write_map(1 << 20, MappedFlush::Synchronous)
        .unwrap();
    w.set_mapped_flush(MappedFlush::Asynchronous).unwrap();
    let mapped = w.mapped_allocation().unwrap().unwrap();
    let cap = w.reserve(b"data", b"mapped", 48_000).unwrap();
    w.reservation_value_mut(&cap).unwrap().fill(9);
    w.end_reservation(cap).unwrap();
    w.put(b"data", b"k", b"B").unwrap();
    eprintln!("ENCODED prepare.begin");
    let prepared = w.prepare();
    eprintln!("ENCODED prepare.end");
    if std::env::var_os("ULMDB_EXPECT_MSYNC_EIO").is_some() {
        match prepared {
            Err(ulmdb::Error::Io(e)) => assert_eq!(e.raw_os_error(), Some(libc::EIO)),
            _ => panic!("expected actual encoded msync EIO"),
        };
        assert_eq!(fs::read(root.join("CURRENT")).unwrap(), before);
        assert_eq!(
            e.begin_read().get(b"data", b"k").unwrap(),
            Some(b"A".as_slice())
        );
        assert_eq!(a, b"A");
        let mut retry = e.begin_write().unwrap();
        retry.put(b"data", b"k", b"recovered").unwrap();
        assert_eq!(retry.commit().unwrap(), 2);
        assert_eq!(
            e.begin_read().get(b"data", b"k").unwrap(),
            Some(b"recovered".as_slice())
        );
        println!("EXPECTED EIO DONE");
        return;
    }
    let p = prepared.unwrap();
    assert_eq!(p.data_policy(), Durability::NoSync);
    eprintln!("ENCODED final.begin");
    p.commit_with_metadata_policy(Durability::NoSync).unwrap();
    eprintln!("ENCODED final.end");
    assert_eq!(a, b"A");
    let read = e.begin_read();
    let bytes = read.get(b"data", b"mapped").unwrap().unwrap();
    assert_eq!(bytes, vec![9; 48_000]);
    if fixed {
        let info = e.fixed_map_info().unwrap().unwrap();
        assert!((info.address..info.address + info.capacity).contains(&(bytes.as_ptr() as usize)));
    }
    let manifest = read.file_manifest().unwrap();
    let entry = manifest.segments.last().unwrap();
    let path = ulmdb_storage::segment_path(&root, entry);
    assert_ne!(fs::metadata(&path).unwrap().ino(), mapped.file_inode);
    println!("ENCODED PATH {}", path.display());
    let selected = fs::read(root.join("CURRENT")).unwrap();
    e.set_durability(Durability::NoSync).unwrap();
    eprintln!("ENCODED skip.begin");
    assert!(e.sync_with_force(false).unwrap().is_none());
    eprintln!("ENCODED skip.end");
    eprintln!("ENCODED async.begin");
    if external {
        e.begin_read()
            .external_synchronization()
            .unwrap()
            .flush_async(Some(ctx.clone()))
            .unwrap();
    } else {
        ulmdb_storage::flush_async_file_environment(&root, Some(ctx.clone())).unwrap();
    }
    eprintln!("ENCODED async.end");
    eprintln!("ENCODED force.begin");
    e.sync().unwrap();
    eprintln!("ENCODED force.end");
    assert_eq!(fs::read(root.join("CURRENT")).unwrap(), selected);
    assert_eq!(a, b"A");
    drop(read);
    drop(old);
    drop(e);
    let reopened = if fixed {
        Environment::open_fixed_map_with_transform_options(
            &root,
            ResidencyPolicy::Demand,
            ReadAhead::Normal,
            Some(ctx.clone()),
        )
        .unwrap()
    } else {
        Environment::open_with_transform(&root, ctx.clone()).unwrap()
    };
    assert_eq!(
        reopened
            .begin_read()
            .get(b"data", b"mapped")
            .unwrap()
            .unwrap(),
        vec![9; 48_000]
    );
    println!("ENCODED DONE");
}

/// The worker deliberately has no subprocesses. Its isolated exec owns its
/// environment and FD table; a failed or timed-out worker is killed and reaped.
#[test]
fn checksum_async_borrows_reopen_and_sync_across_all_file_roles() {
    use std::{
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };
    struct Directory(PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    struct Worker(Option<Child>);
    impl Drop for Worker {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let directory = Directory(std::env::temp_dir().join(format!(
        "checksum-async-default-{:032x}",
        ulmdb_storage::new_identity().unwrap()
    )));
    fs::create_dir(&directory.0).unwrap();
    let executable = std::env::current_exe().unwrap();
    for format in ["2", "3", "4"] {
        for fixed in ["0", "1"] {
            for external in ["0", "1"] {
                let root = directory.0.join(format!("{format}-{fixed}-{external}"));
                let mut command = Command::new(&executable);
                command
                    .args([
                        "--exact",
                        "checksum_async_worker",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("ULMDB_CHECKSUM_ASYNC_ROOT", root)
                    .env("ULMDB_CHECKSUM_ASYNC_FORMAT", format)
                    .env("ULMDB_CHECKSUM_ASYNC_FIXED", fixed)
                    .env("ULMDB_CHECKSUM_ASYNC_EXTERNAL", external)
                    .env_remove("ULMDB_EXPECT_MSYNC_EIO")
                    .env_remove("ULMDB_FAIL_AT")
                    .env_remove("ULMDB_CRASH_AT")
                    .env_remove("ULMDB_PAUSE_AT")
                    .env_remove("ULMDB_TRACE_FILE")
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                let mut worker = Worker(Some(command.spawn().unwrap()));
                let deadline = Instant::now() + Duration::from_secs(15);
                loop {
                    if worker.0.as_mut().unwrap().try_wait().unwrap().is_some() {
                        break;
                    }
                    if Instant::now() >= deadline {
                        let _ = worker.0.as_mut().unwrap().kill();
                        let output = worker.0.take().unwrap().wait_with_output().unwrap();
                        panic!(
                            "{format}/{fixed}/{external} worker deadline: {}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                let output = worker.0.take().unwrap().wait_with_output().unwrap();
                assert!(
                    output.status.success(),
                    "{format}/{fixed}/{external}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(String::from_utf8_lossy(&output.stdout).contains("ENCODED DONE"));
            }
        }
    }
}
