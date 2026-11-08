//! Snapshot inventory statistics; no traversal of user records or OS metadata.
use crate::{DatabaseKind, Environment, ReadTxn};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stats {
    pub page_size: usize,
    pub sequence: u64,
    pub generation: u128,
    pub commit: u128,
    pub databases: usize,
    pub kv_databases: usize,
    pub duplicate_databases: usize,
    pub ipv4_indexes: usize,
    pub ipv6_indexes: usize,
    pub segments: usize,
    /// Complete inventory bytes, including currently unreachable historical data.
    pub segment_bytes: u64,
    pub format_minor: u16,
}
impl ReadTxn {
    pub fn stats(&self) -> Stats {
        let manifest = &self.snapshot.manifest;
        let mut stats = Stats {
            page_size: manifest.page_size.get(),
            sequence: manifest.sequence,
            generation: manifest.generation,
            commit: manifest.commit,
            databases: self.snapshot.catalog.len(),
            kv_databases: 0,
            duplicate_databases: 0,
            ipv4_indexes: 0,
            ipv6_indexes: 0,
            segments: manifest.segments.len(),
            // At most 1,048,575 inventory entries of at most 256 MiB each.
            segment_bytes: manifest.segments.iter().map(|segment| segment.length).sum(),
            format_minor: if self.snapshot.range_backed {
                if manifest.format_minor == 6 { 6 } else { 5 }
            } else {
                manifest.format_minor
            },
        };
        for database in self.snapshot.catalog.values() {
            match database.kind {
                DatabaseKind::Kv => stats.kv_databases += 1,
                DatabaseKind::Duplicates { .. } => stats.duplicate_databases += 1,
                DatabaseKind::Ipv4 => stats.ipv4_indexes += 1,
                DatabaseKind::Ipv6 => stats.ipv6_indexes += 1,
            }
        }
        stats
    }
}
impl Environment {
    pub fn stats(&self) -> Stats {
        self.begin_read().stats()
    }
}
