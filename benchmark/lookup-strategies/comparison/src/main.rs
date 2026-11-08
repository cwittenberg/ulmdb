mod campaign;
mod data;
mod lmdb_adapter;
mod observe;
mod protocol;
mod reads;
mod ulmdb_adapter;
mod writes;

use data::{Config, Record};
use observe::Report;
use protocol::{Engine, Result, Snapshot, context};
use serde_json::{Value, json};
use std::{fs, time::Instant};
use ulmdb_lmdb_baseline as lmdb;

struct LmdbSource<'env> {
    env: &'env lmdb::Environment,
    db: lmdb::Database<'env>,
}
struct LmdbSnapshot<'txn, 'env> {
    txn: lmdb::ReadTransaction<'env>,
    db: &'txn lmdb::Database<'env>,
}
impl<'env> Engine for LmdbSource<'env> {
    type Read<'a>
        = LmdbSnapshot<'a, 'env>
    where
        Self: 'a;
    fn snapshot(&self) -> Result<Self::Read<'_>> {
        Ok(LmdbSnapshot {
            txn: self.env.begin_read()?,
            db: &self.db,
        })
    }
    fn apply(&self, rows: &[Record], changes: &[(usize, [u8; 32])]) -> Result<()> {
        lmdb_adapter::apply(self.env, &self.db, rows, changes)
    }
}
impl<'env> Snapshot for LmdbSnapshot<'_, 'env> {
    type Reader<'a>
        = lmdb_adapter::Read<'a, 'env>
    where
        Self: 'a;
    fn prepare(&self, _: &Config) -> Result<Self::Reader<'_>> {
        lmdb_adapter::Read::new(&self.txn, self.db)
    }
    fn identity(&self) -> Value {
        json!({"engine":"pinned native LMDB read transaction"})
    }
}
fn execute(c: &Config) -> Result<Value> {
    let mut report = Report::default();
    report.rss("process_start_before_fixture");
    let mut rows = data::records(c);
    report.rss("fixture_ready");
    let mut ballast = vec![0u8; c.ballast_mib * (1 << 20)];
    for page in ballast.chunks_mut(4096) {
        page[0] = 0x5a;
    }
    report.rss("resident_ballast_ready");
    if let Some(parent) = c.directory.parent() {
        fs::create_dir_all(parent)?;
    }
    let outcome = (|| -> Result<()> {
        if c.arm == "lmdb" {
            fs::create_dir(&c.directory)?;
            let map_bytes = (c.keys * 2048 + (64 << 20)).max(256 << 20);
            let start = Instant::now();
            let env = lmdb::Environment::open_notls(&c.directory, map_bytes)
                .map_err(|e| context(e, "LMDB create/open environment"))?;
            report.life("create_environment",start.elapsed().as_nanos(),json!({"map_bytes":map_bytes,"flags":env.flags(),"normal_sync":true,"notls":"allows held old reader and same-thread writer"}));
            let start = Instant::now();
            let db =
                lmdb_adapter::load(&env, &rows).map_err(|e| context(e, "LMDB fixture load"))?;
            report.life("fixture_load",start.elapsed().as_nanos(),json!({"loader":"sorted put calls and named database creation in one durable transaction"}));
            report.rss("loaded_source_only");
            {
                let source = LmdbSource { env: &env, db };
                campaign::run(&source, c, &mut rows, &mut report).map_err(|e| {
                    context(e, &format!("campaign diagnostics={}", source.diagnostics()))
                })?;
            }
            let start = Instant::now();
            drop(env);
            report.life("close_environment", start.elapsed().as_nanos(), json!({}));
            report.rss("closed_environment");
            let start = Instant::now();
            let env = lmdb::Environment::open_notls(&c.directory, map_bytes)
                .map_err(|e| context(e, "LMDB create/open environment"))?;
            report.life(
                "reopen_environment",
                start.elapsed().as_nanos(),
                json!({"scope":"warm OS cache reopen; no cache eviction"}),
            );
            let start = Instant::now();
            let db =
                lmdb_adapter::database(&env).map_err(|e| context(e, "LMDB reopen database"))?;
            report.life(
            "reopen_database_handle",
            start.elapsed().as_nanos(),
            json!({"scope":"native named DBI resolution through a committed write transaction"}),
        );
            let source = LmdbSource { env: &env, db };
            campaign::reopened(&source, c, &rows, &mut report)
                .map_err(|e| context(e, "reopened snapshot/preparation/validation"))?;
        } else {
            let start = Instant::now();
            let source = ulmdb_adapter::Source::create(c)
                .map_err(|e| context(e, "ULMDB create environment"))?;
            report.life(
                "create_environment",
                start.elapsed().as_nanos(),
                json!({"resident_options":format!("{:?}",ulmdb_adapter::options(c.keys))}),
            );
            let start = Instant::now();
            let loader = source.load(&rows).map_err(|e| {
                context(
                    e,
                    &format!("ULMDB fixture load diagnostics={}", source.diagnostics()),
                )
            })?;
            report.life("fixture_load",start.elapsed().as_nanos(),json!({"loader":loader,"comparison_scope":"loader APIs differ across engines; reported separately"}));
            report.rss("loaded_source_only");
            campaign::run(&source, c, &mut rows, &mut report).map_err(|e| {
                context(e, &format!("campaign diagnostics={}", source.diagnostics()))
            })?;
            let start = Instant::now();
            drop(source);
            report.life("close_environment", start.elapsed().as_nanos(), json!({}));
            report.rss("closed_environment");
            let start = Instant::now();
            let source = ulmdb_adapter::Source::open(c)
                .map_err(|e| context(e, "ULMDB reopen environment"))?;
            report.life(
                "reopen_environment",
                start.elapsed().as_nanos(),
                json!({"scope":"warm OS cache reopen; no cache eviction"}),
            );
            campaign::reopened(&source, c, &rows, &mut report)
                .map_err(|e| context(e, "reopened snapshot/preparation/validation"))?;
        }
        Ok(())
    })();
    if let Err(error) = outcome {
        report.rss("failure");
        eprintln!(
            "COMPARISON_PARTIAL {}",
            json!({"config":{"keys":c.keys,"arm":c.arm,"backend":c.backend,"key_kind":c.key_kind,"samples":c.samples,"transactions":c.transactions},"error":error.to_string(),"samples":report.samples,"lifecycle":report.lifecycle,"memory":report.memory})
        );
        return Err(error);
    }
    std::hint::black_box(&ballast);
    Ok(
        json!({"schema":1,"benchmark":"direct_completed_lookup_strategy_comparison",
        "config":{"keys":c.keys,"queries":c.queries,"samples":c.samples,"seed":c.seed,"arm":c.arm,"backend":if c.arm=="lmdb"{"native_lmdb"}else{&c.backend},"key_kind":c.key_kind,"value_bytes":32,"write_transactions_per_sample":c.transactions,"ballast_mib":c.ballast_mib,"hash_index_budget_bytes":512usize<<20},
        "samples":report.samples,"lifecycle":report.lifecycle,"memory":report.memory,
        "files":observe::file_bytes(&c.directory),"lmdb_version":lmdb::version(),"lmdb_build_info":lmdb::BUILD_INFO,
        "validation":{"passed":true,"all_initial_old_new_reopened_records_exact_bytes":true,"all_timed_queries_checked":true,"forward_reverse_bounded_ranges":true,"native_arm_never_constructs_hash":c.arm!="hash"},
        "scope":{"reads":"single-thread warm full borrowed payload consumption; arms run in independent processes",
            "writes":"same deterministic durable updates; explicit charged Resident Collect before long-lived pin, Legacy/LMDB no-op; source-only held old snapshot and hash held old snapshot+index; also unpinned before/after controls; no automatic hash rebuild; retention remains bounded by source pool capacity",
            "rss":"actual per-process Linux VmRSS/VmHWM checkpoints include input/query/model memory and optional touched retained ballast; high-water is cumulative, allocator may retain freed storage; ballast is not controlled OS memory pressure",
            "range_timing":"same precomputed bounds, expected rows and complete key/value digests; fixture bound calculation excluded",
            "lifecycle":"warm reopen, no crash/power-loss injection; initial loader APIs differ and are identified; full new hash build charged separately"}}),
    )
}
fn main() {
    let result = Config::parse().and_then(|c| execute(&c));
    match result {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        Err(e) => {
            eprintln!("comparison failed: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_arms_and_backends_exact_bytes_ranges_durable_writes_and_reopen() {
        for backend in ["legacy", "resident"] {
            for arm in ["native", "hash", "lmdb"] {
                if arm == "lmdb" && backend == "resident" {
                    continue;
                }
                let directory = std::env::temp_dir().join(format!(
                    "ulmdb-direct-strategy-{}-{backend}-{arm}",
                    std::process::id()
                ));
                assert!(!directory.exists());
                let c = Config {
                    keys: 129,
                    queries: 31,
                    samples: 1,
                    seed: 73,
                    arm: arm.into(),
                    backend: backend.into(),
                    key_kind: "hostname".into(),
                    directory: directory.clone(),
                    transactions: 1,
                    ballast_mib: 0,
                };
                let report = execute(&c).unwrap();
                assert_eq!(report["validation"]["passed"], true);
                fs::remove_dir_all(directory).unwrap();
            }
        }
    }
}
