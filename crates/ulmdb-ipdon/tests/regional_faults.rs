#![cfg(feature = "fault-injection")]
//! Native storage-fault qualification of typed regional transactions and recovery.
//! Child exits retain the kernel cache. These are not physical power-loss tests.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command as Process, Stdio},
    thread,
    time::{Duration, Instant},
};
use ulmdb::{Environment, Error, PublicationMode};
use ulmdb_ipdon::{
    Address, Dnssec, EvidenceId, Prefix, ResolverId, Role, ScopeId, SourceId, VantageId,
    journal::{self, Limits},
    profiles,
    regional::{Command, Regional},
    rrsets,
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ipdon-native-faults-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
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
fn payload_point(mode: PublicationMode, suffix: &str) -> String {
    format!(
        "{}.{suffix}",
        if mode == PublicationMode::Legacy {
            "segment"
        } else {
            "cohort"
        }
    )
}
fn address(revision: u64) -> Address {
    Address::V4(0xc000_0200 + revision as u32)
}
fn rrset(revision: u64) -> rrsets::Rrset {
    rrsets::Rrset {
        key: rrsets::Key {
            origin: 1,
            source: SourceId(77),
            scope: ScopeId(1),
            resolver: ResolverId(2),
            vantage: VantageId(3),
            role: Role::Direct,
            hostname: "fault.test".into(),
            kind: rrsets::RecordType::A,
        },
        revision,
        addresses: vec![address(revision)],
        dnssec: Dnssec::Secure,
    }
}
fn freshness(revision: u64) -> rrsets::Freshness {
    rrsets::Freshness {
        observed_at: 10 * revision,
        recorded_at: 10 * revision + 1,
        expires_at: 1000 * revision,
        evidence: EvidenceId::from_bytes(&revision.to_be_bytes()),
    }
}
fn claim(revision: u64) -> profiles::Claim {
    profiles::Claim {
        id: profiles::ClaimId::from_origin_source(2, 77, b"fault-country"),
        generation: revision,
        field: profiles::FieldId(1),
        prefix: Prefix::new(Address::V4(0xc000_0200), 24).unwrap(),
        source: 77,
        value: if revision == 1 { b"NL" } else { b"DE" }.to_vec(),
        priority: 100,
        observed_at: 10 * revision,
        recorded_at: 10 * revision + 1,
        expires_at: 1000 * revision,
        evidence: [revision as u8; 32],
    }
}
fn events(revision: u64) -> Vec<journal::Event> {
    vec![
        Command::ReplaceRrset {
            set: rrset(revision),
            freshness: freshness(revision),
        }
        .event(1, revision, 10 * revision + 1)
        .unwrap(),
        Command::PutClaim(claim(revision))
            .event(2, revision, 10 * revision + 1)
            .unwrap(),
    ]
}
fn apply(env: &Environment, revision: u64) {
    let mut write = env.begin_write().unwrap();
    Regional::apply_page(&mut write, &events(revision), Limits::default()).unwrap();
    write.commit().unwrap();
}
fn initialize(path: &Path, mode: PublicationMode) {
    let env = Environment::create_with_publication(path, mode).unwrap();
    let mut write = env.begin_write().unwrap();
    Regional::initialize_in_realm(&mut write, 7, 0x4641_554c_5453).unwrap();
    Regional::apply_page(&mut write, &events(1), Limits::default()).unwrap();
    assert_eq!(write.commit().unwrap(), 1);
}

fn check_projection(env: &Environment, revision: u64) {
    let read = env.begin_read();
    let view = Regional::view(&read).unwrap();
    for origin in [1, 2] {
        let cursor = view.watermarks[&origin];
        assert_eq!((cursor.applied, cursor.serving), (revision, revision));
        let page = journal::page(&read, origin, 0, Limits::default()).unwrap();
        assert_eq!(page.events.len() as u64, revision);
        assert_eq!(page.events.last().unwrap().sequence, revision);
    }
    let set = view
        .rrsets
        .rrset(rrset(1).key.id().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(set.0.revision, revision);
    assert_eq!(set.0.addresses, vec![address(revision)]);
    assert_eq!(set.1.freshness, freshness(revision));
    let query = rrsets::Query::Hostname {
        scope: ScopeId(1),
        role: Role::Direct,
        hostname: "fault.test".into(),
    };
    let rows = view
        .rrsets
        .query(&query, 50, None, rrsets::Limits::default())
        .unwrap();
    assert_eq!(rows.relations.len(), 1);
    assert_eq!(rows.relations[0].address, address(revision));
    for candidate in [1, 2] {
        let query = rrsets::Query::Ip {
            scope: ScopeId(1),
            role: Role::Direct,
            address: address(candidate),
        };
        let rows = view
            .rrsets
            .query(&query, 50, None, rrsets::Limits::default())
            .unwrap();
        assert_eq!(rows.relations.len(), usize::from(candidate == revision));
    }
    let selected = view
        .profiles
        .lookup(address(revision), profiles::FieldId(1), 50)
        .unwrap()
        .unwrap();
    assert_eq!(selected.value, claim(revision).value);
    assert_eq!(selected.generation, revision);
    assert_eq!(selected.recorded_at, 10 * revision + 1);
}

fn open_worker(path: &Path) -> Environment {
    // A concurrent test's fork can temporarily inherit a CLOEXEC lock before
    // exec. Retry only this bounded worker startup Busy condition.
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match Environment::open(path) {
            Ok(env) => return env,
            Err(Error::Busy) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(1))
            }
            Err(error) => panic!("fault worker open {}: {error:?}", path.display()),
        }
    }
}

