#![cfg(target_os = "linux")]
//! Runtime/local policy, selected-state and exported-snapshot behavior. Actual
//! publication fences have a separate syscall oracle; neither proves power loss.
use std::{fs, path::PathBuf};
use ulmdb::{DatabaseKind, Durability, Environment, PublicationMode};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-runtime-policy-{}",
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

fn selected_state(directory: &Directory) -> ulmdb_storage::FileDurabilityState {
    let (_, capture, pin, state) = ulmdb_storage::coordination::Coordination::capture_directory(
        &directory.0,
        Default::default(),
    )
    .unwrap()
    .capture_current_with_durability()
    .unwrap();
    assert!(state.matches(pin.root().checkpoint()));
    drop(pin);
    drop(capture);
    state
}

fn exercise_sync_policy(env: &Environment, directory: &Directory) {
    let current = directory.0.join("CURRENT");
    let hidden = directory.0.join("CURRENT-hidden-by-test");
    let before = fs::read(&current).unwrap();
    env.set_durability(Durability::NoSync).unwrap();
    assert_eq!(env.durability(), Durability::NoSync);
    fs::rename(&current, &hidden).unwrap();
    let skipped = env.sync_with_force(false);
    let forced = env.sync_with_force(true);
    fs::rename(&hidden, &current).unwrap();
    assert!(matches!(skipped, Ok(None)));
    // A forced operation actually consults the selected files. It cannot
    // report a successful fence while the selected pointer is absent.
    assert!(forced.is_err());
    assert_eq!(fs::read(&current).unwrap(), before);
    env.set_durability(Durability::NoMetaSync).unwrap();
    let report = env.sync_with_force(false).unwrap().unwrap();
    assert!(report.files_synced > 0 && report.directories_synced > 0);
    env.set_durability(Durability::Full).unwrap();
    assert!(env.sync_with_force(false).unwrap().is_some());
    assert_eq!(fs::read(&current).unwrap(), before);
}

#[test]
fn runtime_updates_and_force_sync_work_with_retained_writers() {
    for mode in modes() {
        let directory = Directory::new();
        let env = Environment::create_with_publication(&directory.0, mode).unwrap();
        assert_eq!(env.durability(), Durability::Full);
        let mut seed = env.begin_write().unwrap();
        seed.create_database(b"data", DatabaseKind::Kv).unwrap();
        seed.put(b"data", b"key", b"old").unwrap();
        seed.commit().unwrap();
        let old = env.begin_read();

        let clean = env.begin_write().unwrap();
        exercise_sync_policy(&env, &directory);
        drop(clean);
        assert_eq!(
            env.begin_read().kv(b"data").unwrap().get(b"key").unwrap(),
            Some(b"old".as_slice())
        );

        let mut dirty = env.begin_write().unwrap();
        dirty.put(b"data", b"key", b"dirty").unwrap();
        exercise_sync_policy(&env, &directory);
        assert_eq!(
            env.begin_read().kv(b"data").unwrap().get(b"key").unwrap(),
            Some(b"old".as_slice())
        );
        dirty.commit().unwrap();

        let mut writer = env.begin_write().unwrap();
        writer.put(b"data", b"key", b"prepared").unwrap();
        let prepared = writer.prepare().unwrap();
        exercise_sync_policy(&env, &directory);
        assert_eq!(
            env.begin_read().kv(b"data").unwrap().get(b"key").unwrap(),
            Some(b"dirty".as_slice())
        );
        prepared.commit().unwrap();
        assert_eq!(
            env.begin_read().kv(b"data").unwrap().get(b"key").unwrap(),
            Some(b"prepared".as_slice())
        );
        assert_eq!(
            old.kv(b"data").unwrap().get(b"key").unwrap(),
            Some(b"old".as_slice())
        );
    }
}

#[test]
fn savepoint_policy_commit_abort_and_rollback_preserve_parent_rules() {
    for mode in modes() {
        let directory = Directory::new();
        let env = Environment::create_with_publication(&directory.0, mode).unwrap();
        let mut seed = env.begin_write().unwrap();
        seed.create_database(b"data", DatabaseKind::Kv).unwrap();
        seed.commit().unwrap();
        let mut parent = env.begin_write().unwrap();
        parent.put(b"data", b"kept", b"yes").unwrap();
        {
            let mut child = parent.savepoint().unwrap();
            child.weaken_durability(Durability::NoSync).unwrap();
            child.put(b"data", b"aborted", b"no").unwrap();
            assert_eq!(child.durability(), Durability::NoSync);
            child.abort().unwrap();
        }
        assert_eq!(parent.durability(), Durability::Full);
        {
            let mut clean = parent.savepoint().unwrap();
            clean.weaken_durability(Durability::NoMetaSync).unwrap();
            clean.release().unwrap();
        }
        assert_eq!(parent.durability(), Durability::NoMetaSync);
        {
            let mut child = parent.savepoint().unwrap();
            child.weaken_durability(Durability::NoSync).unwrap();
            child.rollback().unwrap();
            assert_eq!(child.durability(), Durability::NoMetaSync);
            child.weaken_durability(Durability::Full).unwrap();
            child.release().unwrap();
        }
        env.set_durability(Durability::NoSync).unwrap();
        assert_eq!(parent.durability(), Durability::NoSync);
        env.set_durability(Durability::Full).unwrap();
        assert_eq!(parent.durability(), Durability::NoMetaSync);
        parent.commit().unwrap();
        drop(env);
        let reopened = Environment::open(&directory.0).unwrap();
        assert_eq!(reopened.durability(), Durability::Full);
        let read = reopened.begin_read();
        assert_eq!(
            read.kv(b"data").unwrap().get(b"kept").unwrap(),
            Some(b"yes".as_slice())
        );
        assert_eq!(read.kv(b"data").unwrap().get(b"aborted").unwrap(), None);
    }
}

