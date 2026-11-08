use crate::{Error, Manifest, PreparedPublication, Result};
use std::path::{Path, PathBuf};
fn unsupported<T>() -> Result<T> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "physical-root coordination is Linux-only",
    )
    .into())
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_pin_records: usize,
    pub max_inventory_entries: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_pin_records: 4096,
            max_inventory_entries: 1_048_575,
        }
    }
}
#[derive(Clone)]
pub struct Coordination;
pub struct DirectoryCapture;
pub struct CaptureLease;
pub struct PhysicalCopyPair;
impl PhysicalCopyPair {
    pub fn selected(&self) -> &PhysicalPin {
        unreachable!("unsupported copy capture")
    }
    pub fn predecessor(&self) -> Option<&PhysicalPin> {
        None
    }
    pub fn into_pins(self) -> (PhysicalPin, Option<PhysicalPin>) {
        unreachable!("unsupported copy capture")
    }
}
pub struct MaintenanceLease;
#[derive(Clone, Debug)]
pub struct PinnedRoot {
    manifest: Manifest,
    checkpoint: [u8; 64],
}
pub struct PhysicalPin {
    root: PinnedRoot,
    path: PathBuf,
}
pub struct PinScan<'a> {
    _owner: &'a mut MaintenanceLease,
    roots: Vec<PinnedRoot>,
}
impl DirectoryCapture {
    pub fn capture_current_for_copy(self) -> Result<(Coordination, CaptureLease, PhysicalPin)> {
        unsupported()
    }
    pub fn directory(&self) -> Result<&Path> {
        unsupported()
    }
    pub fn directory_identity(&self) -> Result<(u64, u64)> {
        unsupported()
    }
    pub fn directory_binding(&self) -> Result<(&Path, (u64, u64))> {
        unsupported()
    }
    pub fn capture_previous(
        self,
    ) -> Result<
        Option<(
            Coordination,
            CaptureLease,
            crate::PreviousSelection,
            PhysicalPin,
        )>,
    > {
        unsupported()
    }
    pub fn capture_current(self) -> Result<(Coordination, CaptureLease, PhysicalPin)> {
        unsupported()
    }
    pub fn capture_current_with_durability(
        self,
    ) -> Result<(
        Coordination,
        CaptureLease,
        PhysicalPin,
        crate::FileDurabilityState,
    )> {
        unsupported()
    }
    pub fn bind(self, _: [u8; 16]) -> Result<(Coordination, CaptureLease)> {
        unsupported()
    }
}
impl Coordination {
    pub fn capture_bound_directory(&self) -> Result<DirectoryCapture> {
        unsupported()
    }
    pub fn capture_directory(_: impl AsRef<Path>, _: Limits) -> Result<DirectoryCapture> {
        unsupported()
    }
    pub fn capture_directory_with_transform(
        _: impl AsRef<Path>,
        _: Limits,
        _: Option<std::sync::Arc<crate::transform::TransformContext>>,
    ) -> Result<DirectoryCapture> {
        unsupported()
    }
    pub fn open_with_transform(
        _: impl AsRef<Path>,
        _: [u8; 16],
        _: Limits,
        _: Option<std::sync::Arc<crate::transform::TransformContext>>,
    ) -> Result<Self> {
        unsupported()
    }
    pub fn open(_: impl AsRef<Path>, _: [u8; 16], _: Limits) -> Result<Self> {
        unsupported()
    }
    pub fn try_capture(&self) -> Result<CaptureLease> {
        unsupported()
    }
    pub fn flush_async(&self) -> Result<crate::FileAsyncFlushReport> {
        Err(Error::InvalidInput("async mapped sync requires Linux"))
    }
    pub fn sync(&self) -> Result<crate::FileSyncReport> {
        unsupported()
    }
    pub fn try_maintenance(&self) -> Result<MaintenanceLease> {
        unsupported()
    }
}
impl CaptureLease {
    pub fn capture_copy_pair_for(&self, _: PhysicalPin) -> Result<PhysicalCopyPair> {
        unsupported()
    }
    pub fn capture_previous_for(
        &self,
        _: PhysicalPin,
    ) -> Result<Option<(crate::PreviousSelection, PhysicalPin)>> {
        unsupported()
    }
    pub fn pin_current(&self) -> Result<PhysicalPin> {
        unsupported()
    }
    pub fn pin_current_with_durability(&self) -> Result<(PhysicalPin, crate::FileDurabilityState)> {
        unsupported()
    }
    pub fn coordination(&self) -> Coordination {
        Coordination
    }
    pub fn pin_prepared(&self, _: &PreparedPublication, _: &Manifest) -> Result<PhysicalPin> {
        unsupported()
    }
    pub fn pin_checkpoint(&self, _: &[u8; 64]) -> Result<PhysicalPin> {
        unsupported()
    }
}
impl MaintenanceLease {
    pub fn scan(&mut self) -> Result<PinScan<'_>> {
        unsupported()
    }
}
impl PinnedRoot {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn checkpoint(&self) -> &[u8; 64] {
        &self.checkpoint
    }
    pub fn matches_manifest(&self, _: &Manifest) -> Result<bool> {
        unsupported()
    }
}
impl PhysicalPin {
    pub fn matches_directory_handle(&self, _: &std::fs::File) -> Result<bool> {
        unsupported()
    }
    pub fn matches_directory(&self, _: impl AsRef<Path>) -> Result<bool> {
        unsupported()
    }
    pub fn pin_current_with_durability(&self) -> Result<(PhysicalPin, crate::FileDurabilityState)> {
        unsupported()
    }
    pub fn coordination(&self) -> Coordination {
        Coordination
    }
    pub fn root(&self) -> &PinnedRoot {
        &self.root
    }
    pub fn matches_environment(&self, _: impl AsRef<Path>, _: &Manifest) -> Result<bool> {
        unsupported()
    }
    pub fn record_path(&self) -> &Path {
        &self.path
    }
}
impl PinScan<'_> {
    pub fn live_roots(&self) -> &[PinnedRoot] {
        &self.roots
    }
    pub fn stale_records(&self) -> usize {
        0
    }
    pub fn cleanup_stale(self) -> Result<usize> {
        unsupported()
    }
}
