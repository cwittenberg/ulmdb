//! Deterministic backup profile. Setup, identity/data assertions and destination
//! reopening are outside timed calls. ROWS applies to each named index; duplicate
//! keys hold two values. `large` adds one 8-MiB KV payload.
//!
//! backup_profile NEW_DIRECTORY ROWS REPEAT sync|controlled|job [small|large]
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use ulmdb::{
    Address, DatabaseKind, Environment, Error, Prefix, PublicationMode, ReadTxn, Result,
    backup::{self, Summary},
};

#[derive(Default)]
struct Work {
    units: u64,
    bytes: u64,
    steps: u64,
    max_step_ns: u128,
    max_required_bytes: u64,
}
struct Outcome {
    summary: Summary,
    work: Work,
}
fn value(id: u64, version: u64) -> [u8; 64] {
    let mut bytes = [37; 64];
    bytes[..8].copy_from_slice(&id.to_be_bytes());
    bytes[8..16].copy_from_slice(&version.to_be_bytes());
    bytes
}
fn prefix(id: u64, ipv6: bool) -> Prefix {
    Prefix::new(
        if ipv6 {
            Address::V6(u128::from(id) << 64)
        } else {
            Address::V4(id as u32)
        },
        if ipv6 { 128 } else { 32 },
    )
    .unwrap()
}
fn assert_identity(left: &Summary, right: &Summary) {
    assert_eq!(left.uuid, right.uuid);
    assert_eq!(left.generation, right.generation);
    assert_eq!(left.sequence, right.sequence);
    assert_eq!(left.commit, right.commit);
    assert_eq!(left.segments, right.segments);
    assert_eq!(left.segment_bytes, right.segment_bytes);
}
fn assert_data(read: &ReadTxn, rows: u64, version: u64, large: bool) -> Result<()> {
    for id in 0..rows {
        assert_eq!(
            read.get(b"kv", &id.to_be_bytes())?,
            Some(value(id, if id == 0 { version } else { 0 }).as_slice())
        );
        for ipv6 in [false, true] {
            assert_eq!(
                read.exact_prefix(if ipv6 { b"v6" } else { b"v4" }, prefix(id, ipv6))?
                    .unwrap()
                    .1,
                value(id, 0)
            );
        }
        assert_eq!(read.duplicate_count(b"dups", &id.to_be_bytes())?, 2);
    }
    if large {
        let blob = read.get(b"kv", b"large-payload")?.unwrap();
        assert_eq!(blob.len(), 8 * 1024 * 1024);
        assert!(blob.iter().all(|byte| *byte == 73));
    }
    Ok(())
}
fn seed(path: &Path, rows: u64, large: bool) -> Result<Arc<Environment>> {
    let env = Arc::new(Environment::create_with_publication(
        path,
        PublicationMode::Indexed,
    )?);
    let mut writer = env.begin_write()?;
    for (name, kind) in [
        (b"kv".as_slice(), DatabaseKind::Kv),
        (b"v4", DatabaseKind::Ipv4),
        (b"v6", DatabaseKind::Ipv6),
        (b"dups", DatabaseKind::Duplicates { width: None }),
    ] {
        writer.create_database(name, kind)?;
    }
    writer.commit()?;
    let mut writer = env.begin_write()?;
    writer.load_sorted_kv(
        b"kv",
        (0..rows).map(|id| Ok((id.to_be_bytes(), value(id, 0)))),
    )?;
    writer.commit()?;
    for ipv6 in [false, true] {
        let mut writer = env.begin_write()?;
        writer.load_sorted_prefixes(
            if ipv6 { b"v6" } else { b"v4" },
            (0..rows).map(|id| Ok((prefix(id, ipv6), value(id, 0)))),
        )?;
        writer.commit()?;
    }
    let mut writer = env.begin_write()?;
    for id in 0..rows {
        writer.insert_duplicate(b"dups", &id.to_be_bytes(), &id.to_be_bytes())?;
        writer.insert_duplicate(b"dups", &id.to_be_bytes(), &(id + rows).to_be_bytes())?;
    }
    writer.commit()?;
    if large {
        let mut writer = env.begin_write()?;
        writer.put(b"kv", b"large-payload", &vec![73; 8 * 1024 * 1024])?;
        writer.commit()?;
    }
    Ok(env)
}

