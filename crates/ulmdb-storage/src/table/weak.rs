//! Weak ownership of ordinary immutable mapping chunks, without snapshot aliases.
//! This primitive grants no source, graph, physical pin or residency authority.
use super::*;
use std::sync::Weak;

/// An ordinary table's geometry and weak chunk owners. Snapshot Arc uniqueness
/// is unaffected: this type never downgrades a Snapshot Arc.
pub struct WeakPlainSegmentTable {
    creator: u32,
    // Immutable wrapper also makes inherited destruction creator-safe: a child
    // cannot destroy containers/control blocks inherited during parent mutation.
    state: Arc<WeakTableState>,
}

struct WeakTableState {
    page_size: ulmdb_core::PageSize,
    value_limit: ulmdb_core::ValueLimit,
    first_chunk: u64,
    chunks: Vec<Option<Weak<Chunk>>>,
    live_segments: usize,
    read_ahead: crate::ReadAhead,
}

impl Drop for WeakPlainSegmentTable {
    fn drop(&mut self) {
        if self.creator != std::process::id() {
            std::mem::forget(self.state.clone());
        }
    }
}

impl SegmentTable {
    /// Retain no mapping, byte view, pin or fixed-address capability. None means
    /// this profile stays on the established full-admission path. This is only
    /// an ownership primitive, never a graph/content/namespace certificate.
    pub fn downgrade_plain(&self) -> Result<Option<WeakPlainSegmentTable>> {
        if self.cached.is_some() || self.chunk_budget.is_some() || self.transform.is_some() {
            return Ok(None);
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if self.fixed.is_some() || self.fixed_declaration.is_some() || self.fixed_external.is_some()
        {
            return Ok(None);
        }
        if self
            .mappings()
            .any(|mapping| mapping.range.is_some() || mapping.entry.cohort != 0)
        {
            return Ok(None);
        }
        Ok(Some(WeakPlainSegmentTable {
            creator: std::process::id(),
            state: Arc::new(WeakTableState {
                page_size: self.page_size,
                value_limit: self.value_limit,
                first_chunk: self.first_chunk,
                chunks: self
                    .chunks
                    .iter()
                    .map(|chunk| chunk.as_ref().map(Arc::downgrade))
                    .collect(),
                live_segments: self.live_segments,
                read_ahead: self.read_ahead,
            }),
        }))
    }
}

impl WeakPlainSegmentTable {
    /// Upgrade only while every original chunk still has a genuine strong
    /// reader owner. Expiry of one chunk returns None and drops partial upgrades.
    /// A successful table still requires independent fresh pin/namespace/content
    /// authentication and fresh residency admission by its caller.
    pub fn upgrade(&self) -> Result<Option<SegmentTable>> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited weak ordinary table"));
        }
        let mut chunks = Vec::with_capacity(self.state.chunks.len());
        for chunk in &self.state.chunks {
            chunks.push(match chunk {
                None => None,
                Some(chunk) => match chunk.upgrade() {
                    Some(chunk) => Some(chunk),
                    None => return Ok(None),
                },
            });
        }
        let mut table = SegmentTable::empty_with_read_ahead(self.state.read_ahead)?
            .with_page_size(self.state.page_size)?
            .with_value_limit(self.state.value_limit)?;
        table.first_chunk = self.state.first_chunk;
        table.chunks = chunks;
        table.live_segments = self.state.live_segments;
        // The new reader is not an append lineage or an old readiness token.
        table.append_lineage = None;
        table.residency_identity = SegmentTable::fresh_residency_identity()?;
        Ok(Some(table))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Arena, WriteStore};

    #[test]
    fn weak_plain_table_shares_live_bytes_but_releases_mapping_owners_at_last_end() {
        let directory = std::env::temp_dir().join(format!(
            "ulmdb-weak-table-{}",
            crate::new_identity().unwrap()
        ));
        crate::create_layout(&directory).unwrap();
        let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
        let payload = arena.put_payload(b"owned by active readers only").unwrap();
        let (table, _) = arena.seal(&directory).unwrap();
        let mapping = table
            .chunks
            .iter()
            .flatten()
            .flat_map(|chunk| chunk.iter().flatten())
            .next()
            .unwrap();
        let mapping_weak = Arc::downgrade(mapping);
        let weak = table.downgrade_plain().unwrap().unwrap();
        let second = weak.upgrade().unwrap().unwrap();
        assert_eq!(
            table.payload(payload).unwrap().as_ptr(),
            second.payload(payload).unwrap().as_ptr()
        );
        drop(table);
        assert!(mapping_weak.upgrade().is_some());
        assert_eq!(
            second.payload(payload).unwrap(),
            b"owned by active readers only"
        );
        drop(second);
        // Weak control blocks may remain, but Mapping's Drop (and DataMapping's
        // unmap) has run. No dormant data owner is retained by the cache token.
        assert!(mapping_weak.upgrade().is_none());
        assert!(weak.upgrade().unwrap().is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn weak_plain_table_foreign_use_precedes_state_and_foreign_drop_leaks_wrapper() {
        let table = SegmentTable::default();
        let mut weak = table.downgrade_plain().unwrap().unwrap();
        let state = Arc::downgrade(&weak.state);
        weak.creator = weak.creator.wrapping_add(1);
        assert!(matches!(
            weak.upgrade(),
            Err(Error::InvalidInput("inherited weak ordinary table"))
        ));
        drop(weak);
        assert!(state.upgrade().is_some());
    }
}
