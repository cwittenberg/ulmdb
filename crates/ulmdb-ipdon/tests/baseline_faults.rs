#![cfg(feature = "fault-injection")]
//! Actual native publication cuts during new-realm baseline installation.
//! Process exits preserve kernel caches; these do not establish power-loss safety.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command as Process, Stdio},
    thread,
    time::{Duration, Instant},
};
use ulmdb::{Environment, Error, PublicationMode, backup};
use ulmdb_ipdon::{
    Address, Dnssec, EvidenceId, Prefix, ResolverId, Role, ScopeId, SourceId, VantageId,
    journal::{self, Baseline, BaselineActivation, Limits, ProtocolError, Versions},
    profiles,
    regional::{Command, Regional},
    rrsets,
};

const OLD_REALM: u128 = 0x4f4c4442415345;
const NEW_REALM: u128 = 0x4e455742415345;
const IMPORTED_AT: u64 = 1_800_000_000;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ipdon-baseline-faults-{:032x}",
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
fn set() -> rrsets::Rrset {
    rrsets::Rrset {
        key: rrsets::Key {
            origin: 1,
            source: SourceId(9),
            scope: ScopeId(1),
            resolver: ResolverId(2),
            vantage: VantageId(3),
            role: Role::Direct,
            hostname: "baseline-fault.test".into(),
            kind: rrsets::RecordType::A,
        },
        revision: 1,
        addresses: vec![Address::V4(0xc000_0201)],
        dnssec: Dnssec::Secure,
    }
}
fn events() -> Vec<journal::Event> {
    vec![
        Command::ReplaceRrset {
            set: set(),
            freshness: rrsets::Freshness {
                observed_at: 10,
                recorded_at: 11,
                expires_at: 1000,
                evidence: EvidenceId::from_bytes(b"baseline DNS evidence"),
            },
        }
        .event(1, 1, 11)
        .unwrap(),
        Command::PutClaim(profiles::Claim {
            id: profiles::ClaimId::from_origin_source(2, 9, b"country"),
            generation: 1,
            field: profiles::FieldId(1),
            prefix: Prefix::new(Address::V4(0xc000_0200), 24).unwrap(),
            source: 9,
            value: b"NL".to_vec(),
            priority: 100,
            observed_at: 10,
            recorded_at: 11,
            expires_at: 1000,
            evidence: [9; 32],
        })
        .event(2, 1, 11)
        .unwrap(),
    ]
}
fn manifest(env: &Environment) -> Baseline {
    let read = env.begin_read();
    Baseline {
        realm: NEW_REALM,
        application_digest: Regional::application_digest(&read).unwrap(),
        versions: Versions {
            schema: 1,
            policy: 7,
        },
        imported_at: IMPORTED_AT,
        origins: BTreeMap::from([(1, 1), (2, 1), (3, 0)]),
    }
}
fn initialize(root: &Directory, mode: PublicationMode, prepare_copy: bool) {
    let source = Environment::create_with_publication(root.path("source"), mode).unwrap();
    let mut write = source.begin_write().unwrap();
    Regional::initialize_in_realm(&mut write, 7, OLD_REALM).unwrap();
    Regional::apply_page(&mut write, &events(), Limits::default()).unwrap();
    write.commit().unwrap();
    let checkpoint =
        journal::export_snapshot(&source, &source.begin_read(), root.path("old-checkpoint"))
            .unwrap();
    let mut write = source.begin_write().unwrap();
    for origin in [1, 2] {
        journal::prune(
            &mut write,
            &checkpoint,
            origin,
            1,
            Limits::default(),
            &[journal::PeerAck {
                peer: 77,
                origin,
                sequence: 1,
            }],
            1,
        )
        .unwrap();
    }
    write.commit().unwrap();
    for origin in [1, 2] {
        assert_eq!(
            journal::cursor(&source.begin_read(), origin).unwrap().floor,
            1
        );
        assert!(journal::page(&source.begin_read(), origin, 0, Limits::default()).is_err());
    }
    fs::write(
        root.path("source-commit"),
        source.begin_read().commit_identity().to_be_bytes(),
    )
    .unwrap();
    if prepare_copy {
        backup::export_snapshot(&source, &source.begin_read(), root.path("candidate")).unwrap();
    }
}
fn check_application(env: &Environment) {
    let read = env.begin_read();
    let view = Regional::view(&read).unwrap();
    let selected = view
        .profiles
        .lookup(Address::V4(0xc000_0201), profiles::FieldId(1), 50)
        .unwrap()
        .unwrap();
    assert_eq!(selected.value, b"NL");
    let query = rrsets::Query::Ip {
        scope: ScopeId(1),
        role: Role::Direct,
        address: Address::V4(0xc000_0201),
    };
    let page = view
        .rrsets
        .query(&query, 50, None, rrsets::Limits::default())
        .unwrap();
    assert_eq!(page.relations.len(), 1);
    assert_eq!(page.relations[0].hostname, "baseline-fault.test");
    for origin in [1, 2] {
        let c = journal::cursor(&read, origin).unwrap();
        assert_eq!((c.applied, c.serving, c.floor), (1, 1, 1));
    }
}
fn check_old(env: &Environment) {
    check_application(env);
    let read = env.begin_read();
    assert_eq!(journal::realm(&read).unwrap(), OLD_REALM);
    assert!(journal::baseline_provenance(&read).unwrap().is_none());
}
fn check_new(env: &Environment, baseline: &Baseline) {
    check_application(env);
    let read = env.begin_read();
    assert_eq!(journal::realm(&read).unwrap(), NEW_REALM);
    assert_eq!(
        journal::baseline_provenance(&read).unwrap().as_ref(),
        Some(baseline)
    );
    let receipt_id = baseline.identity().unwrap();
    assert_eq!(journal::baseline_identity(&read).unwrap(), Some(receipt_id));
    for origin in [1, 2] {
        assert_ne!(
            journal::checkpoint(&read, origin, 1).unwrap().digest,
            [0; 32]
        );
    }
}
fn open_worker(path: &Path) -> Environment {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match Environment::open(path) {
            Ok(env) => return env,
            Err(Error::Busy) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(1))
            }
            Err(error) => panic!("baseline worker open {}: {error:?}", path.display()),
        }
    }
}

