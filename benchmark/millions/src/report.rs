use crate::{
    Result,
    workload::{self, Config, Record, Workload},
};
use serde::Serialize;
use serde_json::Value;
use std::{hint::black_box, path::Path, time::Instant};

#[derive(Serialize)]
pub struct Row {
    pub engine: String,
    pub operation: &'static str,
    pub operations: usize,
    pub elapsed_ns: u64,
    pub transactions: usize,
}
#[derive(Serialize)]
pub struct EngineResult {
    pub backend: &'static str,
    pub versions: Value,
    pub settings: Value,
    pub rows: Vec<Row>,
    pub setup_ns: u64,
    pub close_reopen_ns: u64,
    pub maintenance: Value,
    pub engine_stats: Value,
    pub validation: Value,
    pub storage_before_close: Storage,
}
#[derive(Serialize)]
pub struct Storage {
    pub files: usize,
    pub apparent_bytes: u64,
    pub allocated_bytes: u64,
}
pub fn storage(path: &Path) -> Result<Storage> {
    let mut sum = Storage {
        files: 0,
        apparent_bytes: 0,
        allocated_bytes: 0,
    };
    for item in std::fs::read_dir(path)? {
        let item = item?;
        let meta = item.metadata()?;
        if meta.is_dir() {
            let x = storage(&item.path())?;
            sum.files += x.files;
            sum.apparent_bytes += x.apparent_bytes;
            sum.allocated_bytes += x.allocated_bytes;
        } else if meta.is_file() {
            sum.files += 1;
            sum.apparent_bytes += meta.len();
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                sum.allocated_bytes += meta.blocks() * 512;
            }
        }
    }
    Ok(sum)
}
pub fn nanos(start: Instant) -> u64 {
    start
        .elapsed()
        .as_nanos()
        .try_into()
        .expect("elapsed time exceeds u64 ns")
}
pub fn writes(
    engine: &str,
    operation: &'static str,
    records: &[Record],
    c: &Config,
    mut write: impl FnMut(&[Record]) -> Result<()>,
) -> Result<Row> {
    let start = Instant::now();
    for batch in records.chunks(c.batch) {
        write(black_box(batch))?;
    }
    let elapsed_ns = nanos(start);
    Ok(Row {
        engine: engine.into(),
        operation,
        operations: records.len(),
        elapsed_ns,
        transactions: records.len().div_ceil(c.batch),
    })
}
pub fn reads(
    engine: &str,
    w: &Workload,
    mut get: impl FnMut(&[u8; 8]) -> Result<Option<[u8; 8]>>,
) -> Result<Row> {
    let mut hits = 0;
    let mut checksum = 0;
    let start = Instant::now();
    for key in &w.queries {
        let value = get(black_box(key))?;
        hits += u64::from(value.is_some());
        checksum = workload::fold(checksum, value);
    }
    black_box((hits, checksum));
    let elapsed_ns = nanos(start);
    if hits != w.identity.read_hits || checksum != w.identity.read_checksum {
        return Err("timed read result checksum/hit oracle mismatch".into());
    }
    Ok(Row {
        engine: engine.into(),
        operation: "read",
        operations: w.queries.len(),
        elapsed_ns,
        transactions: 1,
    })
}
pub fn bytes(value: Option<&[u8]>) -> Result<Option<[u8; 8]>> {
    value
        .map(|v| {
            v.try_into()
                .map_err(|_| "value is not exactly 8 bytes".into())
        })
        .transpose()
}
// Complete point-value and miss validation. Independent of all measured clocks.
pub fn validate(
    w: &Workload,
    updated: bool,
    mut get: impl FnMut(&[u8; 8]) -> Result<Option<[u8; 8]>>,
) -> Result<()> {
    for (i, record) in w.load.iter().enumerate() {
        let expected = if updated {
            w.final_values[i]
        } else {
            record.value
        };
        if get(&record.key)? != Some(expected) {
            return Err(format!("full value oracle failed at key {i}").into());
        }
        if get(&(2 * i as u64 + 1).to_be_bytes())?.is_some() {
            return Err(format!("miss oracle failed at key {i}").into());
        }
    }
    Ok(())
}
pub fn validate_ordered(
    w: &Workload,
    updated: bool,
    mut next: impl FnMut() -> Result<Option<([u8; 8], [u8; 8])>>,
) -> Result<()> {
    for (i, record) in w.load.iter().enumerate() {
        let expected = if updated {
            w.final_values[i]
        } else {
            record.value
        };
        if next()? != Some((record.key, expected)) {
            return Err(format!("ordered cursor oracle failed at row {i}").into());
        }
    }
    if next()?.is_some() {
        return Err("ordered cursor found unexpected extra rows".into());
    }
    Ok(())
}
pub fn validation() -> Value {
    serde_json::json!({"passed":true,"initial_all_values_and_misses":true,"timed_read_hits_and_checksum":true,"final_all_values_and_misses":true,"reopened_all_values_and_misses":true,"exact_row_count":true,"outside_timers":true})
}
