#![cfg(target_os = "linux")]
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use ulmdb::chunked::CacheOptions;
use ulmdb::{DatabaseKind, Environment, Error, PublicationMode, ReadTxn};
use ulmdb_storage::{
    chunk_cache::{ChunkCache, Limits},
    transform::{Direction, Encryption, EncryptionConfig, TransformContext},
};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "chunk-fds-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn modes() -> [PublicationMode; 3] {
    [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ]
}
fn fd_count() -> usize {
    fs::read_dir("/proc/self/fd").unwrap().count()
}
fn cache() -> CacheOptions {
    CacheOptions::new(
        ChunkCache::new(Limits {
            mapped_bytes: 128 << 10,
            windows: 8,
            chunk_bytes: 16384,
            max_window_bytes: 64 << 10,
        })
        .unwrap(),
    )
}
struct Cipher;
impl Encryption for Cipher {
    fn transform(
        &self,
        src: &[u8],
        dst: &mut [u8],
        key: &[u8],
        iv: &[u8],
        auth: &mut [u8],
        direction: Direction,
    ) -> ulmdb::Result<()> {
        use sha2::{Digest, Sha256};
        let tag = |bytes: &[u8]| {
            let mut h = Sha256::new();
            h.update(key);
            h.update(iv);
            h.update(bytes);
            h.finalize()
        };
        if direction == Direction::Decrypt && auth != &tag(src)[..16] {
            return Err(Error::CryptoFailed);
        }
        for (i, (a, b)) in src.iter().zip(dst.iter_mut()).enumerate() {
            *b = *a ^ key[i % key.len()] ^ iv[i % iv.len()]
        }
        if direction == Direction::Encrypt {
            auth.copy_from_slice(&tag(dst)[..16])
        }
        Ok(())
    }
}
fn context() -> Arc<TransformContext> {
    Arc::new(
        TransformContext::new(
            None,
            Some(EncryptionConfig {
                callback: Arc::new(Cipher),
                key: vec![17; 32],
                authentication_bytes: 16,
            }),
            32 << 20,
        )
        .unwrap(),
    )
}
fn seed(dir: &Dir, mode: PublicationMode, transform: Option<Arc<TransformContext>>, commits: u8) {
    let e = Environment::create_with_transform_options(
        &dir.0,
        mode,
        Default::default(),
        Default::default(),
        transform,
    )
    .unwrap();
    // This fixture deliberately retains many immutable sources to stress the
    // descriptor pool. Automatic compaction would erase that input condition.
    e.set_maintenance_policy(ulmdb::MaintenancePolicy::Disabled)
        .unwrap();
    for n in 0..commits {
        let mut w = e.begin_write().unwrap();
        if n == 0 {
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        }
        w.put(b"kv", b"k", &[n; 80]).unwrap();
        w.commit().unwrap();
    }
}
// RLIMIT changes apply only after exec, never to the concurrent libtest parent.
fn worker(test: &str) -> bool {
    const ROLE: &str = "ULMDB_NATIVE_CHUNK_FDS_WORKER";
    if std::env::var(ROLE).as_deref() == Ok(test) {
        let limits = fs::read_to_string("/proc/self/limits").unwrap();
        let row = limits
            .lines()
            .find(|row| row.starts_with("Max open files"))
            .unwrap();
        assert_eq!(
            row.split_whitespace().nth(3),
            Some("64"),
            "worker must actually run under RLIMIT64"
        );
        return true;
    }
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }
    let program = "import os,resource,sys;soft,hard=resource.getrlimit(resource.RLIMIT_NOFILE);assert hard==resource.RLIM_INFINITY or hard>=64;resource.setrlimit(resource.RLIMIT_NOFILE,(64,hard));os.execv(sys.argv[1],sys.argv[1:])";
    let mut child = Child(
        Command::new("python3")
            .args(["-c", program])
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(ROLE, test)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "low-FD exec worker failed: {status}");
            return false;
        }
        assert!(Instant::now() < deadline, "low-FD worker45s deadline");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn many_plain_and_encrypted_segments_open_under_low_rlimit_without_descriptor_leaks() {
    if !worker("many_plain_and_encrypted_segments_open_under_low_rlimit_without_descriptor_leaks") {
        return;
    }
    // The process-wide entropy descriptor is intentionally persistent and is
    // unrelated to cache ownership. Initialize it before the leak baseline.
    let _ = ulmdb_storage::new_identity().unwrap();
    for mode in modes() {
        for encrypted in [false, true] {
            let before = fd_count();
            let d = Dir::new();
            let transform = encrypted.then(context);
            seed(&d, mode, transform.clone(), 140);
            let config = cache();
            let ctl = config.cache.clone();
            let old = ReadTxn::open_coordinated_snapshot_with_cache_options(
                &d.0,
                Default::default(),
                transform.clone(),
                Some(config.clone()),
            )
            .unwrap();
            assert!(old.uses_chunk_cache());
            let segments = old.file_manifest().unwrap().segments.len();
            assert!(segments >= 140);
            println!(
                "LOW_FD_OPEN mode={mode:?} encrypted={encrypted} segments={segments} baseline_fds={before}"
            );
            let held = old.get(b"kv", b"k").unwrap().unwrap();
            assert_eq!(held, [139; 80]);
            assert!(
                fd_count() < before + 24,
                "retained descriptors scale with segment count"
            );
            let e = Environment::open_with_cache_options(
                &d.0,
                Default::default(),
                Default::default(),
                transform,
                Some(config),
            )
            .unwrap();
            let mut w = e.begin_write().unwrap();
            w.put(b"kv", b"k", b"new").unwrap();
            w.commit().unwrap();
            e.compact().unwrap();
            e.prune_inventory().unwrap();
            e.reclaim().unwrap();
            drop(e);
            assert_eq!(old.get(b"kv", b"k").unwrap(), Some([139; 80].as_slice()));
            assert_eq!(held, [139; 80]);
            drop(old);
            drop(ctl);
            drop(d);
            assert_eq!(fd_count(), before, "source descriptors leaked");
            println!("LOW_FD_CASE_PASS {mode:?} encrypted={encrypted}");
        }
    }
}
#[test]
fn cached_source_replacements_and_nonregular_paths_fail_closed_preserving_old_borrows() {
    if !worker("cached_source_replacements_and_nonregular_paths_fail_closed_preserving_old_borrows")
    {
        return;
    }
    let _ = ulmdb_storage::new_identity().unwrap();
    for mode in modes() {
        for action in ["same-bytes-inode", "symlink", "fifo", "directory"] {
            let before = fd_count();
            let d = Dir::new();
            seed(&d, mode, None, 1);
            let config = cache();
            let r = ReadTxn::open_coordinated_snapshot_with_cache_options(
                &d.0,
                Default::default(),
                None,
                Some(config.clone()),
            )
            .unwrap();
            let held = r.get(b"kv", b"k").unwrap().unwrap();
            let entry = r
                .file_manifest()
                .unwrap()
                .segments
                .iter()
                .next()
                .unwrap()
                .clone();
            let file = ulmdb_storage::segment_path(&d.0, &entry);
            let moved = d.0.with_extension("moved");
            match action {
                "same-bytes-inode" => {
                    let saved = file.with_extension("saved");
                    fs::rename(&file, &saved).unwrap();
                    fs::copy(saved, &file).unwrap();
                }
                "symlink" => {
                    let saved = file.with_extension("saved");
                    fs::rename(&file, &saved).unwrap();
                    std::os::unix::fs::symlink(saved, &file).unwrap();
                }
                "fifo" => {
                    fs::remove_file(&file).unwrap();
                    assert!(
                        Command::new("mkfifo")
                            .arg(&file)
                            .status()
                            .unwrap()
                            .success()
                    );
                }
                "directory" => {
                    fs::rename(&d.0, &moved).unwrap();
                    fs::create_dir(&d.0).unwrap();
                }
                _ => unreachable!(),
            }
            assert!(
                r.get(b"kv", b"k").is_err(),
                "replaced {action} silently served cached data"
            );
            assert_eq!(held, [0; 80]);
            drop(r);
            drop(config);
            drop(d);
            let _ = fs::remove_dir_all(moved);
            assert_eq!(fd_count(), before);
            println!("REPLACEMENT_CASE_PASS {mode:?} action={action}");
        }
    }
}
