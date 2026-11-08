//! Independent bit-by-bit prefix model; never calls the production mask/trie.
use crate::dataset::{Record4, Record6};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Answer {
    pub length: u8,
    pub value: u64,
}
pub fn ipv4(records: &[Record4], address: u32) -> Option<Answer> {
    records
        .iter()
        .filter(|r| {
            (0..r.length)
                .all(|bit| ((r.network >> (31 - bit)) & 1) == ((address >> (31 - bit)) & 1))
        })
        .max_by_key(|r| r.length)
        .map(|r| Answer {
            length: r.length,
            value: r.value,
        })
}
pub fn ipv6(records: &[Record6], address: u128) -> Option<Answer> {
    records
        .iter()
        .filter(|r| {
            (0..r.length)
                .all(|bit| ((r.network >> (127 - bit)) & 1) == ((address >> (127 - bit)) & 1))
        })
        .max_by_key(|r| r.length)
        .map(|r| Answer {
            length: r.length,
            value: r.value,
        })
}
pub fn fold(sum: u64, answer: Option<Answer>) -> u64 {
    sum.wrapping_mul(0x9e3779b97f4a7c15)
        .wrapping_add(answer.map_or(0, |a| a.value ^ ((a.length as u64) << 56)))
}
