pub type Record<'a> = (&'a [u8], &'a [u8]);

/// Benchmark-only Strategy: every candidate reads the same pinned snapshot.
/// Production ULMDB dispatch, persisted formats and write semantics are untouched.
pub trait Lookup {
    fn name(&self) -> &'static str;
    fn get(&self, key: &[u8]) -> Option<&[u8]>;
    fn rows<'a>(&'a self, key: &[u8], limit: usize) -> Vec<Record<'a>>;
    fn scan_digest(&self, key: &[u8], limit: usize) -> u64;
}
pub fn value_digest(value: &[u8]) -> u64 {
    u64::from_be_bytes(value[..8].try_into().unwrap())
        ^ u64::from_be_bytes(value[8..16].try_into().unwrap())
}
pub fn row_digest(key: &[u8], value: &[u8]) -> u64 {
    value_digest(value)
        .wrapping_add(u64::from(key[0]))
        .wrapping_add(u64::from(key[key.len() - 1]))
}
