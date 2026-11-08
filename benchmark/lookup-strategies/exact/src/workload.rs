use std::path::PathBuf;

pub type Error = Box<dyn std::error::Error>;
pub type Result<T> = std::result::Result<T, Error>;

pub struct Config {
    pub rows: usize,
    pub queries: usize,
    pub samples: usize,
    pub seed: u64,
    pub backend: String,
    pub key_kind: String,
    pub directory: PathBuf,
}

impl Config {
    pub fn parse() -> Result<Self> {
        let mut c = Self {
            rows: 8192,
            queries: 200_000,
            samples: 7,
            seed: 0xd1b54a32d192ed03,
            backend: "legacy".into(),
            key_kind: "fixed16".into(),
            directory: PathBuf::new(),
        };
        let mut args = std::env::args().skip(1);
        while let Some(flag) = args.next() {
            if flag == "--help" {
                println!(
                    "--rows N --queries N --samples N --seed NONZERO_U64 --backend legacy|resident --keys fixed16|hostname --directory FRESH_PATH"
                );
                std::process::exit(0);
            }
            let value = args.next().ok_or("missing option value")?;
            match flag.as_str() {
                "--rows" => c.rows = value.parse()?,
                "--queries" => c.queries = value.parse()?,
                "--samples" => c.samples = value.parse()?,
                "--seed" => c.seed = value.parse()?,
                "--backend" => c.backend = value,
                "--keys" => c.key_kind = value,
                "--directory" => c.directory = PathBuf::from(value),
                _ => return Err(format!("unknown flag {flag}").into()),
            }
        }
        if c.rows == 0 || c.rows > 10_000_000 || c.queries == 0 || c.samples < 3 || c.seed == 0 {
            return Err("rows/queries/seed must be positive, rows <=10M, samples >=3".into());
        }
        if !["legacy", "resident"].contains(&c.backend.as_str())
            || !["fixed16", "hostname"].contains(&c.key_kind.as_str())
            || c.directory.as_os_str().is_empty()
            || c.directory.exists()
        {
            return Err("invalid backend/keys or directory missing/already exists".into());
        }
        Ok(c)
    }
}

pub struct Record {
    pub key: Vec<u8>,
    pub value: [u8; 32],
}

pub fn key(kind: &str, number: u64) -> Vec<u8> {
    if kind == "fixed16" {
        let mut key = Vec::with_capacity(16);
        key.extend(number.to_be_bytes());
        key.extend(number.wrapping_mul(0x9e3779b97f4a7c15).to_be_bytes());
        key
    } else {
        format!(
            "host-{number:012x}.s{}.region-{}.example.net",
            number % 97,
            number % 17
        )
        .into_bytes()
    }
}

pub fn records(c: &Config) -> Vec<Record> {
    let mut records: Vec<_> = (0..c.rows)
        .map(|i| {
            let mut value = [0; 32];
            for part in 0..4 {
                value[part * 8..part * 8 + 8].copy_from_slice(
                    &(i as u64)
                        .wrapping_mul(0x9e3779b97f4a7c15)
                        .wrapping_add(part as u64 + 1)
                        .to_be_bytes(),
                );
            }
            Record {
                key: key(&c.key_kind, i as u64 * 2),
                value,
            }
        })
        .collect();
    records.sort_unstable_by(|a, b| a.key.cmp(&b.key));
    records
}

pub fn random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

pub fn queries(c: &Config, workload: &str) -> Vec<Vec<u8>> {
    let mut state = c.seed;
    (0..c.queries)
        .map(|_| {
            let rand = random(&mut state);
            let index = if workload == "hot90" && rand % 10 != 0 {
                random(&mut state) as usize % (c.rows / 100).max(1)
            } else {
                random(&mut state) as usize % c.rows
            };
            let missing = workload == "miss" || (workload == "mixed50" && rand & 1 == 1);
            key(&c.key_kind, index as u64 * 2 + u64::from(missing))
        })
        .collect()
}

pub fn digest(value: Option<&[u8]>) -> u64 {
    value.map_or(0x517cc1b727220a95, |bytes| {
        let first = u64::from_be_bytes(bytes[..8].try_into().unwrap());
        let last = u64::from_be_bytes(bytes[bytes.len() - 8..].try_into().unwrap());
        first ^ last.rotate_left(17) ^ bytes.len() as u64
    })
}
