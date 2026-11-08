use crate::{
    Result,
    report::{self, EngineResult},
    workload::{Config, Workload},
};
use std::{path::Path, time::Instant};
use ulmdb::{DatabaseKind, Environment};

pub fn run(dir: &Path, c: &Config, w: &Workload) -> Result<EngineResult> {
    let setup = Instant::now();
    let env = Environment::create(dir)?;
    if env.is_range_backed() || env.stats().format_minor != 2 {
        return Err("ULMDB public default is not the expected Legacy format 2".into());
    }
    {
        let mut tx = env.begin_write()?;
        tx.create_database(b"kv", DatabaseKind::Kv)?;
        tx.commit()?;
    }
    let setup_ns = report::nanos(setup);
    let load = report::writes("ulmdb", "load", &w.load, c, |rows| {
        let mut tx = env.begin_write()?;
        for r in rows {
            tx.put(b"kv", &r.key, &r.value)?;
        }
        tx.commit()?;
        Ok(())
    })?;
    let read = {
        let tx = env.begin_read();
        let view = tx.kv(b"kv")?;
        report::validate(w, false, |key| report::bytes(view.get(key)?))?;
        report::reads("ulmdb", w, |key| report::bytes(view.get(key)?))?
    };
    let update = report::writes("ulmdb", "update", &w.updates, c, |rows| {
        let mut tx = env.begin_write()?;
        for r in rows {
            tx.put(b"kv", &r.key, &r.value)?;
        }
        tx.commit()?;
        Ok(())
    })?;
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
    let stats = env.stats();
    let engine_stats = serde_json::json!({"format_minor":stats.format_minor,"sequence":stats.sequence,"segments":stats.segments,"segment_bytes_including_history":stats.segment_bytes});
    let storage_before_close = report::storage(dir)?;
    let reopen = Instant::now();
    drop(env);
    let env = Environment::open(dir)?;
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
        backend: "ulmdb::Environment default Legacy",
        versions: serde_json::json!({"ulmdb":"0.1.0","format_minor":2}),
        settings: serde_json::json!({"publication":"Legacy","default_features":true,"reclamation":"none during workload","read_api":"prepared KvRead"}),
        rows: vec![load, read, update],
        setup_ns,
        close_reopen_ns,
        maintenance: serde_json::json!({"explicit_maintenance":false,"historical_segments_retained":true}),
        engine_stats,
        validation: report::validation(),
        storage_before_close,
    })
}
