//! Callback contract vectors; the deterministic test cipher is NOT cryptography.
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use ulmdb::{DatabaseKind, Error};
use ulmdb_storage::transform::{
    Checksum, ChecksumConfig, Direction, Encryption, EncryptionConfig, TransformContext,
};
struct Sum {
    log: Arc<Mutex<Vec<&'static str>>>,
    wrong: bool,
}
impl Checksum for Sum {
    fn compute(&self, src: &[u8], dst: &mut [u8], key: Option<&[u8]>) -> ulmdb::Result<()> {
        self.log
            .lock()
            .unwrap()
            .push(if key.is_some() { "keyed-sum" } else { "sum" });
        assert_eq!(dst.len(), 32);
        let mut h = Sha256::new();
        h.update(src);
        if let Some(key) = key {
            h.update(key);
        }
        dst.copy_from_slice(&h.finalize());
        if self.wrong {
            dst[0] ^= 1;
        }
        Ok(())
    }
}
struct Cipher {
    log: Arc<Mutex<Vec<&'static str>>>,
}
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
        assert_eq!(iv.len(), 16);
        assert_eq!(src.len(), dst.len());
        assert_eq!(auth.len(), 16);
        self.log
            .lock()
            .unwrap()
            .push(if direction == Direction::Encrypt {
                "encrypt"
            } else {
                "decrypt"
            });
        let tag = |bytes: &[u8]| {
            let mut h = Sha256::new();
            h.update(iv);
            h.update(key);
            h.update(bytes);
            h.finalize()
        };
        if direction == Direction::Decrypt && auth != &tag(src)[..16] {
            return Err(Error::InvalidInput("test MAC"));
        }
        for (i, (a, b)) in src.iter().zip(dst.iter_mut()).enumerate() {
            *b = *a ^ key[i % key.len()] ^ iv[i % 16];
        }
        if direction == Direction::Encrypt {
            auth.copy_from_slice(&tag(dst)[..16]);
        }
        Ok(())
    }
}
fn context(
    log: &Arc<Mutex<Vec<&'static str>>>,
    encrypted: bool,
    wrongsum: bool,
    key: u8,
    limit: u64,
) -> Arc<TransformContext> {
    Arc::new(
        TransformContext::new(
            Some(ChecksumConfig {
                callback: Arc::new(Sum {
                    log: log.clone(),
                    wrong: wrongsum,
                }),
                bytes: 32,
            }),
            encrypted.then(|| EncryptionConfig {
                callback: Arc::new(Cipher { log: log.clone() }),
                key: vec![key; 32],
                authentication_bytes: 16,
            }),
            limit,
        )
        .unwrap(),
    )
}
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "resident-transform-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

