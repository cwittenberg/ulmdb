use crate::{
    data::{Config, Record},
    observe::{Report, file_bytes},
    protocol::{Engine, Lookup, Result, Snapshot},
    reads, writes,
};
use serde_json::json;
use std::{collections::BTreeMap, time::Instant};

pub fn run(
    engine: &impl Engine,
    c: &Config,
    rows: &mut [Record],
    report: &mut Report,
) -> Result<()> {
    let mut revision = 1;
    let before = writes::phase(engine, c, rows, "unpinned_before", &mut revision, report)?;
    writes::apply_model(rows, before);
    let start = Instant::now();
    let maintenance = engine.before_pin()?;
    report.life(
        "maintenance_before_pin",
        start.elapsed().as_nanos(),
        maintenance,
    );
    let start = Instant::now();
    let old = engine.snapshot()?;
    report.life("begin_snapshot", start.elapsed().as_nanos(), old.identity());
    report.rss("before_prepare");
    let start = Instant::now();
    let prepared = old.prepare(c)?;
    report.life(
        "prepare_lookup",
        start.elapsed().as_nanos(),
        prepared.stats(),
    );
    report.rss("after_prepare");
    let stats = prepared.stats();
    if c.arm == "hash" {
        assert_eq!(stats["entries"], c.keys);
        assert!(stats["index_bytes"].as_u64().unwrap() <= 512 << 20);
    } else {
        assert_eq!(stats["index_bytes"], 0);
        if let Some(enumerated) = stats.get("enumerated") {
            assert_eq!(enumerated, false);
        }
    }
    reads::verify(&prepared, rows, &BTreeMap::new())?;
    report.rss("after_full_validation");
    reads::measure(&prepared, c, rows, report)?;
    let changes = writes::phase(engine, c, rows, "pinned", &mut revision, report)?;
    report.rss("after_pinned_writes");
    // Every old value still belongs to the exact retained snapshot.
    reads::verify(&prepared, rows, &BTreeMap::new())?;
    let start = Instant::now();
    let new = engine.snapshot()?;
    let new_lookup = new.prepare(c)?;
    report.life("rebuild_after_pinned_writes",start.elapsed().as_nanos(),json!({"snapshot":new.identity(),"index":new_lookup.stats(),"scope":"begin new snapshot plus complete new lookup preparation"}));
    report.rss("two_generations");
    reads::verify(&new_lookup, rows, &changes)?;
    let start = Instant::now();
    drop(new_lookup);
    report.life("drop_new_lookup", start.elapsed().as_nanos(), json!({}));
    drop(new);
    let start = Instant::now();
    drop(prepared);
    report.life(
        "drop_old_lookup",
        start.elapsed().as_nanos(),
        json!({"note":"allocator may retain freed pages; observe RSS separately"}),
    );
    report.rss("after_old_lookup_drop");
    let start = Instant::now();
    drop(old);
    report.life(
        "release_old_snapshot",
        start.elapsed().as_nanos(),
        json!({}),
    );
    report.rss("after_old_snapshot_release");
    writes::apply_model(rows, changes);
    let after = writes::phase(engine, c, rows, "unpinned_after", &mut revision, report)?;
    writes::apply_model(rows, after);
    let latest = engine.snapshot()?;
    let start = Instant::now();
    let lookup = latest.prepare(c)?;
    report.life(
        "prepare_final_validation",
        start.elapsed().as_nanos(),
        lookup.stats(),
    );
    let start = Instant::now();
    reads::verify(&lookup, rows, &BTreeMap::new())?;
    report.life(
        "final_validation",
        start.elapsed().as_nanos(),
        json!({"all_old_new_final_bytes_verified":true,"files":file_bytes(&c.directory)}),
    );
    Ok(())
}
pub fn reopened(
    engine: &impl Engine,
    c: &Config,
    rows: &[Record],
    report: &mut Report,
) -> Result<()> {
    let snapshot = engine.snapshot()?;
    let start = Instant::now();
    let lookup = snapshot.prepare(c)?;
    report.life(
        "prepare_after_reopen",
        start.elapsed().as_nanos(),
        lookup.stats(),
    );
    reads::verify(&lookup, rows, &BTreeMap::new())?;
    report.rss("after_reopen_full_validation");
    Ok(())
}
