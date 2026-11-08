use crate::{
    data::{Config, Record},
    protocol::{Engine, Lookup, Result, Snapshot, context},
};
use serde_json::{Value, json};
use std::ops::Bound;
use ulmdb::lookup::{HashLimits, Options, PreparedLookup};

#[allow(clippy::large_enum_variant)] // Selected outside every timed lookup; no heap indirection.
pub enum Source {
    Legacy(ulmdb::Environment),
    Resident(ulmdb::resident::Environment),
}
pub enum Read {
    Legacy(ulmdb::ReadTxn),
    Resident(ulmdb::resident::ReadTxn),
}
pub fn options(keys: usize) -> ulmdb::resident::Options {
    let mut o = ulmdb::resident::Options::default();
    // Allow two complete live hostname generations plus a1000-update
    // writer transaction; an old snapshot retains physical pool versions.
    o.pool.kv_pages = (keys.div_ceil(24) + 8192).max(8192);
    o.pool.tiny_pages = 0;
    o.pool.max_reserved_bytes = o.pool.kv_pages as u64 * 8192;
    o.pool.max_metadata_bytes = 256 << 20;
    o.wal_capacity = 256 << 20;
    o.graph_read_budget = 2_000_000_000;
    o.auto_collect_below = o.pool.kv_pages / 4;
    o
}
impl Source {
    pub fn create(c: &Config) -> Result<Self> {
        Ok(if c.backend == "legacy" {
            Self::Legacy(ulmdb::Environment::create(&c.directory)?)
        } else {
            let env = ulmdb::resident::Environment::create_with_encoding(
                &c.directory,
                options(c.keys),
                ulmdb::resident::RedoFormat::CompactV2,
                ulmdb::resident::RedoEncoding::Borrowed,
            )?;
            env.enable_collect_cache(ulmdb::resident::CollectCacheLimits::default())?;
            Self::Resident(env)
        })
    }
    pub fn open(c: &Config) -> Result<Self> {
        Ok(if c.backend == "legacy" {
            Self::Legacy(ulmdb::Environment::open(&c.directory)?)
        } else {
            let env = ulmdb::resident::Environment::open_with_encoding(
                &c.directory,
                options(c.keys),
                ulmdb::resident::RedoFormat::CompactV2,
                ulmdb::resident::RedoEncoding::Borrowed,
            )?;
            env.enable_collect_cache(ulmdb::resident::CollectCacheLimits::default())?;
            Self::Resident(env)
        })
    }
    pub fn load(&self, rows: &[Record]) -> Result<&'static str> {
        match self {
            Self::Legacy(e) => {
                let mut w = e.begin_write()?;
                w.create_database(b"kv", ulmdb::DatabaseKind::Kv)?;
                w.load_sorted_kv(b"kv", rows.iter().map(|r| Ok((&r.key, &r.value))))?;
                w.commit()?;
                Ok("sorted bulk construction and one durable transaction")
            }
            Self::Resident(e) => {
                let mut w = e.begin_write()?;
                w.create_database(b"kv", ulmdb::DatabaseKind::Kv)?;
                w.commit()?;
                for (chunk, batch) in rows.chunks(1000).enumerate() {
                    let mut w = e
                        .begin_write()
                        .map_err(|e| context(e, &format!("Resident load begin chunk={chunk}")))?;
                    for r in batch {
                        w.put(b"kv", &r.key, &r.value)
                            .map_err(|e| context(e, &format!("Resident load put chunk={chunk}")))?;
                    }
                    w.commit()
                        .map_err(|e| context(e, &format!("Resident load commit chunk={chunk}")))?;
                }
                Ok("sorted puts in durable batches of1000; separate catalog transaction")
            }
        }
    }
}
impl Engine for Source {
    fn before_pin(&self) -> Result<Value> {
        if let Self::Resident(e) = self {
            let before = self.diagnostics();
            let collection = e
                .collect()
                .map_err(|e| context(e, "Resident collect before pin"))?;
            Ok(
                json!({"maintenance":"explicit ordinary Collect before retaining long-lived snapshot",
                "before":before,"report":format!("{:?}",collection),"after":self.diagnostics()}),
            )
        } else {
            Ok(json!({"maintenance":"none"}))
        }
    }

    fn diagnostics(&self) -> Value {
        match self {
            Self::Legacy(_) => json!({"engine":"Legacy"}),
            Self::Resident(e) => json!({"engine":"Resident", "options":format!("{:?}",e.options()),
                "pool":format!("{:?}",e.pool_stats()),"private_workspace":format!("{:?}",e.private_workspace_stats()),
                "overflow":format!("{:?}",e.overflow_stats())}),
        }
    }

