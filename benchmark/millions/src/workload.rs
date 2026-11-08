use crate::Result;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Clone, Serialize)]
pub struct Config {
    pub keys: usize,
    pub reads: usize,
    pub updates: usize,
    pub batch: usize,
    pub seed: u64,
    pub lmdb_map_bytes: usize,
}
pub struct Args {
    pub engine: String,
    pub dir: PathBuf,
    pub config: Config,
}
impl Args {
    pub fn parse() -> Result<Self> {
        let mut engine = None;
        let mut dir: Option<PathBuf> = None;
        let mut measure = false;
        let mut config = Config {
            keys: 1_000_000,
            reads: 10_000_000,
            updates: 1_000_000,
            batch: 1000,
            seed: 73,
            lmdb_map_bytes: 1 << 30,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--measure" {
                measure = true;
                continue;
            }
            if arg == "--help" {
                println!(
                    "ulmdb-millions --engine ulmdb|lmdb|sqlite --dir NEW --measure [--keys 1000000 --reads 10000000 --updates 1000000 --batch 1000 --seed 73 --lmdb-map-bytes 1073741824]"
                );
                std::process::exit(0);
            }
            let value = args.next().ok_or("missing argument value")?;
            match arg.as_str() {
                "--engine" => engine = Some(value),
                "--dir" => dir = Some(value.into()),
                "--keys" => config.keys = value.parse()?,
                "--reads" => config.reads = value.parse()?,
                "--updates" => config.updates = value.parse()?,
                "--batch" => config.batch = value.parse()?,
                "--seed" => config.seed = value.parse()?,
                "--lmdb-map-bytes" => config.lmdb_map_bytes = value.parse()?,
                _ => return Err(format!("unknown option: {arg}").into()),
            }
        }
        if !measure {
            return Err(
                "explicit --measure is required; this program executes timed database work".into(),
            );
        }
        if config.keys == 0
            || config.keys > 10_000_000
            || config.reads == 0
            || config.reads > 100_000_000
            || config.updates == 0
            || config.updates > 10_000_000
            || config.batch == 0
            || config.batch > 1_000_000
        {
            return Err(
                "invalid workload size (keys/updates 1..10M, reads 1..100M, batch 1..1M)".into(),
            );
        }
        let engine = engine.ok_or("--engine is required")?;
        if !["ulmdb", "lmdb", "sqlite"].contains(&engine.as_str()) {
            return Err("unknown engine".into());
        }
        let dir = dir.ok_or("--dir is required")?;
        if dir.exists() {
            return Err("--dir must not already exist; fixtures are never overwritten".into());
        }
        Ok(Self {
            engine,
            dir,
            config,
        })
    }
}
#[derive(Clone, Copy)]
pub struct Record {
    pub key: [u8; 8],
    pub value: [u8; 8],
}
#[derive(Serialize)]
pub struct Identity {
    pub dataset_sha256: String,
    pub queries_sha256: String,
    pub updates_sha256: String,
    pub key_bytes: usize,
    pub value_bytes: usize,
    pub read_hits: u64,
    pub read_checksum: u64,
    pub workload_payload_bytes: usize,
}
pub struct Workload {
    pub load: Vec<Record>,
    pub queries: Vec<[u8; 8]>,
    pub updates: Vec<Record>,
    pub final_values: Vec<[u8; 8]>,
    pub identity: Identity,
}
pub fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
fn random(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    mix(*state)
}
fn hash_records(domain: &[u8], rows: &[Record]) -> String {
    let mut h = Sha256::new();
    h.update(domain);
    for r in rows {
        h.update(r.key);
        h.update(r.value);
    }
    format!("{:x}", h.finalize())
}
pub fn fold(sum: u64, value: Option<[u8; 8]>) -> u64 {
    sum.rotate_left(1)
        .wrapping_add(value.map(u64::from_be_bytes).unwrap_or(0x9e3779b97f4a7c15))
}
impl Workload {
    pub fn new(c: &Config) -> Self {
        let load: Vec<_> = (0..c.keys)
            .map(|i| Record {
                key: (2 * i as u64).to_be_bytes(),
                value: mix(i as u64 ^ c.seed).to_be_bytes(),
            })
            .collect();
        let mut rng = c.seed;
        let mut hits = 0;
        let mut checksum = 0;
        let mut query_hash = Sha256::new();
        query_hash.update(b"millions-v1-queries");
        let queries = (0..c.reads)
            .map(|_| {
                let number = random(&mut rng) % (2 * c.keys as u64);
                let key = number.to_be_bytes();
                query_hash.update(key);
                let value = if number % 2 == 0 {
                    hits += 1;
                    Some(load[number as usize / 2].value)
                } else {
                    None
                };
                checksum = fold(checksum, value);
                key
            })
            .collect();
        let mut order: Vec<usize> = (0..c.keys).collect();
        for i in (1..c.keys).rev() {
            let j = (random(&mut rng) % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
        let mut final_values: Vec<_> = load.iter().map(|r| r.value).collect();
        let updates = (0..c.updates)
            .map(|n| {
                let i = order[n % c.keys];
                let value = (u64::from_be_bytes(final_values[i]) ^ ((n as u64).wrapping_add(1)))
                    .to_be_bytes();
                final_values[i] = value;
                Record {
                    key: load[i].key,
                    value,
                }
            })
            .collect::<Vec<_>>();
        let identity = Identity {
            dataset_sha256: hash_records(b"millions-v1-load", &load),
            queries_sha256: format!("{:x}", query_hash.finalize()),
            updates_sha256: hash_records(b"millions-v1-updates", &updates),
            key_bytes: 8,
            value_bytes: 8,
            read_hits: hits,
            read_checksum: checksum,
            workload_payload_bytes: load.len() * 16
                + c.reads * 8
                + updates.len() * 16
                + final_values.len() * 8,
        };
        Self {
            load,
            queries,
            updates,
            final_values,
            identity,
        }
    }
}
