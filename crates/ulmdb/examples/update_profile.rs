//! Deterministic, dependency-free update/no-regression profile.
//! Run a release build with NEW_DIRECTORY ROWS REPEAT legacy|indexed point|batch.
//! CSV timings exclude setup; update_total includes durable commit.
use std::{hint::black_box, ops::Bound, path::PathBuf, time::Instant};
use ulmdb::{DatabaseKind, Environment, PublicationMode};

fn row(metric: &str, repeat: u64, operations: usize, elapsed: u128, dirty: usize) {
    println!("{metric},{repeat},{operations},{elapsed},{dirty}");
}
fn value(id: u64, version: u64) -> [u8; 64] {
    let mut value = [0; 64];
    value[..8].copy_from_slice(&id.to_le_bytes());
    value[8..16].copy_from_slice(&version.to_le_bytes());
    value
}
fn random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 6 {
        return Err(
            "usage: update_profile NEW_DIRECTORY ROWS REPEAT legacy|indexed point|batch".into(),
        );
    }
    let path = PathBuf::from(&args[1]);
    let rows: usize = args[2].parse()?;
    let repeat: u64 = args[3].parse()?;
    if rows < 256 {
        return Err("ROWS must be at least 256".into());
    }
    let mode = match args[4].as_str() {
        "legacy" => PublicationMode::Legacy,
        "indexed" => PublicationMode::Indexed,
        _ => return Err("mode must be legacy or indexed".into()),
    };
    let batch = match args[5].as_str() {
        "point" => false,
        "batch" => true,
        _ => return Err("method must be point or batch".into()),
    };
    let env = Environment::create_with_publication(&path, mode)?;
    let mut write = env.begin_write()?;
    write.create_database(b"kv", DatabaseKind::Kv)?;
    write.load_sorted_kv(
        b"kv",
        (0..rows as u64).map(|id| Ok(((id * 2).to_be_bytes(), value(id, 0)))),
    )?;
    write.commit()?;
    println!("metric,repeat,operations,elapsed_ns,dirty_bytes");
    // The immutable baseline snapshot keeps identical old-generation overlap.
    let old = env.begin_read();
    for (phase, (metric, count, stride)) in [
        ("sparse", (rows / 256).max(1), 256),
        ("dense25", rows / 4, 1),
        ("dense100", rows, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let records: Vec<_> = (0..count as u64)
            .map(|offset| {
                let id = offset * stride;
                (
                    (id * 2).to_be_bytes(),
                    value(id, repeat * 16 + phase as u64 + 1),
                )
            })
            .collect();
        let start = Instant::now();
        let mut write = env.begin_write()?;
        if batch {
            write.upsert_sorted_kv(b"kv", records.iter().map(|(key, value)| Ok((key, value))))?;
        } else {
            for (key, value) in &records {
                write.put(b"kv", key, value)?;
            }
        }
        let elapsed = start.elapsed().as_nanos();
        let dirty = write.dirty_bytes();
        let commit = Instant::now();
        write.commit()?;
        let commit_elapsed = commit.elapsed().as_nanos();
        let total_elapsed = start.elapsed().as_nanos();
        row(&format!("{metric}_mutation"), repeat, count, elapsed, dirty);
        row(
            &format!("{metric}_commit"),
            repeat,
            count,
            commit_elapsed,
            dirty,
        );
        row(
            &format!("{metric}_total"),
            repeat,
            count,
            total_elapsed,
            dirty,
        );
        let read = env.begin_read();
        for (key, expected) in &records {
            assert_eq!(read.get(b"kv", key)?, Some(expected.as_slice()));
        }
    }
    assert_eq!(
        old.get(b"kv", &0u64.to_be_bytes())?,
        Some(value(0, 0).as_slice())
    );
    drop(old);
    // Begin/get/end, warm hit/miss, and scans use the same deterministic keys.
    let read = env.begin_read();
    let view = read.kv(b"kv")?;
    let operations = rows.max(1_000_000);
    for (metric, miss) in [("lookup_hit", 0), ("lookup_miss", 1)] {
        let mut state = 0x1b87_3593_c826_14a5;
        let start = Instant::now();
        for _ in 0..operations {
            let id = random(&mut state) % rows as u64;
            black_box(view.get(black_box(&(id * 2 + miss).to_be_bytes()))?);
        }
        row(metric, repeat, operations, start.elapsed().as_nanos(), 0);
    }
    let start = Instant::now();
    let mut scanned = 0;
    for _ in 0..32 {
        for record in read.range(b"kv", Bound::Unbounded, Bound::Unbounded, false)? {
            black_box(record?);
            scanned += 1;
        }
    }
    row("scan", repeat, scanned, start.elapsed().as_nanos(), 0);
    drop(view);
    drop(read);
    let transaction_operations = 100_000;
    let start = Instant::now();
    for id in 0..transaction_operations {
        let read = env.begin_read();
        black_box(read.get(b"kv", &((id as u64 % rows as u64) * 2).to_be_bytes())?);
    }
    row(
        "transaction_get",
        repeat,
        transaction_operations,
        start.elapsed().as_nanos(),
        0,
    );
    // Durable single random writes exercise the unchanged point-write path.
    let mut state = 0x1b87_3593_c826_14a5;
    let single_operations = 256;
    let start = Instant::now();
    let mut dirty = 0;
    for _ in 0..single_operations {
        let id = random(&mut state) % rows as u64;
        let mut write = env.begin_write()?;
        write.put(b"kv", &(id * 2).to_be_bytes(), &value(id, repeat + 1_000))?;
        dirty += write.dirty_bytes();
        write.commit()?;
    }
    row(
        "single_write",
        repeat,
        single_operations,
        start.elapsed().as_nanos(),
        dirty,
    );
    drop(env);
    let start = Instant::now();
    let reopened = Environment::open(&path)?;
    black_box(reopened.begin_read().get(b"kv", &0u64.to_be_bytes())?);
    row("reopen_get", repeat, 1, start.elapsed().as_nanos(), 0);
    drop(reopened);
    // Mixed inserts/updates run after controls so their key layout is irrelevant
    // to the no-regression hit/miss/scan workloads above.
    let env = Environment::open(&path)?;
    let records: Vec<_> = (0..(rows / 256) as u64)
        .flat_map(|offset| {
            let id = offset * 256;
            [
                ((id * 2).to_be_bytes(), value(id, repeat + 10_000)),
                ((id * 2 + 1).to_be_bytes(), value(id, repeat + 10_001)),
            ]
        })
        .collect();
    let start = Instant::now();
    let mut write = env.begin_write()?;
    if batch {
        write.upsert_sorted_kv(b"kv", records.iter().map(|(key, value)| Ok((key, value))))?;
    } else {
        for (key, value) in &records {
            write.put(b"kv", key, value)?;
        }
    }
    let mutation_elapsed = start.elapsed().as_nanos();
    let dirty = write.dirty_bytes();
    write.commit()?;
    let total_elapsed = start.elapsed().as_nanos();
    row(
        "sparse_mixed_mutation",
        repeat,
        records.len(),
        mutation_elapsed,
        dirty,
    );
    row(
        "sparse_mixed_total",
        repeat,
        records.len(),
        total_elapsed,
        dirty,
    );
    let read = env.begin_read();
    for (key, expected) in &records {
        assert_eq!(read.get(b"kv", key)?, Some(expected.as_slice()));
    }
    Ok(())
}
