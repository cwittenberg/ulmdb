#![cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::{
    fs,
    ops::Bound,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use ulmdb::{
    Address, DatabaseKind, Prefix,
    resident::{Environment, Options, RedoEncoding, RedoFormat},
};

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "resident-export-{:032x}",
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
    let mut options = Options::default();
    options.pool.kv_pages = 2048;
    options.pool.tiny_pages = 128;
    options.pool.max_reserved_bytes = 128 << 20;
    options.pool.max_metadata_bytes = 64 << 20;
    options.wal_capacity = 32 << 20;
    options
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
fn pinned_complete_export_survives_source_writes_collection_owner_drop_and_removal() {
    for (format, encoding) in modes() {
        let source = Dir::new();
        let target = Dir::new();
        let env =
            Environment::create_with_encoding(&source.0, options(), format, encoding).unwrap();
        let empty = env.capture_snapshot().unwrap();
        let genesis = Dir::new();
        assert_eq!(empty.export_to(&genesis.0).unwrap().sequence, 0);
        let reopened =
            Environment::open_with_encoding(&genesis.0, empty.options(), format, encoding).unwrap();
        assert_eq!(reopened.begin_read().database_count(), 0);
        drop(reopened);
        drop(empty);
        let value = vec![0x73; (1 << 20) + 19];
        let prefix = Prefix::new(Address::V4(0x0a000000), 8).unwrap();
        let mut write = env.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        write
            .create_database(b"dups", DatabaseKind::Duplicates { width: None })
            .unwrap();
        write
            .create_database(b"fixed", DatabaseKind::Duplicates { width: Some(65536) })
            .unwrap();
        write.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
        write.put(b"kv", b"large", &value).unwrap();
        write.put(b"kv", b"empty", b"").unwrap();
        write.insert_duplicate(b"dups", b"k", b"z").unwrap();
        write.insert_duplicate(b"dups", b"k", b"a").unwrap();
        write.insert_duplicate(b"fixed", b"f", &[3; 65536]).unwrap();
        write.insert_prefix(b"ip", prefix, &value).unwrap();
        assert!(matches!(env.capture_snapshot(), Err(ulmdb::Error::Busy)));
        write.commit().unwrap();
        let captured = env.capture_snapshot().unwrap();
        let before = captured.begin_read();
        let original_commit = before.commit_identity();
        let borrow = before.get(b"kv", b"large").unwrap().unwrap();
        let mut write = env.begin_write().unwrap();
        write.put(b"kv", b"large", b"new").unwrap();
        write.delete_duplicate(b"dups", b"k", b"a").unwrap();
        write.commit().unwrap();
        env.collect().unwrap();
        env.checkpoint().unwrap();
        assert_eq!(borrow, value);
        let reopened_options = captured.options();
        drop(env);
        fs::remove_dir_all(&source.0).unwrap();
        let report = captured.export_to(&target.0).unwrap();
        assert_eq!(report.sequence, 1);
        assert_eq!(report.commit, original_commit);
        assert_eq!(report.target_lsn, report.source_lsn + 1);
        let copy =
            Environment::open_with_encoding(&target.0, reopened_options, format, encoding).unwrap();
        let read = copy.begin_read();
        assert_eq!(read.sequence(), 1);
        assert_eq!(read.commit_identity(), original_commit);
        assert_eq!(read.get(b"kv", b"large").unwrap(), Some(value.as_slice()));
        assert_eq!(read.get(b"kv", b"empty").unwrap(), Some(b"".as_slice()));
        let pairs = read
            .duplicates(b"dups")
            .unwrap()
            .pairs(Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .map(|v| v.map(|(k, v)| (k.to_vec(), v.to_vec())).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            pairs,
            vec![
                (b"k".to_vec(), b"a".to_vec()),
                (b"k".to_vec(), b"z".to_vec())
            ]
        );
        assert_eq!(read.duplicates(b"fixed").unwrap().count(b"f").unwrap(), 1);
        assert_eq!(
            read.duplicates(b"fixed").unwrap().first(b"f").unwrap(),
            Some([3; 65536].as_slice())
        );
        assert_eq!(
            read.longest_match(b"ip", Address::V4(0x0a010203))
                .unwrap()
                .unwrap()
                .1,
            value
        );
        let current = fs::read(target.0.join("RESIDENT.wal")).unwrap();
        assert!(captured.export_to(&target.0).is_err());
        assert_eq!(fs::read(target.0.join("RESIDENT.wal")).unwrap(), current);
        let mut write = copy.begin_write().unwrap();
        write.put(b"kv", b"future", b"independent").unwrap();
        assert_eq!(write.commit().unwrap(), 2);
        assert_eq!(borrow, value);
    }
}

struct Cipher(AtomicBool);
impl ulmdb_storage::transform::Encryption for Cipher {
    fn transform(
        &self,
        source: &[u8],
        destination: &mut [u8],
        key: &[u8],
        _iv: &[u8],
        _authentication: &mut [u8],
        _direction: ulmdb_storage::transform::Direction,
    ) -> ulmdb::Result<()> {
        if self.0.load(Ordering::Acquire) {
            return Err(ulmdb::Error::InvalidInput("test callback failure"));
        }
        for (index, (from, to)) in source.iter().zip(destination).enumerate() {
            *to = *from ^ key[index % key.len()];
        }
        Ok(())
    }
}
#[test]
fn transformed_export_retains_callbacks_and_cleans_failed_owned_staging() {
    for (format, encoding) in modes() {
        let source = Dir::new();
        let target = Dir::new();
        let fail_target = Dir::new();
        let callback = Arc::new(Cipher(AtomicBool::new(false)));
        let context = Arc::new(
            ulmdb_storage::transform::TransformContext::new(
                None,
                Some(ulmdb_storage::transform::EncryptionConfig {
                    callback: callback.clone(),
                    key: vec![0x96, 0x47],
                    authentication_bytes: 0,
                }),
                128 << 20,
            )
            .unwrap(),
        );
        let env = Environment::create_with_encoding_and_transform(
            &source.0,
            options(),
            format,
            encoding,
            Some(context.clone()),
        )
        .unwrap();
        let mut write = env.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        write.put(b"kv", b"key", &[0x17; 65537]).unwrap();
        write.commit().unwrap();
        let capture = env.capture_snapshot().unwrap();
        drop(env);
        callback.0.store(true, Ordering::Release);
        assert!(capture.export_to(&fail_target.0).is_err());
        assert!(!fail_target.0.exists());
        callback.0.store(false, Ordering::Release);
        capture.export_to(&target.0).unwrap();
        assert!(Environment::open(&target.0, capture.options()).is_err());
        let copy = Environment::open_with_encoding_and_transform(
            &target.0,
            capture.options(),
            format,
            encoding,
            Some(context),
        )
        .unwrap();
        assert_eq!(
            copy.begin_read().get(b"kv", b"key").unwrap().unwrap(),
            &[0x17; 65537]
        );
    }
}
