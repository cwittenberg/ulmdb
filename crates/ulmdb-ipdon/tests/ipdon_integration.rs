use std::{collections::BTreeMap, path::PathBuf};
use ulmdb::{Environment, ReadTxn};
use ulmdb_ipdon::{
    journal, profiles,
    regional::{Command, Regional},
    rrsets, *,
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ipdon-addon-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn seed(path: &std::path::Path) -> Environment {
    let env = Environment::create(path).unwrap();
    let mut write = env.begin_write().unwrap();
    Regional::initialize_in_realm(&mut write, 7, 100).unwrap();
    let set = rrsets::Rrset {
        key: rrsets::Key {
            origin: 1,
            source: SourceId(8),
            scope: ScopeId(1),
            resolver: ResolverId(1),
            vantage: VantageId(1),
            role: Role::Direct,
            hostname: "example.test".into(),
            kind: rrsets::RecordType::A,
        },
        revision: 1,
        addresses: vec![Address::V4(0xc0000201)],
        dnssec: Dnssec::Secure,
    };
    let freshness = rrsets::Freshness {
        observed_at: 10,
        recorded_at: 11,
        expires_at: 1000,
        evidence: EvidenceId([9; 32]),
    };
    let mut events = vec![
        Command::ReplaceRrset { set, freshness }
            .event(1, 1, 11)
            .unwrap(),
    ];
    for (sequence, value) in [(1, b"NL"), (2, b"US")] {
        let claim = profiles::Claim {
            id: profiles::ClaimId::from_origin_source(2, sequence, value),
            generation: 1,
            field: profiles::FieldId(1),
            prefix: Prefix::new(Address::V4(0xc0000200), 24).unwrap(),
            source: sequence,
            value: value.to_vec(),
            priority: 100,
            observed_at: 10,
            recorded_at: 11,
            expires_at: 1000,
            evidence: [sequence as u8; 32],
        };
        events.push(Command::PutClaim(claim).event(2, sequence, 11).unwrap());
    }
    Regional::apply_page(&mut write, &events, journal::Limits::default()).unwrap();
    write.commit().unwrap();
    env
}
fn baseline(read: &ReadTxn) -> journal::Baseline {
    let mut origins = journal::cursors(read)
        .unwrap()
        .into_iter()
        .map(|(id, c)| (id, c.applied))
        .collect::<BTreeMap<_, _>>();
    origins.insert(3, 0);
    journal::Baseline {
        realm: 200,
        application_digest: Regional::application_digest(read).unwrap(),
        versions: journal::Versions {
            schema: 1,
            policy: 7,
        },
        imported_at: 100,
        origins,
    }
}

#[test]
fn typed_baseline_from_pruned_source_preserves_conflicts_and_new_verified_tail() {
    let dir = Directory::new();
    let source = seed(&dir.0.join("source"));
    let snapshot =
        journal::export_snapshot(&source, &source.begin_read(), dir.0.join("old-checkpoint"))
            .unwrap();
    let mut write = source.begin_write().unwrap();
    for (origin, through) in [(1, 1), (2, 2)] {
        journal::prune(
            &mut write,
            &snapshot,
            origin,
            through,
            journal::Limits::default(),
            &[journal::PeerAck {
                peer: 10,
                origin,
                sequence: through,
            }],
            1,
        )
        .unwrap();
    }
    write.commit().unwrap();
    let read = source.begin_read();
    let manifest = baseline(&read);
    let (a, receipt) =
        Regional::create_from_baseline(&source, &read, dir.0.join("a"), &manifest).unwrap();
    let (b, _) =
        Regional::create_from_baseline(&source, &read, dir.0.join("b"), &manifest).unwrap();
    assert_eq!(journal::realm(&source.begin_read()).unwrap(), 100);
    assert_eq!(journal::realm(&a.begin_read()).unwrap(), 200);
    assert_eq!(
        receipt.id(),
        journal::baseline_identity(&b.begin_read())
            .unwrap()
            .unwrap()
    );
    assert_eq!(
        Regional::application_digest(&a.begin_read()).unwrap(),
        manifest.application_digest
    );
    let query = profiles::FieldId(1);
    let address = Address::V4(0xc0000201);
    let r = a.begin_read();
    let view = Regional::view(&r).unwrap();
    let candidates = view
        .profiles
        .covering_candidates(
            address,
            query,
            20,
            None,
            profiles::CandidateLimits {
                max_results: 10,
                max_scan: 10,
            },
        )
        .unwrap();
    assert!(candidates.complete);
    assert_eq!(candidates.candidates.len(), 2);
    assert_eq!(
        candidates
            .candidates
            .iter()
            .map(|c| c.value)
            .collect::<Vec<_>>(),
        vec![&b"NL"[..], &b"US"[..]]
    );
    assert!(matches!(
        journal::checkpoint(&r, 2, 1),
        Err(journal::ProtocolError::SnapshotRequired { floor: 2, .. })
    ));
    assert!(journal::validate_full_handover(&source, &a).is_err());
    drop(Regional::validate_baseline_handover(&source, &a).unwrap());
    let mut write = a.begin_write().unwrap();
    let event = Command::WithdrawRrset {
        id: rrsets::Key {
            origin: 1,
            source: SourceId(8),
            scope: ScopeId(1),
            resolver: ResolverId(1),
            vantage: VantageId(1),
            role: Role::Direct,
            hostname: "example.test".into(),
            kind: rrsets::RecordType::A,
        }
        .id()
        .unwrap(),
        revision: 1,
    }
    .event(1, 2, 120)
    .unwrap();
    Regional::apply_page(&mut write, &[event], journal::Limits::default()).unwrap();
    write.commit().unwrap();
    let remote = a.begin_read();
    let page = journal::verified_page(&remote, 1, 1, journal::Limits::default()).unwrap();
    let mut write = b.begin_write().unwrap();
    let report =
        Regional::apply_verified_page(&mut write, &page, journal::Limits::default()).unwrap();
    assert_eq!(report.work.removed_members, 1);
    write.commit().unwrap();
    assert_eq!(
        Regional::application_digest(&a.begin_read()).unwrap(),
        Regional::application_digest(&b.begin_read()).unwrap()
    );
    // A new-realm candidate must activate at the baseline, before live tails.
    assert!(Regional::validate_baseline_handover(&source, &a).is_err());
    let bundle = dir.0.join("new-checkpoint");
    let receipt = journal::export_snapshot(&a, &a.begin_read(), &bundle).unwrap();
    assert_eq!(receipt.baseline(), Some(&manifest));
    assert_eq!(receipt.realm(), 200);
    assert_eq!(
        receipt.checkpoints().find(|c| c.origin == 1).unwrap(),
        journal::checkpoint(&a.begin_read(), 1, 2).unwrap()
    );
    let restored = journal::restore_snapshot(&bundle, dir.0.join("restored")).unwrap();
    assert_eq!(
        journal::baseline_provenance(&restored.begin_read()).unwrap(),
        Some(manifest)
    );
}

#[test]
fn baseline_source_verifier_rejects_digest_vector_versions_and_same_realm_before_copy() {
    let dir = Directory::new();
    let source = seed(&dir.0.join("source"));
    let read = source.begin_read();
    let expected = baseline(&read);
    for (name, mut invalid) in [
        ("digest", expected.clone()),
        ("vector", expected.clone()),
        ("versions", expected.clone()),
        ("realm", expected.clone()),
    ] {
        match name {
            "digest" => invalid.application_digest[0] ^= 1,
            "vector" => {
                invalid.origins.insert(2, 1);
            }
            "versions" => invalid.versions.policy = 8,
            "realm" => invalid.realm = 100,
            _ => unreachable!(),
        };
        let path = dir.0.join(name);
        assert!(Regional::create_from_baseline(&source, &read, &path, &invalid).is_err());
        assert!(!path.exists());
    }
    assert_eq!(journal::realm(&source.begin_read()).unwrap(), 100);
}

#[test]
fn baseline_handover_checks_late_source_and_unjournaled_candidate_mutation() {
    let dir = Directory::new();
    let source = seed(&dir.0.join("source"));
    let read = source.begin_read();
    let manifest = baseline(&read);
    let (candidate, _) =
        Regional::create_from_baseline(&source, &read, dir.0.join("candidate"), &manifest).unwrap();
    // Direct application mutation has no origin cursor change; actual table
    // digest verification must still reject an incomplete/modified candidate.
    let mut write = candidate.begin_write().unwrap();
    write
        .put(b"ipdon.rrsets.meta", b"unexpected", b"mutation")
        .unwrap();
    write.commit().unwrap();
    assert!(Regional::validate_baseline_handover(&source, &candidate).is_err());
    let (clean, _) =
        Regional::create_from_baseline(&source, &read, dir.0.join("clean"), &manifest).unwrap();
    let claim = profiles::Claim {
        id: profiles::ClaimId::from_origin_source(3, 9, b"late"),
        generation: 1,
        field: profiles::FieldId(2),
        prefix: Prefix::new(Address::V4(0), 0).unwrap(),
        source: 9,
        value: b"AS64500".to_vec(),
        priority: 100,
        observed_at: 20,
        recorded_at: 21,
        expires_at: 1000,
        evidence: [8; 32],
    };
    let mut write = source.begin_write().unwrap();
    Regional::apply_page(
        &mut write,
        &[Command::PutClaim(claim).event(3, 1, 21).unwrap()],
        journal::Limits::default(),
    )
    .unwrap();
    write.commit().unwrap();
    assert!(Regional::validate_baseline_handover(&source, &clean).is_err());
    assert_eq!(journal::realm(&source.begin_read()).unwrap(), 100);
}

#[test]
fn snapshots_keep_original_format_and_bind_new_baseline_provenance() {
    let dir = Directory::new();
    let source = seed(&dir.0.join("source"));
    let original = dir.0.join("original-snapshot");
    let old = journal::export_snapshot(&source, &source.begin_read(), &original).unwrap();
    assert!(old.baseline().is_none());
    assert_eq!(
        &std::fs::read(original.join("SNAPSHOT")).unwrap()[..8],
        b"IPDJNL01"
    );
    journal::verify_snapshot(&original).unwrap();
    let read = source.begin_read();
    let manifest = baseline(&read);
    let (candidate, _) =
        Regional::create_from_baseline(&source, &read, dir.0.join("new-realm"), &manifest).unwrap();
    let bundle = dir.0.join("baseline-snapshot");
    journal::export_snapshot(&candidate, &candidate.begin_read(), &bundle).unwrap();
    assert_eq!(
        &std::fs::read(bundle.join("SNAPSHOT")).unwrap()[..8],
        b"IPDJNL02"
    );
    assert_eq!(
        journal::verify_snapshot(&bundle).unwrap().baseline(),
        Some(&manifest)
    );
    // Baseline provenance is historical. A separately managed generic schema
    // migration may advance current versions without rewriting its old anchor.
    let mut write = candidate.begin_write().unwrap();
    journal::set_versions(
        &mut write,
        journal::Versions {
            schema: 2,
            policy: 8,
        },
    )
    .unwrap();
    write.commit().unwrap();
    let migrated = dir.0.join("migrated-snapshot");
    journal::export_snapshot(&candidate, &candidate.begin_read(), &migrated).unwrap();
    let checked = journal::verify_snapshot(&migrated).unwrap();
    assert_eq!(
        checked.versions(),
        journal::Versions {
            schema: 2,
            policy: 8
        }
    );
    assert_eq!(checked.baseline(), Some(&manifest));
}
