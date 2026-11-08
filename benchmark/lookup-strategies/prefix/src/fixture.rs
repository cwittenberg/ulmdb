use std::collections::BTreeMap;
use ulmdb_core::{Address, Prefix};

pub struct Fixture {
    pub entries: Vec<(Prefix, [u8; 16])>,
    pub hits: Vec<Prefix>,
    pub mixed: Vec<Prefix>,
    pub nested: Vec<Prefix>,
    pub v6: bool,
}
pub fn random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}
fn addr(v6: bool, number: u128) -> Address {
    if v6 {
        Address::V6(number)
    } else {
        Address::V4(number as u32)
    }
}
fn numeric(a: Address) -> u128 {
    match a {
        Address::V4(n) => n as u128,
        Address::V6(n) => n,
    }
}
impl Fixture {
    pub fn new(keys: usize, queries: usize, v6: bool, seed: u64) -> Self {
        assert!(keys >= 64 && queries > 0);
        let width = if v6 { 128 } else { 32 };
        let mut state = seed
            ^ if v6 {
                0x1234_5678_90ab_cdef
            } else {
                0xabcd_ef98_7654_3210
            };
        let mut map = BTreeMap::new();
        let chain_address = if v6 {
            0x2001_0db8_1234_5678_90ab_cdef_1234_5678
        } else {
            0x0a12_3456
        };
        let chain_lengths: &[u8] = if v6 {
            &[
                16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 96, 104, 112, 120, 128,
            ]
        } else {
            &[8, 12, 16, 20, 24, 28, 32]
        };
        for &length in chain_lengths {
            let p = Prefix::normalize(addr(v6, chain_address), length).unwrap();
            let id = map.len() as u64;
            map.insert(p, payload(id));
        }
        // Leave upper half empty (root rejection) and nibble 4 empty inside the
        // root (a genuine interior gap). Preserve deliberate nested claims.
        while map.len() < keys {
            let raw = if v6 {
                ((random(&mut state) as u128) << 64 | random(&mut state) as u128) & (u128::MAX >> 1)
            } else {
                (random(&mut state) as u32 & (u32::MAX >> 1)) as u128
            };
            if (raw >> (width - 4)) & 15 == 4 {
                continue;
            }
            let lengths: &[u8] = if v6 {
                &[32, 48, 56, 64, 96, 120, 128]
            } else {
                &[8, 12, 16, 20, 24, 28, 32]
            };
            let length = lengths[(random(&mut state) as usize) % lengths.len()];
            let p = Prefix::normalize(addr(v6, raw), length).unwrap();
            let id = map.len() as u64;
            map.entry(p).or_insert_with(|| payload(id));
        }
        let entries: Vec<_> = map.into_iter().collect();
        let mut hits = Vec::with_capacity(queries);
        let mut mixed = Vec::with_capacity(queries);
        let mut nested = Vec::with_capacity(queries);
        for i in 0..queries {
            let p = entries[(random(&mut state) as usize) % entries.len()].0;
            hits.push(p);
            let q = match i % 4 {
                1 => Prefix::normalize(
                    addr(v6, numeric(p.network()) | (1u128 << (width - 1))),
                    p.length(),
                )
                .unwrap(),
                3 => {
                    let bits = (numeric(p.network()) & ((1u128 << (width - 4)) - 1))
                        | (4u128 << (width - 4));
                    Prefix::normalize(addr(v6, bits), p.length()).unwrap()
                }
                _ => p,
            };
            mixed.push(q);
            nested.push(
                Prefix::normalize(addr(v6, chain_address), *chain_lengths.last().unwrap()).unwrap(),
            );
        }
        Self {
            entries,
            hits,
            mixed,
            nested,
            v6,
        }
    }
    pub fn host(&self, p: Prefix) -> Prefix {
        Prefix::new(p.network(), if self.v6 { 128 } else { 32 }).unwrap()
    }
    pub fn full(&self) -> Prefix {
        Prefix::new(
            if self.v6 {
                Address::V6(0)
            } else {
                Address::V4(0)
            },
            0,
        )
        .unwrap()
    }
    pub fn oracle(&self, query: Prefix, exact: bool, ancestors: bool) -> Vec<(Prefix, Vec<u8>)> {
        let mut matches: Vec<_> = self
            .entries
            .iter()
            .filter(|(p, _)| {
                if exact {
                    *p == query
                } else {
                    p.contains_prefix(query)
                }
            })
            .map(|(p, v)| (*p, v.to_vec()))
            .collect();
        matches.sort_by_key(|(p, _)| std::cmp::Reverse(p.length()));
        if !ancestors {
            matches.truncate(1);
        }
        matches
    }
}
fn payload(id: u64) -> [u8; 16] {
    let mut value = [0; 16];
    value[..8].copy_from_slice(&id.to_le_bytes());
    value[8..].copy_from_slice(&(id ^ 0x55aa_6633_0f0f_f0f0).to_le_bytes());
    value
}
