use std::{cell::Cell, fs, path::PathBuf};
use ulmdb::{DatabaseKind, Environment, Error};
use ulmdb_ipdon::journal::{self, ApplyMode, Event, Limits, PeerAck, ProtocolError, Versions};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ipdon-verified-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn environment(&self, name: &str, realm: u128) -> Environment {
        let env = Environment::create(self.0.join(name)).unwrap();
        let mut write = env.begin_write().unwrap();
        journal::initialize_with_realm(&mut write, realm).unwrap();
        write.create_database(b"app", DatabaseKind::Kv).unwrap();
        write.commit().unwrap();
        env
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn event(sequence: u64) -> Event {
    Event {
        origin: 7,
        sequence,
        recorded_at: 100 + sequence,
        payload: format!("value-{sequence}").into_bytes(),
    }
}
fn apply(env: &Environment, events: &[Event]) {
    let mut write = env.begin_write().unwrap();
    journal::apply_page(
        &mut write,
        events,
        Limits::default(),
        ApplyMode::Synchronous,
        |txn, e| txn.put(b"app", &e.origin.to_be_bytes(), &e.payload),
    )
    .unwrap();
    write.commit().unwrap();
}
fn apply_verified(env: &Environment, page: &journal::VerifiedPage) -> journal::ApplyReport {
    let mut write = env.begin_write().unwrap();
    let report = journal::apply_verified_page(
        &mut write,
        page,
        Limits::default(),
        ApplyMode::Synchronous,
        |txn, e| txn.put(b"app", &e.origin.to_be_bytes(), &e.payload),
    )
    .unwrap();
    write.commit().unwrap();
    report
}

#[test]
fn public_checkpoints_lengths_and_pages_come_from_one_pinned_snapshot() {
    let fixture = Fixture::new();
    let source = fixture.environment("source", 4);
    apply(&source, &[event(1), event(2), event(3)]);
    let read = source.begin_read();
    let genesis = journal::checkpoint(&read, 7, 0).unwrap();
    assert_eq!(genesis.digest, [0; 32]);
    let native = journal::page(&read, 7, 0, Limits::default()).unwrap();
    assert_eq!(
        native.encoded_bytes,
        native
            .events
            .iter()
            .map(|e| journal::encoded_len(e).unwrap())
            .sum()
    );
    let page = journal::verified_page(
        &read,
        7,
        0,
        Limits {
            max_events: 2,
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(page.base, genesis);
    assert_eq!(page.tail, journal::checkpoint(&read, 7, 2).unwrap());
    assert_eq!(page.cursor.applied, 3);
    assert!(page.more());
    assert_eq!(page.realm, 4);
    assert_eq!(page.versions, Versions::default());
    assert_eq!(page.baseline_id, None);
    apply(&source, &[event(4)]);
    let frozen = journal::verified_page(&read, 7, 2, Limits::default()).unwrap();
    assert_eq!(frozen.cursor.applied, 3);
    assert_eq!(frozen.events, vec![event(3)]);
    assert!(!frozen.more());
    let fresh = source.begin_read();
    assert_eq!(journal::checkpoint(&fresh, 7, 4).unwrap().sequence, 4);
    assert!(journal::checkpoint(&read, 7, 4).is_err());
}

#[test]
fn correctly_resumed_partial_and_exact_replay_preserve_callback_semantics() {
    let fixture = Fixture::new();
    let source = fixture.environment("source", 4);
    let target = fixture.environment("target", 4);
    apply(&source, &[event(1), event(2), event(3), event(4)]);
    apply(&target, &[event(1)]);
    let page = journal::verified_page(
        &source.begin_read(),
        7,
        0,
        Limits {
            max_events: 3,
            ..Limits::default()
        },
    )
    .unwrap();
    let report = apply_verified(&target, &page);
    assert_eq!(report.applied, 2);
    assert_eq!(report.replayed, 1);
    assert_eq!(
        target
            .begin_read()
            .get(b"app", &7u64.to_be_bytes())
            .unwrap(),
        Some(event(3).payload.as_slice())
    );
    let before = target.begin_read().commit_identity();
    let mut write = target.begin_write().unwrap();
    let report = journal::apply_verified_page(
        &mut write,
        &page,
        Limits::default(),
        ApplyMode::Synchronous,
        |_, _| panic!("exact replay callback"),
    )
    .unwrap();
    assert_eq!(report.applied, 0);
    assert_eq!(report.replayed, 3);
    assert_eq!(write.dirty_bytes(), 0);
    write.commit().unwrap();
    assert_eq!(target.begin_read().commit_identity(), before);
    let tail = journal::verified_page(&source.begin_read(), 7, 3, Limits::default()).unwrap();
    assert_eq!(apply_verified(&target, &tail).applied, 1);
    assert_eq!(
        apply_verified(&target, &page).replayed,
        3,
        "an older complete page can be replayed after a newer tail"
    );
    let path = fixture.0.join("target");
    drop(target);
    let target = Environment::open(path).unwrap();
    assert_eq!(
        journal::checkpoint(&target.begin_read(), 7, 4).unwrap(),
        journal::checkpoint(&source.begin_read(), 7, 4).unwrap()
    );
}

#[test]
fn equal_numeric_base_with_divergent_earlier_history_fails_before_callback() {
    let fixture = Fixture::new();
    let source = fixture.environment("source", 4);
    let target = fixture.environment("target", 4);
    let mut fork = event(1);
    fork.payload = b"different first event".to_vec();
    apply(&source, &[event(1), event(2), event(3)]);
    apply(&target, &[fork, event(2)]);
    let page = journal::verified_page(&source.begin_read(), 7, 2, Limits::default()).unwrap();
    let before = target.begin_read().commit_identity();
    let calls = Cell::new(0);
    let mut write = target.begin_write().unwrap();
    let error = journal::apply_verified_page(
        &mut write,
        &page,
        Limits::default(),
        ApplyMode::Synchronous,
        |_, _| {
            calls.set(calls.get() + 1);
            Ok(())
        },
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ProtocolError::Fork {
            origin: 7,
            sequence: 2
        }
    ));
    assert_eq!(calls.get(), 0);
    write.commit().unwrap();
    assert_eq!(target.begin_read().commit_identity(), before);
    assert_eq!(journal::cursor(&target.begin_read(), 7).unwrap().applied, 2);
}

#[test]
fn fork_inside_an_overlapping_page_is_detected_before_new_tail_callback() {
    let fixture = Fixture::new();
    let source = fixture.environment("source", 4);
    let target = fixture.environment("target", 4);
    apply(&source, &[event(1), event(2), event(3)]);
    let mut fork = event(2);
    fork.payload = b"divergent second event".to_vec();
    apply(&target, &[event(1), fork]);
    let page = journal::verified_page(&source.begin_read(), 7, 0, Limits::default()).unwrap();
    let mut write = target.begin_write().unwrap();
    let calls = Cell::new(0);
    assert!(matches!(
        journal::apply_verified_page(
            &mut write,
            &page,
            Limits::default(),
            ApplyMode::Synchronous,
            |_, _| {
                calls.set(calls.get() + 1);
                Ok(())
            }
        ),
        Err(ProtocolError::Fork {
            origin: 7,
            sequence: 2
        })
    ));
    assert_eq!(calls.get(), 0);
    assert_eq!(write.dirty_bytes(), 0);
}

#[test]
fn tampered_payload_time_digest_realm_and_metadata_fail_without_callback_changes() {
    let fixture = Fixture::new();
    let source = fixture.environment("source", 4);
    let target = fixture.environment("target", 4);
    apply(&source, &[event(1), event(2), event(3)]);
    apply(&target, &[event(1)]);
    let good = journal::verified_page(&source.begin_read(), 7, 1, Limits::default()).unwrap();
    let mut variants = Vec::new();
    let mut page = good.clone();
    page.events[1].payload[0] ^= 1;
    variants.push(page);
    let mut page = good.clone();
    page.events[1].recorded_at += 1;
    variants.push(page);
    let mut page = good.clone();
    page.tail.digest[0] ^= 1;
    variants.push(page);
    let mut page = good.clone();
    page.base.digest[0] ^= 1;
    variants.push(page);
    let mut page = good.clone();
    page.realm = 5;
    variants.push(page);
    let mut page = good.clone();
    page.versions.policy = 2;
    variants.push(page);
    let mut page = good.clone();
    page.versions.schema = 2;
    variants.push(page);
    let mut page = good.clone();
    page.encoded_bytes += 1;
    variants.push(page);
    let mut page = good.clone();
    page.events[0].origin = 8;
    variants.push(page);
    let mut page = good.clone();
    page.events[0].sequence = 3;
    variants.push(page);
    let mut page = good.clone();
    page.tail.sequence = 4;
    variants.push(page);
    let mut page = good.clone();
    page.cursor.serving = 4;
    variants.push(page);
    let mut page = good.clone();
    page.baseline_id = Some([8; 32]);
    variants.push(page);
    for page in variants {
        let calls = Cell::new(0);
        let mut write = target.begin_write().unwrap();
        write.put(b"app", b"parent", b"keep").unwrap();
        let before = write.dirty_bytes();
        assert!(
            journal::apply_verified_page(
                &mut write,
                &page,
                Limits::default(),
                ApplyMode::Synchronous,
                |txn, _| {
                    calls.set(calls.get() + 1);
                    txn.put(b"app", b"should-not", b"appear")
                }
            )
            .is_err()
        );
        assert_eq!(calls.get(), 0);
        assert_eq!(write.dirty_bytes(), before);
        write.commit().unwrap();
        let read = target.begin_read();
        assert_eq!(read.get(b"app", b"parent").unwrap(), Some(&b"keep"[..]));
        assert_eq!(read.get(b"app", b"should-not").unwrap(), None);
        assert_eq!(journal::cursor(&read, 7).unwrap().applied, 1);
    }
}

#[test]
fn typed_realm_version_and_limit_errors_precede_callbacks() {
    let fixture = Fixture::new();
    let source = fixture.environment("source", 4);
    let target = fixture.environment("target", 5);
    apply(&source, &[event(1), event(2)]);
    let page = journal::verified_page(&source.begin_read(), 7, 0, Limits::default()).unwrap();
    let mut write = target.begin_write().unwrap();
    assert!(matches!(
        journal::apply_verified_page(
            &mut write,
            &page,
            Limits::default(),
            ApplyMode::Synchronous,
            |_, _| panic!("realm callback")
        ),
        Err(ProtocolError::ForeignRealm {
            expected: 5,
            actual: 4
        })
    ));
    let target = fixture.environment("versioned", 4);
    let mut write = target.begin_write().unwrap();
    journal::set_versions(
        &mut write,
        Versions {
            schema: 2,
            policy: 3,
        },
    )
    .unwrap();
    write.commit().unwrap();
    let mut write = target.begin_write().unwrap();
    assert!(matches!(
        journal::apply_verified_page(
            &mut write,
            &page,
            Limits::default(),
            ApplyMode::Synchronous,
            |_, _| panic!("version callback")
        ),
        Err(ProtocolError::VersionMismatch {
            expected: Versions {
                schema: 2,
                policy: 3
            },
            actual: Versions {
                schema: 1,
                policy: 1
            }
        })
    ));
    let target = fixture.environment("bounded", 4);
    let mut write = target.begin_write().unwrap();
    for limits in [
        Limits {
            max_events: 1,
            ..Limits::default()
        },
        Limits {
            max_events: 2,
            max_bytes: journal::encoded_len(&event(1)).unwrap(),
        },
    ] {
        assert!(matches!(
            journal::apply_verified_page(
                &mut write,
                &page,
                limits,
                ApplyMode::Synchronous,
                |_, _| panic!("limit callback")
            ),
            Err(ProtocolError::Storage(Error::LimitExceeded))
        ));
    }
}

#[test]
fn callback_failure_restores_all_new_events_and_preserves_parent_changes() {
    let fixture = Fixture::new();
    let source = fixture.environment("source", 4);
    let target = fixture.environment("target", 4);
    apply(&source, &[event(1), event(2)]);
    let page = journal::verified_page(&source.begin_read(), 7, 0, Limits::default()).unwrap();
    let mut write = target.begin_write().unwrap();
    write.put(b"app", b"parent", b"keep").unwrap();
    let result = journal::apply_verified_page(
        &mut write,
        &page,
        Limits::default(),
        ApplyMode::Synchronous,
        |txn, e| {
            txn.put(b"app", &e.sequence.to_be_bytes(), &e.payload)?;
            if e.sequence == 2 {
                return Err(Error::InvalidInput("application rejects final event"));
            }
            Ok(())
        },
    );
    assert!(matches!(
        result,
        Err(ProtocolError::Storage(Error::InvalidInput(_)))
    ));
    write.commit().unwrap();
    let read = target.begin_read();
    assert_eq!(journal::cursor(&read, 7).unwrap().applied, 0);
    assert_eq!(read.get(b"app", &1u64.to_be_bytes()).unwrap(), None);
    assert_eq!(read.get(b"app", &2u64.to_be_bytes()).unwrap(), None);
    assert_eq!(read.get(b"app", b"parent").unwrap(), Some(&b"keep"[..]));
}

#[test]
fn callback_cannot_publish_events_under_a_changed_replication_version() {
    let fixture = Fixture::new();
    let source = fixture.environment("source", 4);
    let target = fixture.environment("target", 4);
    apply(&source, &[event(1)]);
    let page = journal::verified_page(&source.begin_read(), 7, 0, Limits::default()).unwrap();
    let mut write = target.begin_write().unwrap();
    let result = journal::apply_verified_page(
        &mut write,
        &page,
        Limits::default(),
        ApplyMode::Synchronous,
        |txn, _| {
            journal::set_versions(
                txn,
                Versions {
                    schema: 2,
                    policy: 1,
                },
            )
        },
    );
    assert!(matches!(result, Err(ProtocolError::InvalidPage(_))));
    write.commit().unwrap();
    let read = target.begin_read();
    assert_eq!(journal::versions(&read).unwrap(), Versions::default());
    assert_eq!(journal::cursor(&read, 7).unwrap().applied, 0);
}

#[test]
fn retained_floor_is_public_but_earlier_checkpoints_require_snapshot() {
    let fixture = Fixture::new();
    let source = fixture.environment("source", 4);
    let target = fixture.environment("target", 4);
    apply(&source, &[event(1), event(2), event(3)]);
    apply(&target, &[event(1)]);
    let old_page = journal::verified_page(&source.begin_read(), 7, 0, Limits::default()).unwrap();
    let old_head = journal::checkpoint(&source.begin_read(), 7, 1).unwrap();
    let receipt =
        journal::export_snapshot(&source, &source.begin_read(), fixture.0.join("snapshot"))
            .unwrap();
    let mut write = source.begin_write().unwrap();
    journal::prune(
        &mut write,
        &receipt,
        7,
        1,
        Limits::default(),
        &[PeerAck {
            peer: 9,
            origin: 7,
            sequence: 3,
        }],
        1,
    )
    .unwrap();
    write.commit().unwrap();
    let read = source.begin_read();
    assert_eq!(journal::checkpoint(&read, 7, 1).unwrap(), old_head);
    assert!(matches!(
        journal::checkpoint(&read, 7, 0),
        Err(ProtocolError::SnapshotRequired {
            origin: 7,
            requested: 0,
            floor: 1
        })
    ));
    assert!(matches!(
        journal::verified_page(&read, 7, 0, Limits::default()),
        Err(ProtocolError::SnapshotRequired {
            origin: 7,
            requested: 0,
            floor: 1
        })
    ));
    let mut write = source.begin_write().unwrap();
    assert!(matches!(
        journal::apply_verified_page(
            &mut write,
            &old_page,
            Limits::default(),
            ApplyMode::Synchronous,
            |_, _| panic!("pruned replay callback")
        ),
        Err(ProtocolError::SnapshotRequired {
            origin: 7,
            requested: 0,
            floor: 1
        })
    ));
    drop(write);
    let resumed = journal::verified_page(&read, 7, 1, Limits::default()).unwrap();
    assert_eq!(apply_verified(&target, &resumed).applied, 2);
}

#[test]
fn empty_pages_are_explicit_and_source_cursors_do_not_advance_destination_serving() {
    let fixture = Fixture::new();
    let source = fixture.environment("source", 4);
    let target = fixture.environment("target", 4);
    let empty = journal::verified_page(&source.begin_read(), 7, 0, Limits::default()).unwrap();
    assert!(empty.events.is_empty());
    assert_eq!(empty.encoded_bytes, 0);
    assert_eq!(empty.base, empty.tail);
    assert!(!empty.more());
    assert_eq!(apply_verified(&target, &empty).applied, 0);
    apply(&source, &[event(1)]);
    let page = journal::verified_page(&source.begin_read(), 7, 0, Limits::default()).unwrap();
    let mut write = target.begin_write().unwrap();
    journal::apply_verified_page(
        &mut write,
        &page,
        Limits::default(),
        ApplyMode::EvidenceOnly,
        |txn, e| txn.put(b"app", &e.origin.to_be_bytes(), &e.payload),
    )
    .unwrap();
    write.commit().unwrap();
    let cursor = journal::cursor(&target.begin_read(), 7).unwrap();
    assert_eq!(cursor.applied, 1);
    assert_eq!(
        cursor.serving, 0,
        "source materialization watermark never authorizes local projection completeness"
    );
}
