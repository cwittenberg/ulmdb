//! Native policy/lifetime tests run in isolated resource-limited subprocesses.
use std::{
    fs,
    io::Cursor as Input,
    path::{Path, PathBuf},
    process::Command,
};
use ulmdb::replication::{
    self, ApplyOutcome, Cursor, PublisherKey, ReplicationError as RError, TransferLimits,
    VerifiedTransfer,
};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix, PublicationMode};
use ulmdb_storage::{ResidencyController, ResidencyPolicy, SegmentTable};
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        let path = parent.join(format!(
            "ulmdb-residency-{}",
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
fn populate(env: &Environment, large: usize) {
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
    w.create_database(b"dups", DatabaseKind::Duplicates { width: None })
        .unwrap();
    w.put(b"kv", b"state", b"old").unwrap();
    w.put(b"kv", b"payload", &vec![0x71; large]).unwrap();
    w.insert_prefix(b"ip", prefix(), b"old").unwrap();
    w.insert_duplicate(b"dups", b"state", b"old").unwrap();
    w.commit().unwrap();
}
fn update(env: &Environment) -> ulmdb::Result<u64> {
    let mut w = env.begin_write()?;
    w.put(b"kv", b"state", b"new")?;
    w.insert_prefix(b"ip", prefix(), b"new")?;
    w.delete_duplicates(b"dups", b"state")?;
    w.insert_duplicate(b"dups", b"state", b"new")?;
    w.commit()
}
fn inspect(read: &ulmdb::ReadTxn, new: bool) {
    let bytes: &[u8] = if new { b"new" } else { b"old" };
    assert_eq!(read.get(b"kv", b"state").unwrap(), Some(bytes));
    assert_eq!(
        read.exact_prefix(b"ip", prefix()).unwrap(),
        Some((prefix(), bytes))
    );
    assert!(
        read.duplicates(b"dups")
            .unwrap()
            .contains(b"state", bytes)
            .unwrap()
    );
}
fn footprint(path: &Path) -> u64 {
    let m = ulmdb_storage::read_current(path).unwrap();
    let maps = SegmentTable::load(path, &m.segments).unwrap();
    let controller = ResidencyController::new(ResidencyPolicy::Prefault {
        max_bytes: u64::MAX,
    })
    .unwrap();
    controller
        .admit(&maps)
        .unwrap()
        .report()
        .page_rounded_bytes
        .unwrap()
}
fn locked_kib() -> u64 {
    let status = fs::read_to_string("/proc/self/status").unwrap();
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmLck:"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap()
}
fn each_media(mut case: impl FnMut(&Path, PublicationMode)) {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        assert!(parent.is_dir());
        for mode in MODES {
            case(&parent, mode);
        }
    }
}
fn prefault_lifecycle() {
    each_media(|parent, mode| {
        let d = Directory::new(parent);
        let path = d.child("db");
        let policy = ResidencyPolicy::Prefault {
            max_bytes: 1_048_576,
        };
        let env = Environment::create_with_residency(&path, mode, policy).unwrap();
        assert_eq!(env.residency_report().page_rounded_bytes, Some(0));
        populate(&env, 8192);
        let report = env.residency_report();
        assert_eq!(report.policy, policy);
        assert!(report.mapped_bytes.unwrap() > 0);
        assert_eq!(report.prefaulted_bytes, report.page_rounded_bytes.unwrap());
        assert_eq!(report.locked_bytes, 0);
        let old = env.begin_read();
        update(&env).unwrap();
        inspect(&old, false);
        inspect(&env.begin_read(), true);
        drop(old);
        drop(env);
        let env = Environment::open_with_residency(&path, policy).unwrap();
        inspect(&env.begin_read(), true);
        assert_eq!(env.residency_report().policy, policy);
        drop(env);
        let demand = Environment::open(&path).unwrap();
        let report = demand.residency_report();
        assert_eq!(report.policy, ResidencyPolicy::Demand);
        assert_eq!(report.mapped_bytes, Some(demand.stats().segment_bytes));
        assert_eq!(report.page_rounded_bytes, None);
        assert_eq!(report.locked_bytes, 0);
    });
}
fn prefault_budget_and_retention() {
    each_media(|parent, mode| {
        let d = Directory::new(parent);
        let path = d.child("db");
        let env = Environment::create_with_publication(&path, mode).unwrap();
        populate(&env, 100_000);
        env.retain_snapshot("large", &env.begin_read()).unwrap();
        let mut w = env.begin_write().unwrap();
        w.put(b"kv", b"payload", b"small").unwrap();
        w.commit().unwrap();
        env.compact().unwrap();
        env.compact().unwrap();
        drop(env);
        let budget = footprint(&path);
        let env = Environment::open_with_residency(
            &path,
            ResidencyPolicy::Prefault { max_bytes: budget },
        )
        .unwrap();
        let held = env.begin_read();
        let pointer = fs::read(path.join("CURRENT")).unwrap();
        assert!(matches!(update(&env), Err(Error::LimitExceeded)));
        assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
        inspect(&held, false);
        inspect(&env.begin_read(), false);
        assert!(matches!(
            env.open_retained("large"),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(
            env.begin_write().unwrap().commit().unwrap(),
            held.sequence()
        );
    });
}
fn retained_fresh_corruption_check() {
    let d = Directory::new(&std::env::temp_dir());
    let path = d.child("db");
    let env = Environment::create_with_residency(
        &path,
        PublicationMode::Legacy,
        ResidencyPolicy::Prefault {
            max_bytes: 1_048_576,
        },
    )
    .unwrap();
    populate(&env, 8192);
    let held = env.begin_read();
    env.retain_snapshot("current", &held).unwrap();
    let m = ulmdb_storage::read_current(&path).unwrap();
    let original = ulmdb_storage::segment_path(&path, &m.segments[0]);
    fs::rename(&original, d.child("original-inode")).unwrap();
    fs::write(&original, b"corrupt replacement").unwrap();
    assert!(matches!(
        env.open_retained("current"),
        Err(Error::Corrupt(_))
    ));
    inspect(&held, false);
}
fn locked_lifetime_and_overlap() {
    each_media(|parent, mode| {
        assert_eq!(locked_kib(), 0);
        let d = Directory::new(parent);
        let path = d.child("db");
        let env = Environment::create_with_publication(&path, mode).unwrap();
        populate(&env, 8192);
        drop(env);
        let env = Environment::open_with_residency(
            &path,
            ResidencyPolicy::RequiredLocked {
                max_bytes: 1_048_576,
            },
        )
        .unwrap();
        let old = env.begin_read();
        let first = env.residency_report();
        assert!(first.locked_bytes > 0);
        assert_eq!(
            locked_kib() * 1024,
            env.residency_status().live_locked_bytes
        );
        update(&env).unwrap();
        let current = env.begin_read();
        let bytes = env.residency_status().live_locked_bytes;
        assert_eq!(bytes, env.residency_report().locked_bytes);
        assert!(bytes > first.locked_bytes);
        drop(old);
        assert_eq!(env.residency_status().live_locked_bytes, bytes);
        assert_eq!(locked_kib() * 1024, bytes);
        drop(env);
        inspect(&current, true);
        assert_eq!(locked_kib() * 1024, bytes);
        drop(current);
        assert_eq!(locked_kib(), 0);
        let budget = footprint(&path);
        let env = Environment::open_with_residency(
            &path,
            ResidencyPolicy::RequiredLocked { max_bytes: budget },
        )
        .unwrap();
        let before = fs::read(path.join("CURRENT")).unwrap();
        assert!(matches!(env.compact(), Err(Error::LimitExceeded)));
        assert_eq!(fs::read(path.join("CURRENT")).unwrap(), before);
        assert_eq!(env.residency_status().reserved_bytes, budget);
        assert_eq!(locked_kib() * 1024, budget);
        env.begin_write().unwrap().abort();
        drop(env);
        assert_eq!(locked_kib(), 0);
    });
}
fn locked_retained_budget() {
    each_media(|parent, mode| {
        let d = Directory::new(parent);
        let path = d.child("db");
        let env = Environment::create_with_publication(&path, mode).unwrap();
        populate(&env, 8192);
        let old_bytes = footprint(&path);
        env.retain_snapshot("old", &env.begin_read()).unwrap();
        update(&env).unwrap();
        drop(env);
        let active_bytes = footprint(&path);
        let env = Environment::open_with_residency(
            &path,
            ResidencyPolicy::RequiredLocked {
                max_bytes: active_bytes + old_bytes - 1,
            },
        )
        .unwrap();
        assert!(matches!(
            env.open_retained("old"),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(env.residency_status().reserved_bytes, active_bytes);
        assert_eq!(locked_kib() * 1024, active_bytes);
        inspect(&env.begin_read(), true);
        drop(env);
        assert_eq!(locked_kib(), 0);
    });
}
fn rlimit_failure() {
    assert_eq!(locked_kib(), 0);
    each_media(|parent, mode| {
        let d = Directory::new(parent);
        let path = d.child("db");
        let env = Environment::create_with_publication(&path, mode).unwrap();
        populate(&env, 8192);
        drop(env);
        let pointer = fs::read(path.join("CURRENT")).unwrap();
        assert!(matches!(
            Environment::open_with_residency(
                &path,
                ResidencyPolicy::RequiredLocked {
                    max_bytes: 1_048_576
                }
            ),
            Err(Error::Io(_))
        ));
        assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
        assert_eq!(locked_kib(), 0);
        let env = Environment::open_with_residency(
            &path,
            ResidencyPolicy::Prefault {
                max_bytes: 1_048_576,
            },
        )
        .unwrap();
        inspect(&env.begin_read(), false);
        let read = env.begin_read();
        let cursor = Cursor::from_read(&read).unwrap();
        let key = PublisherKey::new(9, cursor.uuid, cursor.epoch, [0x61; 32]).unwrap();
        let transfer = frame(&env, &read, None, &key, &d.0);
        let target = d.child("lock-rejected-bootstrap");
        assert!(matches!(
            replication::bootstrap_with_residency(
                &transfer,
                &target,
                ResidencyPolicy::RequiredLocked {
                    max_bytes: 1_048_576
                }
            ),
            Err(RError::Engine(Error::Io(_)))
        ));
        assert!(!target.join("CURRENT").exists());
        assert_eq!(locked_kib(), 0);
    });
}
fn locked_append_native_limit_failure() {
    each_media(|parent, mode| {
        assert_eq!(locked_kib(), 0);
        let d = Directory::new(parent);
        let path = d.child("db");
        let env = Environment::create_with_residency(
            &path,
            mode,
            ResidencyPolicy::RequiredLocked {
                max_bytes: 1_048_576,
            },
        )
        .unwrap();
        populate(&env, 8192);
        let held = env.begin_read();
        let pointer = fs::read(path.join("CURRENT")).unwrap();
        let before = env.residency_status();
        assert!(before.live_locked_bytes > 0);
        let limits = fs::read_to_string("/proc/self/limits").unwrap();
        let soft: u64 = limits
            .lines()
            .find(|line| line.starts_with("Max locked memory"))
            .unwrap()
            .split_whitespace()
            .nth(3)
            .unwrap()
            .parse()
            .unwrap();
        let change_soft = |bytes: u64| {
            // Only this already-isolated worker's limit changes; hard ceiling
            // stays unchanged, allowing restoration without CAP_IPC_LOCK.
            assert!(
                Command::new("prlimit")
                    .args([
                        "--pid",
                        &std::process::id().to_string(),
                        &format!("--memlock={bytes}:")
                    ])
                    .status()
                    .unwrap()
                    .success()
            );
        };
        change_soft(0);
        let failed = update(&env);
        change_soft(soft);
        assert!(
            matches!(failed, Err(Error::Io(ref error))
            if matches!(error.raw_os_error(), Some(1 | 11 | 12))),
            "expected Linux EPERM/EAGAIN/ENOMEM from new-map mlock, got {failed:?}"
        );
        assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
        assert_eq!(env.residency_status().reserved_bytes, before.reserved_bytes);
        assert_eq!(
            env.residency_status().live_locked_bytes,
            before.live_locked_bytes
        );
        assert_eq!(env.residency_status().retained_failed_bytes, 0);
        assert_eq!(locked_kib() * 1024, before.live_locked_bytes);
        inspect(&held, false);
        inspect(&env.begin_read(), false);
        update(&env).unwrap();
        inspect(&env.begin_read(), true);
        inspect(&held, false);
        assert!(env.residency_report().locked_bytes > before.live_locked_bytes);
        assert_eq!(
            locked_kib() * 1024,
            env.residency_status().live_locked_bytes
        );
        drop(held);
        drop(env);
        assert_eq!(locked_kib(), 0);
    });
}
fn frame(
    env: &Environment,
    read: &ulmdb::ReadTxn,
    base: Option<&ulmdb::ReadTxn>,
    key: &PublisherKey,
    parent: &Path,
) -> VerifiedTransfer {
    let mut bytes = Vec::new();
    replication::send_snapshot(env, read, base, key, &mut bytes).unwrap();
    replication::receive(
        &mut Input::new(bytes),
        key,
        TransferLimits::default(),
        parent,
    )
    .unwrap()
}
fn replica_admission() {
    each_media(|parent, mode| {
        let d = Directory::new(parent);
        let path = d.child("primary");
        let env = Environment::create_with_publication(&path, mode).unwrap();
        populate(&env, 8192);
        let first = env.begin_read();
        let c = Cursor::from_read(&first).unwrap();
        let key = PublisherKey::new(7, c.uuid, c.epoch, [0x31; 32]).unwrap();
        // Force copied destination mappings rather than staged hard-link
        // aliases. A one-snapshot lock budget must still admit bootstrap.
        let stage_parent = if parent == Path::new("/dev/shm") {
            std::env::temp_dir()
        } else {
            PathBuf::from("/dev/shm")
        };
        let stage = Directory::new(&stage_parent);
        let full = frame(&env, &first, None, &key, &stage.0);
        let rejected = d.child("rejected");
        assert!(matches!(
            replication::bootstrap_with_residency(
                &full,
                &rejected,
                ResidencyPolicy::Prefault { max_bytes: 0 }
            ),
            Err(RError::Engine(Error::LimitExceeded))
        ));
        assert!(!rejected.join("CURRENT").exists());
        let replica_path = d.child("replica");
        let budget = footprint(&path);
        let replica = replication::bootstrap_with_residency(
            &full,
            &replica_path,
            ResidencyPolicy::Prefault { max_bytes: budget },
        )
        .unwrap();
        let held = replica.begin_read();
        let pointer = fs::read(replica_path.join("CURRENT")).unwrap();
        update(&env).unwrap();
        let second = env.begin_read();
        let delta = frame(&env, &second, Some(&first), &key, &stage.0);
        assert!(matches!(
            replica.apply_replication(&delta),
            Err(RError::Engine(Error::LimitExceeded))
        ));
        assert_eq!(fs::read(replica_path.join("CURRENT")).unwrap(), pointer);
        let locked_path = d.child("locked-replica");
        let locked = replication::bootstrap_with_residency(
            &full,
            &locked_path,
            ResidencyPolicy::RequiredLocked { max_bytes: budget },
        )
        .unwrap();
        assert_eq!(locked.residency_status().reserved_bytes, budget);
        assert_eq!(locked_kib() * 1024, budget);
        let locked_pointer = fs::read(locked_path.join("CURRENT")).unwrap();
        assert!(matches!(
            locked.apply_replication(&delta),
            Err(RError::Engine(Error::LimitExceeded))
        ));
        assert_eq!(
            fs::read(locked_path.join("CURRENT")).unwrap(),
            locked_pointer
        );
        assert_eq!(locked.residency_status().reserved_bytes, budget);
        inspect(&locked.begin_read(), false);
        assert_eq!(
            locked.apply_replication(&full).unwrap().outcome,
            ApplyOutcome::AlreadyCurrent
        );
        drop(locked);
        assert_eq!(locked_kib(), 0);
        inspect(&held, false);
        inspect(&replica.begin_read(), false);
        assert_eq!(
            replica.apply_replication(&full).unwrap().outcome,
            ApplyOutcome::AlreadyCurrent
        );
        drop(held);
        drop(replica);
        let replica = Environment::open_with_residency(
            &replica_path,
            ResidencyPolicy::Prefault {
                max_bytes: 1_048_576,
            },
        )
        .unwrap();
        assert!(replica.is_replica());
        assert_eq!(
            replica.apply_replication(&delta).unwrap().outcome,
            ApplyOutcome::Activated
        );
        inspect(&replica.begin_read(), true);
        assert!(matches!(replica.begin_write(), Err(Error::ReadOnly)));
    });
}
fn cli_policies() {
    each_media(|parent, mode| {
        let d = Directory::new(parent);
        let path = d.child("primary");
        let env = Environment::create_with_publication(&path, mode).unwrap();
        populate(&env, 8192);
        drop(env);
        let executable = env!("CARGO_BIN_EXE_ulmdb-replicate");
        let key = d.child("publisher.key");
        let frame = d.child("full.frame");
        let run = |arguments: &[&std::ffi::OsStr], success: bool| {
            let output = Command::new(executable).args(arguments).output().unwrap();
            assert_eq!(
                output.status.success(),
                success,
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            output
        };
        run(
            &[
                "keygen".as_ref(),
                path.as_os_str(),
                key.as_os_str(),
                "7".as_ref(),
            ],
            true,
        );
        run(
            &[
                "send".as_ref(),
                path.as_os_str(),
                key.as_os_str(),
                frame.as_os_str(),
            ],
            true,
        );
        for policy in ["prefault:0", "locked:0"] {
            let rejected = d.child(&policy.replace(':', "-"));
            run(
                &[
                    "--residency".as_ref(),
                    policy.as_ref(),
                    "bootstrap".as_ref(),
                    frame.as_os_str(),
                    key.as_os_str(),
                    d.0.as_os_str(),
                    rejected.as_os_str(),
                ],
                false,
            );
            assert!(!rejected.join("CURRENT").exists());
        }
        let target = d.child("locked-cli-replica");
        run(
            &[
                "--residency".as_ref(),
                "locked:1048576".as_ref(),
                "bootstrap".as_ref(),
                frame.as_os_str(),
                key.as_os_str(),
                d.0.as_os_str(),
                target.as_os_str(),
            ],
            true,
        );
        let replica = Environment::open(&target).unwrap();
        assert!(replica.is_replica());
        inspect(&replica.begin_read(), false);
        drop(replica);
        let replay = run(
            &[
                "--residency".as_ref(),
                "prefault:1048576".as_ref(),
                "apply".as_ref(),
                target.as_os_str(),
                frame.as_os_str(),
                key.as_os_str(),
                d.0.as_os_str(),
            ],
            true,
        );
        assert!(
            String::from_utf8(replay.stdout)
                .unwrap()
                .contains("AlreadyCurrent")
        );
    });
}
#[test]
fn residency_worker() {
    let Ok(case) = std::env::var("ULMDB_RESIDENCY_CASE") else {
        return;
    };
    let status = fs::read_to_string("/proc/self/status").unwrap();
    let caps = u64::from_str_radix(
        status
            .lines()
            .find_map(|l| l.strip_prefix("CapEff:\t"))
            .unwrap(),
        16,
    )
    .unwrap();
    assert_eq!(caps & (1 << 14), 0, "worker retains CAP_IPC_LOCK");
    match case.as_str() {
        "prefault" => prefault_lifecycle(),
        "budget" => prefault_budget_and_retention(),
        "corruption" => retained_fresh_corruption_check(),
        "lifetime" => locked_lifetime_and_overlap(),
        "retained" => locked_retained_budget(),
        "rlimit" => rlimit_failure(),
        "append-rlimit" => locked_append_native_limit_failure(),
        "replica" => replica_admission(),
        "cli" => cli_policies(),
        _ => panic!("unknown case"),
    }
}
fn child(case: &str, limit_kib: u64) {
    let status = fs::read_to_string("/proc/self/status").unwrap();
    let caps = u64::from_str_radix(
        status
            .lines()
            .find_map(|l| l.strip_prefix("CapEff:\t"))
            .unwrap(),
        16,
    )
    .unwrap();
    let mut command = if caps & (1 << 14) != 0 {
        let mut c = Command::new("setpriv");
        c.args(["--bounding-set=-ipc_lock", "--", "/bin/sh"]);
        c
    } else {
        Command::new("/bin/sh")
    };
    let output = command
        .args([
            "-c",
            "ulimit -l \"$1\" || exit 99; shift; exec \"$@\"",
            "ulmdb-residency",
        ])
        .arg(limit_kib.to_string())
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", "residency_worker", "--nocapture"])
        .env("ULMDB_RESIDENCY_CASE", case)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{case}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn prefault_and_demand_disk_ram_all_formats() {
    child("prefault", 1024);
}
#[test]
fn budget_failures_preserve_current_and_retained_open() {
    child("budget", 1024);
}
#[test]
fn retained_open_revalidates_physical_files() {
    child("corruption", 1024);
}
#[test]
fn required_lock_mapping_lifetimes_and_compaction_overlap() {
    child("lifetime", 1024);
}
#[test]
fn required_retained_open_enforces_overlap_budget() {
    child("retained", 1024);
}
#[test]
fn real_rlimit_failure_is_confined_to_subprocess() {
    child("rlimit", 0);
}
#[test]
fn replicas_admit_final_maps_before_publication() {
    child("replica", 1024);
}

#[test]
fn replication_cli_residency_flags_reject_before_publication() {
    child("cli", 1024);
}

#[test]
fn required_append_real_rlimit_error_keeps_current_readers_and_retry_usable() {
    child("append-rlimit", 1024);
}
