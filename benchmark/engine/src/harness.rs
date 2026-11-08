use crate::{
    Result,
    oracle::{self, Answer},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    hint::black_box,
    io::{Seek, Write},
    path::Path,
    time::Instant,
};

pub struct Samples {
    file: File,
    hash: Sha256,
}
#[derive(Serialize)]
pub struct RawSamples {
    pub offset_bytes: u64,
    pub count: usize,
    pub sha256: String,
}
impl Samples {
    pub fn create(path: &Path) -> Result<Self> {
        Ok(Self {
            file: File::create_new(path)?,
            hash: Sha256::new(),
        })
    }
    pub fn append(&mut self, values: &[u64]) -> Result<RawSamples> {
        let offset = self.file.stream_position()?;
        let mut bytes = Vec::with_capacity(values.len() * 8);
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        self.file.write_all(&bytes)?;
        self.hash.update(&bytes);
        Ok(RawSamples {
            offset_bytes: offset,
            count: values.len(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        })
    }
    pub fn finish(mut self) -> Result<String> {
        self.file.flush()?;
        Ok(format!("{:x}", self.hash.finalize()))
    }
}
#[derive(Serialize)]
pub struct Timed {
    pub elapsed_ns: u64,
    pub ns_per_operation: f64,
    pub timer_inclusive_p50_ns: u64,
    pub timer_inclusive_p95_ns: u64,
    pub timer_inclusive_p99_ns: u64,
    pub raw_latency: RawSamples,
}
#[derive(Serialize)]
pub struct Trial {
    pub number: usize,
    pub first_backend: &'static str,
    pub ulmdb: Timed,
    pub lmdb: Timed,
}
#[derive(Serialize)]
pub struct Case {
    pub name: String,
    pub queries: usize,
    pub hits: usize,
    pub matched_lengths: BTreeMap<String, usize>,
    pub expected_checksum: String,
    pub query_indices: Vec<usize>,
    pub trials: Vec<Trial>,
}
pub fn percentile(values: &[u64], percent: usize) -> u64 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted[(sorted.len() * percent).div_ceil(100).saturating_sub(1)]
}
fn average<Q>(
    indices: &[usize],
    queries: &[Q],
    lookup: &impl Fn(&Q) -> Option<Answer>,
) -> (u64, u64) {
    let start = Instant::now();
    let mut checksum = 0;
    for &index in indices {
        checksum = oracle::fold(checksum, black_box(lookup(black_box(&queries[index]))));
    }
    (
        start.elapsed().as_nanos().try_into().unwrap(),
        black_box(checksum),
    )
}
fn latency<Q>(
    indices: &[usize],
    queries: &[Q],
    lookup: &impl Fn(&Q) -> Option<Answer>,
) -> (Vec<u64>, u64) {
    let mut samples = Vec::with_capacity(indices.len());
    let mut checksum = 0;
    for &index in indices {
        let start = Instant::now();
        checksum = oracle::fold(checksum, black_box(lookup(black_box(&queries[index]))));
        samples.push(start.elapsed().as_nanos().try_into().unwrap());
    }
    (samples, black_box(checksum))
}
fn measured(
    operations: usize,
    elapsed: u64,
    latencies: &[u64],
    raw: &mut Samples,
) -> Result<Timed> {
    Ok(Timed {
        elapsed_ns: elapsed,
        ns_per_operation: elapsed as f64 / operations as f64,
        timer_inclusive_p50_ns: percentile(latencies, 50),
        timer_inclusive_p95_ns: percentile(latencies, 95),
        timer_inclusive_p99_ns: percentile(latencies, 99),
        raw_latency: raw.append(latencies)?,
    })
}
#[allow(clippy::too_many_arguments)]
pub fn case<Q>(
    name: String,
    queries: &[Q],
    expected: &[Option<Answer>],
    indices: Vec<usize>,
    trials: usize,
    ulmdb: impl Fn(&Q) -> Option<Answer>,
    lmdb: impl Fn(&Q) -> Option<Answer>,
    raw: &mut Samples,
) -> Result<Case> {
    if indices.is_empty() {
        return Err("empty benchmark cell".into());
    }
    let mut checksum = 0;
    let mut lengths = BTreeMap::new();
    let mut hits = 0;
    for &i in &indices {
        if ulmdb(&queries[i]) != expected[i] || lmdb(&queries[i]) != expected[i] {
            return Err(format!("independent expected answer mismatch in {name}, query{i}").into());
        }
        checksum = oracle::fold(checksum, expected[i]);
        if let Some(answer) = expected[i] {
            hits += 1;
            *lengths.entry(answer.length.to_string()).or_insert(0) += 1;
        }
    }
    let mut results = Vec::with_capacity(trials);
    for trial in 0..trials {
        let (u, l) = if trial % 2 == 0 {
            (
                average(&indices, queries, &ulmdb),
                average(&indices, queries, &lmdb),
            )
        } else {
            let l = average(&indices, queries, &lmdb);
            (average(&indices, queries, &ulmdb), l)
        };
        let (us, ls) = if trial % 2 == 0 {
            (
                latency(&indices, queries, &ulmdb),
                latency(&indices, queries, &lmdb),
            )
        } else {
            let l = latency(&indices, queries, &lmdb);
            (latency(&indices, queries, &ulmdb), l)
        };
        if [u.1, l.1, us.1, ls.1].iter().any(|&x| x != checksum) {
            return Err(format!("timed result checksum mismatch in {name}").into());
        }
        results.push(Trial {
            number: trial,
            first_backend: if trial % 2 == 0 { "ulmdb" } else { "lmdb" },
            ulmdb: measured(indices.len(), u.0, &us.0, raw)?,
            lmdb: measured(indices.len(), l.0, &ls.0, raw)?,
        });
    }
    Ok(Case {
        name,
        queries: indices.len(),
        hits,
        matched_lengths: lengths,
        expected_checksum: format!("{checksum:016x}"),
        query_indices: indices,
        trials: results,
    })
}
pub fn groups(expected: &[Option<Answer>]) -> Vec<(&'static str, Vec<usize>)> {
    vec![
        ("mixed", (0..expected.len()).collect()),
        (
            "hits",
            expected
                .iter()
                .enumerate()
                .filter_map(|(i, x)| x.is_some().then_some(i))
                .collect(),
        ),
        (
            "misses",
            expected
                .iter()
                .enumerate()
                .filter_map(|(i, x)| x.is_none().then_some(i))
                .collect(),
        ),
    ]
}
pub fn prefix_groups(expected: &[Option<Answer>]) -> Vec<(&'static str, Vec<usize>)> {
    let mut result = groups(expected);
    result.push((
        "outside_cover_misses",
        (0..expected.len()).filter(|i| i % 4 == 0).collect(),
    ));
    result.push((
        "inside_gap_misses",
        (0..expected.len()).filter(|i| i % 4 == 1).collect(),
    ));
    result
}
pub fn timer_control(queries: &[[u8; 8]], raw: &mut Samples) -> Result<Timed> {
    let indices: Vec<_> = (0..queries.len()).collect();
    let control = |_: &[u8; 8]| {
        black_box(1u64);
        None
    };
    let (elapsed, _) = average(&indices, queries, &control);
    let (samples, _) = latency(&indices, queries, &control);
    measured(queries.len(), elapsed, &samples, raw)
}
