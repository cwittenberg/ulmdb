use crate::{
    backend::View,
    workload::{Result, digest},
};
use std::{collections::HashMap, hint::black_box, mem::size_of, time::Instant};

/// GoF Strategy contract, confined to this benchmark. Both implementations return
/// bytes borrowed from the same immutable ULMDB read transaction.
pub trait LookupStrategy {
    fn get(&self, key: &[u8]) -> Option<&[u8]>;
    fn scan(&self, lower: &[u8], count: usize) -> u64;
}
impl LookupStrategy for View<'_> {
    #[inline]
    fn get(&self, key: &[u8]) -> Option<&[u8]> {
        View::get(self, key)
    }
    fn scan(&self, lower: &[u8], count: usize) -> u64 {
        View::scan(self, lower, count)
    }
}

pub struct HashSideIndex<'a, 'v> {
    pub map: HashMap<Box<[u8]>, &'a [u8]>,
    pub ordered: &'v View<'a>,
    pub key_bytes: usize,
}
impl<'a, 'v> HashSideIndex<'a, 'v> {
    pub fn build(view: &'v View<'a>, count: usize) -> Result<(Self, u128)> {
        let start = Instant::now();
        let mut map = HashMap::with_capacity(count);
        let mut key_bytes = 0;
        view.visit(|key, value| {
            key_bytes += key.len();
            assert!(map.insert(key.to_vec().into_boxed_slice(), value).is_none());
        })?;
        assert_eq!(map.len(), count);
        Ok((
            Self {
                map,
                ordered: view,
                key_bytes,
            },
            start.elapsed().as_nanos(),
        ))
    }
    pub fn estimate_bytes(&self) -> usize {
        // Approximation for std's SwissTable backing layout: power-of-two slots,
        // one control byte per slot and a control-group tail. Excludes allocator
        // headers/rounding, RSS, snapshots and original mapped database bytes.
        let buckets = (self.map.capacity() + 1).next_power_of_two();
        buckets * (size_of::<Box<[u8]>>() + size_of::<&[u8]>() + 1) + 16 + self.key_bytes
    }
}
impl LookupStrategy for HashSideIndex<'_, '_> {
    #[inline]
    fn get(&self, key: &[u8]) -> Option<&[u8]> {
        self.map.get(key).copied()
    }
    fn scan(&self, lower: &[u8], count: usize) -> u64 {
        self.ordered.scan(lower, count)
    }
}

pub fn timed<S: LookupStrategy>(strategy: &S, queries: &[Vec<u8>]) -> (u128, u64) {
    timed_fn(queries, |key| strategy.get(key))
}

/// Select the backend before the clock starts, so the native baseline pays no
/// extra backend-enum match per query relative to the hash candidate.
pub fn timed_native(view: &View<'_>, queries: &[Vec<u8>]) -> (u128, u64) {
    match view {
        View::Legacy(v) => timed_fn(queries, |key| v.get(key).unwrap()),
        View::Resident(v) => timed_fn(queries, |key| v.get(key).unwrap()),
    }
}

fn timed_fn<'a>(
    queries: &[Vec<u8>],
    mut get: impl FnMut(&[u8]) -> Option<&'a [u8]>,
) -> (u128, u64) {
    let start = Instant::now();
    let sum = queries.iter().fold(0u64, |sum, key| {
        sum.wrapping_add(digest(black_box(get(black_box(key)))))
    });
    (start.elapsed().as_nanos(), black_box(sum))
}
pub fn timed_scan<S: LookupStrategy>(strategy: &S, queries: &[Vec<u8>]) -> (u128, u64) {
    let start = Instant::now();
    let sum = queries.iter().fold(0u64, |sum, key| {
        sum.wrapping_add(black_box(strategy.scan(black_box(key), 100)))
    });
    (start.elapsed().as_nanos(), black_box(sum))
}
