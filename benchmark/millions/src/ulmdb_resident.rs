//! Opt-in Resident adapter; the workload and common timing/oracle code are shared.
//! Fixed pilot capacity is disclosed, never enlarged or replaced by Legacy.
use crate::{
    Result,
    report::{self, EngineResult},
    workload::{Config, Record, Workload},
};
use std::{path::Path, time::Instant};
use ulmdb::{
    DatabaseKind,
    resident::{CollectCacheLimits, Environment, Options, RedoEncoding, RedoFormat},
};

const BACKEND: &str = "ulmdb::resident::Environment CompactV2 Borrowed";

fn options() -> Options {
    let mut options = Options::default();
    // All existing slots serve KV: this workload creates no prefix/Tiny objects.
    options.pool.kv_pages = 8192;
    options.pool.tiny_pages = 0;
    options.pool.max_reserved_bytes = 64 << 20;
    options.pool.max_metadata_bytes = 64 << 20;
    options.wal_capacity = 256 << 20;
    // Existing configurable allowance, including cumulative full cold audits.
    options.graph_read_budget = 1_000_000_000;
    // Keep the original quarter-pool collection policy as capacity grows.
    // Collection runs inside begin_write and remains charged to write timing.
    options.auto_collect_below = options.pool.kv_pages / 4;
    options
}

fn write_phase(
    env: &Environment,
    dir: &Path,
    operation: &'static str,
    records: &[Record],
    config: &Config,
    workload: &Workload,
    completed_rows: &[&report::Row],
) -> Result<report::Row> {
    let mut committed_records = 0usize;
    let mut committed_batches = 0usize;
    let result = report::writes("ulmdb", operation, records, config, |rows| {
        let mut tx = env.begin_write()?;
        for r in rows {
            tx.put(b"kv", &r.key, &r.value)?;
        }
        tx.commit()?;
        // One progress update per acknowledged batch; no per-key instrumentation.
        committed_records += rows.len();
        committed_batches += 1;
        Ok(())
    });
    result.map_err(|error| {
        // The failed transaction has dropped before these observations. These
        // diagnostics run only on error and never produce a completed rate row.
        let message = format!(
            "experimental Resident {operation} failed after {committed_records}/{} \
             committed records ({committed_batches} acknowledged batches); \
             configured pool={} KV/0 Tiny, WAL selected_end={}/{} bytes; \
             pool={:?}; original error: {error}. Workload not completed; \
             no smaller workload or backend fallback was used",
            records.len(),
            env.options().pool.kv_pages,
            env.wal_bytes(),
            env.wal_capacity(),
            env.pool_stats(),
        );
        let pool = env.pool_stats();
        let pool_json = match pool {
            Ok(p) => serde_json::json!({
                "free_pages":p.free_pages,"held_maps":p.held_maps,
                "maps_created":p.maps_created,"reused_pages":p.reused_pages,
                "allocations":p.allocations,"quarantined_maps":p.quarantined_maps,
                "exhausted_maps":p.exhausted_maps,"poisoned":p.poisoned,
                "rounded_mapping_bytes":p.rounded_mapping_bytes,
                "metadata_bound_bytes":p.metadata_bound_bytes,"view_roots":p.view_roots
            }),
            Err(e) => serde_json::json!({"observation_error":e.to_string()}),
        };
        let selected_state = match validate_partial(env, workload, operation, committed_records) {
            Ok(state) => state,
            Err(e) => serde_json::json!({
                "validation_passed":false,"error":e.to_string(),
                "scope":"selected state did not validate against acknowledged batch prefix"
            }),
        };
        let diagnostic = serde_json::json!({
            "schema_version":1,"engine":"ulmdb","backend":BACKEND,
            "status":"failed","full_workload_completed":false,"qualified":false,
            "failed_phase":operation,"committed_records_in_failed_phase":committed_records,
            "acknowledged_batches_in_failed_phase":committed_batches,
            "requested_records_in_failed_phase":records.len(),"original_error":error.to_string(),
            "config":config,"workload":workload.identity,"rows":completed_rows,
            "validation":{"passed":false,
                "completed_phases":if operation=="update" { vec!["load","read"] } else { vec![] },
                "failure_state_audit":selected_state["validation_passed"]==true},
            "failure":{"phase":operation,"error":error.to_string(),
                "committed_records":committed_records,"committed_batches":committed_batches,
                "requested_records":records.len()},
            "settings":{"pool_kv_pages":env.options().pool.kv_pages,"pool_tiny_pages":0,
                "pool_max_reserved_bytes":env.options().pool.max_reserved_bytes,
                "pool_max_metadata_bytes":env.options().pool.max_metadata_bytes,
                "wal_capacity":env.wal_capacity(),"graph_read_budget":env.options().graph_read_budget,
                "auto_collect_below":env.options().auto_collect_below,
                "publication":"Resident CompactV2 Borrowed","experimental":true,
                "backend_fallback":false},
            "completed_phase_validation":{
                "load_and_read_all_point_values_and_misses_passed":operation=="update",
                "timed_read_hits_and_checksum_passed":operation=="update",
                "final_and_reopened_full_workload_validation_passed":false
            },
            "selected_state":selected_state,"pool":pool_json,
            "pool_kv_pages":env.options().pool.kv_pages,"pool_tiny_pages":0,
            "wal_selected_end":env.wal_bytes(),"wal_capacity":env.wal_capacity(),
            "wal_file_bytes":std::fs::metadata(dir.join("RESIDENT.wal")).ok().map(|m|m.len()),
            "sequence":env.sequence(),"physical_lsn":env.physical_lsn(),
            "fixture_preserved":true,"backend_fallback":false,
            "timing_scope":"completed rows use common timing; failed phase has no completed rate; error diagnostics are untimed"
        });
        let receipt = serde_json::to_vec_pretty(&diagnostic)
            .map_err(std::io::Error::other)
            .and_then(|bytes| std::fs::write(dir.join("resident-failure.json"), bytes));
        if let Err(e) = receipt {
            eprintln!("could not preserve Resident failure receipt: {e}");
        }
        message.into()
    })
}

