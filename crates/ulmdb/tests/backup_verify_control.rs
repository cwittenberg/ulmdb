use std::{
    collections::BTreeMap,
    ops::ControlFlow,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use ulmdb::{
    Address, DatabaseKind, Environment, Error, Prefix, PublicationMode, ReadTxn,
    backup::{self, BackupControl, BackupLimits, BackupStopReason, CancellationToken},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-verify-control-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn seed(path: &std::path::Path, mode: PublicationMode) -> Environment {
    let env = Environment::create_with_publication(path, mode).unwrap();
    let mut w = env.begin_write().unwrap();
    for (name, kind) in [
        (b"kv".as_slice(), DatabaseKind::Kv),
        (b"v4", DatabaseKind::Ipv4),
        (b"v6", DatabaseKind::Ipv6),
        (b"dups", DatabaseKind::Duplicates { width: None }),
    ] {
        w.create_database(name, kind).unwrap();
    }
    for n in 0..300u32 {
        w.put(b"kv", &n.to_be_bytes(), &[n as u8; 64]).unwrap();
        w.insert_prefix(
            b"v4",
            Prefix::new(Address::V4(n), 32).unwrap(),
            &[n as u8; 32],
        )
        .unwrap();
        w.insert_prefix(
            b"v6",
            Prefix::new(Address::V6(u128::from(n) << 64), 128).unwrap(),
            &[n as u8; 32],
        )
        .unwrap();
        w.insert_duplicate(b"dups", &n.to_be_bytes(), &n.to_le_bytes())
            .unwrap();
        w.insert_duplicate(b"dups", &n.to_be_bytes(), &[255; 7])
            .unwrap();
    }
    w.commit().unwrap();
    env
}

#[test]
fn controlled_verification_matches_complete_sync_audits_for_all_plain_formats() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Directory::new();
        let env = seed(&d.0, mode);
        let expected = backup::verify(&env).unwrap();
        let control = BackupControl::default();
        assert_eq!(
            backup::verify_with_control(&env, &control).unwrap(),
            expected
        );
        assert!(control.progress().units > 1_000);
        assert!(control.progress().bytes > expected.segment_bytes);
        let read = ReadTxn::open_coordinated_snapshot(&d.0).unwrap();
        assert_eq!(
            backup::verify_independent_snapshot_with_control(
                &d.0,
                &read,
                &BackupControl::default()
            )
            .unwrap(),
            expected
        );
    }
}

#[test]
fn cancellation_and_unit_or_byte_limits_interrupt_inside_graph_work() {
    let d = Directory::new();
    let env = seed(&d.0, PublicationMode::Indexed);
    let pre = CancellationToken::new();
    pre.cancel();
    let control = BackupControl::default().with_cancel_token(pre);
    assert!(matches!(
        backup::verify_with_control(&env, &control),
        Err(ref error) if error.is_cancelled()
    ));
    assert_eq!(control.progress().units, 0);
    let cancelled = BackupControl::default().with_progress_callback(Arc::new(|p| {
        if p.units >= 100 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }));
    assert!(matches!(
        backup::verify_with_control(&env, &cancelled),
        Err(ref error) if error.is_cancelled()
    ));
    assert!(cancelled.progress().units >= 100);
    for (max_units, max_bytes, reason) in [
        (20, u64::MAX, BackupStopReason::UnitLimit),
        (u64::MAX, 8192, BackupStopReason::ByteLimit),
    ] {
        let control = BackupControl::new(BackupLimits {
            max_units,
            max_bytes,
            deadline: None,
        });
        assert!(matches!(
            backup::verify_with_control(&env, &control),
            Err(ref error) if error.is_work_limit_exceeded()
        ));
        assert_eq!(control.stop_reason(), Some(reason));
        assert!(control.progress().units <= max_units);
        assert!(control.progress().bytes <= max_bytes);
    }
    assert_eq!(
        env.begin_read().get(b"kv", &0u32.to_be_bytes()).unwrap(),
        Some([0u8; 64].as_slice())
    );
    backup::verify(&env).unwrap();
}

#[test]
fn large_payload_extents_are_refused_before_unbudgeted_value_traversal() {
    let d = Directory::new();
    let env = Environment::create_with_publication(&d.0, PublicationMode::Indexed).unwrap();
    let value = vec![29; 8 * 1024 * 1024];
    let mut writer = env.begin_write().unwrap();
    writer.create_database(b"kv", DatabaseKind::Kv).unwrap();
    writer.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
    writer.put(b"kv", b"large", &value).unwrap();
    writer
        .insert_prefix(b"v4", Prefix::new(Address::V4(0), 0).unwrap(), &value)
        .unwrap();
    writer.commit().unwrap();
    let complete = BackupControl::default();
    assert_eq!(
        backup::verify_with_control(&env, &complete).unwrap(),
        backup::verify(&env).unwrap()
    );
    assert!(complete.progress().bytes > (value.len() * 2) as u64);
    let limited = BackupControl::new(BackupLimits {
        max_bytes: 1024 * 1024,
        ..Default::default()
    });
    assert!(matches!(
        backup::verify_with_control(&env, &limited),
        Err(ref error) if error.is_work_limit_exceeded()
    ));
    assert_eq!(limited.stop_reason(), Some(BackupStopReason::ByteLimit));
    assert!(limited.progress().bytes <= 1024 * 1024);
    assert_eq!(
        env.begin_read().get(b"kv", b"large").unwrap(),
        Some(value.as_slice())
    );
}

