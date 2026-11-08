use std::{fs, path::PathBuf};
use ulmdb::{DatabaseKind, Environment, Error};
use ulmdb_ipdon::journal::{self, ApplyMode, Event, Limits, PeerAck, Versions};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "ulmdb-ipdon-journal-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn environment(&self, name: &str) -> Environment {
        let e = Environment::create(self.path(name)).unwrap();
        let mut w = e.begin_write().unwrap();
        journal::initialize(&mut w).unwrap();
        w.create_database(b"app", DatabaseKind::Kv).unwrap();
        w.commit().unwrap();
        e
    }
    fn environment_in_realm(&self, name: &str, realm: u128) -> Environment {
        let e = Environment::create(self.path(name)).unwrap();
        let mut w = e.begin_write().unwrap();
        journal::initialize_with_realm(&mut w, realm).unwrap();
        w.create_database(b"app", DatabaseKind::Kv).unwrap();
        w.commit().unwrap();
        e
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn event(origin: u64, sequence: u64) -> Event {
    Event {
        origin,
        sequence,
        recorded_at: 100 + sequence,
        payload: format!("value-{origin}-{sequence}").into_bytes(),
    }
}
fn apply(e: &Environment, events: &[Event]) {
    let mut w = e.begin_write().unwrap();
    journal::apply_page(
        &mut w,
        events,
        Limits::default(),
        ApplyMode::Synchronous,
        |w, e| w.put(b"app", &e.origin.to_be_bytes(), &e.payload),
    )
    .unwrap();
    w.commit().unwrap();
}

#[test]
fn per_origin_order_replay_conflicts_and_reopen() {
    let f = Fixture::new();
    let e = f.environment("db");
    apply(&e, &[event(1, 1), event(2, 1), event(1, 2)]);
    let original_commit = e.begin_read().commit_identity();
    let mut w = e.begin_write().unwrap();
    let report = journal::apply_page(
        &mut w,
        &[event(1, 1), event(1, 2)],
        Limits::default(),
        ApplyMode::Synchronous,
        |_, _| panic!("replay callback"),
    )
    .unwrap();
    assert_eq!(report.replayed, 2);
    assert_eq!(report.applied, 0);
    let mut conflict = event(1, 2);
    conflict.recorded_at += 1;
    assert!(
        journal::apply_page(
            &mut w,
            &[conflict],
            Limits::default(),
            ApplyMode::Synchronous,
            |_, _| Ok(())
        )
        .is_err()
    );
    assert!(
        journal::apply_page(
            &mut w,
            &[event(2, 3)],
            Limits::default(),
            ApplyMode::Synchronous,
            |_, _| Ok(())
        )
        .is_err()
    );
    w.commit().unwrap();
    assert_eq!(
        e.begin_read().commit_identity(),
        original_commit,
        "exact replays and rejected pages publish no new native commit"
    );
    drop(e);
    let e = Environment::open(f.path("db")).unwrap();
    let read = e.begin_read();
    assert_eq!(journal::cursor(&read, 1).unwrap().applied, 2);
    assert_eq!(journal::cursor(&read, 2).unwrap().applied, 1);
    assert_eq!(journal::cursors(&read).unwrap().len(), 2);
}

#[test]
fn failed_page_rolls_back_every_callback_and_cursor_but_keeps_parent_work() {
    let f = Fixture::new();
    let e = f.environment("db");
    let mut w = e.begin_write().unwrap();
    w.put(b"app", b"parent", b"keep").unwrap();
    let result = journal::apply_page(
        &mut w,
        &[event(1, 1), event(1, 2)],
        Limits::default(),
        ApplyMode::Synchronous,
        |w, e| {
            w.put(b"app", &e.sequence.to_be_bytes(), &e.payload)?;
            if e.sequence == 2 {
                return Err(Error::InvalidInput("injected callback failure"));
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    w.commit().unwrap();
    let r = e.begin_read();
    assert_eq!(r.get(b"app", b"parent").unwrap(), Some(&b"keep"[..]));
    assert_eq!(r.get(b"app", &1u64.to_be_bytes()).unwrap(), None);
    assert_eq!(r.get(b"app", &2u64.to_be_bytes()).unwrap(), None);
    assert_eq!(journal::cursor(&r, 1).unwrap().applied, 0);
}

#[test]
fn evidence_and_serving_watermarks_advance_independently() {
    let f = Fixture::new();
    let e = f.environment("db");
    let mut w = e.begin_write().unwrap();
    journal::apply_page(
        &mut w,
        &[event(1, 1), event(1, 2)],
        Limits::default(),
        ApplyMode::EvidenceOnly,
        |_, _| Ok(()),
    )
    .unwrap();
    w.commit().unwrap();
    let c = journal::cursor(&e.begin_read(), 1).unwrap();
    assert_eq!((c.applied, c.serving), (2, 0));
    let mut w = e.begin_write().unwrap();
    assert!(
        journal::apply_page(
            &mut w,
            &[event(1, 3)],
            Limits::default(),
            ApplyMode::Synchronous,
            |_, _| Ok(())
        )
        .is_err()
    );
    assert!(journal::mark_serving(&mut w, 1, 3).is_err());
    journal::mark_serving(&mut w, 1, 2).unwrap();
    assert!(journal::mark_serving(&mut w, 1, 1).is_err());
    journal::apply_page(
        &mut w,
        &[event(1, 3)],
        Limits::default(),
        ApplyMode::Synchronous,
        |_, _| Ok(()),
    )
    .unwrap();
    w.commit().unwrap();
    let c = journal::cursor(&e.begin_read(), 1).unwrap();
    assert_eq!((c.applied, c.serving), (3, 3));
}

#[test]
fn count_and_encoded_byte_limits_bound_input_and_output() {
    let f = Fixture::new();
    let e = f.environment("db");
    let mut w = e.begin_write().unwrap();
    let limits = Limits {
        max_events: 1,
        max_bytes: 1024,
    };
    assert!(
        journal::apply_page(
            &mut w,
            &[event(1, 1), event(1, 2)],
            limits,
            ApplyMode::Synchronous,
            |_, _| panic!("over-limit callback")
        )
        .is_err()
    );
    assert!(
        journal::apply_page(
            &mut w,
            &[event(1, 1)],
            Limits {
                max_events: 1,
                max_bytes: 93
            },
            ApplyMode::Synchronous,
            |_, _| panic!("over-limit callback")
        )
        .is_err()
    );
    w.abort();
    apply(&e, &[event(1, 1), event(1, 2), event(1, 3)]);
    let r = e.begin_read();
    let p = journal::page(&r, 1, 0, limits).unwrap();
    assert_eq!(p.events.len(), 1);
    assert!(p.more);
    assert_eq!(p.encoded_bytes, p.events[0].payload.len() + 93);
    let p = journal::page(
        &r,
        1,
        1,
        Limits {
            max_events: 100,
            max_bytes: p.encoded_bytes,
        },
    )
    .unwrap();
    assert_eq!(p.events.len(), 1);
    assert!(p.more);
    assert!(
        journal::page(
            &r,
            1,
            0,
            Limits {
                max_events: 1,
                max_bytes: 93
            }
        )
        .is_err()
    );
    assert!(journal::page(&r, 1, 4, limits).is_err());
    assert!(journal::page(&r, 0, 0, limits).is_err());
}

#[test]
fn pinned_export_has_one_commit_vector_and_validated_versions() {
    let f = Fixture::new();
    let e = f.environment("db");
    apply(&e, &[event(1, 1)]);
    let old = e.begin_read();
    apply(&e, &[event(1, 2), event(2, 1)]);
    let receipt = journal::export_snapshot(&e, &old, f.path("snapshot")).unwrap();
    assert_eq!(receipt.cursors()[&1].applied, 1);
    assert!(!receipt.cursors().contains_key(&2));
    assert_eq!(receipt.native().commit, old.commit_identity());
    assert_eq!(
        journal::verify_snapshot(f.path("snapshot")).unwrap(),
        receipt
    );
    let restored = journal::restore_snapshot(f.path("snapshot"), f.path("restored")).unwrap();
    let r = restored.begin_read();
    assert_eq!(journal::cursor(&r, 1).unwrap().applied, 1);
    assert_eq!(journal::cursor(&r, 2).unwrap().applied, 0);
    let mut w = e.begin_write().unwrap();
    journal::set_versions(
        &mut w,
        Versions {
            schema: 2,
            policy: 7,
        },
    )
    .unwrap();
    w.commit().unwrap();
    assert_eq!(
        journal::versions(&e.begin_read()).unwrap(),
        Versions {
            schema: 2,
            policy: 7
        }
    );
}

#[test]
fn snapshot_manifest_tampering_or_truncation_is_rejected() {
    let f = Fixture::new();
    let e = f.environment("db");
    apply(&e, &[event(1, 1)]);
    journal::export_snapshot(&e, &e.begin_read(), f.path("snapshot")).unwrap();
    let p = f.path("snapshot/SNAPSHOT");
    let bytes = fs::read(&p).unwrap();
    let mut corrupt = bytes.clone();
    corrupt[30] ^= 1;
    fs::write(&p, &corrupt).unwrap();
    assert!(journal::verify_snapshot(f.path("snapshot")).is_err());
    fs::write(&p, &bytes[..bytes.len() - 1]).unwrap();
    assert!(journal::restore_snapshot(f.path("snapshot"), f.path("failed")).is_err());
    assert!(!f.path("failed").exists());
}

#[test]
fn prune_requires_recovery_lineage_distinct_durable_peers_and_bounded_pages() {
    let f = Fixture::new();
    let e = f.environment("db");
    apply(&e, &[event(1, 1), event(1, 2), event(1, 3)]);
    let receipt = journal::export_snapshot(&e, &e.begin_read(), f.path("snapshot")).unwrap();
    let other = f.environment("other");
    apply(&other, &[event(1, 1)]);
    let foreign = journal::export_snapshot(&other, &other.begin_read(), f.path("foreign")).unwrap();
    let a = PeerAck {
        peer: 9,
        origin: 1,
        sequence: 3,
    };
    let b = PeerAck { peer: 10, ..a };
    let mut w = e.begin_write().unwrap();
    assert!(journal::prune(&mut w, &foreign, 1, 1, Limits::default(), &[a], 1).is_err());
    assert!(journal::prune(&mut w, &receipt, 1, 3, Limits::default(), &[a, a], 2).is_err());
    let report = journal::prune(
        &mut w,
        &receipt,
        1,
        3,
        Limits {
            max_events: 1,
            max_bytes: 1024,
        },
        &[a, b],
        2,
    )
    .unwrap();
    assert_eq!((report.removed, report.floor), (1, 1));
    w.commit().unwrap();
    let r = e.begin_read();
    assert!(journal::page(&r, 1, 0, Limits::default()).is_err());
    assert_eq!(
        journal::page(&r, 1, 1, Limits::default())
            .unwrap()
            .events
            .len(),
        2
    );
    let mut w = e.begin_write().unwrap();
    assert!(
        journal::apply_page(
            &mut w,
            &[event(1, 1)],
            Limits::default(),
            ApplyMode::Synchronous,
            |_, _| Ok(())
        )
        .is_err()
    );
    let report = journal::prune(&mut w, &receipt, 1, 3, Limits::default(), &[a, b], 2).unwrap();
    assert_eq!((report.removed, report.floor), (2, 3));
    w.commit().unwrap();
    assert!(journal::page(&e.begin_read(), 1, 0, Limits::default()).is_err());
    assert!(
        !journal::page(&e.begin_read(), 1, 3, Limits::default())
            .unwrap()
            .more
    );
    apply(&e, &[event(1, 4)]);
    assert_eq!(
        journal::page(&e.begin_read(), 1, 3, Limits::default())
            .unwrap()
            .events[0]
            .sequence,
        4
    );
}

#[test]
fn prune_cannot_pass_unmaterialized_or_newer_than_checkpoint_events() {
    let f = Fixture::new();
    let e = f.environment("db");
    apply(&e, &[event(1, 1)]);
    let receipt = journal::export_snapshot(&e, &e.begin_read(), f.path("snapshot")).unwrap();
    let mut w = e.begin_write().unwrap();
    journal::apply_page(
        &mut w,
        &[event(1, 2)],
        Limits::default(),
        ApplyMode::EvidenceOnly,
        |_, _| Ok(()),
    )
    .unwrap();
    let ack = PeerAck {
        peer: 9,
        origin: 1,
        sequence: 2,
    };
    assert!(journal::prune(&mut w, &receipt, 1, 2, Limits::default(), &[ack], 1).is_err());
    w.commit().unwrap();
    let receipt = journal::export_snapshot(&e, &e.begin_read(), f.path("new-snapshot")).unwrap();
    let mut w = e.begin_write().unwrap();
    assert!(journal::prune(&mut w, &receipt, 1, 2, Limits::default(), &[ack], 1).is_err());
    w.abort();
}

#[test]
fn configured_realms_are_stable_and_foreign_empty_snapshots_cannot_rebase() {
    let f = Fixture::new();
    let local = f.environment_in_realm("local", 77);
    let foreign = f.environment_in_realm("foreign", 88);
    apply(&local, &[event(7, 1)]);
    let mut w = local.begin_write().unwrap();
    journal::initialize_with_realm(&mut w, 77).unwrap();
    assert!(journal::initialize_with_realm(&mut w, 88).is_err());
    w.commit().unwrap();
    assert_eq!(journal::realm(&local.begin_read()).unwrap(), 77);
    journal::export_snapshot(&foreign, &foreign.begin_read(), f.path("foreign-snapshot")).unwrap();
    assert!(
        journal::rebase_snapshot(
            f.path("foreign-snapshot"),
            &local,
            7,
            f.path("rejected"),
            Limits::default(),
            |_, _| Ok(())
        )
        .is_err()
    );
    assert!(!f.path("rejected").exists());
}

#[test]
fn same_realm_divergent_prefix_cannot_pass_equal_last_event_anchor_or_prune() {
    let f = Fixture::new();
    let local = f.environment_in_realm("local", 77);
    let fork = f.environment_in_realm("fork", 77);
    apply(&local, &[event(7, 1), event(7, 2)]);
    let mut wrong = event(7, 1);
    wrong.payload = b"different earlier history".to_vec();
    apply(&fork, &[wrong, event(7, 2)]);
    let receipt =
        journal::export_snapshot(&fork, &fork.begin_read(), f.path("fork-snapshot")).unwrap();
    assert!(
        journal::rebase_snapshot(
            f.path("fork-snapshot"),
            &local,
            7,
            f.path("rejected"),
            Limits::default(),
            |_, _| Ok(())
        )
        .is_err()
    );
    assert!(!f.path("rejected").exists());
    let mut w = local.begin_write().unwrap();
    assert!(journal::prune(&mut w, &receipt, 7, 1, Limits::default(), &[], 0).is_err());
    w.abort();
}

#[test]
fn full_rebase_preserves_all_local_tails_and_handover_detects_late_commits() {
    let f = Fixture::new();
    let local = f.environment("local");
    apply(&local, &[event(1, 1), event(2, 1)]);
    journal::export_snapshot(&local, &local.begin_read(), f.path("base")).unwrap();
    let peer = journal::restore_snapshot(f.path("base"), f.path("peer")).unwrap();
    apply(&local, &[event(1, 2), event(2, 2), event(3, 1)]);
    apply(&peer, &[event(4, 1)]);
    journal::export_snapshot(&peer, &peer.begin_read(), f.path("peer-snapshot")).unwrap();
    let candidate = journal::rebase_all_snapshot(
        f.path("peer-snapshot"),
        &local,
        f.path("candidate"),
        Limits {
            max_events: 1,
            max_bytes: 1024,
        },
        |w, e| w.put(b"app", &e.origin.to_be_bytes(), &e.payload),
    )
    .unwrap();
    let read = candidate.begin_read();
    for (origin, applied) in [(1, 2), (2, 2), (3, 1), (4, 1)] {
        assert_eq!(journal::cursor(&read, origin).unwrap().applied, applied);
    }
    let guard = journal::validate_full_handover(&local, &candidate).unwrap();
    assert_eq!(guard.local_commit(), local.begin_read().commit_identity());
    assert_eq!(
        guard.candidate_commit(),
        candidate.begin_read().commit_identity()
    );
    drop(guard);
    apply(&local, &[event(2, 3)]);
    assert!(journal::validate_full_handover(&local, &candidate).is_err());
    drop(journal::validate_handover(&local, &candidate, 1).unwrap());
    apply(&local, &[event(1, 3)]);
    assert!(journal::validate_handover(&local, &candidate, 1).is_err());
}

#[test]
fn handover_guard_fences_source_and_candidate_writers_until_activation_finishes() {
    use std::{sync::mpsc, time::Duration};
    let f = Fixture::new();
    let source = f.environment("source");
    apply(&source, &[event(1, 1)]);
    journal::export_snapshot(&source, &source.begin_read(), f.path("snapshot")).unwrap();
    let candidate = journal::restore_snapshot(f.path("snapshot"), f.path("candidate")).unwrap();
    let guard = journal::validate_full_handover(&source, &candidate).unwrap();
    std::thread::scope(|scope| {
        let (sent, received) = mpsc::channel();
        for environment in [&source, &candidate] {
            let sent = sent.clone();
            scope.spawn(move || {
                let w = environment.begin_write().unwrap();
                sent.send(()).unwrap();
                w.abort();
            });
        }
        assert!(received.recv_timeout(Duration::from_millis(25)).is_err());
        drop(guard);
        received.recv_timeout(Duration::from_secs(2)).unwrap();
        received.recv_timeout(Duration::from_secs(2)).unwrap();
    });
}