/// Inspect the published state only after the failed transaction has dropped.
/// A failed/unknown commit may disagree with this prefix; that is reported as
/// validation failure instead of assuming that the transaction was rolled back.
fn validate_partial(
    env: &Environment,
    w: &Workload,
    operation: &str,
    committed: usize,
) -> Result<serde_json::Value> {
    let count = if operation == "load" {
        committed
    } else {
        w.load.len()
    };
    let mut values: Vec<_> = w.load.iter().map(|r| r.value).collect();
    if operation == "update" {
        for record in &w.updates[..committed] {
            let index = (u64::from_be_bytes(record.key) / 2) as usize;
            values[index] = record.value;
        }
    }
    let tx = env.begin_read();
    let view = tx.kv(b"kv")?;
    let mut cursor = view.cursor(
        std::ops::Bound::Unbounded,
        std::ops::Bound::Unbounded,
        false,
    )?;
    for (index, record) in w.load[..count].iter().enumerate() {
        let pair = cursor.next().transpose()?;
        if pair != Some((record.key.as_slice(), values[index].as_slice())) {
            return Err(
                format!("selected committed-prefix cursor oracle failed at row {index}").into(),
            );
        }
    }
    if cursor.next().transpose()?.is_some() {
        return Err("selected committed-prefix cursor found extra rows".into());
    }
    Ok(serde_json::json!({
        "validation_passed":true,"row_count":count,
        "first_key":w.load.first().filter(|_|count>0).map(|r|format!("{:016x}",u64::from_be_bytes(r.key))),
        "last_key":count.checked_sub(1).and_then(|i|w.load.get(i)).map(|r|format!("{:016x}",u64::from_be_bytes(r.key))),
        "scope":"full ordered cursor count/keys/values against acknowledged batches, not full requested workload"
    }))
}

