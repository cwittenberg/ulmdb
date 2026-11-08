use crate::{
    Result,
    report::{self, EngineResult},
    workload::{Config, Workload},
};
use std::{path::Path, time::Instant};
use ulmdb_lmdb_baseline::{DatabaseKind, Environment};

pub fn run(dir: &Path, c: &Config, w: &Workload) -> Result<EngineResult> {
    let setup = Instant::now();
    std::fs::create_dir(dir)?;
    let env = Environment::open(dir, c.lmdb_map_bytes)?;
    if env.flags() != 0 {
        return Err("LMDB requires normal durable flags 0".into());
    }
    let db = {
        let mut tx = env.begin_write()?;
        let db = tx.open_database("kv", DatabaseKind::Ordinary, true)?;
        tx.commit()?;
        db
    };
    let setup_ns = report::nanos(setup);
    let load = report::writes("lmdb", "load", &w.load, c, |rows| {
        let mut tx = env.begin_write()?;
        for r in rows {
            tx.put(&db, &r.key, &r.value)?;
        }
        tx.commit()?;
        Ok(())
    })?;
    let read = {
        let tx = env.begin_read()?;
        let view = tx.database(&db)?;
        report::validate(w, false, |key| report::bytes(view.get(key)?))?;
        report::reads("lmdb", w, |key| report::bytes(view.get(key)?))?
    };
    let update = report::writes("lmdb", "update", &w.updates, c, |rows| {
        let mut tx = env.begin_write()?;
        for r in rows {
            tx.put(&db, &r.key, &r.value)?;
        }
        tx.commit()?;
        Ok(())
    })?;
    {
        let tx = env.begin_read()?;
        let view = tx.database(&db)?;
        report::validate(w, true, |key| report::bytes(view.get(key)?))?;
        let mut cursor = tx.cursor(&db)?;
        let mut first = true;
        report::validate_ordered(w, true, || {
            let pair = if first {
                first = false;
                cursor.first()?
            } else {
                cursor.next_pair()?
            };
            pair.map(|(key, value)| Ok((key.try_into()?, value.try_into()?)))
                .transpose()
        })?;
    }
    let storage_before_close = report::storage(dir)?;
    let reopen = Instant::now();
    drop(db);
    drop(env);
    let env = Environment::open(dir, c.lmdb_map_bytes)?;
    let db = {
        let mut tx = env.begin_write()?;
        let db = tx.open_database("kv", DatabaseKind::Ordinary, false)?;
        tx.commit()?;
        db
    };
    let close_reopen_ns = report::nanos(reopen);
    {
        let tx = env.begin_read()?;
        let view = tx.database(&db)?;
        report::validate(w, true, |key| report::bytes(view.get(key)?))?;
        let mut cursor = tx.cursor(&db)?;
        let mut first = true;
        report::validate_ordered(w, true, || {
            let pair = if first {
                first = false;
                cursor.first()?
            } else {
                cursor.next_pair()?
            };
            pair.map(|(key, value)| Ok((key.try_into()?, value.try_into()?)))
                .transpose()
        })?;
    }
    Ok(EngineResult {
        backend: "LMDB local pinned native library",
        versions: serde_json::json!({"lmdb":ulmdb_lmdb_baseline::version(),"source_commit":"c279e0477c713076893e8c2b1a6337e421c15867","native_build_info_toml":ulmdb_lmdb_baseline::BUILD_INFO}),
        settings: serde_json::json!({"flags":env.flags(),"map_bytes":c.lmdb_map_bytes,"database":"ordinary bytewise keys","read_api":"prepared ReadDatabase"}),
        rows: vec![load, read, update],
        setup_ns,
        close_reopen_ns,
        maintenance: serde_json::json!({"explicit_maintenance":false}),
        engine_stats: serde_json::json!({}),
        validation: report::validation(),
        storage_before_close,
    })
}