use ulmdb::resident::{Environment as Resident, Options, RedoEncoding, RedoFormat};
use ulmdb_storage::resident::Config;
fn options() -> Options {
    Options {
        pool: Config {
            kv_pages: 48,
            tiny_pages: 8,
            max_reserved_bytes: 8 << 20,
            max_metadata_bytes: 16 << 20,
            max_views: 64,
        },
        wal_capacity: 256 << 10,
        graph_read_budget: 1_000_000,
        auto_collect_below: 16,
    }
}
fn modes() -> [(RedoFormat, RedoEncoding); 4] {
    [
        (RedoFormat::LiteralV1, RedoEncoding::Owned),
        (RedoFormat::CompactV2, RedoEncoding::Owned),
        (RedoFormat::FullPostimageV2, RedoEncoding::Owned),
        (RedoFormat::CompactV2, RedoEncoding::Borrowed),
    ]
}
#[test]
fn resident_authenticated_packets_checkpoint_rotation_and_pinned_old_pool() {
    use ulmdb::{Address, Prefix};
    for (format, encoding) in modes() {
        for encrypted in [false, true] {
            let d = Dir::new();
            let log = Arc::new(Mutex::new(Vec::new()));
            let ctx = context(&log, encrypted, false, 7, 16 << 20);
            let o = options();
            let mut env = Resident::create_with_encoding_and_transform(
                &d.0,
                o,
                format,
                encoding,
                Some(ctx.clone()),
            )
            .unwrap();
            let prefix = Prefix::new(Address::V4(0), 0).unwrap();
            {
                let mut w = env.begin_write().unwrap();
                w.create_database(b"SECRET-CATALOG", DatabaseKind::Kv)
                    .unwrap();
                w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
                w.put(b"SECRET-CATALOG", b"private-key", b"ORIGINAL-PRIVATE-VALUE")
                    .unwrap();
                w.insert_prefix(b"ip", prefix, b"ORIGINAL-PRIVATE-VALUE")
                    .unwrap();
                w.commit().unwrap();
            }
            let old = env.begin_read();
            let held = old.get(b"SECRET-CATALOG", b"private-key").unwrap().unwrap();
            let id = old.commit_identity();
            for n in 2u64..=160 {
                let mut w = env.begin_write().unwrap();
                let value = format!("PRIVATE-VALUE-{n:08}");
                w.put(b"SECRET-CATALOG", b"private-key", value.as_bytes())
                    .unwrap();
                w.insert_prefix(b"ip", prefix, value.as_bytes()).unwrap();
                assert_eq!(w.commit().unwrap(), n);
                assert_eq!(held, b"ORIGINAL-PRIVATE-VALUE");
                assert_eq!(old.commit_identity(), id);
                assert!(env.wal_physical_bytes().unwrap() <= o.wal_capacity);
                if n % 40 == 0 {
                    env.checkpoint().unwrap();
                    drop(env);
                    env = Resident::open_with_encoding_and_transform(
                        &d.0,
                        o,
                        format,
                        encoding,
                        Some(ctx.clone()),
                    )
                    .unwrap();
                    assert_eq!(
                        env.begin_read()
                            .get(b"SECRET-CATALOG", b"private-key")
                            .unwrap(),
                        Some(value.as_bytes())
                    );
                }
            }
            assert!(env.physical_lsn() > env.sequence());
            assert_eq!(ctx.decoded_bytes() >= 56 * 4096, true);
            if encrypted {
                for entry in fs::read_dir(&d.0).unwrap() {
                    let p = entry.unwrap().path();
                    if p.is_file() {
                        let bytes = fs::read(p).unwrap();
                        for secret in [
                            b"ORIGINAL-PRIVATE-VALUE".as_slice(),
                            b"SECRET-CATALOG",
                            b"PRIVATE-VALUE-00000160",
                        ] {
                            assert!(!bytes.windows(secret.len()).any(|b| b == secret));
                        }
                    }
                }
            }
            drop(env);
            assert!(ctx.decoded_bytes() >= 56 * 4096);
            drop(old);
            assert_eq!(ctx.decoded_bytes(), 0);
            assert!(matches!(
                Resident::open(&d.0, o),
                Err(Error::CryptoMismatch)
            ));
            let wrong = context(&log, encrypted, !encrypted, 8, 16 << 20);
            assert!(matches!(
                Resident::open_with_transform(&d.0, o, wrong),
                Err(Error::BadChecksum) | Err(Error::CryptoFailed)
            ));
        }
    }
}
#[test]
fn checkpoint_and_selected_packet_tampering_never_heals_or_clears_the_wal() {
    for (format, encoding) in modes() {
        for checkpoint in [false, true] {
            let d = Dir::new();
            let log = Arc::new(Mutex::new(Vec::new()));
            let ctx = context(&log, true, false, 7, 16 << 20);
            let o = options();
            let env = Resident::create_with_encoding_and_transform(
                &d.0,
                o,
                format,
                encoding,
                Some(ctx.clone()),
            )
            .unwrap();
            {
                let mut w = env.begin_write().unwrap();
                w.create_database(b"kv", DatabaseKind::Kv).unwrap();
                w.put(b"kv", b"k", b"untampered").unwrap();
                w.commit().unwrap();
            }
            if checkpoint {
                env.checkpoint().unwrap();
            }
            drop(env);
            let wal = d.0.join("RESIDENT.wal");
            let target = if checkpoint {
                fs::read_dir(&d.0)
                    .unwrap()
                    .map(|e| e.unwrap().path())
                    .find(|p| {
                        p.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with(".resident-checkpoint-")
                    })
                    .unwrap()
            } else {
                wal.clone()
            };
            let mut bytes = fs::read(&target).unwrap();
            let at = if checkpoint {
                128 + 48
            } else {
                196608 + 128 + 48
            };
            bytes[at] ^= 1;
            fs::write(&target, bytes).unwrap();
            let before = fs::read(&wal).unwrap();
            assert!(matches!(
                Resident::open_with_encoding_and_transform(
                    &d.0,
                    o,
                    format,
                    encoding,
                    Some(ctx.clone())
                ),
                Err(Error::BadChecksum) | Err(Error::CryptoFailed) | Err(Error::Corrupt(_))
            ));
            assert_eq!(fs::read(&wal).unwrap(), before);
            assert_eq!(ctx.decoded_bytes(), 0);
        }
    }
}
#[cfg(feature = "fault-injection")]
#[test]
fn encrypted_wal_and_checkpoint_cut_errors_preserve_prefix_and_old_readers() {
    use std::process::Command;
    if let Some(path) = std::env::var_os("RESIDENT_TRANSFORM_CHILD") {
        let log = Arc::new(Mutex::new(Vec::new()));
        let ctx = context(&log, true, false, 7, 16 << 20);
        let format = if std::env::var("RESIDENT_TRANSFORM_FORMAT").unwrap() == "literal" {
            RedoFormat::LiteralV1
        } else {
            RedoFormat::CompactV2
        };
        let env = Resident::open_with_encoding_and_transform(
            &path,
            options(),
            format,
            RedoEncoding::Owned,
            Some(ctx),
        )
        .unwrap();
        let read = env.begin_read();
        let held = read.get(b"kv", b"k").unwrap().unwrap();
        let result = if std::env::var_os("RESIDENT_TRANSFORM_CHECKPOINT").is_some() {
            env.checkpoint().map(|_| 1)
        } else {
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"k", b"new").unwrap();
            w.commit()
        };
        if std::env::var_os("RESIDENT_TRANSFORM_UNKNOWN").is_some() {
            assert!(matches!(result, Err(Error::OutcomeUnknown)), "{result:?}");
            assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
        } else {
            assert!(matches!(result, Err(Error::Io(_))), "{result:?}");
            assert!(env.begin_write().is_ok());
        }
        assert_eq!(held, b"old");
        assert_eq!(read.get(b"kv", b"k").unwrap(), Some(b"old".as_slice()));
        return;
    }
    for (format, label, prefix) in [
        (RedoFormat::LiteralV1, "literal", "resident"),
        (RedoFormat::CompactV2, "compact", "resident2"),
    ] {
        for (component, phase, unknown) in [
            ("commit", "before_append", false),
            ("commit", "half_record", false),
            ("commit", "after_data_sync", false),
            ("commit", "half_witness", true),
            ("commit", "after_witness_sync", true),
            ("checkpoint", "after_image", false),
            ("checkpoint", "before_select", false),
            ("checkpoint", "after_select", true),
            ("checkpoint", "after_directory_sync", true),
        ] {
            for exit in [false, true] {
                let d = Dir::new();
                let log = Arc::new(Mutex::new(Vec::new()));
                let ctx = context(&log, true, false, 7, 16 << 20);
                let env = Resident::create_with_encoding_and_transform(
                    &d.0,
                    options(),
                    format,
                    RedoEncoding::Owned,
                    Some(ctx.clone()),
                )
                .unwrap();
                {
                    let mut w = env.begin_write().unwrap();
                    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
                    w.put(b"kv", b"k", b"old").unwrap();
                    w.commit().unwrap();
                }
                let old = env.begin_read();
                drop(env);
                let before = fs::read(d.0.join("RESIDENT.wal")).unwrap();
                let point = format!(
                    "{}.{component}.{phase}",
                    if component == "checkpoint" {
                        "resident"
                    } else {
                        prefix
                    }
                );
                let mut command = Command::new("timeout");
                command
                    .args(["--signal=KILL", "15s"])
                    .arg(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "encrypted_wal_and_checkpoint_cut_errors_preserve_prefix_and_old_readers",
                        "--nocapture",
                    ])
                    .env("RESIDENT_TRANSFORM_CHILD", &d.0)
                    .env("RESIDENT_TRANSFORM_FORMAT", label)
                    .env(
                        if exit {
                            "ULMDB_RESIDENT_EXIT_AT"
                        } else {
                            "ULMDB_RESIDENT_FAIL_AT"
                        },
                        &point,
                    );
                if component == "checkpoint" {
                    command.env("RESIDENT_TRANSFORM_CHECKPOINT", "1");
                }
                if unknown {
                    command.env("RESIDENT_TRANSFORM_UNKNOWN", "1");
                }
                let output = command.output().unwrap();
                assert_eq!(
                    output.status.code(),
                    Some(if exit { 86 } else { 0 }),
                    "{point}/{exit}: {} {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                if !exit && !unknown {
                    assert_eq!(
                        fs::read(d.0.join("RESIDENT.wal")).unwrap(),
                        before,
                        "preselection failure changed selected bytes: {point}"
                    );
                }
                let env = Resident::open_with_encoding_and_transform(
                    &d.0,
                    options(),
                    format,
                    RedoEncoding::Owned,
                    Some(ctx.clone()),
                )
                .unwrap();
                assert!([1, 2].contains(&env.sequence()));
                let expected = if env.sequence() == 1 { b"old" } else { b"new" };
                assert_eq!(
                    env.begin_read().get(b"kv", b"k").unwrap(),
                    Some(expected.as_slice())
                );
                assert_eq!(old.get(b"kv", b"k").unwrap(), Some(b"old".as_slice()));
                let next = env.sequence() + 1;
                {
                    let mut w = env.begin_write().unwrap();
                    w.put(b"kv", b"k", b"retry").unwrap();
                    assert_eq!(w.commit().unwrap(), next);
                }
            }
        }
    }
}

