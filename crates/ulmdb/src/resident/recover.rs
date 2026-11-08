//! Fresh physical owners, authenticated checkpoint and bounded fixed-slot tail replay.
use super::{Options, Snapshot, audit};
use crate::{Error, Result};
use std::sync::Arc;
use ulmdb_storage::resident::{Pool, journal::Recovery};
pub(super) fn rebuild(
    options: Options,
    recovery: &mut Recovery<'_>,
) -> Result<(Arc<Pool>, Snapshot)> {
    let boot = recovery.boot();
    if boot.kv_pages as usize != options.pool.kv_pages
        || boot.tiny_pages as usize != options.pool.tiny_pages
        || recovery.capacity() != options.wal_capacity
    {
        return Err(Error::InvalidInput(
            "resident persisted geometry differs from options",
        ));
    }
    let pool = Pool::create_with_transform(options.pool, recovery.transform_context().cloned())?;
    let mut view = recovery.restore_checkpoint(&pool)?;
    let mut remaining = options.graph_read_budget;
    while let Some(decoded) = recovery.next_record()? {
        let record = decoded.record();
        let candidate = view.replay_literals(record)?;
        if !record.retired.is_empty() {
            let checked = audit::audit(
                candidate.view(),
                record.root,
                record.next_database,
                remaining,
            )?;
            remaining = remaining
                .checked_sub(checked.reads)
                .ok_or(Error::LimitExceeded)?;
        }
        view = candidate.activate();
    }
    let state = recovery.state();
    let checked = audit::audit(&view, state.root(), state.next_database(), remaining)?;
    let snapshot = Snapshot {
        image: view,
        catalog: super::catalog::Catalog::from_map_with_capacity(
            checked.catalog,
            checked.catalog_capacity,
        )?,
        root: state.root(),
        next_database: state.next_database(),
        receipt: state.selected(),
        commit: state.commit(),
    };
    Ok((pool, snapshot))
}

/// RL2 never invokes an RL1 validator or synthesizes an RL1 record. Every source
/// resolves from the exact preceding reconstructed view while that view is held.
pub(super) fn rebuild_v2(
    options: Options,
    recovery: &mut ulmdb_storage::resident::journal_v2::Recovery<'_>,
) -> Result<(Arc<Pool>, Snapshot)> {
    let boot = recovery.boot();
    if (boot.kv_pages as usize, boot.tiny_pages as usize)
        != (options.pool.kv_pages, options.pool.tiny_pages)
        || recovery.capacity() != options.wal_capacity
    {
        return Err(Error::InvalidInput(
            "RL2 persisted geometry differs from options",
        ));
    }
    let pool = Pool::create_with_transform(options.pool, recovery.transform_context().cloned())?;
    let mut view = recovery.restore_checkpoint(&pool)?;
    let mut remaining = options.graph_read_budget;
    loop {
        let parent = recovery.state().selected();
        let parent_commit = recovery.state().commit();
        let Some(decoded) = recovery.next_record()? else {
            break;
        };
        let record = decoded.record();
        if record.parent != parent || record.parent_commit != parent_commit {
            return Err(Error::Corrupt("RL2 replay source parent mismatch"));
        }
        // next_record has already authenticated the frame and prestate Live/
        // Free/Retired/source closure. Physical source bytes remain in old view.
        let candidate = view.replay_recipes(boot, &record.allocations, &record.retired)?;
        if !record.retired.is_empty() {
            let checked = audit::audit(
                candidate.view(),
                record.root,
                record.next_database,
                remaining,
            )?;
            remaining = remaining
                .checked_sub(checked.reads)
                .ok_or(Error::LimitExceeded)?;
        }
        // Old sources drop only after the new immutable candidate is complete.
        view = candidate.activate();
    }
    let state = recovery.state();
    let checked = audit::audit(&view, state.root(), state.next_database(), remaining)?;
    Ok((
        pool,
        Snapshot {
            image: view,
            catalog: super::catalog::Catalog::from_map_with_capacity(
                checked.catalog,
                checked.catalog_capacity,
            )?,
            root: state.root(),
            next_database: state.next_database(),
            receipt: state.selected(),
            commit: state.commit(),
        },
    ))
}
