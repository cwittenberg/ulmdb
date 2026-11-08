#[path = "../../src/contract.rs"]
mod contract;
#[path = "../../src/dataset.rs"]
mod dataset;
mod strategy;
#[path = "../../src/timing.rs"]
mod timing;
use dataset::KeyKind;
use serde_json::json;
use std::{path::PathBuf, time::Instant};
use strategy::{Blessed, Lookup};

fn execute() -> Result<(), Box<dyn std::error::Error>> {
    let mut keys = 0usize;
    let mut query_count = 0usize;
    let mut samples = 0usize;
    let mut seed = 0u64;
    let mut kind = KeyKind::Fixed16;
    let mut fixture = PathBuf::new();
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or("missing option value")?;
        match flag.as_str() {
            "--fixture" => fixture = value.into(),
            "--keys" => keys = value.parse()?,
            "--queries" => query_count = value.parse()?,
            "--samples" => samples = value.parse()?,
            "--seed" => seed = value.parse()?,
            "--kind" => {
                kind = match value.as_str() {
                    "fixed16" => KeyKind::Fixed16,
                    "hostname" => KeyKind::Hostname,
                    _ => return Err("invalid key kind".into()),
                }
            }
            _ => return Err(format!("unknown flag {flag}").into()),
        }
    }
    if keys == 0 || query_count == 0 || samples == 0 {
        return Err("positive counts required".into());
    }
    let read = Instant::now();
    let bytes = std::fs::read(&fixture)?;
    let read_ns = read.elapsed().as_nanos();
    let decode = Instant::now();
    let count = u64::from_le_bytes(bytes.get(..8).ok_or("short fixture")?.try_into()?);
    if count != keys as u64 {
        return Err("fixture record count mismatch".into());
    }
    let mut offset = 8usize;
    let mut records = Vec::with_capacity(keys);
    for _ in 0..keys {
        let length = u64::from_le_bytes(
            bytes
                .get(offset..offset + 8)
                .ok_or("short key length")?
                .try_into()?,
        ) as usize;
        offset += 8;
        let key = bytes
            .get(offset..offset + length)
            .ok_or("short key bytes")?;
        offset += length;
        let value = bytes.get(offset..offset + 16).ok_or("short value bytes")?;
        offset += 16;
        records.push((key, value));
    }
    if offset != bytes.len() {
        return Err("trailing fixture bytes".into());
    }
    let decode_ns = decode.elapsed().as_nanos();
    let expected = dataset::records(kind, keys);
    assert_eq!(
        records,
        expected
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect::<Vec<_>>()
    );
    drop(expected);
    let build = Instant::now();
    let mut map = bbtree::unsync::BBTreeMap::new();
    for (id, (key, _)) in records.iter().enumerate() {
        assert!(map.insert(key.to_vec(), id).is_none());
    }
    let build_ns = build.elapsed().as_nanos();
    let node_bytes = strategy::node_bytes(&map);
    let key_bytes: usize = map.iter().map(|(k, _)| k.capacity()).sum();
    let candidate = Blessed {
        map,
        records: &records,
    };
    let strategies: [&dyn Lookup; 1] = [&candidate];
    let all_queries: Vec<_> = (0..keys)
        .flat_map(|id| [kind.key(id, false), kind.key(id, true)])
        .collect();
    timing::verify(&strategies, &all_queries, &records);
    let mut measurements = Vec::new();
    for (workload, misses) in [
        ("hit", 0),
        ("miss", 100),
        ("mixed_50_50", 50),
        ("range64", 50),
    ] {
        let queries = dataset::queries(kind, query_count, keys, seed, misses);
        timing::verify(&strategies, &queries, &records);
        measurements.extend(timing::measure(&strategies, &queries, samples, workload));
    }
    println!(
        "{}",
        serde_json::to_string(
            &json!({"key_kind":kind.name(),"validation_passed":true,"bbtree_version":"0.2.0","build":{"fixture_read_ns":read_ns as u64,"fixture_decode_ns":decode_ns as u64,"bbtree_insert_copied_keys_ns":build_ns as u64,"complete_helper_build_ns":(read_ns+decode_ns+build_ns) as u64},"memory":{"fixture_bytes":bytes.capacity(),"borrowed_record_vector_bytes":records.capacity()*std::mem::size_of::<strategy::Record<'_>>(),"bbtree_requested_node_bytes":node_bytes,"bbtree_owned_key_capacity_bytes":key_bytes,"bbtree_requested_accelerator_bytes_excluding_fixture":node_bytes+key_bytes+records.capacity()*std::mem::size_of::<strategy::Record<'_>>(),"excludes":"allocator headers, fixture transfer process cost, and benchmark query storage"},"measurements":measurements})
        )?
    );
    Ok(())
}
fn main() {
    if let Err(error) = execute() {
        eprintln!("bbtree helper failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_bbtree_matches_full_bytes_and_sorted_ranges() {
        for kind in [KeyKind::Fixed16, KeyKind::Hostname] {
            let owned = dataset::records(kind, 257);
            let records: Vec<_> = owned
                .iter()
                .map(|(k, v)| (k.as_slice(), v.as_slice()))
                .collect();
            let mut map = bbtree::unsync::BBTreeMap::new();
            for (id, (key, _)) in records.iter().enumerate() {
                map.insert(key.to_vec(), id);
            }
            assert!(strategy::node_bytes(&map) > 0);
            let candidate = Blessed {
                map,
                records: &records,
            };
            let queries: Vec<_> = (0..257)
                .flat_map(|id| [kind.key(id, false), kind.key(id, true)])
                .collect();
            timing::verify(&[&candidate], &queries, &records);
        }
    }
}
