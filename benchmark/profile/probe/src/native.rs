use super::*;
use ulmdb_lmdb_baseline::{Database, DatabaseKind, Environment};
fn validate(env: &Environment, db: &Database<'_>, w: &Workload) -> Result<()> {
    let tx = env.begin_read()?;
    let view = tx.database(db)?;
    validate_points(w, |key| bytes(view.get(key)?))?;
    let mut cursor = tx.cursor(db)?;
    let mut first = true;
    validate_ordered(w, || {
        let pair = if first {
            first = false;
            cursor.first()?
        } else {
            cursor.next_pair()?
        };
        pair.map(|(key, value)| Ok((key.try_into()?, value.try_into()?)))
            .transpose()
    })
}
pub(super) fn run(c: &Config, w: &Workload, clock: Instant) -> Result<Value> {
    std::fs::create_dir(&c.dir)?;
    let env = Environment::open(&c.dir, 1 << 30)?;
    if env.flags() != 0 {
        return Err("LMDB normal durable flags0 required".into());
    }
    let db = {
        let mut tx = env.begin_write()?;
        let db = tx.open_database("kv", DatabaseKind::Ordinary, true)?;
        tx.commit()?;
        db
    };
    if c.mode == Mode::Smallwrites {
        mark(clock, "fixture_load_begin");
        for rows in w.fixture.chunks(1000) {
            let mut tx = env.begin_write()?;
            for row in rows {
                tx.put(&db, &row.key, &row.value)?;
            }
            tx.commit()?;
        }
        mark(clock, "fixture_load_end");
    }
    let mut measurement = Measurement::start(c, clock);
    for rows in w.measured.chunks(c.batch) {
        let all = Instant::now();
        let start = Instant::now();
        let mut tx = env.begin_write()?;
        let begin = start.elapsed().as_nanos();
        let start = Instant::now();
        for row in rows {
            tx.put(&db, &row.key, &row.value)?;
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
    mark(clock, "validation_begin");
    validate(&env, &db, w)?;
    drop(db);
    drop(env);
    mark(clock, "reopen_begin");
    let env = Environment::open(&c.dir, 1 << 30)?;
    let db = {
        let mut tx = env.begin_write()?;
        let db = tx.open_database("kv", DatabaseKind::Ordinary, false)?;
        tx.commit()?;
        db
    };
    mark(clock, "reopen_complete");
    validate(&env, &db, w)?;
    Ok(
        json!({"timings":measured,"settings":{"backend":"pinned native LMDB","version":ulmdb_lmdb_baseline::version(),"flags":env.flags(),"map_bytes":1_u64 << 30}}),
    )
}