#[test]
fn encrypted_nested_writers_restore_private_bytes_and_publish_only_parent() {
    for (format, encoding) in modes() {
        let d = Dir::new();
        let o = options();
        let log = Arc::new(Mutex::new(Vec::new()));
        let ctx = context(&log, true, false, 7, 16 << 20);
        let env = Resident::create_with_encoding_and_transform(
            &d.0,
            o,
            format,
            encoding,
            Some(ctx.clone()),
        )
        .unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"k", b"before").unwrap();
        w.commit().unwrap();
        let old = env.begin_read();
        let borrowed = old.get(b"kv", b"k").unwrap().unwrap();
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"k", b"parent").unwrap();
        {
            let mut child = w.savepoint().unwrap();
            child.put(b"kv", b"k", b"discard").unwrap();
            {
                let mut grandchild = child.savepoint().unwrap();
                grandchild.put(b"kv", b"k", b"discard-grandchild").unwrap();
                grandchild.release().unwrap();
            }
            child.abort().unwrap();
        }
        assert_eq!(w.get(b"kv", b"k").unwrap(), Some(b"parent".as_slice()));
        {
            let mut child = w.savepoint().unwrap();
            child.put(b"kv", b"k", b"kept").unwrap();
            child.commit().unwrap();
        }
        assert_eq!(w.commit().unwrap(), 2);
        assert_eq!(borrowed, b"before");
        env.checkpoint().unwrap();
        assert_eq!(borrowed, b"before");
        drop(old);
        drop(env);
        let env = Resident::open_with_encoding_and_transform(&d.0, o, format, encoding, Some(ctx))
            .unwrap();
        assert_eq!(env.begin_read().sequence(), 2);
        assert_eq!(
            env.begin_read().get(b"kv", b"k").unwrap(),
            Some(b"kept".as_slice())
        );
        assert!(log.lock().unwrap().contains(&"encrypt"));
        assert!(log.lock().unwrap().contains(&"decrypt"));
    }
}

