//! Actual reachable native allocations in persisted logical-page geometry.
//! MAIN entry count includes synthesized named descriptors, whose active writer
//! catalog is in memory; no physical catalog pages are invented for that facade.
use super::*;
pub(super) fn mdb_fields(stats: ulmdb::DatabaseStats) -> FfiResult<MDB_stat> {
    Ok(MDB_stat {
        ms_psize: u32::try_from(stats.page_size).map_err(|_| -30792)?,
        ms_depth: stats.depth,
        ms_branch_pages: usize::try_from(stats.branch_pages).map_err(|_| -30792)?,
        ms_leaf_pages: usize::try_from(stats.leaf_pages).map_err(|_| -30792)?,
        ms_overflow_pages: usize::try_from(stats.overflow_pages).map_err(|_| -30792)?,
        ms_entries: usize::try_from(stats.entries).map_err(|_| -30792)?,
    })
}
impl MDB_txn {
    pub(super) fn entry_count(&self, h: &ResolvedAlias) -> FfiResult<usize> {
        self.alias_physical_stat(h).map(|stats| stats.ms_entries)
    }
    pub(super) fn alias_physical_stat(&self, h: &ResolvedAlias) -> FfiResult<MDB_stat> {
        let native = match (&h.binding.view, self.state.as_ref()) {
            (DataView::Writable(view), Some(State::Write(w))) => w.view_database_stats(view),
            (DataView::Writable(view), Some(State::Child(w))) => w.view_database_stats(view),
            (DataView::Readonly(view), Some(State::PrivateRead(r))) => {
                r.read_scope().database_stats(view)
            }
            (DataView::Direct, _) => {
                if let Some(detached) = self.detached_database(h.id())? {
                    match self.state.as_ref() {
                        Some(State::Write(w)) => w.detached_database_stats(&detached),
                        Some(State::Child(w)) => w.detached_database_stats(&detached),
                        _ => Err(Error::StaleHandle),
                    }
                } else {
                    self.check_native_actual(h.actual()).map_err(code)?;
                    match self.state.as_ref() {
                        Some(State::Read(r)) => r.database_stats_handle(h.actual()),
                        Some(State::PrivateRead(r)) => r.database_stats_handle(h.actual()),
                        Some(State::Write(w)) => w.database_stats_handle(h.actual()),
                        Some(State::Child(w)) => w.database_stats_handle(h.actual()),
                        _ => Err(Error::InvalidInput("inactive statistics source")),
                    }
                }
            }
            _ => Err(Error::InvalidInput("statistics alias source")),
        }
        .map_err(code)?;
        let mut result = mdb_fields(native)?;
        if h.is_main() {
            result.ms_entries = result
                .ms_entries
                .checked_add(self.catalog_pairs()?.len())
                .ok_or(-30792)?;
        }
        Ok(result)
    }
}
