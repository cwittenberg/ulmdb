#![cfg(windows)]
//! Real Windows weak publication/view-flush lifecycle. Requires the supported
//! local NTFS namespace barrier; no power-loss conclusion follows from success.
use std::{fs, path::PathBuf};
use ulmdb::{DatabaseKind, Durability, Environment, MappedFlush, PublicationMode};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-win-policy-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn modes() -> [PublicationMode; 3] {
    [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ]
}
fn state(path: &Directory) -> ulmdb_storage::FileDurabilityState {
    let (_, capture, pin, state) =
        ulmdb_storage::coordination::Coordination::capture_directory(&path.0, Default::default())
            .unwrap()
            .capture_current_with_durability()
            .unwrap();
    assert!(state.matches(pin.root().checkpoint()));
    drop(pin);
    drop(capture);
    state
}
#[test]
fn async_view_flush_preserves_debt_and_old_borrows_until_a_real_full_publication() {
    for mode in modes() {
        let path = Directory::new();
        let env = Environment::create_with_publication(&path.0, mode).unwrap();
        let initial = vec![11; 65536];
        {
            let mut w = env.begin_write().unwrap();
            w.create_database(b"d", DatabaseKind::Kv).unwrap();
            w.put(b"d", b"k", &initial).unwrap();
            assert_eq!(w.commit().unwrap(), 1);
        }
        let old = env.begin_read();
        let borrow = old.get(b"d", b"k").unwrap().unwrap();
        env.set_durability(Durability::NoSync).unwrap();
        let mut w = env.begin_write().unwrap();
        w.enable_write_map(2 << 20, MappedFlush::Asynchronous)
            .unwrap();
        w.put(b"d", b"k", &vec![22; 65536]).unwrap();
        let pending = w.prepare().unwrap();
        env.set_durability(Durability::Full).unwrap();
        assert_eq!(pending.commit().unwrap(), 2);
        assert!(state(&path).has_data_debt());
        let before = fs::read(path.0.join("CURRENT")).unwrap();
        let scheduled = ulmdb_storage::flush_async_file_environment(&path.0, None).unwrap();
        assert!(scheduled.files_scheduled > 0);
        assert!(scheduled.file_bytes_covered > 0);
        assert_eq!(fs::read(path.0.join("CURRENT")).unwrap(), before);
        assert!(state(&path).has_data_debt());
        assert_eq!(borrow, initial.as_slice());
        let forced = env.sync().unwrap();
        assert!(forced.files_synced > 0);
        assert_eq!(forced.selected_checkpoint, scheduled.selected_checkpoint);
        {
            let mut w = env.begin_write().unwrap();
            w.put(b"d", b"k", b"strong").unwrap();
            assert_eq!(w.commit().unwrap(), 3);
        }
        assert!(!state(&path).has_data_debt());
        assert!(!state(&path).is_weak());
        assert_eq!(borrow, initial.as_slice());
        drop(env);
        let reopened = Environment::open(&path.0).unwrap();
        assert_eq!(borrow, initial.as_slice());
        assert_eq!(
            reopened.begin_read().get(b"d", b"k").unwrap(),
            Some(b"strong".as_slice())
        );
    }
}
#[test]
fn no_meta_and_no_sync_are_distinct_and_force_sync_works_with_a_retained_writer() {
    for mode in modes() {
        let path = Directory::new();
        let env = Environment::create_with_publication(&path.0, mode).unwrap();
        env.set_durability(Durability::NoMetaSync).unwrap();
        {
            let mut w = env.begin_write().unwrap();
            w.create_database(b"d", DatabaseKind::Kv).unwrap();
            w.put(b"d", b"k", b"A").unwrap();
            w.commit().unwrap();
        }
        assert!(state(&path).is_weak());
        assert!(!state(&path).has_data_debt());
        env.set_durability(Durability::NoSync).unwrap();
        let mut writer = env.begin_write().unwrap();
        writer.put(b"d", b"k", b"B").unwrap();
        let before = fs::read(path.0.join("CURRENT")).unwrap();
        assert!(env.sync_with_force(false).unwrap().is_none());
        assert!(env.sync_with_force(true).unwrap().is_some());
        assert_eq!(fs::read(path.0.join("CURRENT")).unwrap(), before);
        assert_eq!(
            env.begin_read().get(b"d", b"k").unwrap(),
            Some(b"A".as_slice())
        );
        writer.abort();
        assert_eq!(
            env.begin_read().get(b"d", b"k").unwrap(),
            Some(b"A".as_slice())
        );
    }
}
