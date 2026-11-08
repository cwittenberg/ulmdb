//! Independent cutover checks: journal positions alone cannot prove unchanged
//! application state, and validation must hold both writers through activation.
use std::{collections::BTreeMap, fs, path::PathBuf};
use ulmdb::Environment;
use ulmdb_ipdon::{
    Address, Dnssec, EvidenceId, ResolverId, Role, ScopeId, SourceId, VantageId,
    journal::{self, Baseline, Limits, ProtocolError, Versions},
    regional::{Command, Regional},
    rrsets::{self, Rrsets},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ipdon-baseline-handover-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn source(&self) -> Environment {
        let environment = Environment::create(self.0.join("source")).unwrap();
        let mut write = environment.begin_write().unwrap();
        Regional::initialize_in_realm(&mut write, 3, 4).unwrap();
        let command = Command::ReplaceRrset {
            set: rrsets::Rrset {
                key: rrsets::Key {
                    origin: 1,
                    source: SourceId(7),
                    scope: ScopeId(1),
                    resolver: ResolverId(2),
                    vantage: VantageId(3),
                    role: Role::Direct,
                    hostname: "cutover.example".into(),
                    kind: rrsets::RecordType::A,
                },
                revision: 1,
                addresses: vec![Address::V4(0xc000_0201)],
                dnssec: Dnssec::Secure,
            },
            freshness: rrsets::Freshness {
                observed_at: 100,
                recorded_at: 101,
                expires_at: 200,
                evidence: EvidenceId::from_bytes(b"cutover evidence"),
            },
        };
        Regional::apply_page(
            &mut write,
            &[command.event(1, 1, 101).unwrap()],
            Limits::default(),
        )
        .unwrap();
        write.commit().unwrap();
        environment
    }
    fn candidate(&self, source: &Environment, name: &str) -> Environment {
        let read = source.begin_read();
        let baseline = Baseline {
            realm: 9,
            application_digest: Regional::application_digest(&read).unwrap(),
            versions: Versions {
                schema: 1,
                policy: 3,
            },
            imported_at: 500,
            origins: BTreeMap::from([(1, 1), (2, 0)]),
        };
        Regional::create_from_baseline(source, &read, self.0.join(name), &baseline)
            .unwrap()
            .0
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn expire_without_new_journal_event(environment: &Environment) {
    let read = environment.begin_read();
    let plan = Regional::view(&read)
        .unwrap()
        .rrsets
        .plan_expiry(200, 1)
        .unwrap();
    let mut write = environment.begin_write().unwrap();
    assert_eq!(Rrsets::expire(&mut write, &plan).unwrap().removed, 1);
    write.commit().unwrap();
    assert_eq!(
        journal::cursor(&environment.begin_read(), 1)
            .unwrap()
            .applied,
        1
    );
}

#[test]
fn cutover_checks_actual_source_application_even_without_journal_progress() {
    let fixture = Fixture::new();
    let source = fixture.source();
    let candidate = fixture.candidate(&source, "candidate");
    drop(Regional::validate_baseline_handover(&source, &candidate).unwrap());
    expire_without_new_journal_event(&source);
    assert!(matches!(
        Regional::validate_baseline_handover(&source, &candidate),
        Err(ProtocolError::BaselineMismatch(_))
    ));
    assert_eq!(journal::realm(&source.begin_read()).unwrap(), 4);
    assert_eq!(journal::realm(&candidate.begin_read()).unwrap(), 9);
}

#[test]
fn cutover_checks_actual_candidate_application_even_without_journal_progress() {
    let fixture = Fixture::new();
    let source = fixture.source();
    let candidate = fixture.candidate(&source, "candidate");
    expire_without_new_journal_event(&candidate);
    assert!(matches!(
        Regional::validate_baseline_handover(&source, &candidate),
        Err(ProtocolError::BaselineMismatch(_))
    ));
    let read = source.begin_read();
    let page = Regional::view(&read)
        .unwrap()
        .rrsets
        .query(
            &rrsets::Query::Hostname {
                scope: ScopeId(1),
                role: Role::Direct,
                hostname: "cutover.example".into(),
            },
            150,
            None,
            rrsets::Limits::default(),
        )
        .unwrap();
    assert_eq!(
        page.relations.len(),
        1,
        "candidate mutation leaves active source intact"
    );
}

#[test]
fn baseline_guard_keeps_both_writers_fenced_while_reads_continue() {
    use std::{sync::mpsc, time::Duration};
    let fixture = Fixture::new();
    let source = fixture.source();
    let candidate = fixture.candidate(&source, "candidate");
    let guard = Regional::validate_baseline_handover(&source, &candidate).unwrap();
    assert_eq!(guard.local_commit(), source.begin_read().commit_identity());
    assert_eq!(
        guard.candidate_commit(),
        candidate.begin_read().commit_identity()
    );
    std::thread::scope(|scope| {
        let (sent, received) = mpsc::channel();
        for environment in [&source, &candidate] {
            let sent = sent.clone();
            scope.spawn(move || {
                let write = environment.begin_write().unwrap();
                sent.send(()).unwrap();
                write.abort();
            });
        }
        assert!(received.recv_timeout(Duration::from_millis(25)).is_err());
        assert_eq!(
            Regional::view(&source.begin_read()).unwrap().watermarks[&1].applied,
            1
        );
        assert_eq!(
            Regional::view(&candidate.begin_read()).unwrap().watermarks[&1].floor,
            1
        );
        drop(guard);
        received.recv_timeout(Duration::from_secs(2)).unwrap();
        received.recv_timeout(Duration::from_secs(2)).unwrap();
    });
}
