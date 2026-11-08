use std::{
    fs,
    io::Cursor as Input,
    path::{Path, PathBuf},
};
use ulmdb::{
    DatabaseKind, Environment, PublicationMode,
    replication::{self, Cursor, PublisherKey, ReplicationError, TransferLimits},
};

struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        let path = parent.join(format!(
            "ulmdb-replication-wire-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn children(&self) -> usize {
        fs::read_dir(&self.0).unwrap().count()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn source(parent: &Directory, mode: PublicationMode) -> Environment {
    let env = Environment::create_with_publication(parent.0.join("source"), mode).unwrap();
    let mut tx = env.begin_write().unwrap();
    tx.create_database(b"data", DatabaseKind::Kv).unwrap();
    tx.put(b"data", b"key", &vec![17; 262144]).unwrap();
    tx.commit().unwrap();
    env
}
fn key(read: &ulmdb::ReadTxn, id: u64, secret: u8) -> PublisherKey {
    let c = Cursor::from_read(read).unwrap();
    PublisherKey::new(id, c.uuid, c.epoch, [secret; 32]).unwrap()
}
fn exercise(parent: &Path, mode: PublicationMode) {
    let dir = Directory::new(parent);
    let staging = Directory::new(parent);
    fs::write(staging.0.join("keep"), b"sentinel").unwrap();
    let env = source(&dir, mode);
    let read = env.begin_read();
    let key = key(&read, 7, 91);
    let mut frame = Vec::new();
    let report = replication::send_snapshot(&env, &read, None, &key, &mut frame).unwrap();
    assert_eq!(report.wire_bytes, frame.len() as u64);
    assert_eq!(report.target, Cursor::from_read(&read).unwrap());
    let mut joined = frame.clone();
    joined.extend_from_slice(&frame);
    let mut input = Input::new(joined);
    for index in 1..=2 {
        let received =
            replication::receive(&mut input, &key, TransferLimits::default(), &staging.0).unwrap();
        assert_eq!(input.position(), frame.len() as u64 * index);
        assert_eq!(received.cursor(), report.target);
        assert_eq!(received.base(), None);
        assert_eq!(received.publisher_id(), 7);
        assert_eq!(received.payload_bytes(), report.payload_bytes);
        assert_eq!(received.assets().len(), report.files as usize);
        let stage = fs::read_dir(&staging.0)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.is_dir())
            .unwrap();
        for asset in received.assets() {
            let metadata = fs::metadata(asset.id.path(&stage)).unwrap();
            assert!(metadata.permissions().readonly());
            assert_eq!(metadata.len(), asset.length);
        }
        assert_eq!(staging.children(), 2);
        drop(received);
        assert_eq!(staging.children(), 1);
    }
    assert_eq!(fs::read(staging.0.join("keep")).unwrap(), b"sentinel");
    let replay =
        replication::send_snapshot(&env, &read, Some(&read), &key, &mut Vec::new()).unwrap();
    assert_eq!(
        (replay.files, replay.payload_bytes, replay.wire_bytes),
        (0, 0, 352)
    );
}
#[test]
fn framed_legacy_snapshots_are_sealed_bounded_and_cleaned() {
    exercise(&std::env::temp_dir(), PublicationMode::Legacy);
}
#[test]
fn framed_cohort_snapshots_on_ramdrive() {
    if Path::new("/dev/shm").is_dir() {
        exercise(Path::new("/dev/shm"), PublicationMode::Cohort);
    }
}
#[test]
fn authentication_realm_limits_and_truncation_never_leave_stages() {
    let dir = Directory::new(&std::env::temp_dir());
    let staging = Directory::new(&std::env::temp_dir());
    let env = source(&dir, PublicationMode::Cohort);
    let read = env.begin_read();
    let key = key(&read, 8, 92);
    let mut bytes = Vec::new();
    replication::send_snapshot(&env, &read, None, &key, &mut bytes).unwrap();
    let wrong_secret = PublisherKey::new(key.id(), key.uuid(), key.epoch(), [93; 32]).unwrap();
    assert!(matches!(
        replication::receive(
            &mut bytes.as_slice(),
            &wrong_secret,
            TransferLimits::default(),
            &staging.0
        ),
        Err(ReplicationError::Authentication)
    ));
    for wrong in [
        PublisherKey::new(9, key.uuid(), key.epoch(), [92; 32]).unwrap(),
        PublisherKey::new(8, [3; 16], key.epoch(), [92; 32]).unwrap(),
    ] {
        assert!(matches!(
            replication::receive(
                &mut bytes.as_slice(),
                &wrong,
                TransferLimits::default(),
                &staging.0
            ),
            Err(ReplicationError::UnauthorizedPublisher)
        ));
    }
    let wrong_epoch = PublisherKey::new(8, key.uuid(), key.epoch() + 1, [92; 32]).unwrap();
    assert!(matches!(
        replication::receive(
            &mut bytes.as_slice(),
            &wrong_epoch,
            TransferLimits::default(),
            &staging.0
        ),
        Err(ReplicationError::EpochConflict)
    ));
    for limits in [
        TransferLimits {
            max_files: 0,
            max_bytes: u64::MAX,
        },
        TransferLimits {
            max_files: u32::MAX,
            max_bytes: 0,
        },
    ] {
        assert!(matches!(
            replication::receive(&mut bytes.as_slice(), &key, limits, &staging.0),
            Err(ReplicationError::LimitExceeded)
        ));
    }
    for end in [0, 319, 320, 383, bytes.len() - 33, bytes.len() - 1] {
        assert!(
            replication::receive(
                &mut &bytes[..end],
                &key,
                TransferLimits::default(),
                &staging.0
            )
            .is_err()
        );
        assert_eq!(staging.children(), 0);
    }
    for index in [320 + 64 + 80, bytes.len() - 1] {
        let mut corrupt = bytes.clone();
        corrupt[index] ^= 1;
        assert!(matches!(
            replication::receive(
                &mut corrupt.as_slice(),
                &key,
                TransferLimits::default(),
                &staging.0
            ),
            Err(ReplicationError::Authentication)
        ));
    }
    assert_eq!(staging.children(), 0);
}
#[test]
fn sender_rejects_gap_wrong_epoch_and_another_open_owner_before_output() {
    let dir = Directory::new(&std::env::temp_dir());
    let env = source(&dir, PublicationMode::Legacy);
    let old = env.begin_read();
    let key = key(&old, 11, 81);
    for value in [b"next".as_slice(), b"later"] {
        let mut tx = env.begin_write().unwrap();
        tx.put(b"data", b"key", value).unwrap();
        tx.commit().unwrap();
    }
    let read = env.begin_read();
    let mut output = Vec::new();
    assert!(matches!(
        replication::send_snapshot(&env, &read, Some(&old), &key, &mut output),
        Err(ReplicationError::Gap)
    ));
    assert!(output.is_empty());
    let wrong = PublisherKey::new(11, key.uuid(), key.epoch() + 1, [81; 32]).unwrap();
    assert!(matches!(
        replication::send_snapshot(&env, &read, None, &wrong, &mut output),
        Err(ReplicationError::EpochConflict)
    ));
    let restored = dir.0.join("clone");
    ulmdb::backup::export_snapshot(&env, &old, &restored).unwrap();
    let clone = Environment::open(&restored).unwrap();
    assert!(matches!(
        replication::send_snapshot(&clone, &old, None, &key, &mut output),
        Err(ReplicationError::InvalidInput(_))
    ));
    assert!(output.is_empty());
}
#[test]
fn immediate_logical_and_physical_deltas_send_only_new_assets() {
    let dir = Directory::new(&std::env::temp_dir());
    let staging = Directory::new(&std::env::temp_dir());
    let env = source(&dir, PublicationMode::Legacy);
    let base = env.begin_read();
    let key = key(&base, 13, 83);
    let mut before = Vec::new();
    replication::send_snapshot(&env, &base, None, &key, &mut before).unwrap();
    let full = replication::receive(
        &mut before.as_slice(),
        &key,
        TransferLimits::default(),
        &staging.0,
    )
    .unwrap();
    let mut tx = env.begin_write().unwrap();
    tx.put(b"data", b"small", b"change").unwrap();
    tx.commit().unwrap();
    let next = env.begin_read();
    let mut bytes = Vec::new();
    let delta = replication::send_snapshot(&env, &next, Some(&base), &key, &mut bytes).unwrap();
    let received = replication::receive(
        &mut bytes.as_slice(),
        &key,
        TransferLimits::default(),
        &staging.0,
    )
    .unwrap();
    assert_eq!(delta.base, Some(full.cursor()));
    assert!(
        received
            .assets()
            .iter()
            .all(|a| !full.assets().iter().any(|b| a.id == b.id))
    );
    env.upgrade_publication().unwrap();
    let physical = env.begin_read();
    let mut bytes = Vec::new();
    let report =
        replication::send_snapshot(&env, &physical, Some(&next), &key, &mut bytes).unwrap();
    assert_eq!(report.target.sequence, delta.target.sequence);
    assert_eq!(report.target.commit, delta.target.commit);
    assert_ne!(report.target.checkpoint, delta.target.checkpoint);
    let received = replication::receive(
        &mut bytes.as_slice(),
        &key,
        TransferLimits::default(),
        &staging.0,
    )
    .unwrap();
    assert_eq!(received.assets().len(), 1);
    assert!(matches!(
        received.assets()[0].id,
        replication::AssetId::Cohort(_)
    ));
}

#[test]
fn short_interrupted_io_and_empty_authenticated_replay_are_framed() {
    struct ShortWriter {
        bytes: Vec<u8>,
        interrupted: bool,
    }
    impl std::io::Write for ShortWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            let n = bytes.len().min(13);
            self.bytes.extend_from_slice(&bytes[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    struct ShortReader<'a> {
        bytes: &'a [u8],
        interrupted: bool,
    }
    impl std::io::Read for ShortReader<'_> {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            let n = self.bytes.len().min(output.len()).min(7);
            output[..n].copy_from_slice(&self.bytes[..n]);
            self.bytes = &self.bytes[n..];
            Ok(n)
        }
    }
    let dir = Directory::new(&std::env::temp_dir());
    let stage = Directory::new(&std::env::temp_dir());
    let env = source(&dir, PublicationMode::Cohort);
    let read = env.begin_read();
    let key = key(&read, 19, 85);
    let mut writer = ShortWriter {
        bytes: Vec::new(),
        interrupted: false,
    };
    replication::send_snapshot(&env, &read, None, &key, &mut writer).unwrap();
    let mut reader = ShortReader {
        bytes: &writer.bytes,
        interrupted: false,
    };
    let received =
        replication::receive(&mut reader, &key, TransferLimits::default(), &stage.0).unwrap();
    assert!(reader.bytes.is_empty());
    assert_eq!(received.cursor(), Cursor::from_read(&read).unwrap());
    drop(received);
    let mut replay = Vec::new();
    replication::send_snapshot(&env, &read, Some(&read), &key, &mut replay).unwrap();
    let replay = replication::receive(
        &mut replay.as_slice(),
        &key,
        TransferLimits {
            max_files: 0,
            max_bytes: 0,
        },
        &stage.0,
    )
    .unwrap();
    assert!(replay.assets().is_empty());
    assert_eq!(replay.base(), Some(replay.cursor()));
}

