use crate::oracle::{self, Answer};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const LENGTHS4: [u8; 6] = [12, 16, 20, 24, 28, 32];
pub const LENGTHS6: [u8; 8] = [36, 48, 56, 64, 80, 96, 112, 128];
#[derive(Clone, Copy, Serialize)]
pub struct Config {
    pub kv_keys: usize,
    pub specific_prefix_attempts: usize,
    pub queries: usize,
    pub trials: usize,
    pub commits_per_phase: usize,
    pub seed: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            kv_keys: 8192,
            specific_prefix_attempts: 1024,
            queries: 4000,
            trials: 16,
            commits_per_phase: 8,
            seed: 73,
        }
    }
}
impl Config {
    pub fn smoke() -> Self {
        Self {
            kv_keys: 64,
            specific_prefix_attempts: 32,
            queries: 64,
            trials: 16,
            commits_per_phase: 1,
            seed: 73,
        }
    }
}
#[derive(Clone, Copy)]
pub struct Record4 {
    pub network: u32,
    pub length: u8,
    pub value: u64,
}
#[derive(Clone, Copy)]
pub struct Record6 {
    pub network: u128,
    pub length: u8,
    pub value: u64,
}
pub struct Queries<Q> {
    pub values: Vec<Q>,
    pub expected: Vec<Option<Answer>>,
}
pub struct Dataset {
    pub config: Config,
    pub kv: Vec<([u8; 8], [u8; 8])>,
    pub ipv4: Vec<Record4>,
    pub ipv6: Vec<Record6>,
    pub kv_queries: Queries<[u8; 8]>,
    pub ipv4_queries: Queries<u32>,
    pub ipv6_queries: Queries<u128>,
    pub dataset_sha256: String,
    pub queries_sha256: String,
}
pub fn next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9e3779b97f4a7c15);
    let mut x = *seed;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
pub fn mask4(value: u32, length: u8) -> u32 {
    if length == 0 {
        0
    } else {
        value & (u32::MAX << (32 - length))
    }
}
pub fn mask6(value: u128, length: u8) -> u128 {
    if length == 0 {
        0
    } else {
        value & (u128::MAX << (128 - length))
    }
}
pub fn build(config: Config) -> Dataset {
    assert!(
        config.kv_keys >= 64
            && config.specific_prefix_attempts >= 8
            && config.queries >= 16
            && config.trials >= 15
    );
    let mut seed = config.seed;
    let mut map4 = BTreeMap::new();
    let mut map6 = BTreeMap::new();
    // Every other /12 or /36 leaves genuine holes inside the common covering root.
    for group in (0..16u32).step_by(2) {
        map4.insert((0x0a00_0000 | (group << 20), 12), 0);
        map6.insert(
            (
                0x20010db8000000000000000000000000u128 | ((group as u128) << 92),
                36,
            ),
            0,
        );
    }
    for i in 0..config.specific_prefix_attempts {
        let address4 = 0x0a00_0000 | ((next(&mut seed) as u32) & 0x00ef_ffff);
        let address6 = 0x20010db8000000000000000000000000u128
            | ((((next(&mut seed) as u128) << 64) | next(&mut seed) as u128)
                & 0x00000000efffffffffffffffffffffffu128);
        let length4 = LENGTHS4[i % LENGTHS4.len()];
        let length6 = LENGTHS6[i % LENGTHS6.len()];
        map4.insert((mask4(address4, length4), length4), 0);
        map6.insert((mask6(address6, length6), length6), 0);
    }
    let ipv4: Vec<_> = map4
        .into_keys()
        .enumerate()
        .map(|(i, (network, length))| Record4 {
            network,
            length,
            value: 0x4000_0000 + i as u64,
        })
        .collect();
    let ipv6: Vec<_> = map6
        .into_keys()
        .enumerate()
        .map(|(i, (network, length))| Record6 {
            network,
            length,
            value: 0x6000_0000 + i as u64,
        })
        .collect();
    let kv: Vec<_> = (0..config.kv_keys)
        .map(|n| {
            (
                ((n as u64) * 2 + 2).to_be_bytes(),
                (n as u64 * 17 + 1).to_be_bytes(),
            )
        })
        .collect();
    let mut dataset = Sha256::new();
    dataset.update(b"ULMDB-paired-canonical-records-v1\0");
    for (key, value) in &kv {
        dataset.update(key);
        dataset.update(value);
    }
    for r in &ipv4 {
        dataset.update([4, r.length]);
        dataset.update(r.network.to_be_bytes());
        dataset.update(r.value.to_be_bytes());
    }
    for r in &ipv6 {
        dataset.update([6, r.length]);
        dataset.update(r.network.to_be_bytes());
        dataset.update(r.value.to_be_bytes());
    }
    let mut keys = Vec::new();
    let mut addresses4 = Vec::new();
    let mut addresses6 = Vec::new();
    for n in 0..config.queries {
        keys.push((next(&mut seed) % ((config.kv_keys as u64) * 2 + 4)).to_be_bytes());
        let x = next(&mut seed);
        let y = next(&mut seed);
        let inside4 = 0x0a00_0000 | (x as u32 & 0x00ef_ffff);
        let inside6 = 0x20010db8000000000000000000000000u128
            | ((((x as u128) << 64) | y as u128) & 0x00000000efffffffffffffffffffffffu128);
        let (address4, address6) = match n % 4 {
            0 => (
                0xc000_0000 | (x as u32 & 0x00ff_ffff),
                0x20010db9000000000000000000000000u128 | (inside6 & u128::from(u64::MAX)),
            ),
            1 => (
                inside4 | 0x0010_0000,
                inside6 | 0x00000000100000000000000000000000u128,
            ),
            2 => (inside4, inside6),
            _ => {
                let a = ipv4[(x as usize) % ipv4.len()];
                let b = ipv6[(y as usize) % ipv6.len()];
                (
                    a.network | (!mask4(u32::MAX, a.length) & x as u32),
                    b.network | (!mask6(u128::MAX, b.length) & (((x as u128) << 64) | y as u128)),
                )
            }
        };
        addresses4.push(address4);
        addresses6.push(address6);
    }
    let reference: BTreeMap<_, _> = kv.iter().copied().collect();
    let expected_kv = keys
        .iter()
        .map(|key| {
            reference.get(key).map(|v| Answer {
                length: 0,
                value: u64::from_be_bytes(*v),
            })
        })
        .collect();
    let expected4 = addresses4.iter().map(|&a| oracle::ipv4(&ipv4, a)).collect();
    let expected6 = addresses6.iter().map(|&a| oracle::ipv6(&ipv6, a)).collect();
    let mut query_hash = Sha256::new();
    query_hash.update(b"ULMDB-paired-queries-v1\0");
    for key in &keys {
        query_hash.update(key);
    }
    for a in &addresses4 {
        query_hash.update(a.to_be_bytes());
    }
    for a in &addresses6 {
        query_hash.update(a.to_be_bytes());
    }
    Dataset {
        config,
        kv,
        ipv4,
        ipv6,
        kv_queries: Queries {
            values: keys,
            expected: expected_kv,
        },
        ipv4_queries: Queries {
            values: addresses4,
            expected: expected4,
        },
        ipv6_queries: Queries {
            values: addresses6,
            expected: expected6,
        },
        dataset_sha256: format!("{:x}", dataset.finalize()),
        queries_sha256: format!("{:x}", query_hash.finalize()),
    }
}
