use std::{
    fs,
    io::{BufRead, BufReader},
    ops::ControlFlow,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::Instant,
};
use ulmdb::{
    DatabaseKind, Environment, Error,
    backup::{BackupControl, BackupJob, BackupLimits, CancellationToken, JobStep, WorkBudget},
};
use ulmdb_ipdon::{
    journal::{self, ApplyMode, Event, Limits},
    regional::Regional,
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ipdon-controlled-snapshot-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn environment(&self, bytes: usize) -> Arc<Environment> {
        let env = Arc::new(Environment::create(self.path("source")).unwrap());
        let mut write = env.begin_write().unwrap();
        Regional::initialize_in_realm(&mut write, 1, 55).unwrap();
        write.create_database(b"app", DatabaseKind::Kv).unwrap();
        write.commit().unwrap();
        apply(&env, 1, &vec![7; bytes]);
        env
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn apply(env: &Environment, sequence: u64, value: &[u8]) {
    let mut write = env.begin_write().unwrap();
    let event = Event {
        origin: 7,
        sequence,
        recorded_at: 100 + sequence,
        payload: value.to_vec(),
    };
    journal::apply_page(
        &mut write,
        &[event],
        Limits {
            max_events: 1,
            max_bytes: value.len() + 1024,
        },
        ApplyMode::Synchronous,
        |write, event| write.put(b"app", b"value", &event.payload),
    )
    .unwrap();
    write.commit().unwrap();
}
fn data_started(target: &Path) -> bool {
    ["segments", "cohorts"].into_iter().any(|directory| {
        fs::read_dir(target.join("database").join(directory)).is_ok_and(|mut entries| {
            entries.any(|entry| {
                entry.is_ok_and(|entry| {
                    entry
                        .metadata()
                        .is_ok_and(|meta| meta.is_file() && meta.len() > 64 * 1024)
                })
            })
        })
    })
}
fn finish<T: Send + 'static>(job: &mut BackupJob<T>) -> T {
    for _ in 0..100_000 {
        match job
            .advance(WorkBudget {
                max_units: 32,
                max_bytes: u64::MAX,
            })
            .unwrap()
        {
            JobStep::Complete(value) => return value,
            JobStep::Pending { required, .. } => assert!(required.max_units <= 32),
        }
    }
    panic!("controlled job did not finish");
}

#[test]
fn controlled_export_and_cold_verification_match_the_synchronous_receipt() {
    let fixture = Fixture::new();
    let env = fixture.environment(1024 * 1024);
    let old = env.begin_read();
    let synchronous = journal::export_snapshot(&env, &old, fixture.path("sync")).unwrap();
    apply(&env, 2, b"new current value");
    let controlled = journal::export_snapshot_with_control(
        &env,
        &old,
        fixture.path("controlled"),
        &BackupControl::default(),
    )
    .unwrap();
    assert_eq!(controlled, synchronous);
    assert_eq!(
        journal::verify_snapshot_with_control(
            fixture.path("controlled"),
            &BackupControl::default(),
        )
        .unwrap(),
        synchronous
    );
    assert_eq!(controlled.cursors()[&7].applied, 1);
    assert_eq!(journal::cursor(&env.begin_read(), 7).unwrap().applied, 2);
    let restored =
        journal::restore_snapshot(fixture.path("controlled"), fixture.path("restored")).unwrap();
    assert_eq!(
        restored
            .begin_read()
            .get(b"app", b"value")
            .unwrap()
            .unwrap(),
        vec![7; 1024 * 1024]
    );
}

#[test]
fn pre_cancel_limits_and_deadline_preserve_preexisting_paths_and_create_no_bundle() {
    let fixture = Fixture::new();
    let env = fixture.environment(1024);
    let token = CancellationToken::new();
    token.cancel();
    let cancelled = BackupControl::default().with_cancel_token(token);
    assert!(matches!(
        journal::export_snapshot_with_control(
            &env,
            &env.begin_read(),
            fixture.path("cancelled"),
            &cancelled,
        ),
        Err(ref error) if error.is_cancelled()
    ));
    assert!(!fixture.path("cancelled").exists());
    for (name, limits) in [
        (
            "units",
            BackupLimits {
                max_units: 0,
                ..BackupLimits::UNLIMITED
            },
        ),
        (
            "bytes",
            BackupLimits {
                max_bytes: 1,
                ..BackupLimits::UNLIMITED
            },
        ),
        (
            "deadline",
            BackupLimits {
                deadline: Some(Instant::now()),
                ..BackupLimits::UNLIMITED
            },
        ),
    ] {
        assert!(
            matches!(
                journal::export_snapshot_with_control(
                    &env,
                    &env.begin_read(),
                    fixture.path(name),
                    &BackupControl::new(limits),
                ),
                Err(ref error) if error.is_work_limit_exceeded()
            ),
            "{name}"
        );
        assert!(!fixture.path(name).exists(), "{name}");
    }
    let existing = fixture.path("existing");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("keep"), b"caller data").unwrap();
    assert!(
        journal::export_snapshot_with_control(
            &env,
            &env.begin_read(),
            &existing,
            &BackupControl::default(),
        )
        .is_err()
    );
    assert_eq!(fs::read(existing.join("keep")).unwrap(), b"caller data");
}