#[test]
fn baseline_fault_worker() {
    let Some(action) = std::env::var_os("IPDON_BASELINE_FAULT_ACTION") else {
        return;
    };
    let source = open_worker(Path::new(
        &std::env::var_os("IPDON_BASELINE_FAULT_SOURCE").unwrap(),
    ));
    let target = PathBuf::from(std::env::var_os("IPDON_BASELINE_FAULT_TARGET").unwrap());
    let mode = std::env::var("IPDON_BASELINE_FAULT_MODE").unwrap();
    let point = std::env::var("IPDON_BASELINE_FAULT_POINT").unwrap();
    let baseline = manifest(&source);
    if action == "public" {
        let result =
            Regional::create_from_baseline(&source, &source.begin_read(), &target, &baseline);
        if mode == "io" {
            assert!(matches!(result, Err(ProtocolError::Storage(Error::Io(_)))));
        } else {
            let (env, _) = result.unwrap();
            check_new(&env, &baseline);
        }
        check_old(&source);
        source.begin_write().unwrap().abort();
        return;
    }
    assert_eq!(action, "finish");
    // Prepared outside faulted process so pointer hooks reach the final anchor
    // transaction rather than the preceding native-copy CURRENT publication.
    let candidate = open_worker(&target);
    let before = candidate.begin_read();
    let activated = BaselineActivation {
        application_digest: Regional::application_digest(&before).unwrap(),
        versions: journal::versions(&before).unwrap(),
        origins: baseline.origins.clone(),
    };
    assert_eq!(activated.application_digest, baseline.application_digest);
    let mut write = candidate.begin_write().unwrap();
    // Exactly the private reset_copied_journal operation in Regional's wrapper;
    // native publication below remains the real database commit implementation.
    for table in [
        b"ipdon.journal.meta.v1".as_slice(),
        b"ipdon.journal.events.v1",
        b"ipdon.journal.cursors.v1",
        b"ipdon.journal.heads.v1",
        b"ipdon.journal.floors.v1",
    ] {
        write.drop_database(table).unwrap();
    }
    journal::import_baseline(&mut write, &baseline, |_| Ok(activated)).unwrap();
    let result = write.commit();
    if mode == "io" {
        assert!(matches!(result, Err(Error::OutcomeUnknown)));
        assert!(matches!(
            candidate.begin_write(),
            Err(Error::OutcomeUnknown)
        ));
        assert_eq!(journal::realm(&before).unwrap(), OLD_REALM);
        check_old(&candidate);
    } else {
        result.unwrap();
        check_new(&candidate, &baseline);
        if point == "transaction.after_commit" {
            fs::write(
                std::env::var_os("IPDON_BASELINE_FAULT_ACK").unwrap(),
                b"new baseline committed",
            )
            .unwrap();
            std::process::exit(86);
        }
    }
    check_old(&source);
}