// BEGIN CANDIDATE CONTROLLED ADAPTERS
fn drain(mut job: backup::BackupJob<Summary>) -> Result<Outcome> {
    let mut work = Work::default();
    let ordinary = backup::WorkBudget {
        max_units: 65_536,
        max_bytes: 16 * 1024 * 1024,
    };
    let mut grant = ordinary;
    loop {
        let start = Instant::now();
        let step = job.advance(grant)?;
        work.max_step_ns = work.max_step_ns.max(start.elapsed().as_nanos());
        work.steps += 1;
        match step {
            backup::JobStep::Pending { required, .. } => {
                work.max_required_bytes = work.max_required_bytes.max(required.max_bytes);
                grant = backup::WorkBudget {
                    max_units: ordinary.max_units.max(required.max_units),
                    max_bytes: ordinary.max_bytes.max(required.max_bytes),
                };
            }
            backup::JobStep::Complete(summary) => {
                let progress = job.control().progress();
                work.units = progress.units;
                work.bytes = progress.bytes;
                return Ok(Outcome { summary, work });
            }
        }
    }
}
fn verify(env: &Arc<Environment>, mode: &str) -> Result<Outcome> {
    match mode {
        "sync" => Ok(Outcome {
            summary: backup::verify(env)?,
            work: Work::default(),
        }),
        "controlled" => {
            let control = backup::BackupControl::default();
            let summary = backup::verify_with_control(env, &control)?;
            let progress = control.progress();
            Ok(Outcome {
                summary,
                work: Work {
                    units: progress.units,
                    bytes: progress.bytes,
                    ..Default::default()
                },
            })
        }
        "job" => drain(backup::verify_snapshot_job(
            env.clone(),
            env.begin_read(),
            backup::BackupControl::default(),
        )?),
        _ => Err(Error::InvalidInput("backup profile method")),
    }
}
fn export(env: &Arc<Environment>, read: ReadTxn, target: &Path, mode: &str) -> Result<Outcome> {
    match mode {
        "sync" => Ok(Outcome {
            summary: backup::export_snapshot(env, &read, target)?,
            work: Work::default(),
        }),
        "controlled" => {
            let control = backup::BackupControl::default();
            let summary = backup::export_snapshot_with_control(env, &read, target, &control)?;
            let progress = control.progress();
            Ok(Outcome {
                summary,
                work: Work {
                    units: progress.units,
                    bytes: progress.bytes,
                    ..Default::default()
                },
            })
        }
        "job" => drain(backup::export_snapshot_job(
            env.clone(),
            read,
            target,
            backup::BackupControl::default(),
        )?),
        _ => Err(Error::InvalidInput("backup profile method")),
    }
}
// END CANDIDATE CONTROLLED ADAPTERS

fn row(metric: &str, repeat: u64, operations: u64, elapsed: u128, outcome: &Outcome) {
    let w = &outcome.work;
    println!(
        "{metric},{repeat},{operations},{elapsed},{},{},{},{},{},{}",
        w.units,
        w.bytes,
        w.steps,
        w.max_step_ns,
        w.max_required_bytes,
        outcome.summary.physical_bytes
    );
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if !(5..=6).contains(&args.len()) {
        return Err(
            "usage: backup_profile NEW_DIRECTORY ROWS REPEAT sync|controlled|job [small|large]"
                .into(),
        );
    }
    let root = PathBuf::from(&args[1]);
    let rows: u64 = args[2].parse()?;
    let repeat: u64 = args[3].parse()?;
    let mode = args[4].as_str();
    if rows == 0 || rows > u64::from(u32::MAX) || !["sync", "controlled", "job"].contains(&mode) {
        return Err("ROWS must fit IPv4 and method must be sync, controlled or job".into());
    }
    let large = match args.get(5).map(String::as_str).unwrap_or("large") {
        "small" => false,
        "large" => true,
        _ => return Err("case must be small or large".into()),
    };
    fs::create_dir(&root)?;
    let env = seed(&root.join("source"), rows, large)?;
    let old_summary = backup::verify(&env)?;
    let old = env.begin_read();
    let export_read = env.begin_read();
    let mut writer = env.begin_write()?;
    writer.put(
        b"kv",
        &0u64.to_be_bytes(),
        &value(0, repeat.checked_add(1).ok_or(Error::LimitExceeded)?),
    )?;
    writer.commit()?;
    let current_summary = backup::verify(&env)?;
    assert_data(&old, rows, 0, large)?;
    println!(
        "metric,repeat,operations,elapsed_ns,units,bytes,steps,max_step_ns,max_required_bytes,physical_bytes"
    );
    let operations = rows
        .checked_mul(5)
        .and_then(|n| n.checked_add(u64::from(large)))
        .ok_or(Error::LimitExceeded)?;
    let started = Instant::now();
    let verified = verify(&env, mode)?;
    let elapsed = started.elapsed().as_nanos();
    assert_eq!(verified.summary, current_summary);
    row("verify", repeat, operations, elapsed, &verified);
    let target = root.join("export");
    let started = Instant::now();
    let exported = export(&env, export_read, &target, mode)?;
    let elapsed = started.elapsed().as_nanos();
    assert_identity(&exported.summary, &old_summary);
    row("export", repeat, operations, elapsed, &exported);
    let copied = Environment::open(&target)?;
    assert_eq!(backup::verify(&copied)?, exported.summary);
    assert_data(&copied.begin_read(), rows, 0, large)?;
    assert_data(&old, rows, 0, large)?;
    assert_eq!(
        env.begin_read().get(b"kv", &0u64.to_be_bytes())?,
        Some(value(0, repeat + 1).as_slice())
    );
    Ok(())
}
