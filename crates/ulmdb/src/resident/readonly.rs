//! Read-only cold reconstruction under a negotiated shared capture lease. The
//! result owns anonymous immutable pages, so no source lease/file survives it.
use super::{CaptureLimits, Options, ReadTxn, RedoFormat};
use crate::{Error, Result};
use std::{path::Path, sync::Arc};
use ulmdb_storage::{
    resident::{ReadCapture, journal, journal_v2},
    transform::TransformContext,
};
impl ReadTxn {
    /// Capture a complete committed snapshot while an independent writer stays
    /// open. The writer must first enable_readonly_capture(). No writer LOCK,
    /// recovery healing, truncation or source-file synchronization is performed.
    ///
    /// The shared lease covers checkpoint/tail replay and graph validation. A
    /// competing publication returns Busy, rather than waiting recursively.
    /// Read work is bounded by CaptureLimits, pool quotas and graph_read_budget;
    /// user callbacks and filesystem I/O have no wall-clock deadline here.
    pub fn open_coordinated_snapshot(path: impl AsRef<Path>, options: Options) -> Result<Self> {
        Self::open_coordinated_snapshot_with_limits(path, options, CaptureLimits::default())
    }
    pub fn open_coordinated_snapshot_with_limits(
        path: impl AsRef<Path>,
        options: Options,
        limits: CaptureLimits,
    ) -> Result<Self> {
        Self::open_coordinated_snapshot_with_transform_options(path, options, limits, None)
    }
    pub fn open_coordinated_snapshot_with_transform_options(
        path: impl AsRef<Path>,
        mut options: Options,
        limits: CaptureLimits,
        context: Option<Arc<TransformContext>>,
    ) -> Result<Self> {
        super::environment::validate_options(options)?;
        let capture = ReadCapture::open(path)?;
        let directory = capture.directory();
        let configured =
            ulmdb_storage::transform::check_configuration(directory, context.as_ref())?;
        let format = super::journal::detect_with_transform(
            &directory.join("RESIDENT.wal"),
            context.as_ref(),
        )?;
        let snapshot = match format {
            RedoFormat::LiteralV1 => {
                journal::Journal::read_only_checked(&capture, limits, context, |recovery| {
                    if configured.is_some_and(|uuid| uuid != recovery.boot().uuid) {
                        return Err(Error::CryptoMismatch);
                    }
                    options.wal_capacity = recovery.capacity();
                    Ok(super::recover::rebuild(options, recovery)?.1)
                })?
            }
            RedoFormat::CompactV2 | RedoFormat::FullPostimageV2 => {
                journal_v2::Journal::read_only_checked(&capture, limits, context, |recovery| {
                    if configured.is_some_and(|uuid| uuid != recovery.boot().uuid) {
                        return Err(Error::CryptoMismatch);
                    }
                    options.wal_capacity = recovery.capacity();
                    Ok(super::recover::rebuild_v2(options, recovery)?.1)
                })?
            }
        };
        // Closing read-only descriptors and dropping the SH lease cannot alter
        // the independently owned anonymous image retained by this transaction.
        drop(capture);
        Ok(Self {
            snapshot: Arc::new(snapshot),
        })
    }
}