#[test]
fn preparation_data_and_final_metadata_policies_remain_independent() {
    let policies = [Durability::Full, Durability::NoMetaSync, Durability::NoSync];
    for mode in modes() {
        for data in policies {
            for metadata in policies {
                let directory = Directory::new();
                let env = Environment::create_with_publication(&directory.0, mode).unwrap();
                let mut seed = env.begin_write().unwrap();
                seed.create_database(b"data", DatabaseKind::Kv).unwrap();
                seed.put(b"data", b"key", b"old").unwrap();
                seed.commit().unwrap();
                let old = env.begin_read();
                let before = selected_state(&directory);
                env.set_durability(data).unwrap();
                let mut writer = env.begin_write().unwrap();
                writer.put(b"data", b"key", b"new").unwrap();
                let pending = writer.prepare().unwrap();
                assert_eq!(pending.data_policy(), data);
                assert_eq!(selected_state(&directory), before);
                env.set_durability(metadata).unwrap();
                pending.commit().unwrap();
                let selected = selected_state(&directory);
                let debt = data == Durability::NoSync;
                assert_eq!(
                    selected.has_data_debt(),
                    debt,
                    "{mode:?} {data:?} {metadata:?}"
                );
                assert_eq!(selected.is_weak(), metadata != Durability::Full || debt);
                assert_eq!(
                    env.begin_read().get(b"data", b"key").unwrap(),
                    Some(b"new".as_slice())
                );
                assert_eq!(old.get(b"data", b"key").unwrap(), Some(b"old".as_slice()));
                let bytes = fs::read(directory.0.join("CURRENT")).unwrap();
                env.sync().unwrap();
                assert_eq!(fs::read(directory.0.join("CURRENT")).unwrap(), bytes);
                assert_eq!(selected_state(&directory), selected);
                drop(env);
                let reopened = Environment::open(&directory.0).unwrap();
                assert_eq!(reopened.durability(), Durability::Full);
                assert_eq!(
                    reopened.begin_read().get(b"data", b"key").unwrap(),
                    Some(b"new".as_slice())
                );
                let mut repair = reopened.begin_write().unwrap();
                repair.put(b"data", b"repaired", b"full").unwrap();
                repair.commit().unwrap();
                let repaired = selected_state(&directory);
                assert!(!repaired.is_weak() && !repaired.has_data_debt());
                println!(
                    "NATIVE_POLICY mode={mode:?} data={data:?} metadata={metadata:?} debt={debt}"
                );
            }
        }
    }
}

#[test]
fn explicit_facade_policies_keep_local_weakness_separate_from_environment() {
    for mode in modes() {
        for local in [Durability::Full, Durability::NoMetaSync, Durability::NoSync] {
            let directory = Directory::new();
            let env = Environment::create_with_publication(&directory.0, mode).unwrap();
            let mut seed = env.begin_write().unwrap();
            seed.create_database(b"data", DatabaseKind::Kv).unwrap();
            seed.commit().unwrap();
            let mut writer = env.begin_write_with_durability(local).unwrap();
            writer.put(b"data", b"key", b"new").unwrap();
            let pending = writer
                .prepare_for_commit_with_data_policy(
                    ulmdb::CommitIntent::IfChanged,
                    Durability::NoSync,
                )
                .unwrap();
            assert_eq!(pending.data_policy(), Durability::NoSync);
            pending
                .commit_with_metadata_policy(Durability::Full)
                .unwrap();
            let state = selected_state(&directory);
            assert!(state.has_data_debt() && state.is_weak());
            assert_eq!(
                env.begin_read().get(b"data", b"key").unwrap(),
                Some(b"new".as_slice())
            );
        }
    }
}

