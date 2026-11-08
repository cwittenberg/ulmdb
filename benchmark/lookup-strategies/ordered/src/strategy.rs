use std::{collections::BTreeMap, ops::Bound};

pub use crate::contract::*;
pub struct Native<'a> {
    pub reader: ulmdb::KvRead<'a>,
}
impl Lookup for Native<'_> {
    fn name(&self) -> &'static str {
        "ulmdb_legacy_prepared"
    }
    fn get(&self, key: &[u8]) -> Option<&[u8]> {
        self.reader.get(key).unwrap()
    }
    fn rows<'a>(&'a self, key: &[u8], limit: usize) -> Vec<Record<'a>> {
        self.reader
            .cursor(Bound::Included(key), Bound::Unbounded, false)
            .unwrap()
            .take(limit)
            .map(Result::unwrap)
            .collect()
    }
    fn scan_digest(&self, key: &[u8], limit: usize) -> u64 {
        self.reader
            .cursor(Bound::Included(key), Bound::Unbounded, false)
            .unwrap()
            .take(limit)
            .map(|row| {
                let (k, v) = row.unwrap();
                row_digest(k, v)
            })
            .fold(0u64, u64::wrapping_add)
    }
}
pub struct Standard<'a> {
    pub map: BTreeMap<Vec<u8>, usize>,
    pub records: &'a [Record<'a>],
}
impl Lookup for Standard<'_> {
    fn name(&self) -> &'static str {
        "std_btreemap"
    }
    fn get(&self, key: &[u8]) -> Option<&[u8]> {
        self.map.get(key).map(|&id| self.records[id].1)
    }
    fn rows<'a>(&'a self, key: &[u8], limit: usize) -> Vec<Record<'a>> {
        self.map
            .range::<[u8], _>((Bound::Included(key), Bound::Unbounded))
            .take(limit)
            .map(|(_, &id)| self.records[id])
            .collect()
    }
    fn scan_digest(&self, key: &[u8], limit: usize) -> u64 {
        self.map
            .range::<[u8], _>((Bound::Included(key), Bound::Unbounded))
            .take(limit)
            .map(|(_, &id)| {
                let (k, v) = self.records[id];
                row_digest(k, v)
            })
            .fold(0u64, u64::wrapping_add)
    }
}
