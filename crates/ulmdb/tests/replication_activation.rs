//! Local authenticated activation journeys. These are not second-host evidence.
use std::{
    fs,
    io::Cursor as Input,
    path::{Path, PathBuf},
};
use ulmdb::replication::{
    self, ApplyOutcome, Cursor, PublisherKey, ReplicationError as RError, TransferLimits,
    VerifiedTransfer,
};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix, PublicationMode, ReadTxn};
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        let path = parent.join(format!(
            "ulmdb-replication-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn child(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn prefix() -> Prefix {
    Prefix::new(Address::V4(0xc000_0200), 24).unwrap()
}
fn primary(path: &Path) -> Environment {
    let env = Environment::create_with_publication(path, PublicationMode::Cohort).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
    w.create_database(b"dups", DatabaseKind::Duplicates { width: None })
        .unwrap();
    w.put(b"kv", b"state", b"old").unwrap();
    w.insert_prefix(b"ip", prefix(), b"old").unwrap();
    w.insert_duplicate(b"dups", b"state", b"old").unwrap();
    w.put(b"kv", b"large", &vec![0x6b; 100_000]).unwrap();
    w.commit().unwrap();
    env
}
fn update(env: &Environment) {
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"state", b"new").unwrap();
    w.insert_prefix(b"ip", prefix(), b"new").unwrap();
    w.delete_duplicates(b"dups", b"state").unwrap();
    w.insert_duplicate(b"dups", b"state", b"new").unwrap();
    w.commit().unwrap();
}
fn inspect(read: &ReadTxn, new: bool) {
    let value: &[u8] = if new { b"new" } else { b"old" };
    assert_eq!(read.get(b"kv", b"state").unwrap(), Some(value));
    assert_eq!(
        read.exact_prefix(b"ip", prefix()).unwrap(),
        Some((prefix(), value))
    );
    assert_eq!(read.duplicate_count(b"dups", b"state").unwrap(), 1);
    assert!(
        read.duplicates(b"dups")
            .unwrap()
            .contains(b"state", value)
            .unwrap()
    );
    assert_eq!(
        read.get(b"kv", b"large").unwrap().unwrap(),
        vec![0x6b; 100_000]
    );
}
fn key(read: &ReadTxn, id: u64) -> PublisherKey {
    let c = Cursor::from_read(read).unwrap();
    PublisherKey::new(id, c.uuid, c.epoch, [0x37; 32]).unwrap()
}
fn frame(env: &Environment, read: &ReadTxn, base: Option<&ReadTxn>, key: &PublisherKey) -> Vec<u8> {
    let mut bytes = Vec::new();
    replication::send_snapshot(env, read, base, key, &mut bytes).unwrap();
    bytes
}
fn receive(bytes: &[u8], key: &PublisherKey, parent: &Path) -> VerifiedTransfer {
    replication::receive(
        &mut Input::new(bytes),
        key,
        TransferLimits::default(),
        parent,
    )
    .unwrap()
}
fn assert_fenced(env: &Environment) {
    assert!(env.is_replica());
    assert!(matches!(env.begin_write(), Err(Error::ReadOnly)));
    assert!(matches!(env.compact(), Err(Error::ReadOnly)));
    assert!(matches!(env.prune_inventory(), Err(Error::ReadOnly)));
    assert!(matches!(env.upgrade_publication(), Err(Error::ReadOnly)));
}
fn activation_journey(parent: &Path) {
    let directory = Directory::new(parent);
    let env = primary(&directory.child("primary"));
    let first = env.begin_read();
    let key = key(&first, 7);
    let full = frame(&env, &first, None, &key);
    let verified = receive(&full, &key, &directory.0);
    let replica_path = directory.child("replica");
    let replica = replication::bootstrap(&verified, &replica_path).unwrap();
    assert_eq!(
        Cursor::from_read(&replica.begin_read()).unwrap(),
        Cursor::from_read(&first).unwrap()
    );
    assert_fenced(&replica);
    // A lost bootstrap acknowledgment is recovered by reopening the receiver
    // and applying the same authenticated full frame, which is an exact replay.
    drop(replica);
    let replica = Environment::open(&replica_path).unwrap();
    let replay = replica.apply_replication(&verified).unwrap();
    assert_eq!(replay.outcome, ApplyOutcome::AlreadyCurrent);
    assert_eq!((replay.imported_files, replay.imported_bytes), (0, 0));
    inspect(&replica.begin_read(), false);
    assert!(replication::bootstrap(&verified, &replica_path).is_err());
    let pinned = replica.begin_read();
    replica.retain_snapshot("before", &pinned).unwrap();
    update(&env);
    let second = env.begin_read();
    let delta = frame(&env, &second, Some(&first), &key);
    let verified_delta = receive(&delta, &key, &directory.0);
    let report = replica.apply_replication(&verified_delta).unwrap();
    assert_eq!(report.outcome, ApplyOutcome::Activated);
    assert!(report.imported_files > 0);
    assert!(report.imported_bytes > 0);
    assert_eq!(report.cursor, Cursor::from_read(&second).unwrap());
    inspect(&replica.begin_read(), true);
    inspect(&pinned, false);
    let pointer = fs::read(replica_path.join("CURRENT")).unwrap();
    let replay = replica.apply_replication(&verified_delta).unwrap();
    assert_eq!(replay.outcome, ApplyOutcome::AlreadyCurrent);
    assert_eq!((replay.imported_files, replay.imported_bytes), (0, 0));
    assert_eq!(fs::read(replica_path.join("CURRENT")).unwrap(), pointer);
    replica.reclaim().unwrap();
    inspect(&pinned, false);
    env.compact().unwrap();
    let physical = env.begin_read();
    let physical_frame = frame(&env, &physical, Some(&second), &key);
    let physical_delta = receive(&physical_frame, &key, &directory.0);
    assert_eq!(
        replica.apply_replication(&physical_delta).unwrap().outcome,
        ApplyOutcome::Activated
    );
    assert_eq!(physical.sequence(), second.sequence());
    assert_ne!(
        Cursor::from_read(&physical).unwrap(),
        Cursor::from_read(&second).unwrap()
    );
    assert!(matches!(
        replica.apply_replication(&verified_delta),
        Err(RError::Gap) | Err(RError::Conflict)
    ));
    inspect(&replica.begin_read(), true);
    inspect(&pinned, false);
    let export = directory.child("export");
    ulmdb::backup::export(&replica, &export).unwrap();
    let restore = directory.child("restore");
    ulmdb::backup::restore(&export, &restore).unwrap();
    assert_fenced(&Environment::open(&restore).unwrap());
    drop(replica);
    let reopened = Environment::open(&replica_path).unwrap();
    assert_fenced(&reopened);
    inspect(&reopened.begin_read(), true);
}
#[test]
fn authenticated_cross_index_activation_replay_and_readonly_reopen() {
    activation_journey(&std::env::temp_dir());
}
#[test]
fn authenticated_activation_on_ramdrive() {
    if Path::new("/dev/shm").is_dir() {
        activation_journey(Path::new("/dev/shm"));
    }
}
#[test]
fn authentication_limits_authority_and_gap_do_not_activate() {
    let d = Directory::new(&std::env::temp_dir());
    let env = primary(&d.child("primary"));
    let first = env.begin_read();
    let key = key(&first, 7);
    let full = frame(&env, &first, None, &key);
    let verified = receive(&full, &key, &d.0);
    let path = d.child("replica");
    let replica = replication::bootstrap(&verified, &path).unwrap();
    let pointer = fs::read(path.join("CURRENT")).unwrap();
    let mut bad = full.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert!(matches!(
        replication::receive(&mut Input::new(&bad), &key, TransferLimits::default(), &d.0),
        Err(RError::Authentication)
    ));
    let wrong = PublisherKey::new(7, key.uuid(), key.epoch(), [0x38; 32]).unwrap();
    assert!(matches!(
        replication::receive(
            &mut Input::new(&full),
            &wrong,
            TransferLimits::default(),
            &d.0
        ),
        Err(RError::Authentication)
    ));
    let epoch = PublisherKey::new(7, key.uuid(), key.epoch() + 1, [0x37; 32]).unwrap();
    assert!(matches!(
        replication::receive(
            &mut Input::new(&full),
            &epoch,
            TransferLimits::default(),
            &d.0
        ),
        Err(RError::EpochConflict)
    ));
    assert!(matches!(
        replication::receive(
            &mut Input::new(&full),
            &key,
            TransferLimits {
                max_files: 0,
                max_bytes: u64::MAX
            },
            &d.0
        ),
        Err(RError::LimitExceeded)
    ));
    let other = PublisherKey::new(8, key.uuid(), key.epoch(), [0x37; 32]).unwrap();
    let wrong_authority = receive(&frame(&env, &first, None, &other), &other, &d.0);
    assert!(matches!(
        replica.apply_replication(&wrong_authority),
        Err(RError::UnauthorizedPublisher)
    ));
    update(&env);
    let second = env.begin_read();
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", b"third", b"value").unwrap();
    w.commit().unwrap();
    let third = env.begin_read();
    let skipped = receive(&frame(&env, &third, Some(&second), &key), &key, &d.0);
    assert!(matches!(
        replica.apply_replication(&skipped),
        Err(RError::Gap) | Err(RError::Conflict)
    ));
    assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
    inspect(&replica.begin_read(), false);
}
#[test]
fn immutable_collision_is_rejected_without_overwrite() {
    let d = Directory::new(&std::env::temp_dir());
    let env = primary(&d.child("primary"));
    let first = env.begin_read();
    let key = key(&first, 7);
    let full = receive(&frame(&env, &first, None, &key), &key, &d.0);
    let path = d.child("replica");
    let replica = replication::bootstrap(&full, &path).unwrap();
    update(&env);
    let second = env.begin_read();
    let delta = receive(&frame(&env, &second, Some(&first), &key), &key, &d.0);
    let asset = delta
        .assets()
        .iter()
        .find(|a| !a.id.path(&path).exists())
        .unwrap();
    let collision = asset.id.path(&path);
    fs::write(&collision, b"existing incompatible inode").unwrap();
    let pointer = fs::read(path.join("CURRENT")).unwrap();
    assert!(replica.apply_replication(&delta).is_err());
    assert_eq!(
        fs::read(&collision).unwrap(),
        b"existing incompatible inode"
    );
    assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
    inspect(&replica.begin_read(), false);
}

