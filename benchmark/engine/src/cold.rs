use crate::{Result, backend, dataset::Dataset, native, oracle::Answer};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Bound,
    path::Path,
    time::Instant,
};
use ulmdb::Address;
#[derive(Serialize)]
pub struct Cold {
    pub environment_drop_ns: u64,
    pub open_ns: u64,
    pub experimental_reenable_ns: Option<u64>,
    pub validation_ns: u64,
    pub complete_cold_check_ns: u64,
    pub kv_rows: usize,
    pub ip_queries_per_reader: usize,
    pub old_reader_survived_environment_drop: bool,
    pub escaped_value_pointer_unchanged: bool,
    pub reopened_value_uses_different_live_mapping: bool,
    pub scope: &'static str,
}
pub fn check(e: backend::Environment, path: &Path, d: &Dataset) -> Result<Cold> {
    let total = Instant::now();
    let old = e.begin_read();
    let kv = old.kv(b"kv")?;
    let mut rows = BTreeMap::new();
    for row in kv.cursor(Bound::Unbounded, Bound::Unbounded, false)? {
        let (k, v) = row?;
        if rows.insert(k.to_vec(), v.to_vec()).is_some() {
            return Err("cold duplicate key".into());
        }
    }
    let expected_keys: BTreeSet<_> = d.kv.iter().map(|(k, _)| k.to_vec()).collect();
    if rows.len() != d.kv.len() || rows.keys().cloned().collect::<BTreeSet<_>>() != expected_keys {
        return Err("cold cursor key set mismatch".into());
    }
    let first = rows.first_key_value().ok_or("cold empty corpus")?.0;
    let held = old.get(b"kv", first)?.ok_or("cold missing old value")?;
    let ptr = held.as_ptr();
    let sequence = old.sequence();
    let commit = old.commit_identity();
    let at = Instant::now();
    drop(e);
    let environment_drop_ns = at.elapsed().as_nanos().try_into()?;
    let at = Instant::now();
    let e = backend::open(path)?;
    let open_ns = at.elapsed().as_nanos().try_into()?;
    #[cfg(feature = "resident")]
    let experimental_reenable_ns = {
        let at = Instant::now();
        backend::after_setup(&e)?;
        Some(at.elapsed().as_nanos().try_into()?)
    };
    #[cfg(not(feature = "resident"))]
    let experimental_reenable_ns = None;
    let at = Instant::now();
    let new = e.begin_read();
    if new.sequence() != sequence || new.commit_identity() != commit {
        return Err("cold selected identity mismatch".into());
    }
    if new
        .get(b"kv", first)?
        .ok_or("cold missing new value")?
        .as_ptr()
        == ptr
    {
        return Err("cold reused mapping still pinned by old reader".into());
    }
    for (k, v) in &rows {
        if old.get(b"kv", k)? != Some(v.as_slice()) || new.get(b"kv", k)? != Some(v.as_slice()) {
            return Err("cold old/new KV mismatch".into());
        }
    }
    for reader in [&old, &new] {
        let v4 = reader.prefix_index(b"ipv4")?;
        let v6 = reader.prefix_index(b"ipv6")?;
        for (&q, expected) in d.ipv4_queries.values.iter().zip(&d.ipv4_queries.expected) {
            if v4.longest_match(Address::V4(q))?.map(|(p, v)| Answer {
                length: p.length(),
                value: native::decode(v),
            }) != *expected
            {
                return Err("cold IPv4 mismatch".into());
            }
        }
        for (&q, expected) in d.ipv6_queries.values.iter().zip(&d.ipv6_queries.expected) {
            if v6.longest_match(Address::V6(q))?.map(|(p, v)| Answer {
                length: p.length(),
                value: native::decode(v),
            }) != *expected
            {
                return Err("cold IPv6 mismatch".into());
            }
        }
    }
    if held != rows[first].as_slice() || held.as_ptr() != ptr {
        return Err("escaped old bytes changed".into());
    }
    Ok(Cold {
        environment_drop_ns,
        open_ns,
        experimental_reenable_ns,
        validation_ns: at.elapsed().as_nanos().try_into()?,
        complete_cold_check_ns: total.elapsed().as_nanos().try_into()?,
        kv_rows: rows.len(),
        ip_queries_per_reader: d.ipv4_queries.values.len() + d.ipv6_queries.values.len(),
        old_reader_survived_environment_drop: true,
        escaped_value_pointer_unchanged: true,
        reopened_value_uses_different_live_mapping: true,
        scope: "warm latencies exclude this complete cold check; total includes snapshot cursor capture, Env drop, full open/recovery, experimental re-enable and full old/new KV/IP validation; open is reported separately",
    })
}