fn run(root: &Directory, action: &str, mode: &str, point: &str) {
    let trace = root.path("trace");
    let mut command = Process::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "baseline_fault_worker", "--nocapture"])
        .env("IPDON_BASELINE_FAULT_ACTION", action)
        .env("IPDON_BASELINE_FAULT_MODE", mode)
        .env("IPDON_BASELINE_FAULT_POINT", point)
        .env("IPDON_BASELINE_FAULT_SOURCE", root.path("source"))
        .env("IPDON_BASELINE_FAULT_TARGET", root.path("candidate"))
        .env("IPDON_BASELINE_FAULT_ACK", root.path("acknowledged"))
        .env("ULMDB_TRACE_FILE", &trace)
        .env_remove("ULMDB_CRASH_AT")
        .env_remove("ULMDB_FAIL_AT")
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
                "baseline child timeout: {} {}",
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
        assert_eq!(
            fs::read_to_string(trace).unwrap().lines().last(),
            Some(point)
        );
    }
}

fn verify_source(root: &Directory) -> Environment {
    let env = Environment::open(root.path("source")).unwrap();
    check_old(&env);
    let before = u128::from_be_bytes(
        fs::read(root.path("source-commit"))
            .unwrap()
            .try_into()
            .unwrap(),
    );
    assert_eq!(env.begin_read().commit_identity(), before);
    env
}
fn verify_candidate(root: &Directory, published: bool) {
    let source = verify_source(root);
    let baseline = manifest(&source);
    let candidate = Environment::open(root.path("candidate")).unwrap();
    if published {
        check_new(&candidate, &baseline);
        drop(Regional::validate_baseline_handover(&source, &candidate).unwrap());
        let event = Command::RefreshRrset {
            id: set().key.id().unwrap(),
            revision: 1,
            freshness: rrsets::Freshness {
                observed_at: 20,
                recorded_at: 21,
                expires_at: 2000,
                evidence: EvidenceId::from_bytes(b"postbaseline"),
            },
        }
        .event(1, 2, 21)
        .unwrap();
        let mut write = candidate.begin_write().unwrap();
        Regional::apply_page(&mut write, &[event], Limits::default()).unwrap();
        write.commit().unwrap();
        assert_eq!(
            journal::cursor(&candidate.begin_read(), 1).unwrap().applied,
            2
        );
    } else {
        check_old(&candidate);
        assert!(Regional::validate_baseline_handover(&source, &candidate).is_err());
    }
}

#[test]
fn public_baseline_copy_final_payload_faults_preserve_pruned_source_and_old_candidate() {
    for publication in modes() {
        for mode in ["crash", "io"] {
            for suffix in ["before_write", "after_file_sync"] {
                let root = Directory::new();
                initialize(&root, publication, false);
                let point = format!(
                    "{}.{suffix}",
                    if publication == PublicationMode::Legacy {
                        "segment"
                    } else {
                        "cohort"
                    }
                );
                run(&root, "public", mode, &point);
                verify_candidate(&root, false);
            }
        }
    }
}

#[test]
fn final_baseline_anchor_commit_is_atomic_at_native_current_boundaries() {
    for publication in modes() {
        for point in [
            "pointer.CURRENT.before_rename",
            "pointer.CURRENT.after_rename",
            "pointer.CURRENT.after_directory_sync",
            "transaction.after_commit",
        ] {
            let root = Directory::new();
            initialize(&root, publication, true);
            run(&root, "finish", "crash", point);
            if point == "transaction.after_commit" {
                assert!(root.path("acknowledged").exists());
            }
            verify_candidate(&root, point != "pointer.CURRENT.before_rename");
        }
    }
}

#[test]
fn returned_final_baseline_publication_errors_leave_old_live_reader_and_coherent_reopen() {
    for publication in modes() {
        for point in [
            "pointer.CURRENT.before_rename",
            "pointer.CURRENT.after_rename",
        ] {
            let root = Directory::new();
            initialize(&root, publication, true);
            run(&root, "finish", "io", point);
            verify_candidate(&root, point == "pointer.CURRENT.after_rename");
        }
    }
}
