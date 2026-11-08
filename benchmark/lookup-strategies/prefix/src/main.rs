mod arena;
mod fixture;
mod measure;
mod strategy;

use fixture::Fixture;
use measure::Operation;
use std::{env, time::Instant};
use strategy::Algorithm;
use ulmdb_core::storage::Result;

fn number(args: &[String], flag: &str, default: usize) -> usize {
    args.iter()
        .position(|v| v == flag)
        .map(|i| {
            args.get(i + 1)
                .expect("flag value")
                .parse()
                .expect("positive integer")
        })
        .unwrap_or(default)
}
fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    let keys = number(&args, "--keys", 8192);
    let queries = number(&args, "--queries", 8192);
    let rounds = number(&args, "--rounds", 7);
    let iterations = number(&args, "--iterations", 4);
    let seed = number(&args, "--seed", 1) as u64;
    assert!(rounds > 0 && iterations > 0);
    println!(
        "kind,family,keys,algorithm,operation,distribution,round,ns_per_op,checksum,build_ms,arena_bytes,reachable_bytes"
    );
    for v6 in [false, true] {
        let f = Fixture::new(keys, queries, v6, 0x1402_5623_4150_6767 ^ seed);
        let family = if v6 { "ipv6" } else { "ipv4" };
        let algorithms = [Algorithm::Binary, Algorithm::Multibit];
        let mut indexes = Vec::new();
        for algo in &algorithms {
            let start = Instant::now();
            let (arena, root) = algo.build(&f.entries)?;
            let build_ms = start.elapsed().as_secs_f64() * 1000.0;
            let live = algo.reachable(&arena, root, f.full())?;
            println!(
                "build,{family},{keys},{},construction,sorted,0,0,0,{build_ms:.6},{},{}",
                algo.name(),
                arena.bytes.len(),
                live
            );
            indexes.push((arena, root));
        }
        // Every requested query is compared in full before timing, including order,
        // matched prefix and complete payload. An independent brute-force oracle probes
        // the actual fixture plus a separate deterministic small prefix fixture below.
        for q in f.hits.iter().chain(&f.mixed).chain(&f.nested) {
            let a = algorithms[0].reader(&indexes[0].0, indexes[0].1)?;
            let b = algorithms[1].reader(&indexes[1].0, indexes[1].1)?;
            assert_eq!(a.exact(*q)?, b.exact(*q)?, "exact {q:?}");
            assert_eq!(a.lpm(*q)?, b.lpm(*q)?, "lpm {q:?}");
            let host = f.host(*q);
            assert_eq!(
                algorithms[0].ancestors(&indexes[0].0, indexes[0].1, host)?,
                algorithms[1].ancestors(&indexes[1].0, indexes[1].1, host)?,
                "ancestors {q:?}"
            );
        }
        for q in f.hits.iter().chain(&f.mixed).step_by((queries / 32).max(1)) {
            let reader = algorithms[0].reader(&indexes[0].0, indexes[0].1)?;
            let exact: Vec<_> = reader
                .exact(*q)?
                .into_iter()
                .map(|(p, v)| (p, v.to_vec()))
                .collect();
            assert_eq!(exact, f.oracle(*q, true, false));
            let host = f.host(*q);
            let lpm: Vec<_> = reader
                .lpm(host)?
                .into_iter()
                .map(|(p, v)| (p, v.to_vec()))
                .collect();
            assert_eq!(lpm, f.oracle(host, false, false));
            let all: Vec<_> = algorithms[0]
                .ancestors(&indexes[0].0, indexes[0].1, host)?
                .into_iter()
                .map(|(p, v)| (p, v.to_vec()))
                .collect();
            assert_eq!(all, f.oracle(host, false, true));
        }
        oracle_probes(v6)?;
        for op in [Operation::Exact, Operation::Lpm, Operation::Ancestors] {
            for (distribution, source) in [
                ("hit", &f.hits),
                ("mixed50", &f.mixed),
                ("nested_chain", &f.nested),
            ] {
                let host;
                let qs = if matches!(op, Operation::Ancestors) {
                    host = source.iter().map(|q| f.host(*q)).collect::<Vec<_>>();
                    &host
                } else {
                    source
                };
                // Warm each exact operation using the same query list.
                for (i, algo) in algorithms.iter().enumerate() {
                    std::hint::black_box(measure::run(
                        algo,
                        &indexes[i].0,
                        indexes[i].1,
                        qs,
                        op,
                        1,
                    )?);
                }
                for round in 0..rounds {
                    let mut sums = [0; 2];
                    // Rotate order to avoid always granting either strategy first run.
                    for slot in 0..2 {
                        let i = (slot + round) % 2;
                        let algo = &algorithms[i];
                        let (ns, checksum) =
                            measure::run(algo, &indexes[i].0, indexes[i].1, qs, op, iterations)?;
                        sums[i] = checksum;
                        println!(
                            "lookup,{family},{keys},{},{},{distribution},{round},{ns:.6},{checksum},0,0,0",
                            algo.name(),
                            op.name()
                        );
                    }
                    assert_eq!(sums[0], sums[1], "timed checksum mismatch");
                }
            }
        }
    }
    Ok(())
}
fn oracle_probes(v6: bool) -> Result<()> {
    let f = Fixture::new(128, 128, v6, 0x9786_3421_3514);
    for algo in [Algorithm::Binary, Algorithm::Multibit] {
        let (arena, root) = algo.build(&f.entries)?;
        let reader = algo.reader(&arena, root)?;
        for q in f.hits.iter().chain(&f.mixed).chain(&f.nested) {
            let exact: Vec<_> = reader
                .exact(*q)?
                .into_iter()
                .map(|(p, v)| (p, v.to_vec()))
                .collect();
            assert_eq!(exact, f.oracle(*q, true, false));
            let host = f.host(*q);
            let lpm: Vec<_> = reader
                .lpm(host)?
                .into_iter()
                .map(|(p, v)| (p, v.to_vec()))
                .collect();
            assert_eq!(lpm, f.oracle(host, false, false));
            let all: Vec<_> = algo
                .ancestors(&arena, root, host)?
                .into_iter()
                .map(|(p, v)| (p, v.to_vec()))
                .collect();
            assert_eq!(all, f.oracle(host, false, true));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn both_strategies_agree_with_independent_oracle() {
        super::oracle_probes(false).unwrap();
        super::oracle_probes(true).unwrap();
    }
}
