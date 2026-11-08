//! Bottom-up canonical pages from strictly increasing raw byte keys.
mod incremental;
mod page;
pub use incremental::SortedBuilder;
#[cfg(test)]
mod tests;
use self::page::{Buffer, Node, rebalance};
use crate::{
    page::MAX_DEPTH,
    write::{Separator, branch_cell, leaf_cell},
};
use ulmdb_core::storage::{Error, Reference, Result, WriteStore};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BulkBuild {
    pub root: Reference,
    pub records: u64,
    pub pages: u64,
    pub leaf_pages: u64,
    pub branch_pages: u64,
    /// Zero for an empty tree, otherwise root level plus one.
    pub height: u16,
    /// At most64 levels; each buffers at most two pages, plus bounded rebalance scratch.
    pub frontier_levels: usize,
}
#[derive(Default)]
struct Level {
    pending: Option<Buffer>,
    current: Option<Buffer>,
}
struct Builder {
    levels: Vec<Level>,
    report: BulkBuild,
}
impl Builder {
    fn new() -> Self {
        Self {
            levels: Vec::new(),
            report: BulkBuild {
                root: Reference::NULL,
                records: 0,
                pages: 0,
                leaf_pages: 0,
                branch_pages: 0,
                height: 0,
                frontier_levels: 0,
            },
        }
    }
    fn level(&mut self, level: usize) -> Result<()> {
        if level >= usize::from(MAX_DEPTH) {
            return Err(Error::LimitExceeded);
        }
        if self.levels.len() <= level {
            self.levels.resize_with(level + 1, Level::default);
        }
        self.report.frontier_levels = self.report.frontier_levels.max(self.levels.len());
        Ok(())
    }
    fn seal<S: WriteStore>(&mut self, store: &mut S, page: Buffer) -> Result<Node> {
        let pages = self
            .report
            .pages
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        let counter = if page.level == 0 {
            &mut self.report.leaf_pages
        } else {
            &mut self.report.branch_pages
        };
        let count = counter.checked_add(1).ok_or(Error::LimitExceeded)?;
        let node = page.seal(store)?;
        *counter = count;
        self.report.pages = pages;
        Ok(node)
    }
    // Iterative carry: no recursive stack proportional to the input/tree size.
    fn node<S: WriteStore>(
        &mut self,
        store: &mut S,
        mut level: usize,
        mut node: Node,
    ) -> Result<()> {
        loop {
            self.level(level)?;
            if usize::from(node.level) + 1 != level {
                return Err(Error::Corrupt("bulk child level mismatch"));
            }
            if self.levels[level].current.is_none() {
                self.levels[level].current = Some(Buffer::new_for(
                    store.page_size(),
                    level as u16,
                    node.reference,
                    node.minimum,
                )?);
                return Ok(());
            }
            let cell = branch_cell(store, &node.minimum, node.reference)?;
            if self.levels[level]
                .current
                .as_mut()
                .unwrap()
                .push(cell.as_bytes())?
            {
                return Ok(());
            }
            let previous = self.levels[level].pending.take();
            self.levels[level].pending = self.levels[level].current.take();
            self.levels[level].current = Some(Buffer::new_for(
                store.page_size(),
                level as u16,
                node.reference,
                node.minimum,
            )?);
            let Some(previous) = previous else {
                return Ok(());
            };
            node = self.seal(store, previous)?;
            level += 1;
        }
    }
    fn record<S: WriteStore>(&mut self, store: &mut S, key: &[u8], value: &[u8]) -> Result<()> {
        let records = self
            .report
            .records
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        let cell = leaf_cell(store, key, value, None)?;
        self.level(0)?;
        if self.levels[0].current.is_none() {
            self.levels[0].current = Some(Buffer::new_for(
                store.page_size(),
                0,
                Reference::NULL,
                Separator::from_cell(cell.as_bytes(), 0)?,
            )?);
        }
        if !self.levels[0]
            .current
            .as_mut()
            .unwrap()
            .push(cell.as_bytes())?
        {
            let previous = self.levels[0].pending.take();
            self.levels[0].pending = self.levels[0].current.take();
            let mut next = Buffer::new_for(
                store.page_size(),
                0,
                Reference::NULL,
                Separator::from_cell(cell.as_bytes(), 0)?,
            )?;
            if !next.push(cell.as_bytes())? {
                return Err(Error::LimitExceeded);
            }
            self.levels[0].current = Some(next);
            if let Some(previous) = previous {
                let node = self.seal(store, previous)?;
                self.node(store, 1, node)?;
            }
        }
        self.report.records = records;
        Ok(())
    }
    fn finish<S: WriteStore>(mut self, store: &mut S) -> Result<BulkBuild> {
        if self.report.records == 0 {
            return Ok(self.report);
        }
        for level in 0..usize::from(MAX_DEPTH) {
            if level >= self.levels.len() {
                break;
            }
            let pending = self.levels[level].pending.take();
            let current = self.levels[level].current.take();
            let higher = self
                .levels
                .iter()
                .skip(level + 1)
                .any(|level| level.current.is_some() || level.pending.is_some());
            let Some(current) = current else {
                if pending.is_some() {
                    return Err(Error::Corrupt("bulk pending page without tail"));
                }
                continue;
            };
            if pending.is_none() && !higher {
                if level != 0 && current.count == 0 {
                    self.report.root = current.first;
                    self.report.height = level as u16;
                } else {
                    let node = self.seal(store, current)?;
                    self.report.root = node.reference;
                    self.report.height = node.level + 1;
                }
                return Ok(self.report);
            }
            if let Some(pending) = pending {
                let (left, right) = rebalance(store, pending, current)?;
                let left = self.seal(store, left)?;
                self.node(store, level + 1, left)?;
                let right = self.seal(store, right)?;
                self.node(store, level + 1, right)?;
            } else {
                let node = self.seal(store, current)?;
                self.node(store, level + 1, node)?;
            }
        }
        Err(Error::LimitExceeded)
    }
}

