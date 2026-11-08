//! Diagnostic phase decomposition, never a substitute for the accepted suites.
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

mod native;
mod resident;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Smallwrites,
    Load,
}

#[derive(Debug, Serialize)]
struct Config {
    mode: Mode,
    engine: String,
    dir: PathBuf,
    keys: usize,
    transactions: usize,
    batch: usize,
    profile_delay_ms: u64,
    seed: u64,
}
impl Config {
    fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let mut mode = Mode::Smallwrites;
        let (mut engine, mut dir, mut keys, mut transactions, mut batch) =
            (None, None, None, None, None);
        let mut profile_delay_ms = 0;
        while let Some(arg) = args.next() {
            if arg == "--help" {
                println!(
                    "ulmdb-profile-probe --mode smallwrites|load --engine ulmdb|lmdb --dir NEW [--keys N --transactions N --batch N --profile-delay-ms N]\nDefaults: smallwrites keys8192/transactions20000/batch1; load keys1000000/batch1000/transactions=ceil(keys/batch). Diagnostic timings only; normal durability; existing directories rejected."
                );
                std::process::exit(0);
            }
            let value = args.next().ok_or("missing option value")?;
            match arg.as_str() {
                "--mode" => {
                    mode = match value.as_str() {
                        "smallwrites" => Mode::Smallwrites,
                        "load" => Mode::Load,
                        _ => return Err("unknown mode".into()),
                    }
                }
                "--engine" => engine = Some(value),
                "--dir" => dir = Some(PathBuf::from(value)),
                "--keys" => keys = Some(value.parse::<usize>()?),
                "--transactions" => transactions = Some(value.parse::<usize>()?),
                "--batch" => batch = Some(value.parse::<usize>()?),
                "--profile-delay-ms" => profile_delay_ms = value.parse()?,
                _ => return Err(format!("unknown option {arg}").into()),
            }
        }
        let keys = keys.unwrap_or(if mode == Mode::Load { 1_000_000 } else { 8192 });
        let batch = batch.unwrap_or(if mode == Mode::Load { 1000 } else { 1 });
        if keys == 0 || keys > 1_000_000 || batch == 0 || batch > 1000 || profile_delay_ms > 60_000
        {
            return Err("keys must be 1..1M, batch 1..1000 and profile delay 0..60000ms".into());
        }
        let transactions = transactions.unwrap_or(if mode == Mode::Load {
            keys.div_ceil(batch)
        } else {
            20_000
        });
        if transactions == 0 || transactions > 100_000 {
            return Err("transactions must be 1..100000".into());
        }
        if mode == Mode::Load && transactions != keys.div_ceil(batch) {
            return Err(
                "load transactions must equal ceil(keys/batch); all requested keys must be loaded"
                    .into(),
            );
        }
        let engine = engine.ok_or("--engine is required")?;
        if !["ulmdb", "lmdb"].contains(&engine.as_str()) {
            return Err("unknown engine".into());
        }
        let dir = dir.ok_or("--dir is required")?;
        if dir.exists() {
            return Err("--dir must be new; fixtures are never overwritten".into());
        }
        Ok(Self {
            mode,
            engine,
            dir,
            keys,
            transactions,
            batch,
            profile_delay_ms,
            seed: 73,
        })
    }
}

#[derive(Clone, Copy)]
struct Row {
    key: [u8; 8],
    value: [u8; 8],
}
struct Workload {
    fixture: Vec<Row>,
    measured: Vec<Row>,
    expected: Vec<[u8; 8]>,
    trace_sha256: String,
}
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
impl Workload {
    fn new(c: &Config) -> Self {
        let fixture: Vec<_> = (0..c.keys)
            .map(|i| Row {
                key: (2 * i as u64).to_be_bytes(),
                value: mix(i as u64 ^ c.seed).to_be_bytes(),
            })
            .collect();
        let mut expected: Vec<_> = fixture.iter().map(|r| r.value).collect();
        let measured = if c.mode == Mode::Load {
            fixture.clone()
        } else {
            let mut order: Vec<_> = (0..c.keys).collect();
            let mut rng = c.seed;
            for i in (1..c.keys).rev() {
                rng = rng.wrapping_add(0x9e3779b97f4a7c15);
                order.swap(i, (mix(rng) % (i as u64 + 1)) as usize);
            }
            (0..c.transactions * c.batch)
                .map(|n| {
                    let i = order[n % c.keys];
                    let value = (u64::from_be_bytes(expected[i]) ^ (n as u64 + 1)).to_be_bytes();
                    expected[i] = value;
                    Row {
                        key: fixture[i].key,
                        value,
                    }
                })
                .collect()
        };
        let mut hash = Sha256::new();
        hash.update(b"ulmdb-diagnostic-probe-v1");
        for r in &measured {
            hash.update(r.key);
            hash.update(r.value);
        }
        Self {
            fixture,
            measured,
            expected,
            trace_sha256: format!("{:x}", hash.finalize()),
        }
    }
}

