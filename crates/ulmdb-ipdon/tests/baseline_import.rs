use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::PathBuf};
use ulmdb::{DatabaseKind, Environment, Error, ReadTxn, Result, WriteTxn};
use ulmdb_ipdon::journal::{
    self, ApplyMode, Baseline, BaselineActivation, Event, Limits, ProtocolError, Versions,
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ipdon-baseline-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const VALUE: &[u8] = b"verified application baseline";
fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn manifest() -> Baseline {
    Baseline {
        realm: 0x4253454c494e45,
        application_digest: digest(VALUE),
        versions: Versions {
            schema: 1,
            policy: 7,
        },
        imported_at: 1_800_000_000,
        origins: BTreeMap::from([(1, 10), (2, 5), (3, 0)]),
    }
}
fn install(txn: &mut WriteTxn<'_>, baseline: &Baseline) -> Result<BaselineActivation> {
    txn.create_database(b"app", DatabaseKind::Kv)?;
    txn.put(b"app", b"baseline", VALUE)?;
    let actual = txn
        .get(b"app", b"baseline")?
        .ok_or(Error::Corrupt("incomplete baseline application"))?;
    Ok(BaselineActivation {
        application_digest: digest(actual),
        versions: baseline.versions,
        origins: baseline.origins.clone(),
    })
}
fn candidate(f: &Fixture, name: &str, baseline: &Baseline) -> Environment {
    journal::create_from_baseline(f.path(name), baseline, |txn| install(txn, baseline))
        .unwrap()
        .0
}
fn next(origin: u64, sequence: u64) -> Event {
    Event {
        origin,
        sequence,
        recorded_at: 1_800_000_100 + sequence,
        payload: format!("tail-{origin}-{sequence}").into_bytes(),
    }
}
fn apply(env: &Environment, events: &[Event]) {
    let mut txn = env.begin_write().unwrap();
    journal::apply_page(
        &mut txn,
        events,
        Limits::default(),
        ApplyMode::Synchronous,
        |w, e| w.put(b"app", &e.origin.to_be_bytes(), &e.payload),
    )
    .unwrap();
    txn.commit().unwrap();
}
fn app(read: &ReadTxn, key: &[u8]) -> Option<Vec<u8>> {
    read.get(b"app", key).unwrap().map(<[u8]>::to_vec)
}

#[test]
fn identical_baselines_produce_deterministic_anchors_and_allocate_above_imported_positions() {
    let f = Fixture::new();
    let b = manifest();
    let a = candidate(&f, "a", &b);
    let c = candidate(&f, "c", &b);
    let ar = a.begin_read();
    let cr = c.begin_read();
    assert_eq!(journal::baseline_provenance(&ar).unwrap(), Some(b.clone()));
    assert_eq!(
        journal::baseline_identity(&ar).unwrap(),
        Some(b.identity().unwrap())
    );
    for (origin, sequence) in [(1, 10), (2, 5), (3, 0)] {
        let x = journal::checkpoint(&ar, origin, sequence).unwrap();
        assert_eq!(x, journal::checkpoint(&cr, origin, sequence).unwrap());
        if sequence > 0 {
            assert_ne!(x.digest, [0; 32]);
            assert_eq!(journal::cursor(&ar, origin).unwrap().floor, sequence);
        }
    }
    assert_eq!(journal::cursor(&ar, 3).unwrap().applied, 0);
    apply(&a, &[next(1, 11), next(2, 6), next(3, 1)]);
    let read = a.begin_read();
    for (origin, seq) in [(1, 11), (2, 6), (3, 1)] {
        assert_eq!(journal::cursor(&read, origin).unwrap().applied, seq);
    }
    let old = journal::checkpoint(&read, 1, 9).unwrap_err();
    assert!(matches!(
        old,
        ProtocolError::SnapshotRequired {
            origin: 1,
            requested: 9,
            floor: 10
        }
    ));
    assert!(journal::page(&read, 1, 9, Limits::default()).is_err());
    assert_eq!(
        journal::page(&read, 1, 10, Limits::default())
            .unwrap()
            .events,
        vec![next(1, 11)]
    );
    drop(read);
    drop(ar);
    drop(a);
    let reopened = Environment::open(f.path("a")).unwrap();
    assert_eq!(
        journal::baseline_provenance(&reopened.begin_read()).unwrap(),
        Some(b)
    );
}

#[test]
fn existing_realm_even_empty_cannot_be_rewritten_and_parent_work_survives_failed_activation() {
    let f = Fixture::new();
    let b = manifest();
    let env = Environment::create(f.path("db")).unwrap();
    let mut txn = env.begin_write().unwrap();
    txn.create_database(b"parent", DatabaseKind::Kv).unwrap();
    txn.put(b"parent", b"keep", b"yes").unwrap();
    let error = journal::import_baseline(&mut txn, &b, |txn| {
        let mut activation = install(txn, &b)?;
        activation.application_digest = digest(b"incomplete");
        Ok(activation)
    })
    .unwrap_err();
    assert!(matches!(error, ProtocolError::BaselineMismatch(_)));
    assert!(matches!(
        txn.get(b"app", b"baseline"),
        Err(Error::MissingDatabase)
    ));
    assert!(matches!(
        txn.get(b"ipdon.journal.meta.v1", b"format"),
        Err(Error::MissingDatabase)
    ));
    assert_eq!(txn.get(b"parent", b"keep").unwrap(), Some(&b"yes"[..]));
    journal::import_baseline(&mut txn, &b, |txn| install(txn, &b)).unwrap();
    txn.commit().unwrap();
    let commit = env.begin_read().commit_identity();
    let mut txn = env.begin_write().unwrap();
    assert!(matches!(
        journal::import_baseline(&mut txn, &b, |_| panic!("existing realm callback")),
        Err(ProtocolError::BaselineMismatch(_))
    ));
    txn.commit().unwrap();
    assert_eq!(env.begin_read().commit_identity(), commit);
    let empty = Environment::create(f.path("empty")).unwrap();
    let mut txn = empty.begin_write().unwrap();
    journal::initialize_with_realm(&mut txn, b.realm).unwrap();
    assert!(
        journal::import_baseline(&mut txn, &b, |_| panic!("empty existing realm callback"))
            .is_err()
    );
}

#[test]
fn mismatch_of_versions_vector_or_callback_history_rolls_back_all_application_state() {
    let f = Fixture::new();
    let b = manifest();
    for mode in 0..4 {
        let env = Environment::create(f.path(&format!("mode-{mode}"))).unwrap();
        let mut txn = env.begin_write().unwrap();
        assert!(
            journal::import_baseline(&mut txn, &b, |txn| {
                let mut activated = install(txn, &b)?;
                match mode {
                    0 => activated.versions.policy += 1,
                    1 => {
                        activated.origins.remove(&3);
                    }
                    2 => {
                        journal::set_versions(
                            txn,
                            Versions {
                                schema: 2,
                                policy: 7,
                            },
                        )?;
                    }
                    _ => {
                        journal::apply_page(
                            txn,
                            &[next(1, 1)],
                            Limits::default(),
                            ApplyMode::Synchronous,
                            |_, _| Ok(()),
                        )?;
                    }
                }
                Ok(activated)
            })
            .is_err()
        );
        assert!(matches!(
            txn.get(b"app", b"baseline"),
            Err(Error::MissingDatabase)
        ));
        txn.commit().unwrap();
        assert_eq!(env.stats().databases, 0);
    }
}

#[test]
fn staged_import_failure_preserves_active_environment_and_different_baselines_cannot_merge() {
    let f = Fixture::new();
    let b = manifest();
    let active = candidate(&f, "active", &b);
    apply(&active, &[next(1, 11)]);
    let before = active.begin_read().commit_identity();
    assert!(
        journal::create_from_baseline(f.path("failed"), &b, |txn| {
            let _ = install(txn, &b)?;
            Err(Error::InvalidInput("parse truncated application baseline"))
        })
        .is_err()
    );
    assert_eq!(active.begin_read().commit_identity(), before);
    let mut other = b.clone();
    other.application_digest = digest(b"different");
    let alternative = journal::create_from_baseline(f.path("alternative"), &other, |txn| {
        txn.create_database(b"app", DatabaseKind::Kv)?;
        txn.put(b"app", b"baseline", b"different")?;
        Ok(BaselineActivation {
            application_digest: digest(txn.get(b"app", b"baseline")?.unwrap()),
            versions: other.versions,
            origins: other.origins.clone(),
        })
    })
    .unwrap()
    .0;
    assert_ne!(
        journal::checkpoint(&active.begin_read(), 1, 10).unwrap(),
        journal::checkpoint(&alternative.begin_read(), 1, 10).unwrap()
    );
    assert!(journal::validate_full_handover(&active, &alternative).is_err());
    let page = journal::verified_page(&active.begin_read(), 1, 10, Limits::default()).unwrap();
    let mut txn = alternative.begin_write().unwrap();
    assert!(matches!(
        journal::apply_verified_page(
            &mut txn,
            &page,
            Limits::default(),
            ApplyMode::Synchronous,
            |_, _| panic!("foreign baseline callback")
        ),
        Err(ProtocolError::BaselineMismatch(_))
    ));
    drop(txn);
    // Previously unseen origins share the all-zero genesis digest. They must
    // still be separated by the complete baseline identity, not only by a
    // per-origin common-prefix check.
    apply(&active, &[next(4, 1)]);
    let new_origin_page =
        journal::verified_page(&active.begin_read(), 4, 0, Limits::default()).unwrap();
    assert_eq!(new_origin_page.base.digest, [0; 32]);
    let mut txn = alternative.begin_write().unwrap();
    assert!(matches!(
        journal::apply_verified_page(
            &mut txn,
            &new_origin_page,
            Limits::default(),
            ApplyMode::Synchronous,
            |_, _| panic!("unseen-origin foreign baseline callback")
        ),
        Err(ProtocolError::BaselineMismatch(_))
    ));
    drop(txn);
    let mut realm = b.clone();
    realm.realm += 1;
    let foreign = candidate(&f, "foreign-realm", &realm);
    let mut txn = foreign.begin_write().unwrap();
    assert!(matches!(
        journal::apply_verified_page(
            &mut txn,
            &page,
            Limits::default(),
            ApplyMode::Synchronous,
            |_, _| panic!("foreign realm callback")
        ),
        Err(ProtocolError::ForeignRealm { .. })
    ));
}

#[test]
fn post_baseline_tails_survive_snapshot_rebase_and_guarded_handover() {
    let f = Fixture::new();
    let b = manifest();
    let local = candidate(&f, "local", &b);
    let peer = candidate(&f, "peer", &b);
    apply(&local, &[next(1, 11), next(2, 6)]);
    apply(&peer, &[next(1, 11)]);
    let bundle = f.path("checkpoint");
    let receipt = journal::export_snapshot(&peer, &peer.begin_read(), &bundle).unwrap();
    assert_eq!(receipt.realm(), b.realm);
    assert_eq!(receipt.baseline(), Some(&b));
    let restored = journal::rebase_all_snapshot(
        &bundle,
        &local,
        f.path("recovered"),
        Limits::default(),
        |txn, event| txn.put(b"app", &event.origin.to_be_bytes(), &event.payload),
    )
    .unwrap();
    let guard = journal::validate_full_handover(&local, &restored).unwrap();
    let read = restored.begin_read();
    assert_eq!(
        journal::baseline_provenance(&read).unwrap(),
        Some(b.clone())
    );
    assert_eq!(journal::cursor(&read, 1).unwrap().applied, 11);
    assert_eq!(journal::cursor(&read, 2).unwrap().applied, 6);
    assert_eq!(app(&read, &2u64.to_be_bytes()), Some(next(2, 6).payload));
    drop(guard);
    apply(&restored, &[next(2, 7)]);
    assert_eq!(
        journal::cursor(&restored.begin_read(), 2).unwrap().applied,
        7
    );
    let mut different = b.clone();
    different.imported_at += 1;
    let other = candidate(&f, "different-time", &different);
    let other_bundle = f.path("different-bundle");
    journal::export_snapshot(&other, &other.begin_read(), &other_bundle).unwrap();
    assert!(
        journal::rebase_all_snapshot(
            &other_bundle,
            &local,
            f.path("rejected"),
            Limits::default(),
            |_, _| panic!("baseline conflict replay")
        )
        .is_err()
    );
}

#[test]
fn invalid_baseline_metadata_is_rejected_before_installation() {
    let f = Fixture::new();
    let b = manifest();
    for mode in 0..6 {
        let mut invalid = b.clone();
        match mode {
            0 => invalid.realm = 0,
            1 => invalid.application_digest = [0; 32],
            2 => invalid.imported_at = 0,
            3 => {
                invalid.origins.insert(0, 1);
            }
            4 => {
                invalid.origins.insert(1, u64::MAX);
            }
            _ => invalid.versions.policy = 0,
        }
        assert!(
            journal::create_from_baseline(f.path(&format!("bad-{mode}")), &invalid, |_| panic!(
                "invalid baseline callback"
            ))
            .is_err()
        );
        assert!(!f.path(&format!("bad-{mode}")).exists());
    }
}