#[test]
fn cold_open_under_control_interrupts_opaque_admission_and_restores_observer() {
    let d = Directory::new();
    drop(seed(&d.0, PublicationMode::Legacy));
    let calls = Arc::new(AtomicU64::new(0));
    let count = calls.clone();
    let control = BackupControl::default().with_progress_callback(Arc::new(move |p| {
        count.fetch_add(1, Ordering::Relaxed);
        if p.units >= 8 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }));
    let result = control.scope(|| Environment::open(&d.0));
    assert!(
        matches!(result, Err(ref error) if error.is_cancelled()),
        "unexpected open outcome {:?}, progress {:?}",
        result.err(),
        control.progress()
    );
    assert!(calls.load(Ordering::Relaxed) > 1);
    assert!(ulmdb_core::work::current_observer().is_none());
    backup::verify(&Environment::open(&d.0).unwrap()).unwrap();
}

#[test]
fn independent_verification_callback_can_commit_and_reclaim_while_old_read_survives() {
    let d = Directory::new();
    let env = Arc::new(seed(&d.0, PublicationMode::Indexed));
    let read = ReadTxn::open_coordinated_snapshot(&d.0).unwrap();
    let old_sequence = read.sequence();
    let ran = Arc::new(AtomicBool::new(false));
    let changed = ran.clone();
    let writer_env = env.clone();
    let control = BackupControl::default().with_progress_callback(Arc::new(move |p| {
        if p.units >= 100 && !changed.swap(true, Ordering::AcqRel) {
            let mut w = writer_env.begin_write().unwrap();
            w.put(b"kv", &0u32.to_be_bytes(), b"new committed").unwrap();
            w.commit().unwrap();
            writer_env.reclaim().unwrap();
        }
        ControlFlow::Continue(())
    }));
    let result = backup::verify_independent_snapshot_with_control(&d.0, &read, &control).unwrap();
    assert!(ran.load(Ordering::Acquire));
    assert_eq!(result.sequence, old_sequence);
    assert!(env.begin_read().sequence() > old_sequence);
    assert_eq!(
        read.get(b"kv", &0u32.to_be_bytes()).unwrap(),
        Some([0u8; 64].as_slice())
    );
}

#[test]
fn built_in_and_registered_custom_ordering_keep_complete_verification_semantics() {
    use ulmdb::ordering::{DatabaseComparators, DatabaseOrdering, OrderingSpec};
    let d = Directory::new();
    let env = Environment::create(&d.0).unwrap();
    let cmp: ulmdb::ordering::CustomComparator = Arc::new(|a, b| {
        a.iter()
            .map(u8::to_ascii_lowercase)
            .cmp(b.iter().map(u8::to_ascii_lowercase))
    });
    let callbacks = DatabaseComparators {
        keys: Some(cmp),
        duplicates: None,
    };
    let mut w = env.begin_write().unwrap();
    w.create_database_ordered(
        b"custom",
        DatabaseKind::Kv,
        DatabaseOrdering {
            keys: OrderingSpec::Custom,
            duplicates: OrderingSpec::Bytewise,
        },
    )
    .unwrap();
    w.put_with_ordering(b"custom", b"Alpha", b"first", &callbacks)
        .unwrap();
    w.put_with_ordering(b"custom", b"Beta", b"second", &callbacks)
        .unwrap();
    w.create_database_ordered(
        b"reverse",
        DatabaseKind::Kv,
        DatabaseOrdering {
            keys: OrderingSpec::ReverseBytes,
            duplicates: OrderingSpec::Bytewise,
        },
    )
    .unwrap();
    for key in [b"aa", b"ab", b"ba"] {
        w.put(b"reverse", key, key).unwrap();
    }
    w.commit().unwrap();
    assert!(backup::verify_with_control(&env, &BackupControl::default()).is_err());
    let callbacks = BTreeMap::from([(b"custom".to_vec(), callbacks)]);
    assert_eq!(
        backup::verify_with_comparators_and_control(&env, &callbacks, &BackupControl::default())
            .unwrap(),
        backup::verify_with_comparators(&env, &callbacks).unwrap()
    );
}
