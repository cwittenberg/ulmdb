mod contract;
mod dataset;
mod strategy;
mod timing;
use timing::{measure, verify};

use dataset::KeyKind;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ops::Bound,
    path::{Path, PathBuf},
    time::Instant,
};
use strategy::{Lookup, Native, Standard};

struct Config {
    keys: usize,
    queries: usize,
    samples: usize,
    seed: u64,
    directory: PathBuf,
    worker: Option<PathBuf>,
}
fn config() -> Result<Config, Box<dyn std::error::Error>> {
    let mut c = Config {
        keys: 8192,
        queries: 100_000,
        samples: 7,
        seed: 73,
        directory: PathBuf::new(),
        worker: Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("bbtree/target/release/ulmdb-bbtree-worker"),
        ),
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or("every option requires a value")?;
        match flag.as_str() {
            "--keys" => c.keys = value.parse()?,
            "--queries" => c.queries = value.parse()?,
            "--samples" => c.samples = value.parse()?,
            "--seed" => c.seed = value.parse()?,
            "--dir" => c.directory = value.into(),
            "--bbtree-worker" => c.worker = Some(value.into()),
            _ => return Err(format!("unknown option {flag}").into()),
        }
    }
    if c.keys == 0 || c.queries == 0 || c.samples == 0 || c.directory.as_os_str().is_empty() {
        return Err("usage: --keys N --queries N --samples N --seed N --dir NEW_DIRECTORY; counts must be positive".into());
    }
    if c.directory.exists() {
        return Err("--dir must not exist".into());
    }
    Ok(c)
}
fn file_bytes(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let p = entry.unwrap().path();
            if p.is_dir() {
                file_bytes(&p)
            } else {
                p.metadata().unwrap().len()
            }
        })
        .sum()
}
fn run_kind(config: &Config, kind: KeyKind) -> Result<Value, Box<dyn std::error::Error>> {
    let owned = dataset::records(kind, config.keys);
    let directory = config.directory.join(kind.name());
    let start = Instant::now();
    let env = ulmdb::Environment::create(&directory)?;
    let mut write = env.begin_write()?;
    write.create_database(b"records", ulmdb::DatabaseKind::Kv)?;
    assert_eq!(
        write.load_sorted_kv(
            b"records",
            owned.iter().map(|(k, v)| Ok((k.as_slice(), v.as_slice())))
        )?,
        config.keys as u64
    );
    write.commit()?;
    let load_ns = start.elapsed().as_nanos();
    let read = env.begin_read();
    let reader = read.kv(b"records")?;
    let gather = Instant::now();
    let records = reader
        .cursor(Bound::Unbounded, Bound::Unbounded, false)?
        .collect::<ulmdb::Result<Vec<_>>>()?;
    let gather_ns = gather.elapsed().as_nanos();
    assert_eq!(records.len(), config.keys);
    assert_eq!(
        records.iter().map(|(k, v)| (*k, *v)).collect::<Vec<_>>(),
        owned
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect::<Vec<_>>()
    );
    let build = Instant::now();
    let standard: BTreeMap<_, _> = records
        .iter()
        .enumerate()
        .map(|(id, (k, _))| (k.to_vec(), id))
        .collect();
    let standard_ns = build.elapsed().as_nanos();
    let standard_key_bytes: usize = standard.keys().map(Vec::capacity).sum();
    let native = Native { reader };
    let standard = Standard {
        map: standard,
        records: &records,
    };
    let strategies: [&dyn Lookup; 2] = [&native, &standard];
    // All keys and all constructed misses are checked, independently of timing queries.
    let all_queries: Vec<_> = (0..config.keys)
        .flat_map(|id| [kind.key(id, false), kind.key(id, true)])
        .collect();
    verify(&strategies, &all_queries, &records);
    let exported = Instant::now();
    let fixture = config.directory.join(format!("{}.fixture", kind.name()));
    {
        use std::io::Write;
        let mut file = std::io::BufWriter::new(std::fs::File::create(&fixture)?);
        file.write_all(&(records.len() as u64).to_le_bytes())?;
        for (key, value) in &records {
            file.write_all(&(key.len() as u64).to_le_bytes())?;
            file.write_all(key)?;
            file.write_all(value)?;
        }
        file.flush()?;
    }
    let export_ns = exported.elapsed().as_nanos();
    let run_worker = || -> Result<Option<Value>, Box<dyn std::error::Error>> {
        let Some(executable) = &config.worker else {
            return Ok(None);
        };
        let output = std::process::Command::new(executable)
            .args([
                "--fixture",
                fixture.to_str().ok_or("fixture path is not UTF-8")?,
                "--kind",
                kind.name(),
                "--keys",
                &config.keys.to_string(),
                "--queries",
                &config.queries.to_string(),
                "--samples",
                &config.samples.to_string(),
                "--seed",
                &config.seed.to_string(),
            ])
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "bbtree helper failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(Some(serde_json::from_slice(&output.stdout)?))
    };
    let helper_first = (config.seed ^ u64::from(matches!(kind, KeyKind::Hostname))) & 1 == 0;
    let mut helper = if helper_first { run_worker()? } else { None };
    let mut measurements = Vec::new();
    for (workload, misses) in [
        ("hit", 0),
        ("miss", 100),
        ("mixed_50_50", 50),
        ("range64", 50),
    ] {
        let queries = dataset::queries(kind, config.queries, config.keys, config.seed, misses);
        verify(&strategies, &queries, &records);
        measurements.extend(measure(&strategies, &queries, config.samples, workload));
    }
    if !helper_first {
        helper = run_worker()?;
    }
    if let Some(helper) = &helper {
        for row in helper["measurements"]
            .as_array()
            .ok_or("helper measurement array missing")?
        {
            let checksum = measurements
                .iter()
                .find(|native| {
                    native["strategy"] == "ulmdb_legacy_prepared"
                        && native["workload"] == row["workload"]
                        && native["sample"] == row["sample"]
                })
                .ok_or("helper workload mismatch")?;
            assert_eq!(
                row["checksum"], checksum["checksum"],
                "helper/native checksum mismatch"
            );
            assert_eq!(
                row["queries"], checksum["queries"],
                "helper/native query count mismatch"
            );
            measurements.push(row.clone());
        }
    }
    // Prove existing native and std indexes retain old bytes after new writes and reader publication.
    let changed_key = records[0].0;
    let old_value = records[0].1;
    let mut write = env.begin_write()?;
    write.put(b"records", changed_key, &[0xa5; 16])?;
    write.commit()?;
    for strategy in &strategies {
        assert_eq!(strategy.get(changed_key), Some(old_value));
    }
    assert_eq!(
        env.begin_read().get(b"records", changed_key)?,
        Some([0xa5; 16].as_slice())
    );
    let record_storage = records.capacity() * std::mem::size_of::<strategy::Record<'_>>();
    Ok(
        json!({"key_kind":kind.name(),"keys":config.keys,"value_bytes":16,"validation_passed":true,"old_snapshot_after_write_passed":true,"backend":"ulmdb::Environment Legacy with prepared KvRead", "build":{"ulmdb_create_bulk_load_commit_ns":load_ns as u64,"snapshot_enumeration_ns":gather_ns as u64,"bbtree_fixture_export_ns":export_ns as u64,"std_btreemap_collect_copied_keys_ns":standard_ns as u64,"std_complete_accelerator_build_ns":(gather_ns+standard_ns) as u64},"memory":{"shared_borrowed_record_vector_bytes":record_storage,"std_owned_key_capacity_bytes":standard_key_bytes,"std_entry_payload_lower_bound_bytes":config.keys*std::mem::size_of::<(Vec<u8>,usize)>(),"std_allocator_node_overhead_bytes":Value::Null,"ulmdb_apparent_file_bytes_after_snapshot_test":file_bytes(&directory),"excludes":"allocator headers, mapping RSS and benchmark input/query vectors; std node overhead is unknown"},"bbtree_helper_first":helper_first,"bbtree_helper":helper,"measurements":measurements}),
    )
}
fn execute() -> Result<(), Box<dyn std::error::Error>> {
    let c = config()?;
    std::fs::create_dir(&c.directory)?;
    let mut results = Vec::new();
    for kind in [KeyKind::Fixed16, KeyKind::Hostname] {
        results.push(run_kind(&c, kind)?);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"schema":1,"benchmark":"ordered_lookup_strategies","bbtree_version":"0.2.0","seed":c.seed,"samples":c.samples,"queries":c.queries,"semantics":"ULMDB/std use pinned snapshot; bbtree helper borrows an exact exported immutable fixture; separate dependency graphs required by incompatible Verus pins; helper launch/IPC excluded from lookup timings","results":results})
        )?
    );
    Ok(())
}
fn main() {
    if let Err(error) = execute() {
        eprintln!("ordered benchmark failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_differential_points_ranges_and_pinned_old_snapshot() {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-ordered-strategy-test-{}",
            std::process::id()
        ));
        assert!(!path.exists());
        std::fs::create_dir(&path).unwrap();
        let config = Config {
            keys: 257,
            queries: 101,
            samples: 1,
            seed: 73,
            directory: path.clone(),
            worker: None,
        };
        for kind in [KeyKind::Fixed16, KeyKind::Hostname] {
            run_kind(&config, kind).unwrap();
        }
        std::fs::remove_dir_all(path).unwrap();
    }
}