#[test]
fn regional_fault_worker() {
    let Some(action) = std::env::var_os("IPDON_FAULT_ACTION") else {
        return;
    };
    let mode = std::env::var("IPDON_FAULT_MODE").unwrap();
    let env = open_worker(Path::new(
        &std::env::var_os("IPDON_FAULT_DATABASE").unwrap(),
    ));
    let point = std::env::var("IPDON_FAULT_POINT").unwrap();
    match action.to_str().unwrap() {
        "commit" => {
            let mut write = env.begin_write().unwrap();
            Regional::apply_page(&mut write, &events(2), Limits::default()).unwrap();
            let outcome = write.commit();
            if mode == "io" {
                let error = outcome.unwrap_err();
                if point.starts_with("segment.") || point.starts_with("cohort.") {
                    assert!(matches!(error, Error::Io(_)), "{point}: {error:?}");
                    env.begin_write().unwrap().abort();
                } else {
                    assert!(matches!(error, Error::OutcomeUnknown), "{point}: {error:?}");
                    assert!(matches!(env.begin_write(), Err(Error::OutcomeUnknown)));
                }
                check_projection(&env, 1);
            } else {
                assert_eq!(outcome.unwrap(), 2);
                check_projection(&env, 2);
                if point == "transaction.after_commit" {
                    fs::write(
                        std::env::var_os("IPDON_FAULT_ACK").unwrap(),
                        b"commit acknowledged",
                    )
                    .unwrap();
                    std::process::exit(86);
                }
            }
        }
        "snapshot" => {
            let target = std::env::var_os("IPDON_FAULT_TARGET").unwrap();
            let result = journal::export_snapshot(&env, &env.begin_read(), target);
            if mode == "io" {
                assert!(matches!(result, Err(Error::OutcomeUnknown)));
            } else {
                result.unwrap();
            }
            check_projection(&env, 1);
            env.begin_write().unwrap().abort();
        }
        "rebase" => {
            let source = std::env::var_os("IPDON_FAULT_BUNDLE").unwrap();
            let target = std::env::var_os("IPDON_FAULT_TARGET").unwrap();
            let result = journal::rebase_all_snapshot(
                source,
                &env,
                target,
                Limits::default(),
                Regional::apply_event,
            );
            if mode == "io" {
                let error = result.err().expect("rebase fault was not reached");
                if point.starts_with("segment.") || point.starts_with("cohort.") {
                    assert!(matches!(error, Error::Io(_)), "{point}: {error:?}");
                } else {
                    assert!(matches!(error, Error::OutcomeUnknown), "{point}: {error:?}");
                }
            } else {
                let candidate = result.unwrap();
                check_projection(&candidate, 2);
                drop(journal::validate_full_handover(&env, &candidate).unwrap());
                if point == "transaction.after_commit" {
                    std::process::exit(86);
                }
            }
            check_projection(&env, 2);
            env.begin_write().unwrap().abort();
        }
        _ => panic!("unknown fault action"),
    }
}

