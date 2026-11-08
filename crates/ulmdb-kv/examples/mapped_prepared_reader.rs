//! Same-binary warm mapped-store A/B experiment, not LMDB qualification.
//! Run pinned to one CPU: mapped_prepared_reader OUTPUT_DIRECTORY [queries] [rounds]
//! One invocation covers disk/tmpfs, leaf/height-three roots, mixed/hits/misses.
use std::{
    fs,
    hint::black_box,
    path::{Path, PathBuf},
    time::Instant,
};
use ulmdb_core::storage::Reference;
use ulmdb_kv::{Reader, get, put};
use ulmdb_storage::{Arena, SegmentTable, create_layout};

fn random(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}
fn value(n: u64) -> u64 {
    n.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(17)
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Outcome {
    checksum: u64,
    hits: u64,
}
fn add(result: &mut Outcome, value: Option<&[u8]>) {
    if let Some(bytes) = black_box(value) {
        result.checksum = result
            .checksum
            .wrapping_add(u64::from_le_bytes(bytes.try_into().expect("stored u64")));
        result.hits += 1;
    }
}
#[inline(never)]
fn baseline(store: &SegmentTable, root: Reference, queries: &[[u8; 8]]) -> Outcome {
    let mut result = Outcome {
        checksum: 0,
        hits: 0,
    };
    for key in queries {
        add(
            &mut result,
            get(store, root, black_box(key)).expect("baseline read"),
        );
    }
    result
}
#[inline(never)]
fn prepared(reader: &Reader<'_, SegmentTable>, queries: &[[u8; 8]]) -> Outcome {
    let mut result = Outcome {
        checksum: 0,
        hits: 0,
    };
    for key in queries {
        add(
            &mut result,
            reader.get(black_box(key)).expect("prepared read"),
        );
    }
    result
}
fn time(f: impl FnOnce() -> Outcome) -> (u128, Outcome) {
    let start = Instant::now();
    let result = f();
    (start.elapsed().as_nanos(), result)
}
fn queries(records: usize, count: usize, mode: &str) -> (Vec<[u8; 8]>, Outcome) {
    let mut seed = 0x243f_6a88_85a3_08d3;
    let mut queries = Vec::with_capacity(count);
    let mut expected = Outcome {
        checksum: 0,
        hits: 0,
    };
    for _ in 0..count {
        let n = random(&mut seed) % records as u64;
        let miss = match mode {
            "hits" => false,
            "misses" => true,
            "mixed" => random(&mut seed).is_multiple_of(4),
            _ => unreachable!(),
        };
        queries.push((n * 2 + u64::from(miss)).to_be_bytes());
        if !miss {
            expected.checksum = expected.checksum.wrapping_add(value(n));
            expected.hits += 1;
        }
    }
    (queries, expected)
}
fn hex(hash: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(hash.len() * 2);
    for b in hash {
        write!(s, "{b:02x}").unwrap();
    }
    s
}
fn run_data(parent: &Path, filesystem: &str, records: usize, count: usize, rounds: usize) {
    let path = parent.join(format!("records-{records}"));
    create_layout(&path).expect("new segment directory");
    let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
    let mut root = Reference::NULL;
    for n in 0..records as u64 {
        root = put(
            &mut arena,
            root,
            &(n * 2).to_be_bytes(),
            &value(n).to_le_bytes(),
        )
        .unwrap();
    }
    let (sealed, entry) = arena.seal(&path).expect("seal real immutable segment");
    let entry = entry.expect("nonempty segment");
    drop(sealed);
    // Reload the independently sealed file through its checked read-only mapping.
    let store =
        SegmentTable::load(&path, std::slice::from_ref(&entry)).expect("map checked segment");
    let reader = Reader::new(&store, root).unwrap();
    assert_eq!(
        reader.height(),
        match records {
            64 => 1,
            8192 => 2,
            _ => 3,
        }
    );
    println!(
        "{{\"type\":\"data\",\"filesystem\":\"{filesystem}\",\"records\":{records},\"path\":{:?},\"segment_id\":{},\"segment_sha256\":\"{}\",\"segment_bytes\":{},\"root_segment\":{},\"root_offset\":{},\"height\":{}}}",
        path,
        entry.id,
        hex(&entry.hash),
        entry.length,
        root.segment,
        root.offset,
        reader.height()
    );
    for mode in ["mixed", "hits", "misses"] {
        let (queries, expected) = queries(records, count, mode);
        // Both paths warm identical mapped data and verify mathematical outcomes.
        assert_eq!(baseline(&store, root, &queries), expected);
        assert_eq!(prepared(&reader, &queries), expected);
        println!(
            "{{\"type\":\"group\",\"filesystem\":\"{filesystem}\",\"records\":{records},\"mode\":\"{mode}\",\"queries\":{count},\"rounds\":{rounds},\"expected_checksum\":{},\"expected_hits\":{},\"expected_misses\":{},\"binding\":\"once_before_timing\"}}",
            expected.checksum,
            expected.hits,
            count as u64 - expected.hits
        );
        for round in 0..rounds {
            let (a, x, b, y) = if round.is_multiple_of(2) {
                let (a, x) = time(|| baseline(&store, root, &queries));
                let (b, y) = time(|| prepared(&reader, &queries));
                (a, x, b, y)
            } else {
                let (b, y) = time(|| prepared(&reader, &queries));
                let (a, x) = time(|| baseline(&store, root, &queries));
                (a, x, b, y)
            };
            assert_eq!(x, expected);
            assert_eq!(y, expected);
            let order = if round.is_multiple_of(2) {
                "baseline-prepared"
            } else {
                "prepared-baseline"
            };
            println!(
                "{{\"type\":\"sample\",\"filesystem\":\"{filesystem}\",\"records\":{records},\"mode\":\"{mode}\",\"round\":{round},\"order\":\"{order}\",\"queries\":{count},\"baseline_ns\":{a},\"prepared_ns\":{b},\"baseline_checksum\":{},\"prepared_checksum\":{},\"baseline_hits\":{},\"prepared_hits\":{},\"baseline_misses\":{},\"prepared_misses\":{}}}",
                x.checksum,
                y.checksum,
                x.hits,
                y.hits,
                count as u64 - x.hits,
                count as u64 - y.hits
            );
        }
    }
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let output = PathBuf::from(
        args.first()
            .expect("OUTPUT_DIRECTORY [queries=250000] [rounds=16]"),
    );
    let count: usize = args
        .get(1)
        .map_or(250000, |s| s.parse().expect("query count"));
    let rounds: usize = args.get(2).map_or(16, |s| s.parse().expect("round count"));
    assert!(
        count > 0 && rounds >= 16 && rounds.is_multiple_of(2),
        "at least 16 balanced rounds required"
    );
    fs::create_dir(&output).expect("output directory must not exist");
    let disk = output.join("disk");
    fs::create_dir(&disk).unwrap();
    let tmpfs = PathBuf::from("/dev/shm").join(format!(
        "ulmdb-mapped-reader-ab-{}",
        ulmdb_storage::new_identity().unwrap()
    ));
    fs::create_dir(&tmpfs).expect("tmpfs experiment requires /dev/shm");
    println!(
        "{{\"type\":\"experiment\",\"seed\":\"0x243f6a8885a308d3\",\"output\":{:?},\"tmpfs\":{:?},\"warm\":true,\"overhead_subtraction\":false,\"query_count\":{count},\"rounds\":{rounds}}}",
        output, tmpfs
    );
    for (path, name) in [(&disk, "disk"), (&tmpfs, "tmpfs")] {
        for records in [64, 8192, 100000] {
            run_data(path, name, records, count, rounds);
        }
    }
}
