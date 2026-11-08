use crate::{
    data::{self, Config, Record},
    observe::Report,
    protocol::{Lookup, Result},
};
use serde_json::json;
use std::{hint::black_box, ops::Bound, time::Instant};

pub fn verify(
    reader: &impl Lookup,
    rows: &[Record],
    updates: &std::collections::BTreeMap<usize, [u8; 32]>,
) -> Result<()> {
    for (id, row) in rows.iter().enumerate() {
        let value = updates.get(&id).unwrap_or(&row.value);
        assert_eq!(
            reader.get(&row.key)?,
            Some(value.as_slice()),
            "full-record point mismatch"
        );
    }
    let mut count = 0;
    reader.visit(
        Bound::Unbounded,
        Bound::Unbounded,
        false,
        usize::MAX,
        |k, v| {
            let row = &rows[count];
            assert_eq!(k, row.key);
            assert_eq!(v, updates.get(&count).unwrap_or(&row.value));
            count += 1;
        },
    )?;
    assert_eq!(count, rows.len());
    Ok(())
}
fn point_run(reader: &impl Lookup, queries: &[Vec<u8>]) -> Result<u64> {
    let mut sum = 0u64;
    for key in queries {
        let d = reader
            .get(black_box(key))?
            .map_or(0x517c_c1b7_2722_0a95, |v| data::digest(black_box(v)));
        sum = sum.wrapping_add(black_box(d));
    }
    Ok(black_box(sum))
}
pub fn measure(
    reader: &impl Lookup,
    c: &Config,
    rows: &[Record],
    report: &mut Report,
) -> Result<()> {
    for workload in ["hit", "miss", "mixed50", "hot90"] {
        let queries = data::queries(c, workload);
        let mut expected = 0u64;
        for key in &queries {
            let found = rows
                .binary_search_by(|r| r.key.as_slice().cmp(key))
                .ok()
                .map(|i| rows[i].value.as_slice());
            assert_eq!(reader.get(key)?, found);
            expected = expected.wrapping_add(found.map_or(0x517c_c1b7_2722_0a95, data::digest));
        }
        assert_eq!(point_run(reader, &queries)?, expected);
        report.rss(&format!("warm_{workload}"));
        for sample in 0..c.samples {
            let start = Instant::now();
            let sum = point_run(reader, &queries)?;
            let ns = start.elapsed().as_nanos();
            assert_eq!(sum, expected);
            report.samples.push(json!({"phase":"read","workload":workload,"sample":sample,"operations":queries.len(),"elapsed_ns":ns,"ns_per_op":ns as f64/queries.len() as f64,"checksum":sum}));
        }
    }
    let queries = data::queries(c, "mixed50");
    let queries = &queries[..queries.len().min(1024)];
    for workload in [
        "scan_forward100",
        "scan_reverse100",
        "range_forward64",
        "range_reverse64",
    ] {
        let specifications: Vec<_> = queries.iter().map(|q| spec(rows, q, workload)).collect();
        let mut expected = 0u64;
        for s in &specifications {
            let mut actual = Vec::new();
            reader.visit(s.lower, s.upper, s.reverse, s.limit, |k, v| {
                actual.push((k.to_vec(), v.to_vec()))
            })?;
            let selected: Vec<_> = if s.reverse {
                rows[s.start..s.end].iter().rev().take(s.limit).collect()
            } else {
                rows[s.start..s.end].iter().take(s.limit).collect()
            };
            assert_eq!(actual.len(), selected.len());
            let mut one = 0u64;
            for ((k, v), row) in actual.iter().zip(selected) {
                assert_eq!(k, &row.key);
                assert_eq!(v, &row.value);
                one = one.rotate_left(7) ^ data::pair_digest(k, v);
            }
            expected = expected.wrapping_add(one);
        }
        assert_eq!(scan_run(reader, &specifications)?, expected);
        for sample in 0..c.samples {
            let start = Instant::now();
            let sum = scan_run(reader, &specifications)?;
            let ns = start.elapsed().as_nanos();
            assert_eq!(sum, expected);
            report.samples.push(json!({"phase":"read","workload":workload,"sample":sample,"operations":queries.len(),"elapsed_ns":ns,"ns_per_op":ns as f64/queries.len() as f64,"checksum":sum,"unit":"one cursor seek and up to100 or64 full key/value rows"}));
        }
    }
    Ok(())
}
#[derive(Clone, Copy)]
struct Spec<'a> {
    lower: Bound<&'a [u8]>,
    upper: Bound<&'a [u8]>,
    reverse: bool,
    limit: usize,
    start: usize,
    end: usize,
}
fn spec<'a>(rows: &'a [Record], q: &'a [u8], workload: &str) -> Spec<'a> {
    let reverse = workload.contains("reverse");
    if workload.starts_with("scan") {
        if reverse {
            let end = rows.partition_point(|r| r.key.as_slice() <= q);
            Spec {
                lower: Bound::Unbounded,
                upper: Bound::Included(q),
                reverse,
                limit: 100,
                start: 0,
                end,
            }
        } else {
            let start = rows.partition_point(|r| r.key.as_slice() < q);
            Spec {
                lower: Bound::Included(q),
                upper: Bound::Unbounded,
                reverse,
                limit: 100,
                start,
                end: rows.len(),
            }
        }
    } else {
        let start = rows.partition_point(|r| r.key.as_slice() <= q);
        let end = (start + 64).min(rows.len());
        let upper = if start < end {
            Bound::Included(rows[end - 1].key.as_slice())
        } else {
            Bound::Unbounded
        };
        Spec {
            lower: Bound::Excluded(q),
            upper,
            reverse,
            limit: 64,
            start,
            end,
        }
    }
}
fn scan_run(reader: &impl Lookup, specifications: &[Spec<'_>]) -> Result<u64> {
    let mut sum = 0u64;
    for s in specifications {
        let mut one = 0u64;
        reader.visit(
            black_box(s.lower),
            black_box(s.upper),
            s.reverse,
            s.limit,
            |k, v| one = one.rotate_left(7) ^ data::pair_digest(black_box(k), black_box(v)),
        )?;
        sum = sum.wrapping_add(black_box(one));
    }
    Ok(black_box(sum))
}