#[test]
fn authenticated_stage_on_another_filesystem_and_marker_corruption() {
    if !Path::new("/dev/shm").is_dir() {
        return;
    }
    let disk = Directory::new(&std::env::temp_dir());
    let ram = Directory::new(Path::new("/dev/shm"));
    let env = primary(&disk.child("primary"));
    let first = env.begin_read();
    let key = key(&first, 7);
    let full = receive(&frame(&env, &first, None, &key), &key, &ram.0);
    let path = disk.child("replica");
    let replica = replication::bootstrap(&full, &path).unwrap();
    inspect(&replica.begin_read(), false);
    update(&env);
    let second = env.begin_read();
    let delta = receive(&frame(&env, &second, Some(&first), &key), &key, &ram.0);
    assert_eq!(
        replica.apply_replication(&delta).unwrap().outcome,
        ApplyOutcome::Activated
    );
    inspect(&replica.begin_read(), true);
    drop(delta);
    drop(full);
    drop(ram);
    // Installed copies remain usable after their authenticated stage is removed.
    inspect(&replica.begin_read(), true);
    drop(replica);
    let reopened = Environment::open(&path).unwrap();
    inspect(&reopened.begin_read(), true);
    assert_fenced(&reopened);
    drop(reopened);
    let marker = path.join("REPLICA");
    let mut bytes = fs::read(&marker).unwrap();
    bytes[32] ^= 1;
    fs::write(marker, bytes).unwrap();
    assert!(matches!(Environment::open(&path), Err(Error::Corrupt(_))));
}
