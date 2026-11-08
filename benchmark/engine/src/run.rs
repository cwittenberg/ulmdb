use crate::{
    Result,
    backend::{self, Environment},
    cold,
    dataset::{self, Config, Dataset},
    durable,
    harness::{self, Case, Samples},
    native::{self, Handles},
    oracle::Answer,
    report,
};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::Instant};
use ulmdb::Address;
use ulmdb_lmdb_baseline::Environment as NativeEnvironment;
include!("read_cases.rs");
pub fn execute(directory: &Path, config: Config, measure: bool) -> Result<String> {
    if measure && cfg!(debug_assertions) {
        return Err("bench requires release build".into());
    }
    if let Some(parent) = directory.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(directory)?;
    let start = Instant::now();
    let data = dataset::build(config);
    let dataset_ns = start.elapsed().as_nanos();
    let start = Instant::now();
    let u = backend::create(&directory.join("ulmdb"))?;
    let ulmdb_create_ns = start.elapsed().as_nanos();
    fs::create_dir(directory.join("lmdb"))?;
    let start = Instant::now();
    let l = NativeEnvironment::open(&directory.join("lmdb"), 512 << 20)?;
    let lmdb_create_ns = start.elapsed().as_nanos();
    if l.flags() != 0 {
        return Err("LMDB normal flags0 required".into());
    }
    let start = Instant::now();
    native::load_ulmdb(&u, &data)?;
    let handles = native::load_lmdb(&l, &data)?;
    let fixture_load_ns = start.elapsed().as_nanos();
    let start = Instant::now();
    backend::after_setup(&u)?;
    let after_setup_ns = start.elapsed().as_nanos();
    let before = report::footprints(directory)?;
    let backend_before = backend::observe(&u)?;
    let mut raw = Samples::create(&directory.join("latencies.u64le"))?;
    let timer = if measure {
        Some(harness::timer_control(&data.kv_queries.values, &mut raw)?)
    } else {
        None
    };
    let (reads, _, _) = read_cases(&data, &u, &l, &handles, measure, &mut raw)?;
    let writes = if measure {
        durable::run(&data, &u, &l, &handles, &mut raw)?
    } else {
        durable::smoke(&u, &l, &handles)?;
        Vec::new()
    };
    if measure && (reads.len() != 13 || writes.len() != 2) {
        return Err("full15case inventory mismatch".into());
    }
    let after = report::footprints(directory)?;
    let backend_after = backend::observe(&u)?;
    let hash = raw.finish()?;
    let combined_writes = report::combined(directory, &writes)?;
    let cold = cold::check(u, &directory.join("ulmdb"), &data)?;
    let cpu = fs::read_to_string("/proc/cpuinfo")?;
    let status = fs::read_to_string("/proc/self/status")?;
    let result = report::Report {
        schema: 1,
        suite: "full15",
        backend: backend::LABEL,
        experimental: cfg!(feature = "resident"),
        timing_enabled: measure,
        semantic_validation_passed: true,
        qualification: "rerunnable local benchmark; no statistical parity/nonregression or whole-engine qualification",
        config: data.config,
        dataset_sha256: data.dataset_sha256.clone(),
        query_sha256: data.queries_sha256.clone(),
        canonical_ipv4_records: data.ipv4.len(),
        canonical_ipv6_records: data.ipv6.len(),
        binary_sha256: format!("{:x}", Sha256::digest(fs::read(std::env::current_exe()?)?)),
        source_hashes: report::SOURCE_HASHES.iter().copied().collect(),
        rust_build: report::BUILD_FIELDS.iter().copied().collect(),
        native_version: ulmdb_lmdb_baseline::version(),
        native_flags: l.flags(),
        native_build_info: ulmdb_lmdb_baseline::BUILD_INFO,
        cpu_model: cpu
            .lines()
            .find(|s| s.starts_with("model name"))
            .unwrap_or("unobserved")
            .into(),
        affinity: status
            .lines()
            .find(|s| s.starts_with("Cpus_allowed_list:"))
            .unwrap_or("unobserved")
            .into(),
        kernel: report::command("uname", &["-a"]),
        filesystem: report::command(
            "stat",
            &["-f", "-c", "%T", directory.to_str().unwrap_or("")],
        ),
        raw_latency_file: "latencies.u64le",
        raw_latency_sha256: hash,
        creation: serde_json::json!({"dataset_ns":dataset_ns,"ulmdb_create_ns":ulmdb_create_ns,"lmdb_create_ns":lmdb_create_ns,"fixture_load_both_ns":fixture_load_ns,"after_setup_ns":after_setup_ns,"scope":"all outside warm query/write loops; includes real setup costs, no overhead subtraction"}),
        backend_before,
        backend_after,
        before,
        after,
        reads,
        writes,
        combined_writes,
        cold,
        timer_control: timer,
        limitations: vec![
            "13prepared-read cases and2durable KV-replacement batches; prefix writes occur only in setup",
            "portable single-thread warm synthetic corpus,16trials; no statistical acceptance claim",
            "normal sync flags; tmpfs does not provide reboot durability",
            "sampled read/commit tails include timer cost, retained separately from aggregate means",
            "batch1 then64 share growing state; combined_writes includes all charged business commits across aggregate and sampled phases",
            "per-trial full state validation is outside measured phases; Combined is not total wall time",
            "cold ULMDB open/drop/full old-new validation is reported separately; LMDB cold reopen is not measured",
            "whole-process RSS includes both engines, generator/oracles and latency buffers; admission hints are not RSS",
            "source hashes describe checked-out input context, not an exhaustive compiler dependency proof",
        ],
    };
    let text = serde_json::to_string_pretty(&result)?;
    fs::write(directory.join("report.json"), &text)?;
    Ok(text)
}
