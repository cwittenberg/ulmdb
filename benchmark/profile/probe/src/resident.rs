use super::*;
use ulmdb::{
    DatabaseKind,
    resident::{CollectCacheLimits, Environment, Options, RedoEncoding, RedoFormat},
};

fn options(c: &Config) -> Options {
    let mut o = Options::default();
    o.pool.kv_pages = if c.keys > 8192 { 8192 } else { 1024 };
    // Same small-pool shape as the full suite; load is KV-only like millions.
    o.pool.tiny_pages = if c.keys > 8192 { 0 } else { 1024 };
    o.pool.max_reserved_bytes = 64 << 20;
    o.pool.max_metadata_bytes = 64 << 20;
    o.auto_collect_below = o.pool.kv_pages / 4;
    o.wal_capacity = 256 << 20;
    o.graph_read_budget = 1_000_000_000;
    o
}
fn validate(env: &Environment, w: &Workload) -> Result<()> {
    let tx = env.begin_read();
    let view = tx.kv(b"kv")?;
    validate_points(w, |key| bytes(view.get(key)?))?;
    let mut cursor = view.cursor(
        std::ops::Bound::Unbounded,
        std::ops::Bound::Unbounded,
        false,
    )?;
    validate_ordered(w, || {
        cursor
            .next()
            .transpose()?
            .map(|(key, value)| Ok((key.try_into()?, value.try_into()?)))
            .transpose()
    })
}
fn snapshot_check(env: &Environment, w: &Workload) -> Result<()> {
    let key = w.fixture[0].key;
    let expected = w.expected[0];
    let replacement = (u64::from_be_bytes(expected) ^ u64::MAX).to_be_bytes();
    {
        let old = env.begin_read();
        let old_view = old.kv(b"kv")?;
        let borrowed = old_view.get(&key)?.ok_or("snapshot missing key")?;
        let mut write = env.begin_write()?;
        write.put(b"kv", &key, &replacement)?;
        write.commit()?;
        if borrowed != expected || old_view.get(&key)? != Some(expected.as_slice()) {
            return Err("pinned Resident snapshot changed".into());
        }
        if env.begin_read().get(b"kv", &key)? != Some(replacement.as_slice()) {
            return Err("new Resident snapshot did not publish".into());
        }
    }
    let mut write = env.begin_write()?;
    write.put(b"kv", &key, &expected)?;
    write.commit()?;
    Ok(())
}
pub(super) fn run(c: &Config, w: &Workload, clock: Instant) -> Result<Value> {
    let options = options(c);
    let env = Environment::create_with_encoding(
        &c.dir,
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
        return Err("Resident probe missing expected benchmark features".into());
    }
    {
        let mut write = env.begin_write()?;
        write.create_database(b"kv", DatabaseKind::Kv)?;
        write.commit()?;
    }
    env.enable_collect_cache(CollectCacheLimits::default())?;
    if c.mode == Mode::Smallwrites {
        mark(clock, "fixture_load_begin");
        for rows in w.fixture.chunks(1000) {
            let mut tx = env.begin_write()?;
            for row in rows {
                tx.put(b"kv", &row.key, &row.value)?;
            }
            tx.commit()?;
        }
        mark(clock, "fixture_load_end");
    }
    let pool_before = env.pool_stats()?;
    let wal_before = env.wal_bytes();
    let lsn_before = env.physical_lsn();
    let mut measurement = Measurement::start(c, clock);
    for rows in w.measured.chunks(c.batch) {
        let all = Instant::now();
        let start = Instant::now();
        let mut tx = env.begin_write()?;
        let begin = start.elapsed().as_nanos();
        let start = Instant::now();
        for row in rows {
            tx.put(b"kv", &row.key, &row.value)?;
        }
        let mutation = start.elapsed().as_nanos();
        let start = Instant::now();
        tx.commit()?;
        let commit = start.elapsed().as_nanos();
        measurement.record(
            rows.len(),
            begin,
            mutation,
            commit,
            all.elapsed().as_nanos(),
        );
    }
    let measured = measurement.finish(w.measured.len());
    let pool_after = env.pool_stats()?;
    let engine_stats = json!({"wal_before":wal_before,"wal_after":env.wal_bytes(),"physical_lsn_before":lsn_before,"physical_lsn_after":env.physical_lsn(),
        "pool_allocations_before":pool_before.allocations,"pool_allocations_after":pool_after.allocations,
        "pool_reused_before":pool_before.reused_pages,"pool_reused_after":pool_after.reused_pages,
        "pool_free_before":pool_before.free_pages,"pool_free_after":pool_after.free_pages});
    mark(clock, "validation_begin");
    validate(&env, w)?;
    snapshot_check(&env, w)?;
    drop(env);
    mark(clock, "reopen_begin");
    let env = Environment::open_with_encoding(
        &c.dir,
        options,
        RedoFormat::CompactV2,
        RedoEncoding::Borrowed,
    )?;
    mark(clock, "reopen_complete");
    validate(&env, w)?;
    Ok(
        json!({"timings":measured,"engine_stats":engine_stats,"settings":{"backend":"Resident CompactV2 Borrowed","pool_kv_pages":options.pool.kv_pages,
        "pool_tiny_pages":options.pool.tiny_pages,"wal_capacity":options.wal_capacity,"auto_collect_below":options.auto_collect_below,
        "collect_cache":true,"normal_ordered_data_and_witness_fences":true,"feature_profile":"same nine features as accepted Resident suites",
        "read_layout_unchanged":true,"full_replay_and_cold_graph_audit":true}}),
    )
}