#[test]
fn cancellation_during_native_copy_keeps_bundle_unready_and_source_writable() {
    let fixture = Fixture::new();
    let env = fixture.environment(4 * 1024 * 1024);
    let old = env.begin_read();
    let target = fixture.path("partial");
    let observed = target.clone();
    let control = BackupControl::default().with_progress_callback(Arc::new(move |_| {
        if data_started(&observed) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }));
    assert!(matches!(
        journal::export_snapshot_with_control(&env, &old, &target, &control),
        Err(ref error) if error.is_cancelled()
    ));
    assert!(data_started(&target));
    assert!(!target.join("SNAPSHOT").exists());
    assert!(!target.join("database/CURRENT").exists());
    assert!(journal::verify_snapshot(&target).is_err());
    apply(&env, 2, b"writer remains usable");
    assert_eq!(
        old.get(b"app", b"value").unwrap().unwrap().len(),
        4 * 1024 * 1024
    );
    assert_eq!(
        env.begin_read().get(b"app", b"value").unwrap().unwrap(),
        b"writer remains usable"
    );
}

#[test]
fn cancellation_after_native_publication_does_not_publish_a_journal_receipt() {
    let fixture = Fixture::new();
    let env = fixture.environment(512 * 1024);
    let target = fixture.path("native-only");
    let observed = target.clone();
    let control = BackupControl::default().with_progress_callback(Arc::new(move |_| {
        if observed.join("database/CURRENT").exists() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }));
    assert!(matches!(
        journal::export_snapshot_with_control(&env, &env.begin_read(), &target, &control,),
        Err(ref error) if error.is_cancelled()
    ));
    assert!(target.join("database/CURRENT").exists());
    assert!(!target.join("SNAPSHOT").exists());
    assert!(journal::verify_snapshot(&target).is_err());
}

