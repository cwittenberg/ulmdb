//! The backend is selected at compile time; timed query loops stay monomorphic.
use crate::Result;
use serde_json::{Value, json};
use std::path::Path;
#[cfg(not(feature = "resident"))]
pub use ulmdb::Environment;
#[cfg(feature = "resident")]
pub use ulmdb::resident::Environment;
#[cfg(not(feature = "resident"))]
pub const LABEL: &str = "default_file_legacy";
#[cfg(feature = "resident")]
pub const LABEL: &str = "experimental_resident_compact_borrowed";
#[cfg(feature = "resident")]
fn options() -> ulmdb::resident::Options {
    let mut o = ulmdb::resident::Options::default();
    o.pool.kv_pages = 1024;
    o.pool.tiny_pages = 1024;
    o.pool.max_reserved_bytes = 64 << 20;
    o.pool.max_metadata_bytes = 64 << 20;
    o.wal_capacity = 64 << 20;
    o.auto_collect_below = 256;
    o
}
pub fn create(path: &Path) -> Result<Environment> {
    #[cfg(not(feature = "resident"))]
    {
        let e = Environment::create(path)?;
        if e.publication_mode() != ulmdb::PublicationMode::Legacy {
            return Err("shipped default publication changed; update benchmark label".into());
        }
        Ok(e)
    }
    #[cfg(feature = "resident")]
    {
        Ok(Environment::create_with_encoding(
            path,
            options(),
            ulmdb::resident::RedoFormat::CompactV2,
            ulmdb::resident::RedoEncoding::Borrowed,
        )?)
    }
}
pub fn open(path: &Path) -> Result<Environment> {
    #[cfg(not(feature = "resident"))]
    {
        Ok(Environment::open(path)?)
    }
    #[cfg(feature = "resident")]
    {
        Ok(Environment::open_with_encoding(
            path,
            options(),
            ulmdb::resident::RedoFormat::CompactV2,
            ulmdb::resident::RedoEncoding::Borrowed,
        )?)
    }
}
pub fn after_setup(e: &Environment) -> Result<()> {
    #[cfg(feature = "resident")]
    e.enable_collect_cache(ulmdb::resident::CollectCacheLimits::default())?;
    #[cfg(not(feature = "resident"))]
    let _ = e;
    Ok(())
}
/// These observations happen outside held writes and all timed loops.
pub fn observe(e: &Environment) -> Result<Value> {
    let r = e.begin_read();
    #[cfg(not(feature = "resident"))]
    {
        Ok(
            json!({"backend":LABEL,"experimental":false,"publication":"Legacy: Environment::create shipped default","residency":"Demand","sequence":r.sequence(),"commit_identity":format!("{:032x}",r.commit_identity()),"durability":"normal file backend commit protocol; no relaxed-sync flags"}),
        )
    }
    #[cfg(feature = "resident")]
    {
        let p = e.pool_stats()?;
        let w = e.private_workspace_stats()?;
        let (frame, recipe) = e.redo_workspace_bytes()?;
        let cache = e
            .collect_cache_stats()?
            .ok_or("experimental Collect cache not enabled")?;
        Ok(
            json!({"backend":LABEL,"experimental":true,"publication":"CompactV2 Borrowed","residency":"anonymous demand mappings","sequence":r.sequence(),"commit_identity":format!("{:032x}",r.commit_identity()),"wal_selected_end":e.wal_bytes(),"wal_capacity":e.wal_capacity(),"pool_kv_pages":1024,"pool_tiny_pages":1024,"auto_collect_below":256,"pool_metadata_bound_bytes":p.metadata_bound_bytes,"pool_rounded_mapping_bytes":p.rounded_mapping_bytes,"pool_reused_pages":p.reused_pages,"restored_configuration":"iteration34","metadata_reuse_enabled":true,"private_admitted_capacity_hints":{"frozen":w.frozen_capacity_bytes,"index":w.index_capacity_bytes,"packed":w.packed_capacity_bytes,"charged":w.charged_capacity_bytes},"private_current_global_reserved_bytes":w.aggregate_reserved_bytes,"private_current_global_budget_bytes":w.aggregate_budget_bytes,"private_opaque_leased_flag":w.leased,"retained_frame_workspace_bytes":frame,"retained_recipe_workspace_bytes":recipe,"collect_cache":{"hits":cache.hits,"misses":cache.misses,"retained_bytes":cache.retained_bytes,"entries":cache.entries},"accounting_scope":"admission hints and current charges are not RSS or live-owner counts; allocator/container slack is separate","durability":"normal ordered WAL data and witness fences; no relaxed-sync mode"}),
        )
    }
}
