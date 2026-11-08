//! Seeded development data construction.

use super::{Config, PrefixProfile, Workload};
use crate::{ModelPrefix, PrefixStore};
use std::collections::BTreeMap;
use ulmdb_core::Address;

struct Random(u64);
impl Random {
    // SplitMix64, specified wrapping arithmetic; no platform-dependent random source.
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn address(&mut self, v6: bool) -> Address {
        if v6 {
            Address::V6((u128::from(self.next()) << 64) | u128::from(self.next()))
        } else {
            Address::V4(self.next() as u32)
        }
    }
}

/// Preserve independent oracle generation while guaranteeing known coverage gaps.
fn scoped_address(address: Address, hit: bool) -> Address {
    match address {
        Address::V4(n) => {
            Address::V4((n & 0x00ff_ffff) | if hit { 0x0a00_0000 } else { 0xc000_0000 })
        }
        Address::V6(n) => Address::V6(
            (n & ((1u128 << 96) - 1))
                | ((if hit {
                    0x2001_0db8u128
                } else {
                    0x2001_0db9u128
                }) << 96),
        ),
    }
}

impl Workload {
    pub fn generate(config: Config) -> Result<Self, String> {
        if config.prefixes > 2_000_000 || config.kv > 2_000_000 || config.queries > 2_000_000 {
            return Err("development generator ceiling is 2,000,000 per record family".into());
        }
        if config
            .prefixes
            .checked_mul(config.queries)
            .is_none_or(|n| n > 50_000_000)
        {
            return Err(
                "independent linear oracle budget exceeds 50,000,000 prefix/query comparisons"
                    .into(),
            );
        }
        let mut random = Random(config.seed);
        let mut prefixes = PrefixStore::default();
        let mut candidate = 0usize;
        while prefixes.0.len() < config.prefixes {
            let v6 = !candidate.is_multiple_of(2);
            let width = if v6 { 128 } else { 32 };
            let minimum = if config.prefix_profile == PrefixProfile::Gapped {
                if v6 { 32 } else { 8 }
            } else {
                0
            };
            let length = (minimum + (candidate / 2) % (width - minimum + 1)) as u8;
            let address = random.address(v6);
            let address = if config.prefix_profile == PrefixProfile::Gapped {
                scoped_address(address, true)
            } else {
                address
            };
            let p = ModelPrefix::normalize(address, length).ok_or("invalid generated prefix")?;
            prefixes
                .0
                .entry(p)
                .or_insert_with(|| (candidate as u64).to_be_bytes().to_vec());
            candidate += 1;
        }
        let mut kv = BTreeMap::new();
        for index in 0..config.kv {
            kv.insert(
                (index as u64).to_be_bytes().to_vec(),
                random.next().to_be_bytes().to_vec(),
            );
        }
        let mut queries = Vec::with_capacity(config.queries);
        for i in 0..config.queries {
            let address = random.address(!i.is_multiple_of(2));
            queries.push(if config.prefix_profile == PrefixProfile::Gapped {
                scoped_address(address, (i / 2).is_multiple_of(2))
            } else {
                address
            });
        }
        Ok(Self {
            config,
            prefixes,
            kv,
            queries,
        })
    }
}
