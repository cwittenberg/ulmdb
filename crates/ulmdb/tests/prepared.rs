use std::{fs, sync::mpsc, time::Duration};
use ulmdb::{CommitIntent, DatabaseKind, Environment, Error, PublicationMode, ReadTxn};
#[path = "support/pin_records.rs"]
mod pin_records;

const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-prepared-{}-{}",
            std::process::id(),
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn seed(directory: &Directory, mode: PublicationMode) -> Environment {
    let env = Environment::create_with_publication(&directory.0, mode).unwrap();
    let mut writer = env.begin_write().unwrap();
    writer.create_database(b"kv", DatabaseKind::Kv).unwrap();
    writer
        .create_database(b"dups", DatabaseKind::Duplicates { width: None })
        .unwrap();
    writer.put(b"kv", b"key", b"old").unwrap();
    writer.insert_duplicate(b"dups", b"key", b"old").unwrap();
    assert_eq!(writer.commit().unwrap(), 1);
    env
}
fn pins(directory: &Directory) -> usize {
    pin_records::paths(&directory.0).len()
}

#[test]
fn preparation_keeps_selected_and_borrowed_snapshots_old_until_final_commit() {
    for mode in MODES {
        let directory = Directory::new();
        let env = seed(&directory, mode);
        let old = env.begin_read();
        let old_bytes = old.get(b"kv", b"key").unwrap().unwrap();
        let current = fs::read(directory.0.join("CURRENT")).unwrap();
        let previous = fs::read(directory.0.join("PREVIOUS")).ok();
        let mut writer = env.begin_write().unwrap();
        writer.put(b"kv", b"key", b"new").unwrap();
        writer.delete_duplicates(b"dups", b"key").unwrap();
        writer.insert_duplicate(b"dups", b"key", b"new").unwrap();
        let prepared = writer.prepare().unwrap();
        assert!(prepared.will_publish());
        assert_eq!(prepared.sequence(), 2);
        assert_eq!(fs::read(directory.0.join("CURRENT")).unwrap(), current);
        assert_eq!(fs::read(directory.0.join("PREVIOUS")).ok(), previous);
        assert_eq!(env.begin_read().sequence(), 1);
        let independent = ReadTxn::open_coordinated_snapshot(&directory.0).unwrap();
        assert_eq!(independent.sequence(), 1);
        assert_eq!(
            independent.get(b"kv", b"key").unwrap(),
            Some(b"old".as_slice())
        );
        assert_eq!(prepared.commit().unwrap(), 2);
        assert_eq!(old_bytes, b"old");
        assert_eq!(
            old.duplicates(b"dups").unwrap().first(b"key").unwrap(),
            Some(b"old".as_slice())
        );
        assert_eq!(independent.sequence(), 1);
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"new".as_slice())
        );
        assert_eq!(
            env.begin_read()
                .duplicates(b"dups")
                .unwrap()
                .first(b"key")
                .unwrap(),
            Some(b"new".as_slice())
        );
        drop(env);
        let reopened = Environment::open(&directory.0).unwrap();
        assert_eq!(reopened.begin_read().sequence(), 2);
        assert_eq!(
            reopened.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"new".as_slice())
        );
    }
}

#[test]
fn abort_and_drop_release_candidate_pins_without_selecting_or_poisoning() {
    for mode in MODES {
        let directory = Directory::new();
        let env = seed(&directory, mode);
        let current = fs::read(directory.0.join("CURRENT")).unwrap();
        let baseline = pins(&directory);
        for explicit_abort in [true, false] {
            let mut writer = env.begin_write().unwrap();
            writer.put(b"kv", b"key", &vec![0x51; 65_536]).unwrap();
            let prepared = writer.prepare().unwrap();
            assert_eq!(pins(&directory), baseline + 1);
            if explicit_abort {
                prepared.abort();
            } else {
                drop(prepared);
            }
            assert_eq!(pins(&directory), baseline);
            assert_eq!(fs::read(directory.0.join("CURRENT")).unwrap(), current);
            assert_eq!(env.begin_read().sequence(), 1);
            env.begin_write().unwrap().abort();
        }
        // Aborted immutable files are ordinary orphans; verified maintenance
        // can collect them after the prepared owner's pin and fence are gone.
        let reclaimed = env.reclaim().unwrap();
        assert!(
            reclaimed.removed_segments + reclaimed.removed_cohorts + reclaimed.removed_descriptors
                > 0
        );
    }
}