#[test]
fn cold_verification_cancels_inside_admission_and_has_no_receipt_side_effects() {
    let fixture = Fixture::new();
    let env = fixture.environment(4 * 1024 * 1024);
    let target = fixture.path("complete");
    let expected = journal::export_snapshot(&env, &env.begin_read(), &target).unwrap();
    let manifest = fs::read(target.join("SNAPSHOT")).unwrap();
    let control = BackupControl::default().with_progress_callback(Arc::new(|progress| {
        if progress.bytes > 128 * 1024 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }));
    assert!(matches!(
        journal::verify_snapshot_with_control(&target, &control),
        Err(ref error) if error.is_cancelled()
    ));
    assert!(
        control.progress().bytes < 1024 * 1024,
        "cancellation waited for the complete value"
    );
    assert_eq!(fs::read(target.join("SNAPSHOT")).unwrap(), manifest);
    let bounded = BackupControl::new(BackupLimits {
        max_bytes: 128 * 1024,
        ..BackupLimits::UNLIMITED
    });
    assert!(matches!(
        journal::verify_snapshot_with_control(&target, &bounded),
        Err(ref error) if error.is_work_limit_exceeded()
    ));
    assert!(bounded.progress().bytes <= 128 * 1024);
    assert_eq!(journal::verify_snapshot(&target).unwrap(), expected);

    // Unsupported range selection must be rejected before entering its cold
    // loader. A format marker is enough to exercise this admission boundary;
    // the ordinary bundle envelope remains complete and valid.
    fs::remove_file(target.join("database/CURRENT")).unwrap();
    if target.join("database/PREVIOUS").exists() {
        fs::remove_file(target.join("database/PREVIOUS")).unwrap();
    }
    fs::write(target.join("database/BACKEND"), b"unsupported range marker").unwrap();
    assert!(matches!(
        journal::verify_snapshot_with_control(&target, &BackupControl::default()),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn controlled_metadata_rejects_the_same_corrupt_retained_records_as_sync_export() {
    use sha2::{Digest, Sha256};

    for corruption in ["length", "digest", "identity"] {
        let fixture = Fixture::new();
        let env = fixture.environment(1024);
        let mut key = [0; 16];
        key[..8].copy_from_slice(&7u64.to_be_bytes());
        key[8..].copy_from_slice(&1u64.to_be_bytes());
        let mut record = env
            .begin_read()
            .get(b"ipdon.journal.events.v1", &key)
            .unwrap()
            .unwrap()
            .to_vec();
        match corruption {
            "length" => record[25..29].copy_from_slice(&0u32.to_be_bytes()),
            "digest" => record[29] ^= 1,
            "identity" => {
                record[1..9].copy_from_slice(&0u64.to_be_bytes());
                let digest_at = record.len() - 64;
                let hash = Sha256::digest(&record[..digest_at]);
                record[digest_at..digest_at + 32].copy_from_slice(&hash);
            }
            _ => unreachable!(),
        }
        let mut write = env.begin_write().unwrap();
        write
            .put(b"ipdon.journal.events.v1", &key, &record)
            .unwrap();
        write.commit().unwrap();
        assert!(
            journal::export_snapshot(&env, &env.begin_read(), fixture.path("sync-bad")).is_err(),
            "{corruption}"
        );
        assert!(
            journal::export_snapshot_with_control(
                &env,
                &env.begin_read(),
                fixture.path("controlled-bad"),
                &BackupControl::default(),
            )
            .is_err(),
            "{corruption}"
        );
        assert!(!fixture.path("controlled-bad").join("SNAPSHOT").exists());
    }
}

#[test]
fn paused_export_resumes_without_replay_while_a_writer_advances_the_source() {
    let fixture = Fixture::new();
    let env = fixture.environment(2 * 1024 * 1024);
    let old = env.begin_read();
    let target = fixture.path("resumed");
    let control = BackupControl::default();
    let mut job = journal::export_snapshot_job(env.clone(), old, &target, control.clone()).unwrap();
    let mut paused = false;
    for _ in 0..100_000 {
        match job
            .advance(WorkBudget {
                max_units: 16,
                max_bytes: u64::MAX,
            })
            .unwrap()
        {
            JobStep::Pending { required, .. } => {
                assert!(required.max_units <= 16);
                if data_started(&target) {
                    paused = true;
                    break;
                }
            }
            JobStep::Complete(_) => panic!("export never yielded while copying"),
        }
    }
    assert!(paused);
    let before = control.progress();
    apply(&env, 2, b"newer write while backup is paused");
    assert_eq!(
        control.progress(),
        before,
        "paused worker consumed more work without a grant"
    );
    let receipt = finish(&mut job);
    assert_eq!(receipt.cursors()[&7].applied, 1);
    assert_eq!(
        journal::verify_snapshot_with_control(&target, &BackupControl::default()).unwrap(),
        receipt
    );
    assert_eq!(journal::cursor(&env.begin_read(), 7).unwrap().applied, 2);
}

#[test]
fn abandoning_a_paused_job_and_cli_limits_do_not_mint_a_complete_bundle() {
    let fixture = Fixture::new();
    let env = fixture.environment(16 * 1024 * 1024);
    let target = fixture.path("abandoned");
    let mut job = journal::export_snapshot_job(
        env.clone(),
        env.begin_read(),
        &target,
        BackupControl::default(),
    )
    .unwrap();
    assert!(matches!(
        job.advance(WorkBudget {
            max_units: 1,
            max_bytes: u64::MAX
        })
        .unwrap(),
        JobStep::Pending { .. }
    ));
    drop(job);
    assert!(!target.join("SNAPSHOT").exists());
    drop(env);
    let binary = env!("CARGO_BIN_EXE_ipdon-regional");
    let limited = fixture.path("cli-limited");
    let output = Command::new(binary)
        .arg("snapshot")
        .arg(fixture.path("source"))
        .arg(&limited)
        .args(["--max-units", "0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!limited.exists());
    let cancelled = fixture.path("cli-cancelled");
    let cancel_file = fixture.path("cancel.request");
    let mut child = Command::new(binary)
        .arg("snapshot")
        .arg(fixture.path("source"))
        .arg(&cancelled)
        .args(["--slice-units", "32", "--progress", "--cancel-file"])
        .arg(&cancel_file)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    {
        let mut stderr = BufReader::new(child.stderr.as_mut().unwrap());
        assert!(stderr.read_line(&mut line).unwrap() > 0);
    }
    fs::write(&cancel_file, b"stop").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cancelled"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!cancelled.join("SNAPSHOT").exists());
    let complete = fixture.path("cli-complete");
    let output = Command::new(binary)
        .arg("snapshot")
        .arg(fixture.path("source"))
        .arg(&complete)
        .args(["--slice-units", "32"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(binary)
        .arg("verify-snapshot")
        .arg(&complete)
        .args(["--slice-units", "32"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    journal::verify_snapshot(&complete).unwrap();
}
