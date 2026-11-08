#![cfg(target_os = "linux")]
#[path = "../../ulmdb-storage/tests/support/read_ahead.rs"]
mod support;
use ulmdb::{
    CoordinatedEnvironment, DatabaseKind, Environment, OwnedFileCapture, PreviousSession,
    PublicationMode, ReadAhead, ReadTxn, ResidencyPolicy,
};
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
struct Dir(std::path::PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "read-ahead-native-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn seed(d: &Dir, mode: PublicationMode) -> Environment {
    let env = Environment::create_with_residency_and_read_ahead(
        &d.0,
        mode,
        ResidencyPolicy::Demand,
        ReadAhead::Random,
    )
    .unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", &vec![1; 8192]).unwrap();
    assert_eq!(w.commit().unwrap(), 1);
    env
}
#[test]
fn advice_survives_writes_maintenance_reopen_and_old_borrows() {
    for mode in MODES {
        let d = Dir::new();
        let env = seed(&d, mode);
        let old = env.begin_read();
        let borrowed = old.get(b"kv", b"key").unwrap().unwrap();
        support::assert_random(borrowed, true);
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", &vec![2; 8192]).unwrap();
        assert_eq!(w.prepare().unwrap().commit().unwrap(), 2);
        env.compact().unwrap();
        env.prune_inventory().unwrap();
        env.reclaim().unwrap();
        let now = env.begin_read();
        assert_eq!(now.read_ahead(), ReadAhead::Random);
        support::assert_random(now.get(b"kv", b"key").unwrap().unwrap(), true);
        drop(now);
        drop(env);
        let normal = Environment::open(&d.0).unwrap();
        let read = normal.begin_read();
        support::assert_random(read.get(b"kv", b"key").unwrap().unwrap(), false);
        assert_eq!(borrowed, vec![1; 8192]);
        support::assert_random(borrowed, true);
        let random =
            ReadTxn::open_coordinated_snapshot_with_read_ahead(&d.0, ReadAhead::Random).unwrap();
        support::assert_random(random.get(b"kv", b"key").unwrap().unwrap(), true);
        support::assert_random(read.get(b"kv", b"key").unwrap().unwrap(), false);
        println!(
            "CASE_PASS test=advice_survives_writes_maintenance_reopen_and_old_borrows backend={mode:?}"
        );
    }
}
#[test]
fn coordinated_and_previous_owners_keep_independent_policies() {
    for mode in MODES {
        let d = Dir::new();
        let env = seed(&d, mode);
        drop(env);
        let random = CoordinatedEnvironment::open_with_residency_and_read_ahead(
            &d.0,
            ResidencyPolicy::Demand,
            ReadAhead::Random,
        )
        .unwrap();
        let normal = CoordinatedEnvironment::open(&d.0).unwrap();
        random
            .update(|w| w.put(b"kv", b"key", &vec![2; 8192]))
            .unwrap();
        let r = random.begin_read().unwrap();
        let n = normal.begin_read().unwrap();
        support::assert_random(r.get(b"kv", b"key").unwrap().unwrap(), true);
        support::assert_random(n.get(b"kv", b"key").unwrap().unwrap(), false);
        drop(r);
        drop(n);
        drop(random);
        drop(normal);
        let prev = PreviousSession::open_with_residency_and_read_ahead(
            &d.0,
            ResidencyPolicy::Demand,
            ReadAhead::Random,
        )
        .unwrap();
        let r = prev.begin_read();
        assert_eq!(r.sequence(), 1);
        support::assert_random(r.get(b"kv", b"key").unwrap().unwrap(), true);
        drop(prev);
        assert_eq!(r.get(b"kv", b"key").unwrap().unwrap(), vec![1; 8192]);
        let capture =
            ulmdb_storage::coordination::Coordination::capture_directory(&d.0, Default::default())
                .unwrap();
        let owned = OwnedFileCapture::current(&d.0, capture).unwrap();
        let (r, _) = owned
            .admit_with_read_ahead(ResidencyPolicy::Demand, ReadAhead::Random)
            .unwrap();
        assert_eq!(r.sequence(), 2);
        support::assert_random(r.get(b"kv", b"key").unwrap().unwrap(), true);
        println!(
            "CASE_PASS test=coordinated_and_previous_owners_keep_independent_policies backend={mode:?}"
        );
    }
}
#[test]
fn rollback_retains_policy_and_range_requests_are_explicitly_rejected() {
    for mode in MODES {
        let d = Dir::new();
        let env = seed(&d, mode);
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"key", &vec![2; 8192]).unwrap();
        w.commit().unwrap();
        env.rollback_last_commit(2).unwrap();
        let r = env.begin_read();
        assert_eq!(r.sequence(), 1);
        support::assert_random(r.get(b"kv", b"key").unwrap().unwrap(), true);
        println!(
            "CASE_PASS test=rollback_retains_policy_and_range_requests_are_explicitly_rejected backend={mode:?}"
        );
    }
    for mode in [PublicationMode::RangeV5, PublicationMode::Range] {
        let d = Dir::new();
        assert!(
            Environment::create_with_residency_and_read_ahead(
                &d.0,
                mode,
                ResidencyPolicy::Demand,
                ReadAhead::Random
            )
            .is_err()
        );
        assert!(!d.0.exists());
    }
}

#[test]
fn retained_readers_preserve_policy_old_bytes_and_physical_pins() {
    for mode in MODES {
        for policy in [ReadAhead::Normal, ReadAhead::Random] {
            let d = Dir::new();
            let env = Environment::create_with_residency_and_read_ahead(
                &d.0,
                mode,
                ResidencyPolicy::Demand,
                policy,
            )
            .unwrap();
            let mut w = env.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            w.put(b"kv", b"key", &vec![1; 8192]).unwrap();
            w.commit().unwrap();
            let old = env.begin_read();
            env.retain_snapshot("old", &old).unwrap();
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"key", &vec![2; 8192]).unwrap();
            w.commit().unwrap();
            let retained = env.open_retained("old").unwrap();
            assert_eq!(retained.sequence(), 1);
            assert_eq!(retained.read_ahead(), policy);
            let borrowed = retained.get(b"kv", b"key").unwrap().unwrap();
            assert_eq!(borrowed, vec![1; 8192]);
            support::assert_random(borrowed, policy == ReadAhead::Random);
            env.release_snapshot("old").unwrap();
            env.reclaim().unwrap();
            drop(old);
            drop(env);
            let normal = Environment::open(&d.0).unwrap();
            let current = normal.begin_read();
            assert_eq!(current.get(b"kv", b"key").unwrap().unwrap(), vec![2; 8192]);
            support::assert_random(current.get(b"kv", b"key").unwrap().unwrap(), false);
            assert_eq!(borrowed, vec![1; 8192]);
            support::assert_random(borrowed, policy == ReadAhead::Random);
            println!(
                "CASE_PASS test=retained_readers_preserve_policy_old_bytes_and_physical_pins backend={mode:?} policy={policy:?}"
            );
        }
    }
}