#[test]
fn prepared_owner_retains_writer_exclusion_until_abort() {
    for mode in MODES {
        let directory = Directory::new();
        let env = seed(&directory, mode);
        let mut writer = env.begin_write().unwrap();
        writer.put(b"kv", b"key", b"candidate").unwrap();
        let prepared = writer.prepare().unwrap();
        std::thread::scope(|scope| {
            let (starting, started) = mpsc::channel();
            let (acquired, acquisition) = mpsc::channel();
            let env_ref = &env;
            scope.spawn(move || {
                starting.send(()).unwrap();
                let writer = env_ref.begin_write().unwrap();
                acquired.send(()).unwrap();
                writer.abort();
            });
            started.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(acquisition.recv_timeout(Duration::from_millis(20)).is_err());
            assert_eq!(env.begin_read().sequence(), 1);
            prepared.abort();
            acquisition.recv_timeout(Duration::from_secs(2)).unwrap();
        });
    }
}

#[test]
fn ordinary_noops_remain_noops_and_explicit_intent_publishes_real_generations() {
    for mode in MODES {
        let empty_directory = Directory::new();
        let empty = Environment::create_with_publication(&empty_directory.0, mode).unwrap();
        let head = fs::read(empty_directory.0.join("CURRENT")).unwrap();
        let prepared = empty
            .begin_write()
            .unwrap()
            .prepare_with_intent(CommitIntent::ForcePublication)
            .unwrap();
        assert_eq!(prepared.sequence(), 1);
        assert_eq!(fs::read(empty_directory.0.join("CURRENT")).unwrap(), head);
        assert_eq!(prepared.commit().unwrap(), 1);
        assert!(empty.begin_read().database_names().unwrap().is_empty());
        let directory = Directory::new();
        let env = seed(&directory, mode);
        let initial = fs::read(directory.0.join("CURRENT")).unwrap();
        let mut writer = env.begin_write().unwrap();
        writer.put(b"kv", b"key", b"old").unwrap();
        assert_eq!(writer.commit().unwrap(), 1);
        assert_eq!(fs::read(directory.0.join("CURRENT")).unwrap(), initial);
        let prepared = env.begin_write().unwrap().prepare().unwrap();
        assert!(!prepared.will_publish());
        assert_eq!(prepared.sequence(), 1);
        assert_eq!(prepared.commit().unwrap(), 1);
        let prepared = env
            .begin_write()
            .unwrap()
            .prepare_with_intent(CommitIntent::ForcePublication)
            .unwrap();
        assert!(prepared.will_publish());
        assert_eq!(prepared.sequence(), 2);
        assert_eq!(fs::read(directory.0.join("CURRENT")).unwrap(), initial);
        assert_eq!(prepared.commit().unwrap(), 2);
        assert_ne!(fs::read(directory.0.join("CURRENT")).unwrap(), initial);
        assert_eq!(
            env.begin_write()
                .unwrap()
                .commit_with_intent(CommitIntent::ForcePublication)
                .unwrap(),
            3
        );
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"old".as_slice())
        );
        assert_eq!(env.begin_read().database_names().unwrap().len(), 2);
    }
}

#[test]
fn range_preparation_is_rejected_and_ordinary_range_commits_are_preserved() {
    for mode in [PublicationMode::RangeV5]
        .into_iter()
        .chain(cfg!(feature = "range-lifecycle").then_some(PublicationMode::Range))
    {
        let directory = Directory::new();
        let env = Environment::create_with_publication(&directory.0, mode).unwrap();
        assert!(matches!(
            env.begin_write().unwrap().prepare(),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            env.begin_write()
                .unwrap()
                .prepare_with_intent(CommitIntent::ForcePublication),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            env.begin_write()
                .unwrap()
                .commit_with_intent(CommitIntent::ForcePublication),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(env.begin_write().unwrap().commit().unwrap(), 0);
        let mut writer = env.begin_write().unwrap();
        writer.create_database(b"kv", DatabaseKind::Kv).unwrap();
        writer.put(b"kv", b"key", b"old").unwrap();
        assert_eq!(writer.commit().unwrap(), 1);
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"old".as_slice())
        );
    }
}

#[test]
fn denied_residency_admission_does_not_select_or_leak_candidate_pins() {
    for mode in MODES {
        let directory = Directory::new();
        let env = seed(&directory, mode);
        let mapped = env.begin_read().residency_report().mapped_bytes.unwrap();
        drop(env);
        let env = Environment::open_with_residency(
            &directory.0,
            ulmdb::ResidencyPolicy::Prefault {
                max_bytes: mapped + 4096,
            },
        )
        .unwrap();
        let head = fs::read(directory.0.join("CURRENT")).unwrap();
        let baseline = pins(&directory);
        let mut writer = env.begin_write().unwrap();
        writer.put(b"kv", b"key", &vec![0x41; 131_072]).unwrap();
        assert!(matches!(writer.prepare(), Err(Error::LimitExceeded)));
        assert_eq!(fs::read(directory.0.join("CURRENT")).unwrap(), head);
        assert_eq!(pins(&directory), baseline);
        assert_eq!(env.begin_read().sequence(), 1);
        env.begin_write().unwrap().abort();
    }
}
