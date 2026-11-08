//! Opt-in, genuine writer-private mapped allocation. Immutable reader graphs and
//! publication history remain unchanged. Default writers retain heap allocation.
use crate::{Error, MappedAllocation, MappedFlush, Result, WriteTxn};
impl WriteTxn<'_> {
    /// Switch a pristine writer to one bounded, shared, file-backed mutable map.
    /// Capacity is in bytes, 4096-aligned, at most8GiB on64-bit (also bounded by the selected value profile). Actual reserve/value
    /// pointers refer to that file mapping, not a mirrored heap allocation.
    ///
    /// All three file formats without transforms seal/promote the same data
    /// inode after revoking/unmapping every mutable view. Cohort/Indexed append
    /// canonical metadata before synchronously/asynchronously flushing the real
    /// finished bundle. Checksummed transforms retain their ordinary sealer;
    /// asynchronous mode schedules the finalized encoded inode and retains
    /// data debt until a genuine forced sync covers the selected assets.
    /// Encrypted writers reject mapped plaintext scratch rather than leaking it.
    /// Existing nested savepoint/readonly-view APIs remain supported as extras.
    pub fn enable_write_map(&mut self, capacity: usize, flush: MappedFlush) -> Result<()> {
        self.live()?;
        self.env.require_file_backend()?;
        if self.arena.dirty_bytes() != 0 || !self.changed.is_empty() {
            return Err(Error::InvalidInput(
                "mapped allocation must be configured before mutation",
            ));
        }
        if flush == MappedFlush::Asynchronous
            && self
                .base
                .segments
                .transform()
                .is_some_and(|c| c.encrypted())
        {
            return Err(Error::InvalidInput(
                "encrypted async mapped publication unsupported",
            ));
        }
        let mut arena = ulmdb_storage::Arena::new_mapped(
            self.base.segments.clone(),
            self.base.manifest.next_segment,
            &self.env.directory,
            capacity,
            flush,
        )?;
        arena.set_value_limit(self.value_limit);
        self.arena = arena;
        Ok(())
    }
    /// Avoid redundant zero stores in never-exposed, zero-backed mapped file
    /// space. Rollback/reused extents are always cleared; heap allocation stays
    /// initialized. This changes private allocation work, never stored bytes.
    pub fn set_mapped_no_mem_init(&mut self, enabled: bool) -> Result<()> {
        self.live()?;
        self.arena.set_mapped_no_mem_init(enabled);
        Ok(())
    }
    pub fn mapped_allocation(&self) -> Result<Option<MappedAllocation>> {
        self.live()?;
        self.arena.mapped_allocation()
    }
    /// Data preparation samples this setting; changing it after native prepare
    /// is impossible because the writer has been consumed. Async/None weaken
    /// the data seal to NoSync and retain real weak-publication recovery debt.
    pub fn set_mapped_flush(&mut self, flush: MappedFlush) -> Result<()> {
        self.live()?;
        if !self.arena.is_write_mapped() {
            return Err(Error::InvalidInput("writer has no file mapping"));
        }
        if flush == MappedFlush::Asynchronous
            && self
                .base
                .segments
                .transform()
                .is_some_and(|c| c.encrypted())
        {
            return Err(Error::InvalidInput(
                "encrypted async mapped publication unsupported",
            ));
        }
        self.arena.set_mapped_flush(flush);
        Ok(())
    }
}