fn run(root: &Directory, action: &str, mode: &str, point: &str) {
    let trace = root.path("trace");
    let mut command = Process::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "regional_fault_worker", "--nocapture"])
        .env("IPDON_FAULT_ACTION", action)
        .env("IPDON_FAULT_MODE", mode)
        .env("IPDON_FAULT_POINT", point)
        .env("IPDON_FAULT_DATABASE", root.path("database"))
        .env("IPDON_FAULT_TARGET", root.path("candidate"))
        .env("IPDON_FAULT_BUNDLE", root.path("checkpoint"))
        .env("IPDON_FAULT_ACK", root.path("acknowledged"))
        .env("ULMDB_TRACE_FILE", &trace)
        .env_remove("ULMDB_FAIL_AT")
        .env_remove("ULMDB_CRASH_AT")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if mode == "crash" {
        command.env("ULMDB_CRASH_AT", point);
    }
    if mode == "io" {
        command.env("ULMDB_FAIL_AT", point);
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "fault worker timeout {action}/{mode}/{point}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(5));
    }
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(if mode == "crash" { 86 } else { 0 }),
        "{action}/{mode}/{point}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if mode != "normal" && point != "transaction.after_commit" {
        let trace = fs::read_to_string(trace).unwrap();
        assert_eq!(
            trace.lines().last(),
            Some(point),
            "unreached native fault {action}/{mode}/{point}"
        );
    }
}

fn check_reopened_and_continue(path: &Path, revision: u64) {
    let env = Environment::open(path).unwrap();
    check_projection(&env, revision);
    let old = env.begin_read();
    let old_freshness = Regional::view(&old)
        .unwrap()
        .rrsets
        .rrset(rrset(1).key.id().unwrap())
        .unwrap()
        .unwrap()
        .1
        .freshness;
    let command = Command::RefreshRrset {
        id: rrset(1).key.id().unwrap(),
        revision,
        freshness: freshness(3),
    };
    let mut write = env.begin_write().unwrap();
    Regional::apply_page(
        &mut write,
        &[command.event(1, revision + 1, 31).unwrap()],
        Limits::default(),
    )
    .unwrap();
    write.commit().unwrap();
    assert_eq!(
        journal::cursor(&env.begin_read(), 1).unwrap().applied,
        revision + 1
    );
    assert_eq!(
        Regional::view(&old)
            .unwrap()
            .rrsets
            .rrset(rrset(1).key.id().unwrap())
            .unwrap()
            .unwrap()
            .1
            .freshness,
        old_freshness
    );
}

#[test]
fn typed_transactions_reopen_coherently_at_native_crash_boundaries() {
    for publication in modes() {
        let points = [
            payload_point(publication, "before_write"),
            payload_point(publication, "after_file_sync"),
            "pointer.CURRENT.before_rename".into(),
            "pointer.CURRENT.after_rename".into(),
            "pointer.CURRENT.after_directory_sync".into(),
            "transaction.after_commit".into(),
        ];
        for point in points {
            let root = Directory::new();
            initialize(&root.path("database"), publication);
            run(&root, "commit", "crash", &point);
            let published = matches!(
                point.as_str(),
                "pointer.CURRENT.after_rename"
                    | "pointer.CURRENT.after_directory_sync"
                    | "transaction.after_commit"
            );
            if point == "transaction.after_commit" {
                assert!(root.path("acknowledged").exists());
            }
            check_reopened_and_continue(&root.path("database"), if published { 2 } else { 1 });
        }
    }
}

