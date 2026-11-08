mod backend;
mod strategy;
mod workload;

#[cfg(test)]
mod validation_tests;

use backend::Environment;
use strategy::{HashSideIndex, LookupStrategy};
use workload::{Config, Result, digest};

fn main() -> Result<()> {
    let c = Config::parse()?;
    let rows = workload::records(&c);
    println!(
        "{{\"type\":\"config\",\"schema_version\":1,\"rows\":{},\"queries\":{},\"samples\":{},\"backend\":\"{}\",\"key_kind\":\"{}\",\"value_bytes\":32,\"query_seed\":{},\"hasher\":\"std RandomState (fresh randomized seed per index build)\",\"scope\":\"warm single-threaded prepared lookup; actual file-backed ULMDB snapshot\"}}",
        c.rows, c.queries, c.samples, c.backend, c.key_kind, c.seed
    );
    let env = Environment::create(&c, &rows)?;
    let old = env.snapshot();
    let view = old.view()?;
    let mut builds = Vec::new();
    for _ in 0..2 {
        let (_, ns) = HashSideIndex::build(&view, c.rows)?;
        builds.push(ns);
    }
    let (hash, ns) = HashSideIndex::build(&view, c.rows)?;
    builds.push(ns);
    println!(
        "{{\"type\":\"hash_build\",\"elapsed_ns\":{:?},\"entries\":{},\"capacity\":{},\"copied_key_bytes\":{},\"estimated_extra_bytes\":{},\"memory_scope\":\"SwissTable bucket estimate plus exact copied key payload; excludes allocator rounding and headers, source mappings and pinned-generation retention\",\"sequence\":{},\"commit\":\"{}\",\"timing_scope\":\"complete ordered enumeration, table allocation, key copies and insertion\"}}",
        builds,
        hash.map.len(),
        hash.map.capacity(),
        hash.key_bytes,
        hash.estimate_bytes(),
        old.sequence(),
        old.commit()
    );
    for row in &rows {
        assert_eq!(view.get(&row.key), Some(row.value.as_slice()));
        assert_eq!(hash.get(&row.key), Some(row.value.as_slice()));
        assert_eq!(
            view.get(&row.key).unwrap().as_ptr(),
            hash.get(&row.key).unwrap().as_ptr()
        );
    }
    for workload in ["hit", "miss", "mixed50", "hot90"] {
        let queries = workload::queries(&c, workload);
        let mut checksum = 0u64;
        for key in &queries {
            let expected = rows
                .binary_search_by(|r| r.key.as_slice().cmp(key))
                .ok()
                .map(|i| rows[i].value.as_slice());
            assert_eq!(view.get(key), expected);
            assert_eq!(hash.get(key), expected);
            checksum = checksum.wrapping_add(digest(expected));
        }
        assert_eq!(strategy::timed_native(&view, &queries).1, checksum);
        assert_eq!(strategy::timed(&hash, &queries).1, checksum);
        for sample in 0..c.samples {
            if sample % 2 == 0 {
                emit(
                    "btree",
                    workload,
                    sample,
                    c.queries,
                    strategy::timed_native(&view, &queries),
                    checksum,
                );
                emit(
                    "hash",
                    workload,
                    sample,
                    c.queries,
                    strategy::timed(&hash, &queries),
                    checksum,
                );
            } else {
                emit(
                    "hash",
                    workload,
                    sample,
                    c.queries,
                    strategy::timed(&hash, &queries),
                    checksum,
                );
                emit(
                    "btree",
                    workload,
                    sample,
                    c.queries,
                    strategy::timed_native(&view, &queries),
                    checksum,
                );
            }
        }
    }
    let queries = workload::queries(&c, "hit");
    let scan_queries = &queries[..queries.len().min(2000)];
    let mut expected_scan = 0u64;
    for key in scan_queries {
        let pos = rows
            .binary_search_by(|r| r.key.as_slice().cmp(key))
            .unwrap();
        let expected = rows[pos..]
            .iter()
            .take(100)
            .map(|r| digest(Some(&r.value)))
            .fold(0, u64::wrapping_add);
        assert_eq!(view.scan(key, 100), expected);
        assert_eq!(hash.scan(key, 100), expected);
        expected_scan = expected_scan.wrapping_add(expected);
    }
    for sample in 0..c.samples {
        if sample % 2 == 0 {
            emit(
                "btree",
                "scan100",
                sample,
                scan_queries.len(),
                strategy::timed_scan(&view, scan_queries),
                expected_scan,
            );
            emit(
                "hash_ordered_fallback",
                "scan100",
                sample,
                scan_queries.len(),
                strategy::timed_scan(&hash, scan_queries),
                expected_scan,
            );
        } else {
            emit(
                "hash_ordered_fallback",
                "scan100",
                sample,
                scan_queries.len(),
                strategy::timed_scan(&hash, scan_queries),
                expected_scan,
            );
            emit(
                "btree",
                "scan100",
                sample,
                scan_queries.len(),
                strategy::timed_scan(&view, scan_queries),
                expected_scan,
            );
        }
    }
    // Held read/index pairs must continue borrowing old bytes after publication.
    let replacement = [0xa5; 32];
    env.update(&rows[0].key, &replacement)?;
    let new = env.snapshot();
    assert_ne!(new.commit(), old.commit());
    assert!(new.sequence() > old.sequence());
    let new_view = new.view()?;
    assert_eq!(view.get(&rows[0].key), Some(rows[0].value.as_slice()));
    assert_eq!(hash.get(&rows[0].key), Some(rows[0].value.as_slice()));
    let (new_hash, rebuild_ns) = HashSideIndex::build(&new_view, c.rows)?;
    for (i, row) in rows.iter().enumerate() {
        let expected = if i == 0 {
            replacement.as_slice()
        } else {
            row.value.as_slice()
        };
        assert_eq!(new_view.get(&row.key), Some(expected));
        assert_eq!(new_hash.get(&row.key), Some(expected));
        assert_eq!(hash.get(&row.key), Some(row.value.as_slice()));
    }
    println!(
        "{{\"type\":\"snapshot_validation\",\"passed\":true,\"full_old_and_new_values_checked\":true,\"one_update_full_rebuild_ns\":{},\"old_sequence\":{},\"new_sequence\":{},\"atomic_rebinding\":\"benchmark holds each immutable snapshot-index pair separately; no production publication integration\"}}",
        rebuild_ns,
        old.sequence(),
        new.sequence()
    );
    println!(
        "{{\"type\":\"validation\",\"passed\":true,\"all_records_all_queries_exact_bytes\":true,\"same_borrowed_value_pointers\":true,\"ordered_scan_fallback\":true,\"snapshot_isolation\":true}}"
    );
    Ok(())
}

fn emit(
    strategy: &str,
    workload: &str,
    sample: usize,
    operations: usize,
    timed: (u128, u64),
    expected: u64,
) {
    assert_eq!(timed.1, expected, "timed checksum differs");
    println!(
        "{{\"type\":\"sample\",\"strategy\":\"{strategy}\",\"workload\":\"{workload}\",\"sample\":{sample},\"operations\":{operations},\"elapsed_ns\":{},\"ns_per_op\":{:.6},\"checksum\":{}}}",
        timed.0,
        timed.0 as f64 / operations as f64,
        timed.1
    );
}
