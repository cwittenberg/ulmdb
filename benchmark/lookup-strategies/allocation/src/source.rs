use std::{ops::Bound, path::Path};
use ulmdb::{DatabaseKind, Environment, ReadTxn, lookup::Options};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub const VALUE_BYTES: usize = 8;
pub fn value(id: u64) -> [u8; 8] {
    (id ^ 0x7165_a234_c087_1d9b).to_le_bytes()
}

/// Stream fixed-size stack keys/values: there is no retained fixture Vec whose
/// freed capacity could satisfy the later index allocation under RLIMIT_AS.
pub fn create(path: &Path, keys: usize) -> Result<Environment> {
    let env = Environment::create(path)?;
    let mut write = env.begin_write()?;
    write.create_database(b"kv", DatabaseKind::Kv)?;
    let loaded = write.load_sorted_kv(
        b"kv",
        (0..keys as u64).map(|id| Ok((id.to_be_bytes(), value(id)))),
    )?;
    if loaded != keys as u64 {
        return Err("fixture cardinality mismatch".into());
    }
    write.commit()?;
    Ok(env)
}

pub fn verify(read: &ReadTxn, keys: usize) -> Result<()> {
    let native = read.kv(b"kv")?;
    let ordered = read.prepare_lookup(b"kv", Options::Ordered)?;
    if ordered.stats().enumerated || ordered.stats().index_bytes != 0 {
        return Err("Ordered unexpectedly built an accelerator".into());
    }
    for id in [0, keys as u64 / 2, keys as u64 - 1] {
        let k = id.to_be_bytes();
        let v = value(id);
        if native.get(&k)? != Some(v.as_slice()) || ordered.get(&k)? != Some(v.as_slice()) {
            return Err("native/Ordered bytes mismatch".into());
        }
    }
    let missing = (keys as u64 + 7).to_be_bytes();
    if native.get(&missing)?.is_some() || ordered.get(&missing)?.is_some() {
        return Err("missing key unexpectedly present".into());
    }
    // Prove the same native cursor can enumerate every source record before and
    // after the refused allocation; LimitExceeded must not poison source state.
    let mut count = 0u64;
    for pair in native.cursor(Bound::Unbounded, Bound::Unbounded, false)? {
        let (key, actual) = pair?;
        if key != count.to_be_bytes() || actual != value(count) {
            return Err("full native source verification mismatch".into());
        }
        count += 1;
    }
    if count != keys as u64 {
        return Err("full source count mismatch".into());
    }
    Ok(())
}
