use crate::strategy::{self, Lookup};
use serde_json::{Value, json};
use std::{hint::black_box, time::Instant};
pub fn verify(strategies: &[&dyn Lookup], queries: &[Vec<u8>], records: &[strategy::Record<'_>]) {
    for key in queries {
        let expected = records
            .binary_search_by(|r| r.0.cmp(key))
            .ok()
            .map(|id| records[id].1);
        for candidate in strategies {
            assert_eq!(
                candidate.get(key),
                expected,
                "{} point mismatch",
                candidate.name()
            );
        }
    }
    for key in queries.iter().take(256) {
        let start = records.partition_point(|row| row.0 < key.as_slice());
        let end = (start + 64).min(records.len());
        for candidate in strategies {
            assert_eq!(
                candidate.rows(key, 64),
                records[start..end],
                "{} scan mismatch",
                candidate.name()
            );
        }
    }
}
pub fn measure(
    strategies: &[&dyn Lookup],
    queries: &[Vec<u8>],
    samples: usize,
    workload: &str,
) -> Vec<Value> {
    let mut output = Vec::new();
    let actual_queries = if workload == "range64" {
        queries.len().min(4096)
    } else {
        queries.len()
    };
    let run = |candidate: &dyn Lookup| {
        let mut checksum = 0u64;
        for key in &queries[..actual_queries] {
            checksum = checksum.wrapping_add(if workload == "range64" {
                candidate.scan_digest(black_box(key), 64)
            } else {
                candidate
                    .get(black_box(key))
                    .map(|v| strategy::value_digest(black_box(v)))
                    .unwrap_or(0)
            });
        }
        black_box(checksum)
    };
    for candidate in strategies {
        black_box(run(*candidate));
    }
    let expected = run(strategies[0]);
    for sample in 0..samples {
        for offset in 0..strategies.len() {
            let candidate = strategies[(sample + offset) % strategies.len()];
            let start = Instant::now();
            let checksum = run(candidate);
            let elapsed = start.elapsed().as_nanos();
            assert_eq!(checksum, expected, "timed checksum mismatch");
            output.push(json!({"strategy":candidate.name(),"workload":workload,"sample":sample,"queries":actual_queries,"elapsed_ns":elapsed as u64,"ns_per_op":elapsed as f64/actual_queries as f64,"checksum":checksum}));
        }
    }
    output
}