pub fn run(dir: &Path, c: &Config, w: &Workload) -> Result<EngineResult> {
    let setup = Instant::now();
    let options = options();
    let env = Environment::create_with_encoding(
        dir,
        options,
        RedoFormat::CompactV2,
        RedoEncoding::Borrowed,
    )?;
    if !env.word_search_enabled()
        || !env.single_pass_audit_enabled()
        || !env.private_workspace_enabled()
        || !env.direct_page_patch_enabled()
        || !env.scalar_prefix_enabled()
    {
        return Err("Resident benchmark requires the restored iteration34 feature set".into());
    }
    {
        let mut tx = env.begin_write()?;
        tx.create_database(b"kv", DatabaseKind::Kv)?;
        tx.commit()?;
    }
    env.enable_collect_cache(CollectCacheLimits::default())?;
    let setup_ns = report::nanos(setup);
    let load = write_phase(&env, dir, "load", &w.load, c, w, &[])?;
    let read = {
        let tx = env.begin_read();
        let view = tx.kv(b"kv")?;
        report::validate(w, false, |key| report::bytes(view.get(key)?))?;
        report::reads("ulmdb", w, |key| report::bytes(view.get(key)?))?
    };
    let update = write_phase(&env, dir, "update", &w.updates, c, w, &[&load, &read])?;
    {
        let tx = env.begin_read();
        let view = tx.kv(b"kv")?;
        report::validate(w, true, |key| report::bytes(view.get(key)?))?;
        let mut cursor = view.cursor(
            std::ops::Bound::Unbounded,
            std::ops::Bound::Unbounded,
            false,
        )?;
        report::validate_ordered(w, true, || {
            cursor
                .next()
                .transpose()?
                .map(|(key, value)| Ok((key.try_into()?, value.try_into()?)))
                .transpose()
        })?;
    }
    let pool = env.pool_stats()?;
    let cache = env
        .collect_cache_stats()?
        .ok_or("Resident Collect cache unexpectedly disabled")?;
    let engine_stats = serde_json::json!({
        "sequence":env.sequence(), "physical_lsn":env.physical_lsn(),
        "wal_selected_end":env.wal_bytes(), "wal_capacity":env.wal_capacity(),
        "pool":{"maps_created":pool.maps_created,"page_size":pool.page_size,
            "rounded_mapping_bytes":pool.rounded_mapping_bytes,
            "metadata_bound_bytes":pool.metadata_bound_bytes,
            "free_pages":pool.free_pages,"held_maps":pool.held_maps,
            "allocations":pool.allocations,"reused_pages":pool.reused_pages},
        "collect_cache":{"hits":cache.hits,"misses":cache.misses,
            "retained_bytes":cache.retained_bytes,"entries":cache.entries}
    });
    let storage_before_close = report::storage(dir)?;
    let reopen = Instant::now();
    drop(env);
    let env = Environment::open_with_encoding(
        dir,
        options,
        RedoFormat::CompactV2,
        RedoEncoding::Borrowed,
    )
    .map_err(|error| {
        let message = format!(
            "experimental Resident cold reopen failed after all workload writes; \
             full recovery was required, graph_read_budget={}; original error: {error}",
            options.graph_read_budget,
        );
        let diagnostic = serde_json::json!({
            "schema_version":1,"status":"failed","engine":"ulmdb","backend":BACKEND,
            "config":c,"workload":w.identity,"rows":[&load,&read,&update],
            "validation":{"passed":false,"completed_phases":["load","read","update"],
                "failure_state_audit":false,"final_all_values_and_cursor_passed":true,
                "cold_reopened_validation_passed":false},
            "failure":{"phase":"cold","error":error.to_string(),
                "committed_records":w.updates.len(),
                "committed_batches":w.updates.len().div_ceil(c.batch)},
            "settings":{"pool_kv_pages":options.pool.kv_pages,"pool_tiny_pages":0,
                "wal_capacity":options.wal_capacity,"graph_read_budget":options.graph_read_budget,
                "pool_max_reserved_bytes":options.pool.max_reserved_bytes,
                "pool_max_metadata_bytes":options.pool.max_metadata_bytes,
                "auto_collect_below":options.auto_collect_below,"experimental":true},
            "engine_stats":engine_stats,"fixture_preserved":true,
            "full_workload_completed":false,"qualified":false,"backend_fallback":false
        });
        let receipt = serde_json::to_vec_pretty(&diagnostic)
            .map_err(std::io::Error::other)
            .and_then(|bytes| std::fs::write(dir.join("resident-failure.json"), bytes));
        if let Err(e) = receipt {
            eprintln!("could not preserve Resident cold failure receipt: {e}");
        }
        message
    })?;
    let close_reopen_ns = report::nanos(reopen);
    {
        let tx = env.begin_read();
        let view = tx.kv(b"kv")?;
        report::validate(w, true, |key| report::bytes(view.get(key)?))?;
        let mut cursor = view.cursor(
            std::ops::Bound::Unbounded,
            std::ops::Bound::Unbounded,
            false,
        )?;
        report::validate_ordered(w, true, || {
            cursor
                .next()
                .transpose()?
                .map(|(key, value)| Ok((key.try_into()?, value.try_into()?)))
                .transpose()
        })?;
    }
    Ok(EngineResult {
        backend: BACKEND,
        versions: serde_json::json!({"ulmdb":"0.1.0","wal_family":"RL2 CompactV2 Borrowed"}),
        settings: serde_json::json!({
            "publication":"Resident CompactV2 Borrowed","experimental":true,
            "read_api":"prepared resident::KvRead","pool_kv_pages":options.pool.kv_pages,
            "pool_tiny_pages":0,"pool_max_reserved_bytes":options.pool.max_reserved_bytes,
            "pool_max_metadata_bytes":options.pool.max_metadata_bytes,
            "pool_max_views":options.pool.max_views,"wal_capacity":options.wal_capacity,
            "graph_read_budget":options.graph_read_budget,
            "auto_collect_below":options.auto_collect_below,
            "configuration_scope":"iteration34 algorithms; pool and WAL enlarged within existing limits for million-key attempt",
            "durability":"normal ordered WAL data and witness fences; no relaxed sync",
            "progress_accounting":"one successful-batch update inside write timer; diagnostics only on failure",
            "backend_fallback":false
        }),
        rows: vec![load, read, update],
        setup_ns,
        close_reopen_ns,
        maintenance: serde_json::json!({
            "automatic_collect_charged_inside_begin_write":true,
            "collect_cache_enabled_after_setup":true,
            "reopen_full_wal_replay_and_graph_audit":true,
            "reopen_collect_cache_disabled":true,"wal_retirement":false
        }),
        engine_stats,
        validation: report::validation(),
        storage_before_close,
    })
}