#[test]
fn cohort_source_sends_selected_inventory_without_recursing_obsolete_footer_inventories() {
    let dir = Directory::new(&std::env::temp_dir());
    let stage = Directory::new(&std::env::temp_dir());
    let env = source(&dir, PublicationMode::Cohort);
    for value in [b"middle".as_slice(), b"latest"] {
        let mut tx = env.begin_write().unwrap();
        tx.put(b"data", b"key", value).unwrap();
        tx.commit().unwrap();
    }
    env.prune_inventory().unwrap();
    let read = env.begin_read();
    let key = key(&read, 23, 87);
    let mut bytes = Vec::new();
    let report = replication::send_snapshot(&env, &read, None, &key, &mut bytes).unwrap();
    let received = replication::receive(
        &mut bytes.as_slice(),
        &key,
        TransferLimits::default(),
        &stage.0,
    )
    .unwrap();
    assert!(
        report.files <= 3,
        "only live origin cohorts and current root are needed"
    );
    // Deliberately remove unselected historical cohorts. The live data origin's
    // footer may still describe them; current snapshot export must not recurse.
    let selected = received
        .assets()
        .iter()
        .map(|a| a.id.path(&dir.0.join("source")))
        .collect::<std::collections::BTreeSet<_>>();
    let mut removed = 0;
    for entry in fs::read_dir(dir.0.join("source/cohorts")).unwrap() {
        let path = entry.unwrap().path();
        if !selected.contains(&path) {
            fs::remove_file(path).unwrap();
            removed += 1;
        }
    }
    assert!(removed > 0);
    let mut current = Vec::new();
    replication::send_snapshot(&env, &read, None, &key, &mut current).unwrap();
    let received = replication::receive(
        &mut current.as_slice(),
        &key,
        TransferLimits::default(),
        &stage.0,
    )
    .unwrap();
    let copied = replication::bootstrap(&received, dir.0.join("copy")).unwrap();
    assert_eq!(
        copied.begin_read().get(b"data", b"key").unwrap(),
        Some(b"latest".as_slice())
    );
}
