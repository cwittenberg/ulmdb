use crate::backend::Environment;
use crate::{
    Result,
    dataset::{Dataset, next},
    harness::{RawSamples, Samples, percentile},
    native::Handles,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, time::Instant};
use ulmdb_lmdb_baseline::Environment as NativeEnvironment;

type Entry = ([u8; 8], [u8; 8]);
type Transaction = Vec<Entry>;
#[derive(Serialize)]
pub struct WriteTimed {
    pub aggregate_elapsed_ns: u64,
    pub aggregate_ns_per_transaction: f64,
    pub aggregate_records_per_second: f64,
    pub timer_inclusive_p99_transaction_ns: u64,
    pub raw_latency: RawSamples,
}
#[derive(Serialize)]
pub struct WriteTrial {
    pub number: usize,
    pub first_backend: &'static str,
    pub trace_sha256: String,
    pub ulmdb: WriteTimed,
    pub lmdb: WriteTimed,
}
#[derive(Serialize)]
pub struct WriteCase {
    pub name: &'static str,
    pub records_per_transaction: usize,
    pub commits_per_aggregate_phase: usize,
    pub separate_latency_phase_commits: usize,
    pub trials: Vec<WriteTrial>,
}
fn traces(
    data: &Dataset,
    permutation: &[usize],
    batch: usize,
    trial: usize,
    phase: usize,
) -> Vec<Transaction> {
    (0..data.config.commits_per_phase)
        .map(|transaction| {
            (0..batch)
                .map(|record| {
                    let index = (trial * data.config.commits_per_phase * batch
                        + transaction * batch
                        + record)
                        % permutation.len();
                    let key = data.kv[permutation[index]].0;
                    let value = 0xa500000000000000u64
                        ^ ((batch as u64) << 48)
                        ^ ((phase as u64) << 47)
                        ^ ((trial as u64) << 32)
                        ^ ((transaction as u64) << 16)
                        ^ record as u64;
                    (key, value.to_be_bytes())
                })
                .collect()
        })
        .collect()
}
fn update_ulmdb(environment: &Environment, entries: &Transaction) -> Result<()> {
    let mut write = environment.begin_write()?;
    for (key, value) in entries {
        write.put(b"kv", key, value)?;
    }
    write.commit()?;
    Ok(())
}
fn update_lmdb(
    environment: &NativeEnvironment,
    handles: &Handles<'_>,
    entries: &Transaction,
) -> Result<()> {
    let mut write = environment.begin_write()?;
    for (key, value) in entries {
        write.put(&handles.kv, key, value)?;
    }
    write.commit()?;
    Ok(())
}
fn phase(
    transactions: &[Transaction],
    update: &impl Fn(&Transaction) -> Result<()>,
    timed: bool,
) -> Result<(u64, Vec<u64>)> {
    let mut samples = Vec::with_capacity(if timed { transactions.len() } else { 0 });
    let all = Instant::now();
    for transaction in transactions {
        if timed {
            let start = Instant::now();
            update(transaction)?;
            samples.push(start.elapsed().as_nanos().try_into()?);
        } else {
            update(transaction)?;
        }
    }
    Ok((all.elapsed().as_nanos().try_into()?, samples))
}
fn timed(elapsed: u64, latencies: &[u64], batch: usize, raw: &mut Samples) -> Result<WriteTimed> {
    Ok(WriteTimed {
        aggregate_elapsed_ns: elapsed,
        aggregate_ns_per_transaction: elapsed as f64 / latencies.len() as f64,
        aggregate_records_per_second: (latencies.len() * batch) as f64 * 1e9
            / elapsed.max(1) as f64,
        timer_inclusive_p99_transaction_ns: percentile(latencies, 99),
        raw_latency: raw.append(latencies)?,
    })
}
fn validate(
    ulmdb: &Environment,
    lmdb: &NativeEnvironment,
    handles: &Handles<'_>,
    expected: &BTreeMap<[u8; 8], [u8; 8]>,
) -> Result<()> {
    let u = ulmdb.begin_read();
    let l = lmdb.begin_read()?;
    let up = u.kv(b"kv")?;
    let lp = l.database(&handles.kv)?;
    for (key, value) in expected {
        if up.get(key)? != Some(value.as_slice()) || lp.get(key)? != Some(value.as_slice()) {
            return Err("durable replacement validation failed".into());
        }
    }
    Ok(())
}
pub fn run(
    data: &Dataset,
    ulmdb: &Environment,
    lmdb: &NativeEnvironment,
    handles: &Handles<'_>,
    raw: &mut Samples,
) -> Result<Vec<WriteCase>> {
    let mut permutation: Vec<_> = (0..data.kv.len()).collect();
    let mut seed = data.config.seed ^ 0x44555241424c45;
    for i in (1..permutation.len()).rev() {
        let j = (next(&mut seed) as usize) % (i + 1);
        permutation.swap(i, j);
    }
    let mut expected: BTreeMap<_, _> = data.kv.iter().copied().collect();
    let mut cases = Vec::new();
    for batch in [1, 64] {
        let mut trials = Vec::new();
        for trial in 0..data.config.trials {
            let aggregate = traces(data, &permutation, batch, trial, 0);
            let latency = traces(data, &permutation, batch, trial, 1);
            let mut trace_hash = Sha256::new();
            for transaction in aggregate.iter().chain(&latency) {
                for (key, value) in transaction {
                    trace_hash.update(key);
                    trace_hash.update(value);
                }
            }
            let u = |t: &Transaction| update_ulmdb(ulmdb, t);
            let l = |t: &Transaction| update_lmdb(lmdb, handles, t);
            let (ua, la) = if trial % 2 == 0 {
                (phase(&aggregate, &u, false)?, phase(&aggregate, &l, false)?)
            } else {
                let la = phase(&aggregate, &l, false)?;
                (phase(&aggregate, &u, false)?, la)
            };
            let (ul, ll) = if trial % 2 == 0 {
                (phase(&latency, &u, true)?, phase(&latency, &l, true)?)
            } else {
                let ll = phase(&latency, &l, true)?;
                (phase(&latency, &u, true)?, ll)
            };
            for transaction in aggregate.iter().chain(&latency) {
                for &(key, value) in transaction {
                    expected.insert(key, value);
                }
            }
            validate(ulmdb, lmdb, handles, &expected)?;
            trials.push(WriteTrial {
                number: trial,
                first_backend: if trial % 2 == 0 { "ulmdb" } else { "lmdb" },
                trace_sha256: format!("{:x}", trace_hash.finalize()),
                ulmdb: timed(ua.0, &ul.1, batch, raw)?,
                lmdb: timed(la.0, &ll.1, batch, raw)?,
            });
        }
        cases.push(WriteCase {
            name: "normal_durable_KV_replacement_begin_updates_commit",
            records_per_transaction: batch,
            commits_per_aggregate_phase: data.config.commits_per_phase,
            separate_latency_phase_commits: data.config.commits_per_phase,
            trials,
        });
    }
    Ok(cases)
}
pub fn smoke(ulmdb: &Environment, lmdb: &NativeEnvironment, handles: &Handles<'_>) -> Result<()> {
    let transaction = vec![(2u64.to_be_bytes(), 123456u64.to_be_bytes())];
    update_ulmdb(ulmdb, &transaction)?;
    update_lmdb(lmdb, handles, &transaction)?;
    validate(ulmdb, lmdb, handles, &transaction.into_iter().collect())
}
