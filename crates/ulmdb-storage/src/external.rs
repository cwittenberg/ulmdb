//! Explicit caller-owned synchronization, distinct from physical coordination.
//! The caller serializes every publisher/capture/maintenance operation across
//! all processes and preserves immutable files while live snapshots use them.
use crate::{Error, Manifest, Result};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    sync::Arc,
};
#[derive(Clone)]
pub struct ExternalSynchronization(Arc<Binding>);
struct Binding {
    directory: PathBuf,
    file: File,
    creator: u32,
    uuid: [u8; 16],
    epoch: u64,
    device: u64,
    #[cfg(not(windows))]
    inode: u64,
    #[cfg(windows)]
    inode: u128,
}
impl ExternalSynchronization {
    pub fn discover(
        directory: &Path,
        transform: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<Self> {
        let (manifest, _) =
            crate::publication::copy::read_copy_current_external(directory, transform)?;
        Self::caller_managed(directory, manifest.uuid, manifest.epoch)
    }
    /// Explicitly opt into caller-wide synchronization. No kernel lock or pin is
    /// acquired. This is namespace/realm authority, never a graph or liveness pin.
    pub fn caller_managed(directory: &Path, uuid: [u8; 16], epoch: u64) -> Result<Self> {
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = (directory, uuid, epoch);
            return Err(Error::InvalidInput(
                "external synchronization requires a supported file platform",
            ));
        }
        #[cfg(windows)]
        {
            if uuid == [0; 16] || epoch == 0 {
                return Err(Error::InvalidInput("external realm is empty"));
            }
            let file = ulmdb_platform::open_directory(directory)?;
            let directory = directory.canonicalize()?;
            let named = ulmdb_platform::path_identity(&directory)?;
            let actual = ulmdb_platform::file_identity(&file)?;
            if !named.is_dir() || !named.same_object(&actual) {
                return Err(Error::Corrupt("external directory changed"));
            }
            Ok(Self(Arc::new(Binding {
                directory,
                file,
                creator: std::process::id(),
                uuid,
                epoch,
                device: actual.volume,
                inode: actual.file_id,
            })))
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
            if uuid == [0; 16] || epoch == 0 {
                return Err(Error::InvalidInput("external realm is empty"));
            }
            let directory = directory.canonicalize()?;
            let meta = fs::symlink_metadata(&directory)?;
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err(Error::Corrupt("external directory"));
            }
            let file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&directory)?;
            let actual = file.metadata()?;
            if (meta.dev(), meta.ino()) != (actual.dev(), actual.ino()) {
                return Err(Error::Corrupt("external directory changed"));
            }
            Ok(Self(Arc::new(Binding {
                directory,
                file,
                creator: std::process::id(),
                uuid,
                epoch,
                device: meta.dev(),
                inode: meta.ino(),
            })))
        }
    }
    pub fn check(&self, directory: &Path, uuid: [u8; 16], epoch: u64) -> Result<()> {
        if self.0.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited external synchronization"));
        }
        if uuid != self.0.uuid || epoch != self.0.epoch {
            return Err(Error::Corrupt("external realm changed"));
        }
        let named = fs::symlink_metadata(&self.0.directory)?;
        let actual = self.0.file.metadata()?;
        if !named.is_dir()
            || named.file_type().is_symlink()
            || directory.canonicalize()? != self.0.directory
        {
            return Err(Error::Corrupt("external namespace changed"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if (named.dev(), named.ino()) != (self.0.device, self.0.inode)
                || (actual.dev(), actual.ino()) != (self.0.device, self.0.inode)
            {
                return Err(Error::Corrupt("external directory identity"));
            }
        }
        #[cfg(windows)]
        {
            let named = ulmdb_platform::path_identity(&self.0.directory)?;
            let actual = ulmdb_platform::file_identity(&self.0.file)?;
            if !named.is_dir()
                || !actual.is_dir()
                || (named.volume, named.file_id) != (self.0.device, self.0.inode)
                || !named.same_object(&actual)
            {
                return Err(Error::Corrupt("external directory identity"));
            }
        }
        Ok(())
    }
    pub(crate) fn check_prepared(&self, p: &crate::PreparedPublication) -> Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            self.check(&p.directory, p.uuid, p.epoch)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = p;
            Err(Error::InvalidInput(
                "external synchronization requires a supported file platform",
            ))
        }
    }
    pub fn capture_current(
        &self,
        transform: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<(Manifest, [u8; 64], crate::FileDurabilityState)> {
        self.check(&self.0.directory, self.0.uuid, self.0.epoch)?;
        let (manifest, checkpoint, state) =
            crate::publication::read_current_checkpoint_with_durability_and_transform(
                &self.0.directory,
                transform,
            )?;
        self.check(&self.0.directory, manifest.uuid, manifest.epoch)?;
        Ok((manifest, checkpoint, state))
    }
    pub fn directory(&self) -> Result<PathBuf> {
        self.check(&self.0.directory, self.0.uuid, self.0.epoch)?;
        Ok(self.0.directory.clone())
    }
    pub fn flush_async(
        &self,
        transform: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<crate::FileAsyncFlushReport> {
        self.check(&self.0.directory, self.0.uuid, self.0.epoch)?;
        crate::durability::async_external(
            self,
            &self.0.directory,
            self.0.uuid,
            self.0.epoch,
            transform,
        )
    }
    pub fn sync(
        &self,
        transform: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<crate::FileSyncReport> {
        self.check(&self.0.directory, self.0.uuid, self.0.epoch)?;
        crate::durability::sync_external(
            self,
            &self.0.directory,
            self.0.uuid,
            self.0.epoch,
            transform,
        )
    }
}

/// Exact selected D and eligible P read under the caller's external exclusion.
/// Descriptors authenticate; this is not a logical-graph or pin certificate.
pub struct ExternalCopyRoots {
    authority: ExternalSynchronization,
    selected: Manifest,
    previous: Option<Manifest>,
    checkpoint: [u8; 64],
}
impl ExternalCopyRoots {
    pub fn selected(&self) -> &Manifest {
        &self.selected
    }
    pub fn previous(&self) -> Option<&Manifest> {
        self.previous.as_ref()
    }
    pub fn checkpoint(&self) -> &[u8; 64] {
        &self.checkpoint
    }
    pub fn check(&self) -> Result<()> {
        self.authority.check(
            &self.authority.0.directory,
            self.selected.uuid,
            self.selected.epoch,
        )
    }
}
impl ExternalSynchronization {
    pub fn capture_copy_roots(
        &self,
        transform: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<ExternalCopyRoots> {
        let (_, checkpoint, _) = self.capture_current(transform.clone())?;
        let (selected, previous) = crate::publication::copy::capture_copy_roots_external_inner(
            &self.0.directory,
            &checkpoint,
            transform,
            || self.check(&self.0.directory, self.0.uuid, self.0.epoch),
        )?;
        self.check(&self.0.directory, selected.uuid, selected.epoch)?;
        Ok(ExternalCopyRoots {
            authority: self.clone(),
            selected,
            previous,
            checkpoint,
        })
    }
}

impl ExternalSynchronization {
    pub fn prepare_imported_snapshot(
        &self,
        checkpoint: &[u8; 64],
        expected: &[u8; 64],
        predecessor: Option<&[u8; 64]>,
        transform: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<crate::ExternalSnapshotImport> {
        self.check(&self.0.directory, self.0.uuid, self.0.epoch)?;
        crate::publication::prepare_imported_snapshot_with_transform(
            &self.0.directory,
            checkpoint,
            expected,
            predecessor,
            transform,
        )?
        .externally_synchronized(self)
    }
}

pub use crate::publication::ExternalSnapshotImport;

impl ExternalSynchronization {
    pub fn capture_previous_selection(
        &self,
        transform: Option<Arc<crate::transform::TransformContext>>,
    ) -> Result<Option<crate::PreviousSelection>> {
        let (_, checkpoint, _) = self.capture_current(transform.clone())?;
        let selection = crate::publication::capture_previous_selection_external_inner(
            &self.0.directory,
            Some(&checkpoint),
            transform,
            || self.check(&self.0.directory, self.0.uuid, self.0.epoch),
        )?;
        if let Some(selection) = &selection {
            self.check(
                &self.0.directory,
                selection.selected().uuid,
                selection.selected().epoch,
            )?;
        }
        Ok(selection)
    }
}