#[derive(Default, Serialize)]
struct Totals {
    begin_ns: u128,
    mutation_ns: u128,
    commit_ns: u128,
}
#[derive(Serialize)]
struct Sample {
    transaction: usize,
    records: usize,
    begin_ns: u128,
    mutation_ns: u128,
    commit_ns: u128,
    total_ns: u128,
}
struct Measurement {
    clock: Instant,
    started: Instant,
    totals: Totals,
    samples: Vec<Sample>,
    sample_stride: usize,
    transactions: usize,
}
impl Measurement {
    fn start(c: &Config, clock: Instant) -> Self {
        mark(clock, "ready_for_profile");
        if c.profile_delay_ms != 0 {
            std::thread::sleep(Duration::from_millis(c.profile_delay_ms));
        }
        mark(clock, "measurement_begin");
        Self {
            clock,
            started: Instant::now(),
            totals: Totals::default(),
            samples: Vec::with_capacity(2048),
            sample_stride: c.transactions.div_ceil(2048),
            transactions: 0,
        }
    }
    fn record(&mut self, records: usize, begin: u128, mutation: u128, commit: u128, total: u128) {
        self.totals.begin_ns += begin;
        self.totals.mutation_ns += mutation;
        self.totals.commit_ns += commit;
        if self.transactions.is_multiple_of(self.sample_stride) {
            self.samples.push(Sample {
                transaction: self.transactions,
                records,
                begin_ns: begin,
                mutation_ns: mutation,
                commit_ns: commit,
                total_ns: total,
            });
        }
        self.transactions += 1;
    }
    fn finish(self, records: usize) -> Value {
        let elapsed = self.started.elapsed().as_nanos();
        mark(self.clock, "measurement_end");
        let percentile = |field: fn(&Sample) -> u128, p: usize| -> u128 {
            let mut values: Vec<_> = self.samples.iter().map(field).collect();
            values.sort_unstable();
            values[(values.len() * p)
                .div_ceil(100)
                .saturating_sub(1)
                .min(values.len() - 1)]
        };
        json!({ "elapsed_ns":elapsed, "records":records, "transactions":self.transactions,
            "records_per_second":records as f64 * 1e9 / elapsed as f64,
            "phase_totals":self.totals, "sample_stride":self.sample_stride,
            "sampled_transaction_ns":{"p50":percentile(|s|s.total_ns,50),"p95":percentile(|s|s.total_ns,95),"p99":percentile(|s|s.total_ns,99)},
            "sampled_commit_ns":{"p50":percentile(|s|s.commit_ns,50),"p95":percentile(|s|s.commit_ns,95),"p99":percentile(|s|s.commit_ns,99)},
            "samples":self.samples })
    }
}
fn mark(clock: Instant, phase: &str) {
    let mut timestamp = std::mem::MaybeUninit::<libc::timespec>::uninit();
    // Diagnostic-only executable: use the same clock as the external PC sampler
    // so setup, measured work and cold replay can be separated without guessing.
    let status = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, timestamp.as_mut_ptr()) };
    assert_eq!(status, 0, "diagnostic monotonic clock failed");
    let timestamp = unsafe { timestamp.assume_init() };
    let monotonic_ns = timestamp.tv_sec as u128 * 1_000_000_000 + timestamp.tv_nsec as u128;
    eprintln!(
        "{}",
        json!({"probe_phase":phase,"elapsed_ns":clock.elapsed().as_nanos(),"monotonic_ns":monotonic_ns,"pid":std::process::id()})
    );
}
fn validate_points(
    w: &Workload,
    mut get: impl FnMut(&[u8]) -> Result<Option<[u8; 8]>>,
) -> Result<()> {
    for (i, row) in w.fixture.iter().enumerate() {
        if get(&row.key)? != Some(w.expected[i]) {
            return Err(format!("value mismatch at key {i}").into());
        }
        if get(&(2 * i as u64 + 1).to_be_bytes())?.is_some() {
            return Err(format!("miss unexpectedly present at key {i}").into());
        }
    }
    Ok(())
}
fn validate_ordered(
    w: &Workload,
    mut next: impl FnMut() -> Result<Option<([u8; 8], [u8; 8])>>,
) -> Result<()> {
    for (i, row) in w.fixture.iter().enumerate() {
        if next()? != Some((row.key, w.expected[i])) {
            return Err(format!("cursor mismatch at row {i}").into());
        }
    }
    if next()?.is_some() {
        return Err("cursor returned extra row".into());
    }
    Ok(())
}
fn bytes(value: Option<&[u8]>) -> Result<Option<[u8; 8]>> {
    value.map(|v| v.try_into().map_err(Into::into)).transpose()
}
fn main() -> Result<()> {
    let c = Config::parse()?;
    let clock = Instant::now();
    mark(clock, "workload_begin");
    let w = Workload::new(&c);
    mark(clock, "setup_begin");
    let report = if c.engine == "ulmdb" {
        resident::run(&c, &w, clock)?
    } else {
        native::run(&c, &w, clock)?
    };
    mark(clock, "validated_complete");
    println!(
        "{}",
        serde_json::to_string(
            &json!({"schema_version":1,"diagnostic_only":true,"accepted_benchmark":false,"config":c,
        "trace_sha256":w.trace_sha256,"report":report,"validation":{"all_keys_values":true,"all_interleaved_misses":true,"full_ordered_cursor":true,"short_pinned_snapshot":c.engine == "ulmdb", "snapshot_scope":"Resident only; LMDB normal flags0 thread-affine adapter","full_reopen_values_and_cursor":true},
        "accounting":{"fixture_load_outside_smallwrites_measurement":true,"per_transaction_timers_perturb_latency":true,"sampling_and_accounting_inside_elapsed":true,"automatic_collection_inside_begin_write":true,"snapshot_validation_outside_measurement":true,"normal_durability":true,"instrumentation_scope":"diagnostic timing decomposition; use uninstrumented full and millions suites for qualification"}})
        )?
    );
    Ok(())
}