/// Build a new tree using direct leaf/branch encoding, never ordinary `put`.
/// Keys must increase strictly in unsigned byte order and obey engine limits
/// (4096-byte keys/64MiB values). Empty keys/values and overflow records work.
/// Own buffering is at most two4096B pages per64 levels, bounded suffix scratch
/// and one4096B prior key, independent of record count. The supplied store owns
/// payload/output-arena memory. Any error returns NO root; preceding private
/// allocations may exist, so callers must discard/rollback that store. Existing
/// objects are never rewritten. Engine compaction uses a fresh SegmentBuilder
/// and publishes only after full candidate validation and logical equivalence.
pub fn build_sorted<S, I, K, V>(store: &mut S, records: I) -> Result<BulkBuild>
where
    S: WriteStore,
    I: IntoIterator<Item = Result<(K, V)>>,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
{
    let mut builder = SortedBuilder::new();
    for pair in records {
        let (key, value) = pair?;
        builder.push(store, key.as_ref(), value.as_ref())?;
    }
    builder.finish(store)
}

/// Direct sorted construction under the supplied ordering; equivalent keys
/// are rejected even when their byte strings differ. No row materialization.
pub fn build_sorted_with<S, I, K, V, C>(store: &mut S, records: I, order: C) -> Result<BulkBuild>
where
    S: WriteStore,
    I: IntoIterator<Item = Result<(K, V)>>,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
    C: crate::ordering::Comparator,
{
    let mut builder = SortedBuilder::with_order(order);
    for pair in records {
        let (key, value) = pair?;
        builder.push(store, key.as_ref(), value.as_ref())?;
    }
    builder.finish(store)
}

/// Streaming relocation preserving an already validated tree's physical inorder.
/// This checks geometry and sizes but deliberately provides no ordering certificate.
/// Use `SortedBuilder` when accepting records from an application.
pub struct InOrderBuilder {
    pipeline: Option<Builder>,
    duplicate_values: bool,
}
impl InOrderBuilder {
    pub fn new() -> Self {
        Self {
            pipeline: Some(Builder::new()),
            duplicate_values: false,
        }
    }
    pub fn duplicate_values() -> Self {
        Self {
            pipeline: Some(Builder::new()),
            duplicate_values: true,
        }
    }
    pub fn push<S: WriteStore>(&mut self, store: &mut S, key: &[u8], value: &[u8]) -> Result<()> {
        let result = (|| {
            if key.len()
                > if self.duplicate_values {
                    65536
                } else {
                    store.max_key_size()
                }
                || value.len() > store.max_value_size()
                || (self.duplicate_values && !value.is_empty())
            {
                return Err(Error::InvalidInput("inorder record exceeds size limits"));
            }
            self.pipeline
                .as_mut()
                .ok_or(Error::InvalidInput("inorder builder poisoned"))?
                .record(store, key, value)
        })();
        if result.is_err() {
            self.pipeline = None;
        }
        result
    }
    pub fn finish<S: WriteStore>(mut self, store: &mut S) -> Result<BulkBuild> {
        self.pipeline
            .take()
            .ok_or(Error::InvalidInput("inorder builder poisoned"))?
            .finish(store)
    }
}
impl Default for InOrderBuilder {
    fn default() -> Self {
        Self::new()
    }
}
pub fn rebuild_inorder<S, I, K, V>(store: &mut S, records: I) -> Result<BulkBuild>
where
    S: WriteStore,
    I: IntoIterator<Item = Result<(K, V)>>,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
{
    let mut b = InOrderBuilder::new();
    for pair in records {
        let (k, v) = pair?;
        b.push(store, k.as_ref(), v.as_ref())?;
    }
    b.finish(store)
}
