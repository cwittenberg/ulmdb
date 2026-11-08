//! Isolated warm checked-arena comparison, not an LMDB qualification workload.
//! Run: cargo run --release -p ulmdb-kv --example prepared_reader --locked --
//!      [records=100000] [queries=1000000] [rounds=11] [mixed|hits|misses]
use std::{hint::black_box, time::Instant};
use ulmdb_core::storage::{Error, ReadStore, Reference, Result, WriteStore};
use ulmdb_kv::{Reader, get, put};

#[derive(Default)]
struct Store {
    bytes: Vec<u8>,
}
impl ReadStore for Store {
    #[inline]
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("benchmark segment"));
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes
            .get(start..start.checked_add(length).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("benchmark extent"))
    }
}
impl WriteStore for Store {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        let start = self.bytes.len().div_ceil(alignment) * alignment;
        self.bytes.resize(start, 0);
        self.bytes.extend_from_slice(bytes);
        Ok(Reference {
            segment: 1,
            offset: start as u64,
        })
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], _alignment: usize) -> Result<Reference> {
        let start = old.offset as usize;
        self.bytes[start..start + bytes.len()].copy_from_slice(bytes);
        Ok(old)
    }
}
fn random(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}
fn value(n: u64) -> u64 {
    n.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(17)
}
fn add(checksum: &mut u64, hits: &mut u64, bytes: Option<&[u8]>) {
    if let Some(bytes) = black_box(bytes) {
        *checksum = checksum.wrapping_add(u64::from_le_bytes(
            bytes.try_into().expect("eight-byte value"),
        ));
        *hits += 1;
    }
}
#[inline(never)]
fn baseline(store: &Store, root: Reference, queries: &[[u8; 8]]) -> (u64, u64) {
    let (mut checksum, mut hits) = (0u64, 0u64);
    for key in queries {
        add(
            &mut checksum,
            &mut hits,
            get(store, root, black_box(key)).expect("baseline checked lookup"),
        );
    }
    (checksum, hits)
}
#[inline(never)]
fn prepared(reader: &Reader<'_, Store>, queries: &[[u8; 8]]) -> (u64, u64) {
    let (mut checksum, mut hits) = (0u64, 0u64);
    for key in queries {
        add(
            &mut checksum,
            &mut hits,
            reader.get(black_box(key)).expect("prepared checked lookup"),
        );
    }
    (checksum, hits)
}
fn time<T>(f: impl FnOnce() -> T) -> (f64, T) {
    let start = Instant::now();
    let result = f();
    (start.elapsed().as_nanos() as f64, result)
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let at = |n: usize, default: usize| {
        args.get(n)
            .map_or(default, |s| s.parse().expect("positive integer"))
    };
    let records = at(0, 100000);
    let query_count = at(1, 1000000);
    let rounds = at(2, 11);
    let mode = args.get(3).map(String::as_str).unwrap_or("mixed");
    assert!((1..=1000000).contains(&records) && query_count > 0 && rounds >= 3);
    assert!(["mixed", "hits", "misses"].contains(&mode));
    let mut store = Store::default();
    let mut root = Reference::NULL;
    for n in 0..records as u64 {
        root = put(
            &mut store,
            root,
            &(n * 2).to_be_bytes(),
            &value(n).to_le_bytes(),
        )
        .unwrap();
    }
    let mut seed = 0x243f_6a88_85a3_08d3;
    let mut queries = Vec::with_capacity(query_count);
    let (mut expected_checksum, mut expected_hits) = (0u64, 0u64);
    for _ in 0..query_count {
        let n = random(&mut seed) % records as u64;
        let miss = match mode {
            "hits" => false,
            "misses" => true,
            _ => random(&mut seed).is_multiple_of(4),
        };
        queries.push((n * 2 + u64::from(miss)).to_be_bytes());
        if !miss {
            expected_checksum = expected_checksum.wrapping_add(value(n));
            expected_hits += 1;
        }
    }
    let expected = (expected_checksum, expected_hits);
    let reader = Reader::new(&store, root).unwrap();
    assert_eq!(baseline(&store, root, &queries), expected);
    assert_eq!(prepared(&reader, &queries), expected);
    let (mut baseline_samples, mut prepared_samples) =
        (Vec::with_capacity(rounds), Vec::with_capacity(rounds));
    for round in 0..rounds {
        let (baseline_ns, prepared_ns) = if round.is_multiple_of(2) {
            let (a, x) = time(|| baseline(&store, root, &queries));
            let (b, y) = time(|| prepared(&reader, &queries));
            assert_eq!(x, expected);
            assert_eq!(y, expected);
            (a, b)
        } else {
            let (b, y) = time(|| prepared(&reader, &queries));
            let (a, x) = time(|| baseline(&store, root, &queries));
            assert_eq!(x, expected);
            assert_eq!(y, expected);
            (a, b)
        };
        baseline_samples.push(baseline_ns / query_count as f64);
        prepared_samples.push(prepared_ns / query_count as f64);
    }
    println!(
        "workload=warm-custom-checked-arena records={records} queries={query_count} rounds={rounds} mode={mode} height={} seed=0x243f6a8885a308d3",
        reader.height()
    );
    println!(
        "checksum={} expected_hits={} bytes={} prepared_binding=once_before_timing",
        expected.0,
        expected.1,
        store.bytes.len()
    );
    for (n, (a, b)) in baseline_samples.iter().zip(&prepared_samples).enumerate() {
        println!("round={n} baseline_ns_per_query={a:.3} prepared_ns_per_query={b:.3}");
    }
    baseline_samples.sort_by(f64::total_cmp);
    prepared_samples.sort_by(f64::total_cmp);
    let a = baseline_samples[rounds / 2];
    let b = prepared_samples[rounds / 2];
    println!(
        "median_baseline_ns_per_query={a:.3} median_prepared_ns_per_query={b:.3} speedup={:.4}",
        a / b
    );
}
