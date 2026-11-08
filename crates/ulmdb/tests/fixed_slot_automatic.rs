#![cfg(target_os = "linux")]
use std::{fs, path::PathBuf};
use ulmdb::{
    DatabaseKind, Environment, MaintenancePolicy, PublicationMode, ResumableReclaimBudget,
};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "fixed-auto-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn isolated(name: &str) -> bool {
    if std::env::var("ULMDB_FIXED_NATIVE_WORKER").is_ok_and(|v| v == name) {
        return false;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("ULMDB_FIXED_NATIVE_WORKER", name)
        .spawn()
        .unwrap();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{name}: {status}");
            return true;
        }
        if std::time::Instant::now() > end {
            let _ = child.kill();
            let _ = child.wait();
            panic!("owned fixed test worker exceeded deadline: {name}");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
fn policy() -> MaintenancePolicy {
    MaintenancePolicy::CrossFormatResumable {
        segment_threshold: 8,
        rewrite_budget_bytes: 128 << 10,
        reclaim: ResumableReclaimBudget {
            mark_units: 128,
            unlink_files: 16,
            max_candidates: 4096,
            ..Default::default()
        },
        history_budget_bytes: 1 << 20,
    }
}
fn seed(env: &Environment) {
    let mut w = env.begin_write().unwrap();
    w.create_database(b"db", DatabaseKind::Kv).unwrap();
    w.put(b"db", b"k", &vec![0; 8192]).unwrap();
    w.put(b"db", b"cold", &vec![7; 8192]).unwrap();
    w.commit().unwrap();
}
fn put(env: &Environment, n: u8) {
    let mut w = env.begin_write().unwrap();
    w.put(b"db", b"k", &vec![n; 8192]).unwrap();
    w.commit().unwrap();
}
#[test]
fn automatic_v2_rewrites_and_reuses_without_explicit_compact_or_reclaim() {
    if isolated("automatic_v2_rewrites_and_reuses_without_explicit_compact_or_reclaim") {
        return;
    }

    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new();
        let env = Environment::create_reusable_fixed_map(&d.0, mode, 4 << 20, None).unwrap();
        seed(&env);
        assert_eq!(
            env.maintenance_policy().unwrap(),
            MaintenancePolicy::AUTOMATIC
        );
        let old = env.begin_read();
        let bytes = old.get(b"db", b"k").unwrap().unwrap();
        let pointer = bytes.as_ptr();
        for n in 1..=192 {
            put(&env, n);
            assert_eq!(bytes, vec![0; 8192]);
            assert_eq!(bytes.as_ptr(), pointer);
        }
        let s = env.maintenance_status().unwrap();
        let info = env.fixed_map_info().unwrap().unwrap();
        assert!(s.fallback_rewrite_attempts > 0, "{mode:?}: {s:?}");
        assert!(s.fixed_slots_removed > 0, "{mode:?}: {s:?}");
        assert!(s.fixed_slot_reclaim_passes > 0, "{mode:?}: {s:?}");
        assert!(
            info.assigned_bytes <= info.capacity,
            "{mode:?}: assigned={} {s:?}",
            info.assigned_bytes
        );
        assert_eq!(
            env.begin_read().get(b"db", b"k").unwrap().unwrap(),
            vec![192; 8192]
        );
        drop(old);
        drop(env);
        let env = Environment::open_fixed_map(&d.0).unwrap();
        assert_eq!(
            env.begin_read().get(b"db", b"k").unwrap().unwrap(),
            vec![192; 8192]
        );
    }
}
#[cfg(feature = "fault-injection")]
#[test]
fn optional_failure_keeps_ready_business_candidate_and_reclaims_speculative_slots() {
    const ROLE: &str = "ULMDB_FIXED_OPTIONAL_CHILD";
    if std::env::var_os(ROLE).is_some() {
        for mode in [
            PublicationMode::Legacy,
            PublicationMode::Cohort,
            PublicationMode::Indexed,
        ] {
            let d = Dir::new();
            let env = Environment::create_reusable_fixed_map(&d.0, mode, 4 << 20, None).unwrap();
            seed(&env);
            env.set_maintenance_policy_with_fallback(policy()).unwrap();
            let old = env.begin_read();
            let bytes = old.get(b"db", b"k").unwrap().unwrap();
            for n in 1..=32 {
                put(&env, n);
                assert_eq!(bytes, vec![0; 8192]);
            }
            let s = env.maintenance_status().unwrap();
            assert!(s.fallback_rewrite_deferred > 0, "{mode:?}: {s:?}");
            assert!(s.fixed_slots_removed > 0, "{mode:?}: {s:?}");
            assert!(
                s.last_fallback_rewrite_error
                    .as_deref()
                    .is_some_and(|s| s.contains("injected")),
                "{s:?}"
            );
            drop(old);
            drop(env);
            let env = Environment::open_fixed_map(&d.0).unwrap();
            assert_eq!(
                env.begin_read().get(b"db", b"k").unwrap().unwrap(),
                vec![32; 8192]
            );
        }
        return;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "optional_failure_keeps_ready_business_candidate_and_reclaims_speculative_slots",
            "--nocapture",
        ])
        .env(ROLE, "1")
        .env("ULMDB_FAIL_AT", "maintenance.optional.before_admission")
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{status}");
            break;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("owned optional-failure worker exceeded deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
#[test]
fn automatic_default_handles_half_megabyte_overwrites_before_segment_trigger() {
    if isolated("automatic_default_handles_half_megabyte_overwrites_before_segment_trigger") {
        return;
    }

    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new();
        let env = Environment::create_reusable_fixed_map(&d.0, mode, 4 << 20, None).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"db", DatabaseKind::Kv).unwrap();
        w.put(b"db", b"k", &vec![0; 512 << 10]).unwrap();
        w.commit().unwrap();
        let old = env.begin_read();
        let bytes = old.get(b"db", b"k").unwrap().unwrap();
        let pointer = bytes.as_ptr();
        let mark_budget = match env.maintenance_policy().unwrap() {
            MaintenancePolicy::CrossFormatResumable { reclaim, .. } => reclaim.mark_units,
            other => panic!("unexpected default fixed maintenance: {other:?}"),
        };
        for n in 1..=24u8 {
            let mut w = env.begin_write().unwrap();
            assert!(env.maintenance_status().unwrap().last_resumable_mark_units <= mark_budget);
            w.put(b"db", b"k", &vec![n; 512 << 10]).unwrap();
            w.commit().unwrap();
            assert!(env.maintenance_status().unwrap().last_resumable_mark_units <= mark_budget);
            assert_eq!(bytes, &vec![0; 512 << 10]);
            assert_eq!(bytes.as_ptr(), pointer);
        }
        let status = env.maintenance_status().unwrap();
        assert!(status.fallback_rewrite_attempts > 0, "{mode:?}: {status:?}");
        assert!(status.fixed_slots_removed > 0, "{mode:?}: {status:?}");
        assert_eq!(
            env.begin_read().get(b"db", b"k").unwrap().unwrap(),
            vec![24; 512 << 10]
        );
        drop(old);
        drop(env);
        let env = Environment::open_fixed_map(&d.0).unwrap();
        assert_eq!(
            env.begin_read().get(b"db", b"k").unwrap().unwrap(),
            vec![24; 512 << 10]
        );
    }
}
#[test]
fn pressure_reclaims_aborted_ready_candidates_before_a_new_writer() {
    if isolated("pressure_reclaims_aborted_ready_candidates_before_a_new_writer") {
        return;
    }

    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new();
        let env = Environment::create_reusable_fixed_map(&d.0, mode, 4 << 20, None).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"db", DatabaseKind::Kv).unwrap();
        w.put(b"db", b"k", &vec![0; 512 << 10]).unwrap();
        w.commit().unwrap();
        let old = env.begin_read();
        let bytes = old.get(b"db", b"k").unwrap().unwrap();
        let selected = fs::read(d.0.join("CURRENT")).unwrap();
        for n in 1..=16u8 {
            let mut w = env.begin_write().unwrap();
            w.put(b"db", b"k", &vec![n; 512 << 10]).unwrap();
            let ready = w.prepare().unwrap();
            ready.abort();
            assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), selected);
            assert_eq!(bytes, vec![0; 512 << 10]);
        }
        assert!(env.maintenance_status().unwrap().fixed_slots_removed > 0);
        let mut w = env.begin_write().unwrap();
        w.put(b"db", b"k", &vec![99; 512 << 10]).unwrap();
        w.commit().unwrap();
        assert_eq!(bytes, vec![0; 512 << 10]);
        assert_eq!(
            env.begin_read().get(b"db", b"k").unwrap().unwrap(),
            vec![99; 512 << 10]
        );
    }
}
