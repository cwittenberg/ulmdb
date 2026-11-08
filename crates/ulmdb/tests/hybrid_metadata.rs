use std::{
    fs::File,
    path::{Path, PathBuf},
};
use ulmdb::{
    DatabaseKind, Error,
    hybrid::{Environment, ImageMode, Options, RedoMode},
};
use ulmdb_storage::hybrid::recovery::inspect;
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-hybrid-metadata-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn options(mode: RedoMode, image: ImageMode) -> Options {
    Options {
        image,
        redo: mode,
        chunk: ulmdb_storage::hybrid::ChunkLimits {
            capacity: 1024 * 1024,
            reserve_quantum: 65536,
            max_burned_bytes: 1024 * 1024,
        },
        max_image_bytes: 4 * 1024 * 1024,
        ..Options::default()
    }
}

#[test]
fn last_database_drop_commits_reopens_and_preserves_escaped_readers() {
    for image in [ImageMode::File, ImageMode::Anonymous] {
        for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
            for mode in [RedoMode::Compact, RedoMode::FullPostimage] {
                for rotate in [false, true] {
                    let path = Directory::new(&parent);
                    let env = Environment::create(&path.0, options(mode, image)).unwrap();
                    let mut w = env.begin_write().unwrap();
                    w.create_database(b"only", DatabaseKind::Kv).unwrap();
                    w.put(b"only", b"key", b"old").unwrap();
                    assert_eq!(w.commit().unwrap(), 1);
                    let old = env.begin_read();
                    let old_commit = old.commit_identity();
                    if rotate {
                        env.rotate_chunk().unwrap();
                    }
                    let mut w = env.begin_write().unwrap();
                    {
                        let mut child = w.savepoint().unwrap();
                        child.create_database(b"burned", DatabaseKind::Kv).unwrap();
                        child.drop_database(b"only").unwrap();
                        child.rollback().unwrap();
                        assert_eq!(child.get(b"only", b"key").unwrap(), Some(b"old".as_slice()));
                    }
                    w.drop_database(b"only").unwrap();
                    assert_eq!(w.commit().unwrap(), 2);
                    assert_eq!(env.begin_read().database_count(), 0);
                    assert_eq!(old.get(b"only", b"key").unwrap(), Some(b"old".as_slice()));
                    assert_ne!(env.begin_read().commit_identity(), old_commit);
                    let plan = inspect(&File::open(path.0.join("HYBRID.wal")).unwrap()).unwrap();
                    let previous = plan.records[0].record.root;
                    let last = &plan.records[1].record;
                    assert!(last.root.catalog.is_null());
                    if last.chunks.is_empty() {
                        assert!(last.objects.is_empty());
                        assert_eq!(last.root.next_chunk, previous.next_chunk);
                    } else {
                        assert_eq!(
                            last.root.next_chunk,
                            previous.next_chunk + u64::from(rotate)
                        );
                    }
                    assert_eq!(last.root.next_database, previous.next_database + 1);
                    let cut = env.capture_checkpoint().unwrap();
                    let artifact = cut
                        .stage(
                            path.0.join("checkpoint-after-drop"),
                            77,
                            ulmdb_storage::hybrid::checkpoint::CheckpointLimits::default(),
                        )
                        .unwrap();
                    let descriptor = ulmdb_storage::hybrid::checkpoint::verify_artifact(
                        artifact.directory(),
                        ulmdb_storage::hybrid::checkpoint::CheckpointLimits::default(),
                    )
                    .unwrap();
                    assert_eq!(descriptor.selected, cut.selection());
                    assert!(descriptor.selected.root.catalog.is_null());
                    drop(env);
                    let env = Environment::open(&path.0, options(mode, image)).unwrap();
                    assert_eq!(env.sequence(), 2);
                    assert_eq!(env.begin_read().database_count(), 0);
                    assert!(matches!(
                        env.begin_read().get(b"only", b"key"),
                        Err(Error::MissingDatabase)
                    ));
                    assert_eq!(old.get(b"only", b"key").unwrap(), Some(b"old".as_slice()));
                    let mut w = env.begin_write().unwrap();
                    w.create_database(b"only", DatabaseKind::Kv).unwrap();
                    w.put(b"only", b"key", b"new").unwrap();
                    assert_eq!(w.commit().unwrap(), 3);
                    drop(env);
                    let env = Environment::open(&path.0, options(mode, image)).unwrap();
                    assert_eq!(
                        env.begin_read().get(b"only", b"key").unwrap(),
                        Some(b"new".as_slice())
                    );
                    assert_eq!(old.get(b"only", b"key").unwrap(), Some(b"old".as_slice()));
                }
            }
        }
    }
}

struct NoSources;
impl ulmdb_storage::hybrid::redo::SourceResolver for NoSources {
    fn resolve_committed<'a>(
        &'a self,
        _: &ulmdb_storage::hybrid::redo::Extent,
        _: &ulmdb_storage::hybrid::redo::SourceReceipt,
    ) -> ulmdb::Result<&'a [u8]> {
        Err(Error::Corrupt("metadata record must not resolve a source"))
    }
}
fn append_metadata(path: &Path, jump_chunk: bool) {
    use ulmdb_storage::hybrid::{journal::Journal, redo::Record};
    let (mut journal, mut root) =
        Journal::open_checked(&path.join("HYBRID.wal"), |plan| Ok(plan.root())).unwrap();
    let parent = journal.selected();
    let header = journal.bootstrap();
    let parent_commit = journal.commit_id();
    root.catalog = ulmdb_storage::Reference::NULL;
    root.next_database += 1; // a burned identity is legitimate even with no new objects
    root.next_chunk += u64::from(jump_chunk);
    let record = Record {
        uuid: header.uuid,
        epoch: header.epoch,
        lsn: parent.lsn + 1,
        sequence: parent.sequence + 1,
        commit: header.commit + u128::from(parent.lsn + 1),
        parent_commit,
        parent,
        root,
        chunks: Vec::new(),
        objects: Vec::new(),
    };
    journal
        .commit_checked(&record, &NoSources, |_| Ok(()))
        .unwrap();
}

