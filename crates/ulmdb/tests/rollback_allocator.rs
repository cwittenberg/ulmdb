use std::path::PathBuf;
use ulmdb::{CoordinatedEnvironment, DatabaseKind, Environment, Error, PublicationMode};

const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-rollback-allocator-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn seed(path: &std::path::Path, mode: PublicationMode) -> (Environment, u64) {
    let env = Environment::create_with_publication(path, mode).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"base", DatabaseKind::Kv).unwrap();
    write.put(b"base", b"key", b"old").unwrap();
    assert_eq!(write.commit().unwrap(), 1);
    let mut write = env.begin_write().unwrap();
    write.create_database(b"undone", DatabaseKind::Kv).unwrap();
    let id = write.open_database(b"undone").unwrap().id();
    assert_eq!(write.commit().unwrap(), 2);
    (env, id)
}

#[test]
fn reopened_owner_does_not_reuse_undone_or_live_aborted_database_ids() {
    for mode in MODES {
        let dir = Directory::new();
        let (env, undone) = seed(&dir.0, mode);
        let mut abandoned = env.begin_write().unwrap();
        abandoned
            .create_database(b"abandoned", DatabaseKind::Kv)
            .unwrap();
        let burned = abandoned.open_database(b"abandoned").unwrap().id();
        assert!(burned > undone);
        abandoned.abort();
        env.rollback_last_commit(2).unwrap();
        drop(env);
        let reopened = Environment::open(&dir.0).unwrap();
        let mut next = reopened.begin_write().unwrap();
        next.create_database(b"replacement", DatabaseKind::Kv)
            .unwrap();
        let replacement = next.open_database(b"replacement").unwrap().id();
        assert!(
            replacement > burned,
            "{mode:?}: reused {replacement}, undone={undone}, burned={burned}"
        );
        assert_eq!(next.get(b"base", b"key").unwrap(), Some(b"old".as_slice()));
        assert_eq!(next.commit().unwrap(), 2);
    }
}

#[test]
fn independent_coordinated_owners_allocate_above_the_rolled_back_commit_floor() {
    for mode in MODES {
        let dir = Directory::new();
        let (env, undone) = seed(&dir.0, mode);
        env.rollback_last_commit(2).unwrap();
        drop(env);
        let first = CoordinatedEnvironment::open(&dir.0).unwrap();
        let second = CoordinatedEnvironment::open(&dir.0).unwrap();
        let replacement = first
            .update(|write| {
                write.create_database(b"replacement", DatabaseKind::Kv)?;
                Ok(write.open_database(b"replacement")?.id())
            })
            .unwrap();
        assert!(
            replacement > undone,
            "{mode:?}: reused {replacement}, undone={undone}"
        );
        let later = second
            .update(|write| {
                write.create_database(b"later", DatabaseKind::Kv)?;
                Ok(write.open_database(b"later")?.id())
            })
            .unwrap();
        assert!(later > replacement);
        assert_eq!(
            second.begin_read().unwrap().get(b"base", b"key").unwrap(),
            Some(b"old".as_slice())
        );
    }
}

#[test]
fn importing_floor_is_checked_monotonic_nonpublishing_and_never_allocates_an_id() {
    for mode in MODES {
        let dir = Directory::new();
        let (env, _) = seed(&dir.0, mode);
        let current = std::fs::read(dir.0.join("CURRENT")).unwrap();
        let previous = std::fs::read(dir.0.join("PREVIOUS")).ok();
        let initial = env.database_id_floor();
        assert!(matches!(
            env.raise_database_id_floor(0),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(env.database_id_floor(), initial);
        env.raise_database_id_floor(initial + 100).unwrap();
        env.raise_database_id_floor(initial).unwrap();
        assert_eq!(env.database_id_floor(), initial + 100);
        assert_eq!(std::fs::read(dir.0.join("CURRENT")).unwrap(), current);
        assert_eq!(std::fs::read(dir.0.join("PREVIOUS")).ok(), previous);
        assert_eq!(
            ulmdb_storage::read_current(&dir.0).unwrap().next_database,
            initial
        );
        let mut writer = env.begin_write().unwrap();
        assert_eq!(env.database_id_floor(), initial + 100);
        assert!(matches!(
            env.raise_database_id_floor(initial + 200),
            Err(Error::Busy)
        ));
        assert_eq!(env.database_id_floor(), initial + 100);
        writer.create_database(b"new", DatabaseKind::Kv).unwrap();
        assert_eq!(writer.open_database(b"new").unwrap().id(), initial + 100);
        assert_eq!(env.database_id_floor(), initial + 101);
        writer.abort();
        assert_eq!(env.database_id_floor(), initial + 101);
        env.rollback_last_commit(2).unwrap();
        assert_eq!(
            ulmdb_storage::read_current(&dir.0).unwrap().next_database,
            initial + 101
        );
        drop(env);
        let reopened = Environment::open(&dir.0).unwrap();
        assert_eq!(reopened.database_id_floor(), initial + 101);
    }
}