#[test]
fn weak_cross_index_commits_and_full_maintenance_preserve_native_features() {
    use ulmdb::{Address, Prefix};
    for mode in modes() {
        let directory = Directory::new();
        let env = Environment::create_with_publication(&directory.0, mode).unwrap();
        let v4 = Prefix::new(Address::V4(0x0a000000), 8).unwrap();
        let v6 = Prefix::new(Address::V6(0x20010db8000000000000000000000000), 32).unwrap();
        let mut seed = env.begin_write().unwrap();
        seed.create_database(b"data", DatabaseKind::Kv).unwrap();
        seed.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
        seed.create_database(b"v6", DatabaseKind::Ipv6).unwrap();
        seed.put(b"data", b"key", b"old").unwrap();
        seed.insert_prefix(b"v4", v4, b"old4").unwrap();
        seed.insert_prefix(b"v6", v6, b"old6").unwrap();
        seed.commit().unwrap();
        let old = env.begin_read();
        env.set_durability(Durability::NoSync).unwrap();
        let mut write = env.begin_write().unwrap();
        write.put(b"data", b"key", b"new").unwrap();
        write.insert_prefix(b"v4", v4, b"new4").unwrap();
        write.insert_prefix(b"v6", v6, b"new6").unwrap();
        write.commit().unwrap();
        assert!(selected_state(&directory).has_data_debt());
        let new = env.begin_read();
        assert_eq!(new.get(b"data", b"key").unwrap(), Some(b"new".as_slice()));
        assert_eq!(new.exact_prefix(b"v4", v4).unwrap().unwrap().1, b"new4");
        assert_eq!(new.exact_prefix(b"v6", v6).unwrap().unwrap().1, b"new6");
        assert_eq!(old.get(b"data", b"key").unwrap(), Some(b"old".as_slice()));
        assert_eq!(old.exact_prefix(b"v4", v4).unwrap().unwrap().1, b"old4");
        assert_eq!(old.exact_prefix(b"v6", v6).unwrap().unwrap().1, b"old6");
        // Maintenance remains Full even when business policy is NoSync.
        env.compact().unwrap();
        let compacted = selected_state(&directory);
        assert!(!compacted.has_data_debt() && !compacted.is_weak());
        env.set_durability(Durability::NoMetaSync).unwrap();
        let mut after = env.begin_write().unwrap();
        after.put(b"data", b"after-maintenance", b"yes").unwrap();
        after.commit().unwrap();
        let selected = selected_state(&directory);
        assert!(selected.is_weak() && !selected.has_data_debt());
        drop(env);
        let reopened = Environment::open(&directory.0).unwrap();
        let read = reopened.begin_read();
        assert_eq!(
            read.get(b"data", b"after-maintenance").unwrap(),
            Some(b"yes".as_slice())
        );
        assert_eq!(read.exact_prefix(b"v4", v4).unwrap().unwrap().1, b"new4");
        assert_eq!(read.exact_prefix(b"v6", v6).unwrap().unwrap().1, b"new6");
    }
}

#[test]
fn weak_source_exports_fence_independent_historyless_snapshots() {
    for mode in modes() {
        for policy in [Durability::NoMetaSync, Durability::NoSync] {
            let source = Directory::new();
            let env = Environment::create_with_publication(&source.0, mode).unwrap();
            let mut seed = env.begin_write().unwrap();
            seed.create_database(b"data", DatabaseKind::Kv).unwrap();
            seed.put(b"data", b"key", b"A").unwrap();
            seed.commit().unwrap();
            let old = env.begin_read();
            env.set_durability(policy).unwrap();
            let mut writer = env.begin_write().unwrap();
            writer.put(b"data", b"key", b"B").unwrap();
            writer.commit().unwrap();
            let read = env.begin_read();
            let source_state = selected_state(&source);
            let source_current = fs::read(source.0.join("CURRENT")).unwrap();
            assert!(source_state.is_weak());
            for client_copy in [false, true] {
                let destination = Directory::new();
                if client_copy {
                    ulmdb::backup::export_independent_snapshot(&source.0, &read, &destination.0)
                        .unwrap();
                } else {
                    ulmdb::backup::export(&env, &destination.0).unwrap();
                }
                let exported = Environment::open(&destination.0).unwrap();
                ulmdb::backup::verify(&exported).unwrap();
                let state = selected_state(&destination);
                assert!(!state.is_weak() && !state.has_data_debt());
                assert!(
                    ulmdb_storage::read_roots(&destination.0)
                        .unwrap()
                        .1
                        .is_none()
                );
                assert!(!destination.0.join("PREVIOUS").exists());
                assert_eq!(
                    exported.begin_read().get(b"data", b"key").unwrap(),
                    Some(b"B".as_slice())
                );
                assert!(exported.sync().unwrap().previous_checkpoint.is_none());
                let mut independent = exported.begin_write().unwrap();
                independent.put(b"data", b"key", b"independent").unwrap();
                independent.commit().unwrap();
                assert_eq!(selected_state(&source), source_state);
                assert_eq!(fs::read(source.0.join("CURRENT")).unwrap(), source_current);
                assert_eq!(read.get(b"data", b"key").unwrap(), Some(b"B".as_slice()));
                assert_eq!(old.get(b"data", b"key").unwrap(), Some(b"A".as_slice()));
                println!(
                    "WEAK_SOURCE_EXPORT mode={mode:?} policy={policy:?} client_copy={client_copy}"
                );
            }
        }
    }
}