#[test]
fn encrypted_duplicate_trees_survive_child_abort_checkpoint_and_reopen() {
    use std::ops::Bound;
    for (format, encoding) in modes() {
        let d = Dir::new();
        let o = options();
        let log = Arc::new(Mutex::new(Vec::new()));
        let ctx = context(&log, true, false, 7, 16 << 20);
        let env = Resident::create_with_encoding_and_transform(
            &d.0,
            o,
            format,
            encoding,
            Some(ctx.clone()),
        )
        .unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"dups", DatabaseKind::Duplicates { width: Some(4) })
            .unwrap();
        for i in 0u32..32 {
            w.insert_duplicate(b"dups", b"k", &i.to_be_bytes()).unwrap();
        }
        w.commit().unwrap();
        let old = env.begin_read();
        let borrowed = old.duplicate_first(b"dups", b"k").unwrap().unwrap();
        let mut w = env.begin_write().unwrap();
        {
            let mut child = w.savepoint().unwrap();
            child.delete_duplicates(b"dups", b"k").unwrap();
            child
                .insert_duplicate(b"dups", b"k", &99u32.to_be_bytes())
                .unwrap();
        }
        assert_eq!(w.duplicate_count(b"dups", b"k").unwrap(), 32);
        w.insert_duplicate(b"dups", b"k", &32u32.to_be_bytes())
            .unwrap();
        assert_eq!(w.commit().unwrap(), 2);
        env.collect().unwrap();
        env.checkpoint().unwrap();
        assert_eq!(borrowed, &0u32.to_be_bytes());
        drop(old);
        drop(env);
        let env = Resident::open_with_encoding_and_transform(&d.0, o, format, encoding, Some(ctx))
            .unwrap();
        let r = env.begin_read();
        let actual = r
            .duplicate_values(b"dups", b"k", Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .map(|v| v.unwrap().to_vec())
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            (0u32..33)
                .map(|v| v.to_be_bytes().to_vec())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn encrypted_multi_mib_payload_extents_keep_old_borrows_and_recover() {
    for (format, encoding) in modes() {
        let d = Dir::new();
        let mut o = options();
        o.pool.kv_pages = 1024;
        o.pool.tiny_pages = 32;
        o.pool.max_reserved_bytes = 64 << 20;
        o.pool.max_metadata_bytes = 32 << 20;
        o.wal_capacity = 8 << 20;
        let log = Arc::new(Mutex::new(Vec::new()));
        let ctx = context(&log, true, false, 7, 64 << 20);
        let env = Resident::create_with_encoding_and_transform(
            &d.0,
            o,
            format,
            encoding,
            Some(ctx.clone()),
        )
        .unwrap();
        let a = vec![7; (1 << 20) + 17];
        let b = vec![8; (1 << 20) + 19];
        let p = vec![9; (512 << 10) + 31];
        let prefix = ulmdb::Prefix::new(ulmdb::Address::V4(0), 0).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
        w.put(b"kv", b"key", &a).unwrap();
        w.insert_prefix(b"ip", prefix, &p).unwrap();
        w.commit().unwrap();
        let old = env.begin_read();
        let borrowed = old.get(b"kv", b"key").unwrap().unwrap();
        let mut w = env.begin_write().unwrap();
        {
            let mut child = w.savepoint().unwrap();
            child.put(b"kv", b"key", &b).unwrap();
        }
        assert_eq!(w.get(b"kv", b"key").unwrap(), Some(a.as_slice()));
        w.put(b"kv", b"key", &b).unwrap();
        w.commit().unwrap();
        env.collect().unwrap();
        env.checkpoint().unwrap();
        assert!(borrowed == a.as_slice());
        drop(env);
        let env = Resident::open_with_encoding_and_transform(&d.0, o, format, encoding, Some(ctx))
            .unwrap();
        let r = env.begin_read();
        assert_eq!(r.sequence(), 2);
        assert_eq!(r.get(b"kv", b"key").unwrap(), Some(b.as_slice()));
        assert!(borrowed == a.as_slice());
        assert!(
            r.longest_match(b"ip", ulmdb::Address::V4(1))
                .unwrap()
                .unwrap()
                .1
                == p.as_slice()
        );
    }
}

#[test]
fn encrypted_64k_duplicate_values_keep_fixed_width_and_old_borrow() {
    for (format, encoding) in modes() {
        let d = Dir::new();
        let mut o = options();
        o.pool.kv_pages = 512;
        o.pool.tiny_pages = 32;
        o.pool.max_reserved_bytes = 64 << 20;
        o.pool.max_metadata_bytes = 32 << 20;
        o.wal_capacity = 8 << 20;
        let log = Arc::new(Mutex::new(Vec::new()));
        let ctx = context(&log, true, false, 7, 64 << 20);
        let env = Resident::create_with_encoding_and_transform(
            &d.0,
            o,
            format,
            encoding,
            Some(ctx.clone()),
        )
        .unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"fixed", DatabaseKind::Duplicates { width: Some(65536) })
            .unwrap();
        w.create_database(b"var", DatabaseKind::Duplicates { width: None })
            .unwrap();
        for i in 1u8..3 {
            w.insert_duplicate(b"fixed", b"k", &vec![i; 65536]).unwrap();
        }
        w.insert_duplicate(b"var", b"k", &vec![4; 65535]).unwrap();
        w.commit().unwrap();
        let old = env.begin_read();
        let borrowed = old.duplicate_first(b"fixed", b"k").unwrap().unwrap();
        let mut w = env.begin_write().unwrap();
        {
            let mut child = w.savepoint().unwrap();
            child
                .insert_duplicate(b"fixed", b"k", &vec![3; 65536])
                .unwrap();
        }
        assert_eq!(w.duplicate_count(b"fixed", b"k").unwrap(), 2);
        w.insert_duplicate(b"fixed", b"k", &vec![3; 65536]).unwrap();
        w.commit().unwrap();
        env.collect().unwrap();
        env.checkpoint().unwrap();
        assert!(borrowed.iter().all(|&x| x == 1));
        drop(env);
        let env = Resident::open_with_encoding_and_transform(&d.0, o, format, encoding, Some(ctx))
            .unwrap();
        let r = env.begin_read();
        assert_eq!(r.duplicate_count(b"fixed", b"k").unwrap(), 3);
        assert_eq!(
            r.duplicate_first(b"var", b"k").unwrap().unwrap().len(),
            65535
        );
        assert!(borrowed.iter().all(|&x| x == 1));
    }
}