#[test]
fn returned_native_io_failures_keep_live_reader_old_and_reopen_one_complete_typed_page() {
    for publication in modes() {
        for point in [
            payload_point(publication, "before_write"),
            "pointer.CURRENT.before_rename".into(),
            "pointer.CURRENT.after_rename".into(),
        ] {
            let root = Directory::new();
            initialize(&root.path("database"), publication);
            run(&root, "commit", "io", &point);
            check_reopened_and_continue(
                &root.path("database"),
                if point == "pointer.CURRENT.after_rename" {
                    2
                } else {
                    1
                },
            );
        }
    }
}

#[test]
fn incomplete_native_snapshot_publication_has_no_verified_bundle_or_source_mutation() {
    for publication in modes() {
        for mode in ["crash", "io"] {
            for point in [
                "pointer.CURRENT.before_rename",
                "pointer.CURRENT.after_rename",
            ] {
                let root = Directory::new();
                initialize(&root.path("database"), publication);
                run(&root, "snapshot", mode, point);
                assert!(journal::verify_snapshot(root.path("candidate")).is_err());
                assert!(!root.path("candidate/SNAPSHOT").exists());
                check_reopened_and_continue(&root.path("database"), 1);
            }
        }
        let root = Directory::new();
        initialize(&root.path("database"), publication);
        run(&root, "snapshot", "normal", "");
        let receipt = journal::verify_snapshot(root.path("candidate")).unwrap();
        assert_eq!(receipt.cursors()[&1].applied, 1);
        assert_eq!(receipt.cursors()[&2].applied, 1);
        let restored =
            journal::restore_snapshot(root.path("candidate"), root.path("restored")).unwrap();
        check_projection(&restored, 1);
    }
}

#[test]
fn interrupted_rebase_copy_or_replay_never_authorizes_stale_candidate_handover() {
    for publication in modes() {
        for mode in ["crash", "io"] {
            for point in [
                payload_point(publication, "before_write"),
                payload_point(publication, "after_file_sync"),
                "pointer.CURRENT.before_rename".into(),
                "pointer.CURRENT.after_rename".into(),
            ] {
                let root = Directory::new();
                initialize(&root.path("database"), publication);
                let local = Environment::open(root.path("database")).unwrap();
                journal::export_snapshot(&local, &local.begin_read(), root.path("checkpoint"))
                    .unwrap();
                apply(&local, 2);
                drop(local);
                run(&root, "rebase", mode, &point);
                let local = Environment::open(root.path("database")).unwrap();
                check_projection(&local, 2);
                if root.path("candidate/CURRENT").exists() {
                    let candidate = Environment::open(root.path("candidate")).unwrap();
                    check_projection(&candidate, 1);
                    assert!(journal::validate_full_handover(&local, &candidate).is_err());
                } else {
                    assert!(Environment::open(root.path("candidate")).is_err());
                }
            }
        }
        let root = Directory::new();
        initialize(&root.path("database"), publication);
        let local = Environment::open(root.path("database")).unwrap();
        journal::export_snapshot(&local, &local.begin_read(), root.path("checkpoint")).unwrap();
        apply(&local, 2);
        drop(local);
        run(&root, "rebase", "crash", "transaction.after_commit");
        let local = Environment::open(root.path("database")).unwrap();
        let candidate = Environment::open(root.path("candidate")).unwrap();
        check_projection(&local, 2);
        check_projection(&candidate, 2);
        drop(journal::validate_full_handover(&local, &candidate).unwrap());
    }
}
