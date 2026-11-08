//! Format4 persistent inventory metadata follows snapshot reachability, not
//! the historical inventories embedded in data/metadata origin cohorts.
use std::{
    collections::BTreeSet,
    fs,
    io::Cursor as Input,
    path::{Path, PathBuf},
};
use ulmdb::replication::{
    self, ApplyOutcome, AssetId, Cursor, PublisherKey, TransferLimits, VerifiedTransfer,
};
use ulmdb::{DatabaseKind, Environment, PublicationMode, ReadTxn, backup};
use ulmdb_storage::Manifest;
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        let path = parent.join(format!(
            "ulmdb-indexed-lifecycle-{}",
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
fn cohort(dir: &Path, origin: u128) -> PathBuf {
    dir.join("cohorts").join(format!("{origin:032x}.cohort"))
}
fn physical(dir: &Path, m: &Manifest) -> BTreeSet<PathBuf> {
    let mut result = m
        .segments
        .iter()
        .map(|e| ulmdb_storage::segment_path(dir, e))
        .collect::<BTreeSet<_>>();
    result.insert(ulmdb_storage::descriptor_path(dir, m));
    result.extend(
        m.metadata_origins()
            .into_iter()
            .map(|origin| cohort(dir, origin)),
    );
    result
}
fn insert(env: &Environment, n: u64) {
    let mut w = env.begin_write().unwrap();
    w.put(b"kv", &n.to_be_bytes(), &vec![n as u8; 12_000])
        .unwrap();
    w.commit().unwrap();
}
fn inspect(read: &ReadTxn, n: u64) {
    assert_eq!(
        read.get(b"kv", &n.to_be_bytes()).unwrap().unwrap(),
        vec![n as u8; 12_000]
    );
}
fn fixture(path: &Path) -> (Environment, u128) {
    let env = Environment::create_with_publication(path, PublicationMode::Indexed).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.commit().unwrap();
    // Guarantee unreachable data inventory entries, followed by multiple live
    // segments whose metadata subtrees can survive later path-copy appends.
    for n in 0..6 {
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"obsolete", &vec![n; 12_000]).unwrap();
        w.commit().unwrap();
    }
    for n in 0..18 {
        insert(&env, n);
    }
    assert!(env.prune_inventory().unwrap().changed);
    let rebuilt = ulmdb_storage::read_current(path).unwrap();
    assert_eq!(rebuilt.format_minor, 4);
    assert!(
        !rebuilt
            .segments
            .iter()
            .any(|e| e.cohort == rebuilt.generation)
    );
    for n in 18..21 {
        insert(&env, n);
    }
    let current = ulmdb_storage::read_current(path).unwrap();
    assert!(current.metadata_origins().contains(&rebuilt.generation));
    assert!(
        !current
            .segments
            .iter()
            .any(|e| e.cohort == rebuilt.generation)
    );
    assert_ne!(current.previous, rebuilt.generation);
    (env, rebuilt.generation)
}
fn lifecycle(parent: &Path) {
    let d = Directory::new(parent);
    let source = d.child("source");
    let (env, metadata_only) = fixture(&source);
    let before = ulmdb_storage::read_current(&source).unwrap();
    let expected = physical(&source, &before);
    env.reclaim().unwrap();
    assert!(cohort(&source, metadata_only).exists());
    let held = env.begin_read();
    inspect(&held, 0);
    inspect(&held, 20);
    env.retain_snapshot("metadata", &held).unwrap();
    let summary = backup::export_snapshot(&env, &held, d.child("export")).unwrap();
    assert_eq!(
        summary.physical_bytes,
        expected
            .iter()
            .map(|p| fs::metadata(p).unwrap().len())
            .sum::<u64>()
    );
    let exported = ulmdb_storage::read_current(&d.child("export")).unwrap();
    assert_eq!(exported.format_minor, 4);
    assert_eq!(exported.generation, before.generation);
    assert_eq!(exported.metadata_origins(), before.metadata_origins());
    for original in &expected {
        let relative = original.strip_prefix(&source).unwrap();
        assert_eq!(
            fs::read(original).unwrap(),
            fs::read(d.child("export").join(relative)).unwrap()
        );
    }
    backup::restore(d.child("export"), d.child("restore")).unwrap();
    let restored = Environment::open(d.child("restore")).unwrap();
    inspect(&restored.begin_read(), 20);
    backup::verify(&restored).unwrap();
    env.compact().unwrap();
    env.compact().unwrap();
    let compacted = ulmdb_storage::read_current(&source).unwrap();
    assert_eq!(compacted.format_minor, 4);
    assert!(!compacted.metadata_origins().contains(&metadata_only));
    env.reclaim().unwrap();
    assert!(cohort(&source, metadata_only).exists());
    inspect(&held, 0);
    drop(held);
    let pinned = env.open_retained("metadata").unwrap();
    inspect(&pinned, 20);
    drop(pinned);
    env.release_snapshot("metadata").unwrap();
    env.reclaim().unwrap();
    assert!(!cohort(&source, metadata_only).exists());
    backup::verify(&env).unwrap();
    drop(env);
    let reopened = Environment::open(&source).unwrap();
    inspect(&reopened.begin_read(), 20);
}
#[test]
fn indexed_metadata_export_restore_prune_and_gc_disk() {
    lifecycle(&std::env::temp_dir());
}
#[test]
fn indexed_metadata_export_restore_prune_and_gc_ramdrive() {
    if Path::new("/dev/shm").is_dir() {
        lifecycle(Path::new("/dev/shm"));
    }
}
#[test]
fn missing_or_corrupt_metadata_fails_closed_before_open() {
    let d = Directory::new(&std::env::temp_dir());
    let (env, origin) = fixture(&d.child("source"));
    let missing = d.child("missing");
    backup::export(&env, &missing).unwrap();
    fs::remove_file(cohort(&missing, origin)).unwrap();
    assert!(Environment::open(&missing).is_err());
    let corrupt = d.child("corrupt");
    backup::export(&env, &corrupt).unwrap();
    let path = cohort(&corrupt, origin);
    let mut bytes = fs::read(&path).unwrap();
    // Mutate every byte in an unopened copy: this must be rejected by checked
    // metadata decoding, rather than observed through an existing mmap.
    bytes.fill(0xa5);
    let sealed = fs::metadata(&path).unwrap().permissions();
    assert!(sealed.readonly());
    // The exported copy is intentionally sealed. Give only this unopened test
    // copy owner-write permission for corruption, then restore its protection.
    let mut writable = sealed.clone();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        writable.set_mode(writable.mode() | 0o200);
    }
    #[cfg(not(unix))]
    writable.set_readonly(false);
    fs::set_permissions(&path, writable).unwrap();
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(&path, sealed).unwrap();
    assert!(Environment::open(&corrupt).is_err());
    inspect(&env.begin_read(), 20);
}
fn transfer(
    env: &Environment,
    target: &ReadTxn,
    base: Option<&ReadTxn>,
    key: &PublisherKey,
    parent: &Path,
) -> VerifiedTransfer {
    let mut bytes = Vec::new();
    replication::send_snapshot(env, target, base, key, &mut bytes).unwrap();
    replication::receive(
        &mut Input::new(bytes),
        key,
        TransferLimits::default(),
        parent,
    )
    .unwrap()
}
#[test]
fn indexed_replication_transfers_exact_metadata_union_and_reuses_base_nodes() {
    let d = Directory::new(&std::env::temp_dir());
    let source = d.child("source");
    let (env, metadata_only) = fixture(&source);
    let first = env.begin_read();
    let c = Cursor::from_read(&first).unwrap();
    let key = PublisherKey::new(9, c.uuid, c.epoch, [0x63; 32]).unwrap();
    let full = transfer(&env, &first, None, &key, &d.0);
    assert!(
        full.assets()
            .iter()
            .any(|a| a.id == AssetId::Cohort(metadata_only))
    );
    let replica_path = d.child("replica");
    let replica = replication::bootstrap(&full, &replica_path).unwrap();
    let held = replica.begin_read();
    let base_manifest = ulmdb_storage::read_current(&source).unwrap();
    insert(&env, 21);
    let second = env.begin_read();
    let target_manifest = ulmdb_storage::read_current(&source).unwrap();
    let delta = transfer(&env, &second, Some(&first), &key, &d.0);
    let base_assets = physical(&source, &base_manifest);
    let target_assets = physical(&source, &target_manifest);
    let expected = target_assets
        .difference(&base_assets)
        .cloned()
        .collect::<BTreeSet<_>>();
    let actual = delta
        .assets()
        .iter()
        .map(|a| a.id.path(&source))
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    assert!(
        !delta
            .assets()
            .iter()
            .any(|a| a.id == AssetId::Cohort(metadata_only))
    );
    assert_eq!(
        replica.apply_replication(&delta).unwrap().outcome,
        ApplyOutcome::Activated
    );
    inspect(&replica.begin_read(), 21);
    inspect(&held, 20);
    assert_eq!(held.get(b"kv", &21u64.to_be_bytes()).unwrap(), None);
    assert_eq!(
        replica.apply_replication(&delta).unwrap().outcome,
        ApplyOutcome::AlreadyCurrent
    );
    replica.reclaim().unwrap();
    backup::verify(&replica).unwrap();
    assert!(cohort(&replica_path, metadata_only).exists());
    insert(&env, 22);
    let third = env.begin_read();
    let next = transfer(&env, &third, Some(&second), &key, &d.0);
    assert!(
        ulmdb_storage::read_current(&source)
            .unwrap()
            .metadata_origins()
            .contains(&metadata_only)
    );
    // Missing unchanged metadata is detected before any pointer publication.
    // The old data mappings remain valid, and repairing the absent immutable
    // path allows retrying the same verified transfer on the same receiver.
    let old_pointer = fs::read(replica_path.join("CURRENT")).unwrap();
    let metadata_path = cohort(&replica_path, metadata_only);
    let saved_metadata = fs::read(&metadata_path).unwrap();
    fs::remove_file(&metadata_path).unwrap();
    assert!(replica.apply_replication(&next).is_err());
    assert_eq!(fs::read(replica_path.join("CURRENT")).unwrap(), old_pointer);
    inspect(&replica.begin_read(), 21);
    inspect(&held, 20);
    fs::write(&metadata_path, saved_metadata).unwrap();
    assert_eq!(
        replica.apply_replication(&next).unwrap().outcome,
        ApplyOutcome::Activated
    );
    inspect(&replica.begin_read(), 22);
    drop(replica);
    let reopened = Environment::open(&replica_path).unwrap();
    assert!(reopened.is_replica());
    inspect(&reopened.begin_read(), 22);
}