    type Read<'a> = Read;
    fn snapshot(&self) -> Result<Read> {
        Ok(match self {
            Self::Legacy(e) => Read::Legacy(e.begin_read()),
            Self::Resident(e) => Read::Resident(e.begin_read()),
        })
    }
    fn apply(&self, rows: &[Record], changes: &[(usize, [u8; 32])]) -> Result<()> {
        match self {
            Self::Legacy(e) => {
                let mut w = e
                    .begin_write()
                    .map_err(|e| context(e, "Legacy begin_write"))?;
                for &(id, v) in changes {
                    w.put(b"kv", &rows[id].key, &v)
                        .map_err(|e| context(e, &format!("Legacy put row={id}")))?;
                }
                w.commit().map_err(|e| context(e, "Legacy commit"))?;
            }
            Self::Resident(e) => {
                let mut w = e
                    .begin_write()
                    .map_err(|e| context(e, "Resident begin_write"))?;
                for &(id, v) in changes {
                    w.put(b"kv", &rows[id].key, &v)
                        .map_err(|e| context(e, &format!("Resident put row={id}")))?;
                }
                w.commit().map_err(|e| context(e, "Resident commit"))?;
            }
        }
        Ok(())
    }
}
pub enum Prepared<'a> {
    Legacy(ulmdb::KvRead<'a>),
    Resident(ulmdb::resident::KvRead<'a>),
    Hash(PreparedLookup<'a>),
}
impl Snapshot for Read {
    type Reader<'a> = Prepared<'a>;
    fn prepare(&self, c: &Config) -> Result<Self::Reader<'_>> {
        if c.arm == "hash" {
            let options = Options::Hash(HashLimits {
                max_entries: c.keys,
                max_index_bytes: 512 << 20,
                max_value_bytes: 32,
                max_total_value_bytes: c.keys * 32,
            });
            Ok(Prepared::Hash(match self {
                Self::Legacy(r) => r.prepare_lookup(b"kv", options)?,
                Self::Resident(r) => r.prepare_lookup(b"kv", options)?,
            }))
        } else {
            // Preserve the original allocation-free native prepared path. The
            // new Ordered Strategy wrapper is not charged to the baseline.
            Ok(match self {
                Self::Legacy(r) => Prepared::Legacy(r.kv(b"kv")?),
                Self::Resident(r) => Prepared::Resident(r.kv(b"kv")?),
            })
        }
    }
    fn identity(&self) -> Value {
        match self {
            Self::Legacy(r) => {
                json!({"sequence":r.sequence(),"commit":r.commit_identity().to_string()})
            }
            Self::Resident(r) => {
                json!({"sequence":r.sequence(),"commit":r.commit_identity().to_string()})
            }
        }
    }
}
impl Lookup for Prepared<'_> {
    fn get(&self, key: &[u8]) -> Result<Option<&[u8]>> {
        Ok(match self {
            Self::Legacy(r) => r.get(key)?,
            Self::Resident(r) => r.get(key)?,
            Self::Hash(r) => r.get(key)?,
        })
    }
    fn visit(
        &self,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
        limit: usize,
        callback: impl FnMut(&[u8], &[u8]),
    ) -> Result<()> {
        match self {
            Self::Legacy(r) => consume(r.cursor(lower, upper, reverse)?, limit, callback),
            Self::Resident(r) => consume(r.cursor(lower, upper, reverse)?, limit, callback),
            Self::Hash(r) => consume(r.cursor(lower, upper, reverse)?, limit, callback),
        }
    }
    fn stats(&self) -> Value {
        let Self::Hash(r) = self else {
            return json!({"algorithm":"original native KvRead", "index_bytes":0,"enumerated":false});
        };
        let s = r.stats();
        json!({"algorithm":format!("{:?}",s.algorithm),"entries":s.entries,"index_bytes":s.index_bytes,
            "referenced_key_bytes":s.referenced_key_bytes,"total_value_bytes":s.total_value_bytes,
            "capacity":s.capacity,"enumerated":s.enumerated,"owned_bytes_scope":"actual index slot allocation; borrowed source mappings and retained generations excluded"})
    }
}
fn consume<'a>(
    cursor: impl Iterator<Item = ulmdb::Result<(&'a [u8], &'a [u8])>>,
    limit: usize,
    mut callback: impl FnMut(&[u8], &[u8]),
) -> Result<()> {
    for pair in cursor.take(limit) {
        let (k, v) = pair?;
        callback(k, v);
    }
    Ok(())
}
