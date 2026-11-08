#![allow(dead_code)]
use std::path::PathBuf;
use ulmdb::{
    lookup::{HashLimits, Options},
    resident,
};

pub struct Dir(pub PathBuf);
impl Dir {
    pub fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "lookup-strategy-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub fn limits() -> HashLimits {
    HashLimits {
        max_entries: 10_000,
        max_index_bytes: 8 << 20,
        max_value_bytes: 4 << 20,
        max_total_value_bytes: 16 << 20,
    }
}
pub fn hash() -> Options {
    Options::Hash(limits())
}
pub fn resident_options() -> resident::Options {
    let mut o = resident::Options::default();
    o.pool.kv_pages = 512;
    o.pool.tiny_pages = 32;
    o.pool.max_reserved_bytes = 8 << 20;
    o.pool.max_metadata_bytes = 16 << 20;
    o.pool.max_views = 64;
    o.wal_capacity = 4 << 20;
    o.graph_read_budget = 2_000_000;
    o.auto_collect_below = 32;
    o
}
pub fn fixtures() -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut rows = vec![
        (vec![], vec![]),
        (vec![0, 0xff], vec![0, 0xff, 0]),
        (b"large".to_vec(), vec![0x5a; 65_537]),
    ];
    rows.extend((0..500u32).map(|i| {
        (
            format!("key-{i:08x}").into_bytes(),
            i.to_be_bytes().repeat((i as usize % 9) + 1),
        )
    }));
    rows.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    rows
}

pub fn error_key(result: ulmdb::Result<Option<&[u8]>>) -> String {
    result.unwrap_err().to_string()
}
