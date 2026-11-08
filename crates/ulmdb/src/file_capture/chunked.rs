use super::*;
impl OwnedFileCapture {
    /// Admit directly into bounded decoded windows, without first building a
    /// normal SegmentTable or retaining whole-segment mappings.
    pub fn admit_chunked(
        self,
        cache: std::sync::Arc<ulmdb_storage::chunk_cache::ChunkCache>,
        pins: ulmdb_storage::chunk_table::PinLimits,
        limits: crate::chunked::AdmissionLimits,
        context: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<crate::ChunkedReadTxn> {
        self.check()?;
        let Self {
            directory,
            pin,
            _capture,
            ..
        } = self;
        let read = crate::ChunkedReadTxn::from_pin(&directory, pin, cache, pins, limits, context)?;
        drop(_capture);
        Ok(read)
    }
}
