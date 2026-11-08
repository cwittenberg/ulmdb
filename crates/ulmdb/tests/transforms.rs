//! Callback contract vectors; the deterministic test cipher is NOT cryptography.
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use ulmdb::{DatabaseKind, Environment, Error, PublicationMode};
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

#[test]
#[ignore = "explicit65MiB encrypted incremental and physical-stat resource journey"]
fn large_encrypted_values_keep_profile_through_incremental_and_statistics() {
    let source = Dir::new();
    let destination = Dir::new();
    let log = Arc::new(Mutex::new(Vec::new()));
    let ctx = context(&log, true, false, 7, 512 << 20);
    let env = Environment::create_with_transform(&source.0, PublicationMode::Indexed, ctx.clone())
        .unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"small").unwrap();
    w.commit().unwrap();
    ulmdb::backup::export(&env, &destination.0).unwrap();
    let length = 65 << 20;
    let mut w = env.begin_write().unwrap();
    let reserved = w.reserve(b"kv", b"key", length).unwrap();
    w.reservation_value_mut(&reserved).unwrap().fill(0x5a);
    w.commit().unwrap();
    assert_eq!(
        env.begin_read()
            .database_stats(b"kv")
            .unwrap()
            .overflow_bytes,
        length as u64 + 16
    );
    let mut delta = Vec::new();
    env.dump_incremental(1, &mut delta).unwrap();
    let target = Environment::open_with_transform(&destination.0, ctx.clone()).unwrap();
    target
        .load_incremental(&mut std::io::Cursor::new(delta))
        .unwrap();
    drop(target);
    let target = Environment::open_with_transform(&destination.0, ctx).unwrap();
    let read = target.begin_read();
    let value = read.get(b"kv", b"key").unwrap().unwrap();
    assert_eq!(value.len(), length);
    assert!(value.iter().all(|v| *v == 0x5a));
    assert_eq!(
        read.database_stats(b"kv").unwrap().overflow_bytes,
        length as u64 + 16
    );
    let mut w = target.begin_write().unwrap();
    w.put(b"kv", b"other", b"live writer profile").unwrap();
    w.commit().unwrap();
}

