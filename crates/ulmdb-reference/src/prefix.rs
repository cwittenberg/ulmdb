//! Independent bit-vector prefix model and exhaustive-scan store.

use std::collections::BTreeMap;
use ulmdb_core::Address;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ModelPrefix {
    ipv6: bool,
    bits: Vec<bool>,
}

fn address_bits(address: Address) -> Vec<bool> {
    let bytes = match address {
        Address::V4(v) => v.to_be_bytes().to_vec(),
        Address::V6(v) => v.to_be_bytes().to_vec(),
    };
    bytes
        .into_iter()
        .flat_map(|byte| (0..8).rev().map(move |bit| byte & (1 << bit) != 0))
        .collect()
}

impl ModelPrefix {
    pub(crate) const fn is_ipv6(&self) -> bool {
        self.ipv6
    }

    pub fn normalize(address: Address, length: u8) -> Option<Self> {
        let mut bits = address_bits(address);
        if usize::from(length) > bits.len() {
            return None;
        }
        bits.truncate(usize::from(length));
        Some(Self {
            ipv6: matches!(address, Address::V6(_)),
            bits,
        })
    }

    pub fn length(&self) -> usize {
        self.bits.len()
    }

    pub fn contains_address(&self, address: Address) -> bool {
        self.ipv6 == matches!(address, Address::V6(_))
            && address_bits(address).starts_with(&self.bits)
    }

    pub fn contains_prefix(&self, other: &Self) -> bool {
        self.ipv6 == other.ipv6 && other.bits.starts_with(&self.bits)
    }

    pub fn network(&self) -> Address {
        let mut value = 0u128;
        let width = if self.ipv6 { 128 } else { 32 };
        for i in 0..width {
            value = value * 2 + u128::from(self.bits.get(i).copied().unwrap_or(false));
        }
        if self.ipv6 {
            Address::V6(value)
        } else {
            Address::V4(value as u32)
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PrefixStore(pub BTreeMap<ModelPrefix, Vec<u8>>);

impl PrefixStore {
    pub fn insert(&mut self, prefix: ModelPrefix, value: Vec<u8>) -> Option<Vec<u8>> {
        self.0.insert(prefix, value)
    }
    pub fn delete(&mut self, prefix: &ModelPrefix) -> Option<Vec<u8>> {
        self.0.remove(prefix)
    }
    pub fn exact(&self, prefix: &ModelPrefix) -> Option<&[u8]> {
        self.0.get(prefix).map(Vec::as_slice)
    }
    pub fn longest_match(&self, address: Address) -> Option<(&ModelPrefix, &[u8])> {
        let bits = address_bits(address);
        let ipv6 = matches!(address, Address::V6(_));
        self.0
            .iter()
            .filter(|(p, _)| p.ipv6 == ipv6 && bits.starts_with(&p.bits))
            .max_by_key(|(p, _)| p.length())
            .map(|(p, v)| (p, v.as_slice()))
    }
    pub fn covering(&self, query: &ModelPrefix) -> Option<(&ModelPrefix, &[u8])> {
        self.0
            .iter()
            .filter(|(p, _)| p.contains_prefix(query))
            .max_by_key(|(p, _)| p.length())
            .map(|(p, v)| (p, v.as_slice()))
    }
    pub fn ancestors(&self, query: &ModelPrefix) -> Vec<&ModelPrefix> {
        let mut result: Vec<_> = self.0.keys().filter(|p| p.contains_prefix(query)).collect();
        result.sort_by_key(|p| std::cmp::Reverse(p.length()));
        result
    }
    pub fn subnets(&self, query: &ModelPrefix) -> Vec<&ModelPrefix> {
        let mut result: Vec<_> = self.0.keys().filter(|p| query.contains_prefix(p)).collect();
        result.sort_by_key(|p| (p.network(), p.length()));
        result
    }
    pub fn overlaps(&self, query: &ModelPrefix) -> Vec<&ModelPrefix> {
        let mut result: Vec<_> = self
            .0
            .keys()
            .filter(|p| query.contains_prefix(p) || p.contains_prefix(query))
            .collect();
        result.sort_by_key(|p| (p.network(), p.length()));
        result
    }
}

#[cfg(test)]
mod tests;
