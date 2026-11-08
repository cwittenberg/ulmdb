use crate::protocol::Result;
use std::path::PathBuf;
#[derive(Clone)]
pub struct Config {
    pub keys: usize,
    pub queries: usize,
    pub samples: usize,
    pub seed: u64,
    pub arm: String,
    pub backend: String,
    pub key_kind: String,
    pub directory: PathBuf,
    pub transactions: usize,
    pub ballast_mib: usize,
}
impl Config {
    pub fn parse() -> Result<Self> {
        let mut c = Self {
            keys: 8192,
            queries: 65536,
            samples: 7,
            seed: 73,
            arm: "native".into(),
            backend: "legacy".into(),
            key_kind: "fixed16".into(),
            directory: PathBuf::new(),
            transactions: 4,
            ballast_mib: 0,
        };
        let mut args = std::env::args().skip(1);
        while let Some(flag) = args.next() {
            let value = args.next().ok_or("each option requires a value")?;
            match flag.as_str() {
                "--keys" => c.keys = value.parse()?,
                "--queries" => c.queries = value.parse()?,
                "--samples" => c.samples = value.parse()?,
                "--seed" => c.seed = value.parse()?,
                "--arm" => c.arm = value,
                "--backend" => c.backend = value,
                "--key-kind" => c.key_kind = value,
                "--dir" => c.directory = value.into(),
                "--write-transactions" => c.transactions = value.parse()?,
                "--ballast-mib" => c.ballast_mib = value.parse()?,
                _ => return Err(format!("unknown option {flag}").into()),
            }
        }
        if c.keys < 2
            || c.keys > 10_000_000
            || c.queries == 0
            || c.samples == 0
            || c.seed == 0
            || c.transactions == 0
            || c.ballast_mib > 1024
            || !["native", "hash", "lmdb"].contains(&c.arm.as_str())
            || !["legacy", "resident"].contains(&c.backend.as_str())
            || !["fixed16", "hostname"].contains(&c.key_kind.as_str())
            || c.directory.as_os_str().is_empty()
            || c.directory.exists()
        {
            return Err("invalid configuration; --dir must be a fresh path".into());
        }
        Ok(c)
    }
}
pub struct Record {
    pub key: Vec<u8>,
    pub value: [u8; 32],
}
pub fn key(kind: &str, id: u64) -> Vec<u8> {
    if kind == "fixed16" {
        [
            id.to_be_bytes(),
            id.wrapping_mul(0x9e37_79b9_7f4a_7c15).to_be_bytes(),
        ]
        .concat()
    } else {
        format!("host-{id:012x}.s{}.region-{}.example.net", id % 97, id % 17).into_bytes()
    }
}
pub fn records(c: &Config) -> Vec<Record> {
    let mut rows: Vec<_> = (0..c.keys)
        .map(|id| Record {
            key: key(&c.key_kind, id as u64 * 2),
            value: value(id as u64, 0),
        })
        .collect();
    rows.sort_unstable_by(|a, b| a.key.cmp(&b.key));
    rows
}
pub fn value(id: u64, revision: u64) -> [u8; 32] {
    let mut v = [0; 32];
    for part in 0..4 {
        v[part * 8..part * 8 + 8].copy_from_slice(
            &id.wrapping_mul(0x9e37_79b9_7f4a_7c15)
                .wrapping_add(part as u64 + revision.wrapping_mul(0x7412_5637))
                .to_be_bytes(),
        );
    }
    v
}
pub fn random(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}
pub fn queries(c: &Config, workload: &str) -> Vec<Vec<u8>> {
    let mut s = c.seed;
    (0..c.queries)
        .map(|_| {
            let r = random(&mut s);
            let n = if workload == "hot90" && !r.is_multiple_of(10) {
                (c.keys / 100).max(1)
            } else {
                c.keys
            };
            let id = random(&mut s) as usize % n;
            let miss = workload == "miss" || workload == "mixed50" && r & 1 != 0;
            key(&c.key_kind, id as u64 * 2 + u64::from(miss))
        })
        .collect()
}
pub fn digest(bytes: &[u8]) -> u64 {
    // All returned bytes contribute, identically for all three arms.
    bytes.chunks(8).fold(0x9821_7654, |s, b| {
        let mut word = [0; 8];
        word[..b.len()].copy_from_slice(b);
        s.rotate_left(11) ^ u64::from_le_bytes(word)
    })
}
pub fn pair_digest(key: &[u8], value: &[u8]) -> u64 {
    digest(key).rotate_left(23) ^ digest(value)
}