#[test]
fn encrypted_incremental_restore_keeps_context_geometry_and_old_borrows() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let source = Dir::new();
        let destination = Dir::new();
        let log = Arc::new(Mutex::new(Vec::new()));
        let ctx = context(&log, true, false, 7, 16 << 20);
        let env = Environment::create_with_page_size_and_transform_options(
            &source.0,
            mode,
            ulmdb::PageSize::new(8192).unwrap(),
            Default::default(),
            Default::default(),
            Some(ctx.clone()),
        )
        .unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"key", &[7; 20000]).unwrap();
        w.commit().unwrap();
        ulmdb::backup::export(&env, &destination.0).unwrap();
        let target = Environment::open_with_transform(&destination.0, ctx.clone()).unwrap();
        let old = target.begin_read();
        for value in [8, 9] {
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"key", &[value; 20000]).unwrap();
            w.commit().unwrap();
        }
        let mut delta = Vec::new();
        env.dump_incremental(1, &mut delta).unwrap();
        target
            .load_incremental(&mut std::io::Cursor::new(delta))
            .unwrap();
        assert_eq!(
            target.begin_read().get(b"kv", b"key").unwrap(),
            Some([9; 20000].as_slice())
        );
        assert_eq!(old.get(b"kv", b"key").unwrap(), Some([7; 20000].as_slice()));
        drop(old);
        drop(target);
        let reopened = Environment::open_with_transform(&destination.0, ctx).unwrap();
        assert_eq!(reopened.begin_read().sequence(), 3);
        assert_eq!(reopened.stats().page_size, 8192);
    }
}
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-transform-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn actual_file_callbacks_persist_and_pin_decoded_catalog_overflow_bytes() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        for encrypted in [false, true] {
            let d = Dir::new();
            let log = Arc::new(Mutex::new(Vec::new()));
            let ctx = context(&log, encrypted, false, 7, 16 << 20);
            let env = Environment::create_with_transform(&d.0, mode, ctx.clone()).unwrap();
            let value = vec![b'Q'; 20_000];
            {
                let mut w = env.begin_write().unwrap();
                w.create_database(b"sensitive-catalog-name", DatabaseKind::Kv)
                    .unwrap();
                w.put(
                    b"sensitive-catalog-name",
                    b"sensitive-application-key",
                    &value,
                )
                .unwrap();
                w.commit().unwrap();
            }
            let read = env.begin_read();
            let held = read
                .get(b"sensitive-catalog-name", b"sensitive-application-key")
                .unwrap()
                .unwrap();
            assert_eq!(held, value);
            assert!(ctx.decoded_bytes() > 0);
            drop(env);
            assert_eq!(held, value);
            assert!(ctx.decoded_bytes() > 0);
            if encrypted {
                for folder in ["segments", "cohorts"] {
                    for p in fs::read_dir(d.0.join(folder)).unwrap() {
                        let p = p.unwrap().path();
                        if p.is_file() {
                            let bytes = fs::read(p).unwrap();
                            assert!(
                                !bytes
                                    .windows(b"sensitive-catalog-name".len())
                                    .any(|b| b == b"sensitive-catalog-name")
                            );
                            assert!(!bytes.windows(128).any(|b| b == vec![b'Q'; 128]));
                        }
                    }
                }
            }
            let opened = Environment::open_with_transform(&d.0, ctx.clone()).unwrap();
            assert_eq!(
                opened
                    .begin_read()
                    .get(b"sensitive-catalog-name", b"sensitive-application-key")
                    .unwrap(),
                Some(value.as_slice())
            );
            drop(opened);
            assert!(matches!(
                Environment::open(&d.0),
                Err(Error::CryptoMismatch)
            ));
            let wrong = context(&log, encrypted, !encrypted, 8, 16 << 20);
            assert!(matches!(
                Environment::open_with_transform(&d.0, wrong),
                Err(Error::BadChecksum) | Err(Error::CryptoFailed)
            ));
            drop(read);
            assert_eq!(ctx.decoded_bytes(), 0);
            let events = log.lock().unwrap();
            if encrypted {
                assert!(events.windows(2).any(|w| w == ["keyed-sum", "encrypt"]));
                assert!(events.windows(2).any(|w| w == ["decrypt", "keyed-sum"]));
            } else {
                assert!(events.contains(&"sum"));
            }
        }
    }
}
#[test]
fn tampering_and_decoded_quota_fail_before_exposing_bad_borrows() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        for encrypted in [false, true] {
            let d = Dir::new();
            let log = Arc::new(Mutex::new(Vec::new()));
            let ctx = context(&log, encrypted, false, 7, 16 << 20);
            let env = Environment::create_with_transform(&d.0, mode, ctx.clone()).unwrap();
            {
                let mut w = env.begin_write().unwrap();
                w.create_database(b"db", DatabaseKind::Kv).unwrap();
                w.put(b"db", b"k", b"good").unwrap();
                w.commit().unwrap();
            }
            let file = ulmdb_storage::segment_path(
                &d.0,
                env.begin_read()
                    .file_manifest()
                    .unwrap()
                    .segments
                    .last()
                    .unwrap(),
            );
            drop(env);
            let mut bytes = fs::read(&file).unwrap();
            let original = bytes.clone();
            bytes[128 + 50] ^= 1;
            let mut permissions = fs::metadata(&file).unwrap().permissions();
            permissions.set_readonly(false);
            fs::set_permissions(&file, permissions).unwrap();
            fs::write(&file, &bytes).unwrap();
            assert!(matches!(
                Environment::open_with_transform(&d.0, ctx.clone()),
                Err(Error::BadChecksum) | Err(Error::CryptoFailed)
            ));
            assert_eq!(ctx.decoded_bytes(), 0);
            fs::write(&file, original).unwrap();
            let tiny = context(&log, encrypted, false, 7, 4096);
            assert!(matches!(
                Environment::open_with_transform(&d.0, tiny.clone()),
                Err(Error::LimitExceeded)
            ));
            assert_eq!(tiny.decoded_bytes(), 0);
        }
    }
}
#[test]
fn coordinated_previous_rollback_retention_and_compaction_keep_context() {
    use ulmdb::{CoordinatedEnvironment, PreviousSession, ReadAhead, ReadTxn, ResidencyPolicy};
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new();
        let log = Arc::new(Mutex::new(Vec::new()));
        let ctx = context(&log, true, false, 7, 32 << 20);
        let env = Environment::create_with_transform(&d.0, mode, ctx.clone()).unwrap();
        {
            let mut w = env.begin_write().unwrap();
            w.create_database(b"db", DatabaseKind::Kv).unwrap();
            w.put(b"db", b"k", b"A").unwrap();
            w.commit().unwrap();
        }
        env.retain_snapshot("old", &env.begin_read()).unwrap();
        let held = env.begin_read();
        {
            let mut w = env.begin_write().unwrap();
            w.put(b"db", b"k", b"B").unwrap();
            w.commit().unwrap();
        }
        assert_eq!(
            env.open_retained("old").unwrap().get(b"db", b"k").unwrap(),
            Some(b"A".as_slice())
        );
        drop(env);
        let wrapper = CoordinatedEnvironment::open_with_transform(&d.0, ctx.clone()).unwrap();
        assert_eq!(
            wrapper.begin_read().unwrap().get(b"db", b"k").unwrap(),
            Some(b"B".as_slice())
        );
        let previous = ReadTxn::open_previous_coordinated_snapshot_with_transform_options(
            &d.0,
            ResidencyPolicy::Demand,
            ReadAhead::Normal,
            Some(ctx.clone()),
        )
        .unwrap();
        assert_eq!(previous.get(b"db", b"k").unwrap(), Some(b"A".as_slice()));
        drop(previous);
        wrapper.update(|w| w.put(b"db", b"k", b"C")).unwrap();
        wrapper.compact().unwrap();
        wrapper.prune_inventory().unwrap();
        assert_eq!(held.get(b"db", b"k").unwrap(), Some(b"A".as_slice()));
        drop(wrapper);
        let previous = PreviousSession::open_with_transform_options(
            &d.0,
            ResidencyPolicy::Demand,
            ReadAhead::Normal,
            Some(ctx.clone()),
        )
        .unwrap();
        assert_eq!(
            previous.begin_read().get(b"db", b"k").unwrap(),
            Some(b"C".as_slice())
        );
        drop(previous);
        let env = Environment::open_with_transform(&d.0, ctx.clone()).unwrap();
        assert_eq!(
            env.begin_read().get(b"db", b"k").unwrap(),
            Some(b"C".as_slice())
        );
    }
}
#[test]
fn full_and_compact_directory_copies_preserve_transformed_storage() {
    use ulmdb::{OwnedFileCapture, ReadAhead, ResidencyPolicy};
    use ulmdb_storage::coordination::Coordination;
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new();
        let target = Dir::new();
        let full = Dir::new();
        let compact = Dir::new();
        let log = Arc::new(Mutex::new(Vec::new()));
        let ctx = context(&log, true, false, 7, 32 << 20);
        let env = Environment::create_with_transform(&d.0, mode, ctx.clone()).unwrap();
        {
            let mut w = env.begin_write().unwrap();
            w.create_database(b"sensitive-db", DatabaseKind::Kv)
                .unwrap();
            w.put(b"sensitive-db", b"k", b"A-secret-value").unwrap();
            w.commit().unwrap();
        }
        {
            let mut w = env.begin_write().unwrap();
            w.put(b"sensitive-db", b"k", b"B-secret-value").unwrap();
            w.commit().unwrap();
        }
        ulmdb::backup::export(&env, &target.0).unwrap();
        assert_eq!(
            fs::read(d.0.join("TRANSFORM")).unwrap(),
            fs::read(target.0.join("TRANSFORM")).unwrap()
        );
        let copied = Environment::open_with_transform(&target.0, ctx.clone()).unwrap();
        assert_eq!(
            copied.begin_read().get(b"sensitive-db", b"k").unwrap(),
            Some(b"B-secret-value".as_slice())
        );
        drop(copied);
        drop(env);
        let capture = Coordination::capture_directory_with_transform(
            &d.0,
            Default::default(),
            Some(ctx.clone()),
        )
        .unwrap();
        let pair = OwnedFileCapture::current_for_copy(&d.0, capture)
            .unwrap()
            .into_copy_pair()
            .unwrap()
            .admit_with_transform(
                ResidencyPolicy::Demand,
                ReadAhead::Normal,
                Some(ctx.clone()),
            )
            .unwrap();
        ulmdb::backup::export_file_pair(&pair, &full.0).unwrap();
        drop(pair);
        let copied = Environment::open_with_transform(&full.0, ctx.clone()).unwrap();
        assert_eq!(
            copied.begin_read().get(b"sensitive-db", b"k").unwrap(),
            Some(b"B-secret-value".as_slice())
        );
        drop(copied);
        let capture = Coordination::capture_directory_with_transform(
            &d.0,
            Default::default(),
            Some(ctx.clone()),
        )
        .unwrap();
        let view = OwnedFileCapture::current_for_copy(&d.0, capture)
            .unwrap()
            .admit_copy_view_with_transform(
                ResidencyPolicy::Demand,
                ReadAhead::Normal,
                Some(ctx.clone()),
            )
            .unwrap();
        let report = ulmdb::backup::compact_file_copy(&view, &compact.0).unwrap();
        assert_eq!(report.selected.sequence, 1);
        drop(view);
        let copied = Environment::open_with_transform(&compact.0, ctx.clone()).unwrap();
        assert_eq!(
            copied.begin_read().get(b"sensitive-db", b"k").unwrap(),
            Some(b"B-secret-value".as_slice())
        );
        drop(copied);
        for root in [&target.0, &full.0, &compact.0] {
            for folder in ["segments", "cohorts"] {
                for p in fs::read_dir(root.join(folder)).unwrap() {
                    let b = fs::read(p.unwrap().path()).unwrap();
                    assert!(
                        !b.windows(b"B-secret-value".len())
                            .any(|x| x == b"B-secret-value")
                    );
                }
            }
        }
    }
}
