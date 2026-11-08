use crate::workload::{Config, Record, Result};
use std::{ops::Bound, time::Instant};
use ulmdb::DatabaseKind;

pub enum Environment {
    Legacy(ulmdb::Environment),
    Resident(ulmdb::resident::Environment),
}
pub enum Snapshot {
    Legacy(ulmdb::ReadTxn),
    Resident(ulmdb::resident::ReadTxn),
}
pub enum View<'a> {
    Legacy(ulmdb::KvRead<'a>),
    Resident(ulmdb::resident::KvRead<'a>),
}

impl Environment {
    pub fn create(c: &Config, rows: &[Record]) -> Result<Self> {
        let start = Instant::now();
        let env = if c.backend == "legacy" {
            let env = ulmdb::Environment::create(&c.directory)?;
            let mut write = env.begin_write()?;
            write.create_database(b"kv", DatabaseKind::Kv)?;
            write.load_sorted_kv(b"kv", rows.iter().map(|r| Ok((&r.key, &r.value))))?;
            write.commit()?;
            Self::Legacy(env)
        } else {
            let mut options = ulmdb::resident::Options::default();
            options.pool.kv_pages = (c.rows / 40 + 4096).max(8192);
            options.pool.tiny_pages = 0;
            options.pool.max_reserved_bytes = options.pool.kv_pages as u64 * 8192;
            options.pool.max_metadata_bytes = 128 << 20;
            // Resident's admitted pilot format has a hard 256-MiB WAL cap.
            // Keep the existing smaller-fixture settings, and fail explicitly if
            // the complete larger workload cannot fit the supported capacity.
            options.wal_capacity = (c.rows as u64 * 768 + (128 << 20))
                .next_power_of_two()
                .min(256 << 20);
            options.graph_read_budget = 2_000_000_000;
            options.auto_collect_below = options.pool.kv_pages / 4;
            println!(
                "{{\"type\":\"resident_settings\",\"kv_pages\":{},\"wal_capacity\":{},\"compact_v2\":true,\"borrowed_encoding\":true}}",
                options.pool.kv_pages, options.wal_capacity
            );
            let env = ulmdb::resident::Environment::create_with_encoding(
                &c.directory,
                options,
                ulmdb::resident::RedoFormat::CompactV2,
                ulmdb::resident::RedoEncoding::Borrowed,
            )?;
            env.enable_collect_cache(ulmdb::resident::CollectCacheLimits::default())?;
            let mut write = env.begin_write()?;
            write.create_database(b"kv", DatabaseKind::Kv)?;
            write.commit()?;
            for chunk in rows.chunks(1000) {
                let mut write = env.begin_write()?;
                for row in chunk {
                    write.put(b"kv", &row.key, &row.value)?;
                }
                write.commit()?;
            }
            Self::Resident(env)
        };
        println!(
            "{{\"type\":\"fixture_load\",\"elapsed_ns\":{},\"timing_scope\":\"setup plus durable load; Legacy sorted-load, Resident 1000-row puts; not compared as equivalent loaders\"}}",
            start.elapsed().as_nanos()
        );
        Ok(env)
    }
    pub fn snapshot(&self) -> Snapshot {
        match self {
            Self::Legacy(e) => Snapshot::Legacy(e.begin_read()),
            Self::Resident(e) => Snapshot::Resident(e.begin_read()),
        }
    }
    pub fn update(&self, key: &[u8], value: &[u8]) -> Result<()> {
        match self {
            Self::Legacy(e) => {
                let mut w = e.begin_write()?;
                w.put(b"kv", key, value)?;
                w.commit()?;
            }
            Self::Resident(e) => {
                let mut w = e.begin_write()?;
                w.put(b"kv", key, value)?;
                w.commit()?;
            }
        }
        Ok(())
    }
}

impl Snapshot {
    pub fn view(&self) -> Result<View<'_>> {
        Ok(match self {
            Self::Legacy(s) => View::Legacy(s.kv(b"kv")?),
            Self::Resident(s) => View::Resident(s.kv(b"kv")?),
        })
    }
    pub fn sequence(&self) -> u64 {
        match self {
            Self::Legacy(s) => s.sequence(),
            Self::Resident(s) => s.sequence(),
        }
    }
    pub fn commit(&self) -> u128 {
        match self {
            Self::Legacy(s) => s.commit_identity(),
            Self::Resident(s) => s.commit_identity(),
        }
    }
}

impl<'a> View<'a> {
    pub fn get(&self, key: &[u8]) -> Option<&'a [u8]> {
        match self {
            Self::Legacy(v) => v.get(key).unwrap(),
            Self::Resident(v) => v.get(key).unwrap(),
        }
    }
    pub fn visit(&self, mut callback: impl FnMut(&'a [u8], &'a [u8])) -> Result<()> {
        match self {
            Self::Legacy(v) => {
                for pair in v.cursor(Bound::Unbounded, Bound::Unbounded, false)? {
                    let (k, v) = pair?;
                    callback(k, v);
                }
            }
            Self::Resident(v) => {
                for pair in v.cursor(Bound::Unbounded, Bound::Unbounded, false)? {
                    let (k, v) = pair?;
                    callback(k, v);
                }
            }
        }
        Ok(())
    }
    pub fn scan(&self, lower: &[u8], count: usize) -> u64 {
        use crate::workload::digest;
        match self {
            Self::Legacy(v) => v
                .cursor(Bound::Included(lower), Bound::Unbounded, false)
                .unwrap()
                .take(count)
                .map(|p| digest(Some(p.unwrap().1)))
                .fold(0, u64::wrapping_add),
            Self::Resident(v) => v
                .cursor(Bound::Included(lower), Bound::Unbounded, false)
                .unwrap()
                .take(count)
                .map(|p| digest(Some(p.unwrap().1)))
                .fold(0, u64::wrapping_add),
        }
    }
}
