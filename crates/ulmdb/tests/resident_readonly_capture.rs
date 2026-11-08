#![cfg(target_os = "linux")]
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};
use ulmdb::{
    DatabaseKind, Error,
    resident::{CaptureLimits, Environment, Options, ReadTxn, RedoEncoding, RedoFormat},
};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "resident-capture-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn options() -> Options {
    let mut o = Options::default();
    o.pool.kv_pages = 1024;
    o.pool.tiny_pages = 64;
    o.pool.max_reserved_bytes = 64 << 20;
    o.pool.max_metadata_bytes = 48 << 20;
    o.wal_capacity = 8 << 20;
    o
}
fn modes() -> [(RedoFormat, RedoEncoding); 4] {
    [
        (RedoFormat::LiteralV1, RedoEncoding::Owned),
        (RedoFormat::CompactV2, RedoEncoding::Owned),
        (RedoFormat::FullPostimageV2, RedoEncoding::Owned),
        (RedoFormat::CompactV2, RedoEncoding::Borrowed),
    ]
}
fn tree(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = vec![];
    for entry in fs::read_dir(path).unwrap() {
        let e = entry.unwrap();
        if e.file_type().unwrap().is_file() {
            out.push((
                e.file_name().to_string_lossy().into_owned(),
                Sha256::digest(fs::read(e.path()).unwrap()).to_vec(),
            ));
        }
    }
    out.sort();
    out
}
fn seed(env: &Environment) {
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"k", b"old").unwrap();
    w.commit().unwrap();
}
fn wait(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "worker did not reach handshake"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn finish(child: &mut Child) {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "capture worker {status}");
            return;
        }
        if start.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("capture worker timed out")
        };
        thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn capture_process_worker() {
    let Some(path) = std::env::var_os("RESIDENT_CAPTURE_WORKER") else {
        return;
    };
    let path = PathBuf::from(path);
    let read = ReadTxn::open_coordinated_snapshot(&path, options()).unwrap();
    assert_eq!(read.sequence(), 1);
    let old = read.get(b"kv", b"k").unwrap().unwrap();
    let mut cursor = read
        .kv(b"kv")
        .unwrap()
        .cursor(
            std::ops::Bound::Unbounded,
            std::ops::Bound::Unbounded,
            false,
        )
        .unwrap();
    fs::write(path.join("reader-ready"), b"ok").unwrap();
    wait(&path.join("reader-release"));
    assert_eq!(old, b"old");
    assert_eq!(
        cursor.next().unwrap().unwrap(),
        (b"k".as_slice(), b"old".as_slice())
    );
    assert_eq!(
        ReadTxn::open_coordinated_snapshot(&path, options())
            .unwrap()
            .get(b"kv", b"k")
            .unwrap(),
        Some(b"new".as_slice())
    );
}
#[test]
fn independent_process_capture_old_borrows_survive_live_commit_rotation_and_owner_drop() {
    for (format, encoding) in modes() {
        let dir = Dir::new();
        let env = Environment::create_with_encoding(&dir.0, options(), format, encoding).unwrap();
        seed(&env);
        assert!(matches!(
            ReadTxn::open_coordinated_snapshot(&dir.0, options()),
            Err(Error::InvalidInput(_))
        ));
        env.enable_readonly_capture().unwrap();
        env.enable_readonly_capture().unwrap();
        let original = env.begin_read().commit_identity();
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "capture_process_worker", "--nocapture"])
                .env("RESIDENT_CAPTURE_WORKER", &dir.0)
                .spawn()
                .unwrap(),
        );
        wait(&dir.0.join("reader-ready"));
        let captured = ReadTxn::open_coordinated_snapshot(&dir.0, options()).unwrap();
        let borrow = captured.get(b"kv", b"k").unwrap().unwrap();
        assert_eq!(captured.commit_identity(), original);
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"k", b"new").unwrap();
        assert_eq!(w.commit().unwrap(), 2);
        env.collect().unwrap();
        env.checkpoint().unwrap();
        env.grow_wal(16 << 20).unwrap();
        fs::write(dir.0.join("reader-release"), b"go").unwrap();
        finish(&mut child.0);
        drop(env);
        let reopened = Environment::open_with_encoding(
            &dir.0,
            {
                let mut o = options();
                o.wal_capacity = 16 << 20;
                o
            },
            format,
            encoding,
        )
        .unwrap();
        assert_eq!(reopened.begin_read().sequence(), 2);
        drop(reopened);
        fs::remove_dir_all(&dir.0).unwrap();
        assert_eq!(borrow, b"old");
    }
}
#[test]
fn shared_capture_excludes_publication_checkpoint_and_prune_without_poison() {
    for (format, encoding) in modes() {
        let dir = Dir::new();
        let env = Environment::create_with_encoding(&dir.0, options(), format, encoding).unwrap();
        seed(&env);
        env.enable_readonly_capture().unwrap();
        let lease = ulmdb_storage::resident::ReadCapture::open(&dir.0).unwrap();
        let before = tree(&dir.0);
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"k", b"blocked").unwrap();
        assert!(matches!(w.commit(), Err(Error::Busy)));
        assert!(matches!(env.checkpoint(), Err(Error::Busy)));
        assert_eq!(tree(&dir.0), before);
        assert_eq!(
            env.begin_read().get(b"kv", b"k").unwrap(),
            Some(b"old".as_slice())
        );
        drop(lease);
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"k", b"after").unwrap();
        assert_eq!(w.commit().unwrap(), 2);
        env.checkpoint().unwrap();
        assert_eq!(
            ReadTxn::open_coordinated_snapshot(&dir.0, options())
                .unwrap()
                .get(b"kv", b"k")
                .unwrap(),
            Some(b"after".as_slice())
        );
    }
}
#[test]
fn readonly_recovery_is_bounded_does_not_heal_or_mutate_and_gate_replacement_fails() {
    use std::os::unix::fs::FileExt;
    for (format, encoding) in modes() {
        let dir = Dir::new();
        let env = Environment::create_with_encoding(&dir.0, options(), format, encoding).unwrap();
        seed(&env);
        env.enable_readonly_capture().unwrap();
        let before = tree(&dir.0);
        assert!(matches!(
            ReadTxn::open_coordinated_snapshot_with_limits(
                &dir.0,
                options(),
                CaptureLimits {
                    max_replay_bytes: 0,
                    max_checkpoint_bytes: 0
                }
            ),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(tree(&dir.0), before);
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"k", b"new").unwrap();
        w.commit().unwrap();
        let nonempty_tail = tree(&dir.0);
        assert!(matches!(
            ReadTxn::open_coordinated_snapshot_with_limits(
                &dir.0,
                options(),
                CaptureLimits {
                    max_replay_bytes: 0,
                    max_checkpoint_bytes: 384 << 20,
                }
            ),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(tree(&dir.0), nonempty_tail);
        let selected = env.physical_lsn();
        drop(env);
        let wal = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.0.join("RESIDENT.wal"))
            .unwrap();
        let mut a = [0; 256];
        let mut b = [0; 256];
        wal.read_exact_at(&mut a, 65536).unwrap();
        wal.read_exact_at(&mut b, 131072).unwrap();
        let newer = if u64::from_le_bytes(a[40..48].try_into().unwrap())
            > u64::from_le_bytes(b[40..48].try_into().unwrap())
        {
            65536
        } else {
            131072
        };
        let mut byte = [0];
        wal.read_exact_at(&mut byte, newer + 255).unwrap();
        byte[0] ^= 1;
        wal.write_all_at(&byte, newer + 255).unwrap();
        drop(wal);
        let damaged = tree(&dir.0);
        let read = ReadTxn::open_coordinated_snapshot(&dir.0, options()).unwrap();
        assert_eq!(read.sequence(), 2);
        assert_eq!(read.get(b"kv", b"k").unwrap(), Some(b"new".as_slice()));
        assert_eq!(tree(&dir.0), damaged);
        let reopened =
            Environment::open_with_encoding(&dir.0, options(), format, encoding).unwrap();
        assert_eq!(reopened.physical_lsn(), selected);
        drop(reopened);
        assert_ne!(tree(&dir.0), damaged);
        let old = dir.0.join("old-gate");
        fs::rename(dir.0.join("RESIDENT-CAPTURE"), &old).unwrap();
        fs::copy(&old, dir.0.join("RESIDENT-CAPTURE")).unwrap();
        assert!(matches!(
            ReadTxn::open_coordinated_snapshot(&dir.0, options()),
            Err(Error::Corrupt(_))
        ));
    }
}
struct TestCipher;
impl ulmdb_storage::transform::Encryption for TestCipher {
    fn transform(
        &self,
        source: &[u8],
        destination: &mut [u8],
        key: &[u8],
        iv: &[u8],
        authentication: &mut [u8],
        direction: ulmdb_storage::transform::Direction,
    ) -> ulmdb::Result<()> {
        let tag = |bytes: &[u8]| {
            let mut h = Sha256::new();
            h.update(key);
            h.update(iv);
            h.update(bytes);
            h.finalize()
        };
        if direction == ulmdb_storage::transform::Direction::Decrypt
            && authentication != &tag(source)[..16]
        {
            return Err(Error::InvalidInput("test authentication"));
        }
        for (i, (a, b)) in source.iter().zip(destination.iter_mut()).enumerate() {
            *b = *a ^ key[i % key.len()] ^ iv[i % iv.len()];
        }
        if direction == ulmdb_storage::transform::Direction::Encrypt {
            authentication.copy_from_slice(&tag(destination)[..16]);
        }
        Ok(())
    }
}
fn context() -> std::sync::Arc<ulmdb_storage::transform::TransformContext> {
    use ulmdb_storage::transform::*;
    std::sync::Arc::new(
        TransformContext::new(
            None,
            Some(EncryptionConfig {
                callback: std::sync::Arc::new(TestCipher),
                key: vec![47; 32],
                authentication_bytes: 16,
            }),
            128 << 20,
        )
        .unwrap(),
    )
}
#[test]
fn encrypted_capture_owns_large_kv_ip_and_wide_duplicates_after_source_removal() {
    for (format, encoding) in modes() {
        let dir = Dir::new();
        let ctx = context();
        let env = Environment::create_with_encoding_and_transform(
            &dir.0,
            options(),
            format,
            encoding,
            Some(ctx.clone()),
        )
        .unwrap();
        env.enable_readonly_capture().unwrap();
        let large = vec![71; (1 << 20) + 13];
        let dup = vec![53; 65536];
        let prefix = ulmdb::Prefix::new(ulmdb::Address::V4(0x0a000000), 8).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
        w.create_database(b"dup", DatabaseKind::Duplicates { width: None })
            .unwrap();
        w.put(b"kv", b"large", &large).unwrap();
        w.insert_prefix(b"ip", prefix, &large).unwrap();
        w.insert_duplicate(b"dup", b"k", &dup).unwrap();
        w.commit().unwrap();
        env.checkpoint().unwrap();
        let before = tree(&dir.0);
        assert!(ReadTxn::open_coordinated_snapshot(&dir.0, options()).is_err());
        let read = ReadTxn::open_coordinated_snapshot_with_transform_options(
            &dir.0,
            options(),
            CaptureLimits::default(),
            Some(ctx),
        )
        .unwrap();
        assert_eq!(tree(&dir.0), before);
        let borrow = read.get(b"kv", b"large").unwrap().unwrap();
        assert_eq!(
            read.longest_match(b"ip", ulmdb::Address::V4(0x0a010203))
                .unwrap()
                .unwrap()
                .1,
            large
        );
        assert_eq!(
            read.duplicates(b"dup")
                .unwrap()
                .first(b"k")
                .unwrap()
                .unwrap(),
            dup
        );
        drop(env);
        fs::remove_dir_all(&dir.0).unwrap();
        assert_eq!(borrow, large);
    }
}
#[test]
fn exclusive_gate_denies_reader_and_export_does_not_inherit_source_inode() {
    for (format, encoding) in modes() {
        let dir = Dir::new();
        let env = Environment::create_with_encoding(&dir.0, options(), format, encoding).unwrap();
        seed(&env);
        env.enable_readonly_capture().unwrap();
        let gate = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.0.join("RESIDENT-CAPTURE"))
            .unwrap();
        gate.try_lock().unwrap();
        let before = tree(&dir.0);
        assert!(matches!(
            ReadTxn::open_coordinated_snapshot(&dir.0, options()),
            Err(Error::Busy)
        ));
        assert_eq!(tree(&dir.0), before);
        gate.unlock().unwrap();
        let target = Dir::new();
        env.export_snapshot(&target.0).unwrap();
        assert!(!target.0.join("RESIDENT-CAPTURE").exists());
        assert!(matches!(
            ReadTxn::open_coordinated_snapshot(&target.0, options()),
            Err(Error::InvalidInput(_))
        ));
        let targetenv =
            Environment::open_with_encoding(&target.0, options(), format, encoding).unwrap();
        assert_eq!(
            targetenv.begin_read().get(b"kv", b"k").unwrap(),
            Some(b"old".as_slice())
        );
        targetenv.enable_readonly_capture().unwrap();
        assert_eq!(
            ReadTxn::open_coordinated_snapshot(&target.0, options())
                .unwrap()
                .sequence(),
            1
        );
    }
}
// External old-decoder compatibility control uses this exact current source
// worker to create and retain a negotiated or default environment on disk.
#[test]
fn protocol_fixture_worker() {
    let Some(path) = std::env::var_os("RESIDENT_PROTOCOL_FIXTURE") else {
        return;
    };
    let format = if std::env::var("RESIDENT_PROTOCOL_FORMAT").unwrap() == "literal" {
        RedoFormat::LiteralV1
    } else {
        RedoFormat::CompactV2
    };
    let env = Environment::create_with_redo(&path, options(), format).unwrap();
    seed(&env);
    if std::env::var("RESIDENT_PROTOCOL_ENABLE").unwrap() == "1" {
        env.enable_readonly_capture().unwrap();
    }
    println!("created sequence {}", env.begin_read().sequence());
}
