use crate::{
    Result,
    cold::Cold,
    dataset::Config,
    durable::WriteCase,
    harness::{Case, Timed},
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, os::unix::fs::MetadataExt, path::Path, process::Command};
include!(concat!(env!("OUT_DIR"), "/provenance.rs"));
#[derive(Serialize, Default)]
pub struct Footprint {
    pub files: u64,
    pub apparent_file_bytes: u64,
    pub allocated_file_bytes: u64,
    pub mapped_virtual_kib: Option<u64>,
    pub mapped_rss_kib: Option<u64>,
    pub mapped_pss_kib: Option<u64>,
    pub mapping_scope: &'static str,
}
#[derive(Serialize)]
pub struct Footprints {
    pub ulmdb: Footprint,
    pub lmdb: Footprint,
    pub whole_process_vm_rss: String,
}
fn walk(p: &Path, o: &mut Footprint) -> Result<()> {
    for e in fs::read_dir(p)? {
        let e = e?;
        let m = e.metadata()?;
        if m.is_dir() {
            walk(&e.path(), o)?
        } else if m.is_file() {
            o.files += 1;
            o.apparent_file_bytes += m.len();
            o.allocated_file_bytes += m.blocks() * 512;
        }
    }
    Ok(())
}
fn mappings(p: &Path, o: &mut Footprint) -> Result<()> {
    let needle = p.canonicalize()?.to_string_lossy().into_owned();
    let (mut size, mut rss, mut pss) = (0, 0, 0);
    let mut selected = false;
    for line in fs::read_to_string("/proc/self/smaps")?.lines() {
        if line
            .split_whitespace()
            .next()
            .is_some_and(|s| s.contains('-'))
        {
            selected = line.contains(&needle);
        }
        if selected {
            let n = line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            if line.starts_with("Size:") {
                size += n
            } else if line.starts_with("Rss:") {
                rss += n
            } else if line.starts_with("Pss:") {
                pss += n
            }
        }
    }
    o.mapped_virtual_kib = Some(size);
    o.mapped_rss_kib = Some(rss);
    o.mapped_pss_kib = Some(pss);
    o.mapping_scope = "path-attributed mappings only; excludes heap and unrelated mappings";
    Ok(())
}
pub fn footprints(root: &Path) -> Result<Footprints> {
    let (mut u, mut l) = (Footprint::default(), Footprint::default());
    walk(&root.join("ulmdb"), &mut u)?;
    walk(&root.join("lmdb"), &mut l)?;
    #[cfg(not(feature = "resident"))]
    mappings(&root.join("ulmdb"), &mut u)?;
    #[cfg(feature = "resident")]
    {
        u.mapping_scope = "anonymous Resident images cannot be attributed by pathname; null is unavailable, not zero RSS";
    }
    mappings(&root.join("lmdb"), &mut l)?;
    let status = fs::read_to_string("/proc/self/status")?;
    Ok(Footprints {
        ulmdb: u,
        lmdb: l,
        whole_process_vm_rss: status
            .lines()
            .find(|s| s.starts_with("VmRSS:"))
            .unwrap_or("unobserved")
            .into(),
    })
}
#[derive(Serialize)]
pub struct Combined {
    pub aggregate_elapsed_ns: u64,
    pub sampled_phase_elapsed_ns: u64,
    pub combined_elapsed_ns: u64,
    pub aggregate_commits: u64,
    pub sampled_commits: u64,
    pub combined_commits: u64,
    pub combined_records: u64,
    pub combined_ns_per_commit: f64,
    pub combined_records_per_second: f64,
    pub pooled_sampled_transaction_p50_ns: u64,
    pub pooled_sampled_transaction_p95_ns: u64,
    pub pooled_sampled_transaction_p99_ns: u64,
}
#[derive(Serialize)]
pub struct CombinedWrite {
    pub records_per_transaction: usize,
    pub ulmdb: Combined,
    pub lmdb: Combined,
    pub scope: &'static str,
}
pub fn combined(root: &Path, writes: &[WriteCase]) -> Result<Vec<CombinedWrite>> {
    let raw = fs::read(root.join("latencies.u64le"))?;
    let mut out = Vec::new();
    for c in writes {
        let mut both = Vec::new();
        for native in [false, true] {
            let mut aggregate = 0;
            let mut values = Vec::new();
            for t in &c.trials {
                let v = if native { &t.lmdb } else { &t.ulmdb };
                aggregate += v.aggregate_elapsed_ns;
                if v.raw_latency.count != c.separate_latency_phase_commits
                    || !v.raw_latency.offset_bytes.is_multiple_of(8)
                {
                    return Err("write sample count/alignment mismatch".into());
                }
                let a = usize::try_from(v.raw_latency.offset_bytes)?;
                let n = v
                    .raw_latency
                    .count
                    .checked_mul(8)
                    .ok_or("sample extent overflow")?;
                let b = raw
                    .get(a..a.checked_add(n).ok_or("sample end overflow")?)
                    .ok_or("sample outside raw file")?;
                if format!("{:x}", Sha256::digest(b)) != v.raw_latency.sha256 {
                    return Err("raw write span hash mismatch".into());
                }
                values.extend(
                    b.chunks_exact(8)
                        .map(|b| u64::from_le_bytes(b.try_into().unwrap())),
                );
            }
            let sampled: u64 = values.iter().sum();
            let ac = (c.trials.len() * c.commits_per_aggregate_phase) as u64;
            let sc = values.len() as u64;
            let cc = ac + sc;
            let total = aggregate + sampled;
            both.push(Combined {
                aggregate_elapsed_ns: aggregate,
                sampled_phase_elapsed_ns: sampled,
                combined_elapsed_ns: total,
                aggregate_commits: ac,
                sampled_commits: sc,
                combined_commits: cc,
                combined_records: cc * c.records_per_transaction as u64,
                combined_ns_per_commit: total as f64 / cc as f64,
                combined_records_per_second: (cc * c.records_per_transaction as u64) as f64 * 1e9
                    / total.max(1) as f64,
                pooled_sampled_transaction_p50_ns: crate::harness::percentile(&values, 50),
                pooled_sampled_transaction_p95_ns: crate::harness::percentile(&values, 95),
                pooled_sampled_transaction_p99_ns: crate::harness::percentile(&values, 99),
            });
        }
        let lmdb = both.pop().unwrap();
        let ulmdb = both.pop().unwrap();
        out.push(CombinedWrite{records_per_transaction:c.records_per_transaction,ulmdb,lmdb,scope:"aggregate plus sum of separate timer-inclusive sampled commit latencies; all256 business commits included per batch; excludes per-trial full validation and phase dispatch gaps; this is not total wall time"});
    }
    Ok(out)
}
#[derive(Serialize)]
pub struct Report {
    pub schema: u32,
    pub suite: &'static str,
    pub backend: &'static str,
    pub experimental: bool,
    pub timing_enabled: bool,
    pub semantic_validation_passed: bool,
    pub qualification: &'static str,
    pub config: Config,
    pub dataset_sha256: String,
    pub query_sha256: String,
    pub canonical_ipv4_records: usize,
    pub canonical_ipv6_records: usize,
    pub binary_sha256: String,
    pub source_hashes: BTreeMap<&'static str, &'static str>,
    pub rust_build: BTreeMap<&'static str, &'static str>,
    pub native_version: String,
    pub native_flags: u32,
    pub native_build_info: &'static str,
    pub cpu_model: String,
    pub affinity: String,
    pub kernel: String,
    pub filesystem: String,
    pub raw_latency_file: &'static str,
    pub raw_latency_sha256: String,
    pub creation: Value,
    pub backend_before: Value,
    pub backend_after: Value,
    pub before: Footprints,
    pub after: Footprints,
    pub reads: Vec<Case>,
    pub writes: Vec<WriteCase>,
    pub combined_writes: Vec<CombinedWrite>,
    pub cold: Cold,
    pub timer_control: Option<Timed>,
    pub limitations: Vec<&'static str>,
}
pub fn command(name: &str, args: &[&str]) -> String {
    Command::new(name)
        .args(args)
        .output()
        .ok()
        .filter(|v| v.status.success())
        .map(|v| String::from_utf8_lossy(&v.stdout).trim().into())
        .unwrap_or_else(|| "unobserved".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        durable::{WriteTimed, WriteTrial},
        harness::RawSamples,
    };
    fn timed(bytes: &[u8]) -> WriteTimed {
        WriteTimed {
            aggregate_elapsed_ns: 400,
            aggregate_ns_per_transaction: 50.0,
            aggregate_records_per_second: 0.0,
            timer_inclusive_p99_transaction_ns: 100,
            raw_latency: RawSamples {
                offset_bytes: 0,
                count: 8,
                sha256: format!("{:x}", Sha256::digest(bytes)),
            },
        }
    }
    #[test]
    fn combined_cost_includes_every_sampled_commit_and_validates_extent() {
        let root = std::env::temp_dir().join(format!(
            "ulmdb-combined-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let bytes = [100u64; 8]
            .into_iter()
            .flat_map(u64::to_le_bytes)
            .collect::<Vec<_>>();
        fs::write(root.join("latencies.u64le"), &bytes).unwrap();
        let mut c = WriteCase {
            name: "fixture",
            records_per_transaction: 64,
            commits_per_aggregate_phase: 8,
            separate_latency_phase_commits: 8,
            trials: vec![WriteTrial {
                number: 0,
                first_backend: "ulmdb",
                trace_sha256: String::new(),
                ulmdb: timed(&bytes),
                lmdb: timed(&bytes),
            }],
        };
        let out = combined(&root, std::slice::from_ref(&c)).unwrap();
        assert_eq!(out[0].ulmdb.aggregate_elapsed_ns, 400);
        assert_eq!(out[0].ulmdb.sampled_phase_elapsed_ns, 800);
        assert_eq!(out[0].ulmdb.combined_elapsed_ns, 1200);
        assert_eq!(out[0].ulmdb.combined_commits, 16);
        assert_eq!(out[0].ulmdb.combined_records, 1024);
        assert_eq!(out[0].ulmdb.combined_ns_per_commit, 75.0);
        assert_eq!(out[0].lmdb.pooled_sampled_transaction_p99_ns, 100);
        c.trials[0].ulmdb.raw_latency.count = 7;
        assert!(combined(&root, &[c]).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