#[test]
fn existing_journal_metadata_records_replay_without_phantom_chunk_and_continue() {
    for image in [ImageMode::File, ImageMode::Anonymous] {
        for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
            for mode in [RedoMode::Compact, RedoMode::FullPostimage] {
                let path = Directory::new(&parent);
                let env = Environment::create(&path.0, options(mode, image)).unwrap();
                let mut w = env.begin_write().unwrap();
                w.create_database(b"only", DatabaseKind::Kv).unwrap();
                w.put(b"only", b"key", b"old").unwrap();
                w.commit().unwrap();
                let old = env.begin_read();
                env.rotate_chunk().unwrap(); // unused private physical incarnation
                drop(env);
                append_metadata(&path.0, false);
                append_metadata(&path.0, false); // consecutive no-object roots
                let plan = inspect(&File::open(path.0.join("HYBRID.wal")).unwrap()).unwrap();
                assert!(
                    plan.records[1..]
                        .iter()
                        .all(|r| r.record.chunks.is_empty() && r.record.objects.is_empty())
                );
                assert_eq!(plan.root().next_chunk, 2);
                assert_eq!(plan.root().next_database, 4);
                let env = Environment::open(&path.0, options(mode, image)).unwrap();
                assert_eq!(env.sequence(), 3);
                assert_eq!(env.begin_read().database_count(), 0);
                assert_eq!(old.get(b"only", b"key").unwrap(), Some(b"old".as_slice()));
                let mut w = env.begin_write().unwrap();
                w.create_database(b"recreated", DatabaseKind::Kv).unwrap();
                w.put(b"recreated", b"key", b"new").unwrap();
                w.commit().unwrap();
                let plan = inspect(&File::open(path.0.join("HYBRID.wal")).unwrap()).unwrap();
                assert_eq!(plan.root().next_chunk, 2);
                assert_eq!(plan.root().next_database, 5);
                assert_eq!(plan.records.last().unwrap().record.chunks[0].chunk, 1);
                drop(env);
                let env = Environment::open(&path.0, options(mode, image)).unwrap();
                assert_eq!(
                    env.begin_read().get(b"recreated", b"key").unwrap(),
                    Some(b"new".as_slice())
                );
                assert_eq!(old.get(b"only", b"key").unwrap(), Some(b"old".as_slice()));
            }
        }
    }
}

#[test]
fn metadata_only_cannot_introduce_an_unmaterialized_chunk_counter() {
    for image in [ImageMode::File, ImageMode::Anonymous] {
        for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
            let path = Directory::new(&parent);
            let env = Environment::create(&path.0, options(RedoMode::Compact, image)).unwrap();
            let mut w = env.begin_write().unwrap();
            w.create_database(b"only", DatabaseKind::Kv).unwrap();
            w.commit().unwrap();
            let held = env.begin_read();
            drop(env);
            append_metadata(&path.0, true); // low-level record valid; facade lineage invalid
            let before = std::fs::read(path.0.join("HYBRID.wal")).unwrap();
            assert!(matches!(
                Environment::open(&path.0, options(RedoMode::Compact, image)),
                Err(Error::Corrupt("hybrid metadata-only chunk lineage"))
            ));
            assert_eq!(std::fs::read(path.0.join("HYBRID.wal")).unwrap(), before);
            assert_eq!(held.database_count(), 1);
        }
    }
}

#[test]
fn first_metadata_only_record_reconstructs_empty_active_image_before_data_commit() {
    for image in [ImageMode::File, ImageMode::Anonymous] {
        for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
            let path = Directory::new(&parent);
            let o = options(RedoMode::Compact, image);
            let env = Environment::create(&path.0, o).unwrap();
            drop(env);
            append_metadata(&path.0, false);
            let env = Environment::open(&path.0, o).unwrap();
            assert_eq!(env.sequence(), 1);
            assert_eq!(env.begin_read().database_count(), 0);
            let cut = env.capture_checkpoint().unwrap();
            assert_eq!(cut.chunk_count(), 0);
            assert_eq!(cut.selection().root.next_chunk, 1);
            cut.stage(
                path.0.join("checkpoint-empty-metadata"),
                78,
                ulmdb_storage::hybrid::checkpoint::CheckpointLimits::default(),
            )
            .unwrap();
            let mut w = env.begin_write().unwrap();
            w.create_database(b"first", DatabaseKind::Kv).unwrap();
            w.put(b"first", b"key", b"new").unwrap();
            assert_eq!(w.commit().unwrap(), 2);
            drop(env);
            let env = Environment::open(&path.0, o).unwrap();
            assert_eq!(
                env.begin_read().get(b"first", b"key").unwrap(),
                Some(b"new".as_slice())
            );
        }
    }
}
