#[derive(Clone, Copy, Debug)]
pub enum KeyKind {
    Fixed16,
    Hostname,
}
impl KeyKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Fixed16 => "fixed16",
            Self::Hostname => "hostname",
        }
    }
    pub fn key(self, id: usize, missing: bool) -> Vec<u8> {
        let scrambled = mix(id as u64);
        match self {
            Self::Fixed16 => {
                let mut key = Vec::with_capacity(16);
                key.extend_from_slice(&scrambled.to_be_bytes());
                key.extend_from_slice(&((id as u64) * 2 + u64::from(missing)).to_be_bytes());
                key
            }
            Self::Hostname => {
                let labels = [
                    "www",
                    "api",
                    "cdn",
                    "telemetry",
                    "mail",
                    "edge",
                    "static",
                    "collector",
                ];
                format!(
                    "{}-{scrambled:016x}.zone-{:x}.example.ne{}",
                    labels[id % labels.len()],
                    id % 4096,
                    if missing { 'u' } else { 't' }
                )
                .into_bytes()
            }
        }
    }
}
pub fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
pub fn next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9e3779b97f4a7c15);
    mix(*seed)
}
pub fn records(kind: KeyKind, count: usize) -> Vec<(Vec<u8>, [u8; 16])> {
    let mut rows: Vec<_> = (0..count)
        .map(|id| {
            let mut value = [0; 16];
            value[..8].copy_from_slice(&(id as u64).to_be_bytes());
            value[8..].copy_from_slice(&mix(id as u64).to_be_bytes());
            (kind.key(id, false), value)
        })
        .collect();
    rows.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    rows
}
pub fn queries(
    kind: KeyKind,
    count: usize,
    keys: usize,
    seed: u64,
    misses_percent: u64,
) -> Vec<Vec<u8>> {
    let mut state = seed;
    (0..count)
        .map(|_| {
            let id = (next(&mut state) % keys as u64) as usize;
            let missing = next(&mut state) % 100 < misses_percent;
            kind.key(id, missing)
        })
        .collect()
}
