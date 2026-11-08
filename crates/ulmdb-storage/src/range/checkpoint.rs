//! Opaque, backend-typed selected-range authority and bounded physical export.
use super::{FILE, MARKER, RangeSnapshot, direct, pack};
use crate::{Error, Manifest, ResidencyController, Result, SegmentTable, WriterFence};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    sync::Arc,
};

mod namespace;
mod retained;
pub(in crate::range) use namespace::Namespace;

macro_rules! export_point {
    ($name:expr) => {
        #[cfg(feature = "fault-injection")]
        pack::point($name)?;
    };
}

#[derive(Clone)]
#[repr(transparent)]
pub struct RangeCheckpoint {
    inner: Arc<CheckpointInner>,
}
struct CheckpointInner {
    source: Arc<File>,
    creator: u32,
    namespace: Option<Arc<Namespace>>,
    epoch: u64,
    file: [u8; 16],
    uuid: [u8; 16],
    selected: pack::Receipt,
    parent: Option<pack::Receipt>,
    generation: u128,
    sequence: u64,
    scalar: [u8; 32],
}
/// A projection, not a reconstructible authority. Head blocks are normalized in
/// exported copies; only the selected data prefix is immutable in a live pack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeAssetInfo {
    pub file_id: [u8; 16],
    pub uuid: [u8; 16],
    pub generation: u128,
    pub sequence: u64,
    pub committed_bytes: u64,
    pub selected_frame_hash: [u8; 32],
}
impl RangeCheckpoint {
    pub(in crate::range) fn mint(
        source: Arc<File>,
        file: [u8; 16],
        m: &Manifest,
        selected: pack::Receipt,
        parent: Option<pack::Receipt>,
        scalar: [u8; 32],
    ) -> Result<Self> {
        crate::hybrid::retirement::shared(&source)?;
        Ok(Self {
            inner: Arc::new(CheckpointInner {
                source,
                creator: std::process::id(),
                namespace: None,
                epoch: m.epoch,
                file,
                uuid: m.uuid,
                selected,
                parent,
                generation: m.generation,
                sequence: m.sequence,
                scalar,
            }),
        })
    }
    pub(in crate::range) fn bind_namespace(mut self, namespace: Arc<Namespace>) -> Self {
        // Called only on the private freshly minted token, before it escapes.
        // No IO/admission/allocation is introduced after a successful selector.
        let inner = Arc::get_mut(&mut self.inner).expect("fresh private range checkpoint");
        inner.namespace = Some(namespace);
        self
    }
    pub fn check_creator(&self) -> Result<()> {
        if self.inner.creator != std::process::id() {
            return Err(Error::InvalidInput(
                "range checkpoint belongs to another process",
            ));
        }
        self.inner
            .namespace
            .as_ref()
            .ok_or(Error::InvalidInput("unbound range checkpoint"))?
            .check_creator()
    }
    pub(in crate::range) fn reader(&self) -> Arc<File> {
        Arc::clone(&self.inner.source)
    }
    pub fn physical_asset(&self) -> Result<RangeAssetInfo> {
        self.check_creator()?;
        Ok(RangeAssetInfo {
            file_id: self.inner.file,
            uuid: self.inner.uuid,
            generation: self.inner.generation,
            sequence: self.inner.sequence,
            committed_bytes: self.inner.selected.frame_end()?,
            selected_frame_hash: self.inner.selected.hash,
        })
    }
    pub fn matches_manifest(&self, m: &Manifest) -> bool {
        self.check_creator().is_ok()
            && m.format_minor == 6
            && m.uuid == self.inner.uuid
            && m.generation == self.inner.generation
            && m.sequence == self.inner.sequence
            && direct::checkpoint_scalar(m) == self.inner.scalar
    }
    pub fn matches_directory(&self, directory: &Path) -> Result<bool> {
        self.check_creator()?;
        if !self.inner.namespace.as_ref().unwrap().matches(directory)? {
            return Ok(false);
        }
        let (_file, uuid) = identity(directory)?;
        if uuid != self.inner.uuid {
            return Ok(false);
        }
        let actual = File::open(super::rotation::asset(directory, self.inner.file)?)?.metadata()?;
        let expected = self.inner.source.metadata()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(
                actual.is_file()
                    && actual.dev() == expected.dev()
                    && actual.ino() == expected.ino(),
            )
        }
        #[cfg(not(unix))]
        {
            let _ = (actual, expected);
            Err(Error::InvalidInput("range physical identity requires Unix"))
        }
    }
    fn load<F: FnOnce(&SegmentTable, &Manifest) -> Result<()>>(
        &self,
        controller: &ResidencyController,
        validate: F,
    ) -> Result<RangeSnapshot> {
        self.check_creator()?;
        checked_global(&self.inner.source, self.inner.file)?;
        pack::recovery::verify_reserved_prefix(&self.inner.source)?;
        if self.inner.source.metadata()?.len() < self.inner.selected.frame_end()?
            || self.inner.source.metadata()?.len() > pack::MAX_FILE_BYTES
        {
            return Err(Error::Corrupt("pinned range asset truncated"));
        }
        if let Some(parent) = self.inner.parent {
            pack::recovery::verify_frame(&self.inner.source, self.inner.file, parent)?;
        }
        let frame =
            pack::recovery::verify_frame(&self.inner.source, self.inner.file, self.inner.selected)?;
        if frame.parent != self.inner.parent {
            return Err(Error::Corrupt("pinned range parent changed"));
        }
        let mut state = direct::load_checkpoint(
            &self.inner.source,
            self.inner.file,
            self.inner.uuid,
            self.inner.selected,
            controller,
        )?;
        state.checkpoint = Some(
            state
                .checkpoint
                .take()
                .ok_or(Error::Corrupt("missing range checkpoint"))?
                .bind_namespace(self.inner.namespace.as_ref().unwrap().clone()),
        );
        if !self.matches_manifest(&state.manifest) {
            return Err(Error::Corrupt("pinned range scalar changed"));
        }
        validate(&state.segments, &state.manifest)?;
        Ok(state)
    }
    /// Copy an old or current pinned snapshot without source mutation/recovery.
    /// Target must be absent. All source validation precedes target creation;
    /// copied graph/hash/EOF validation precedes its backend-marker publication.
    pub fn export_checked<F: FnMut(&SegmentTable, &Manifest) -> Result<()>>(
        &self,
        target: &Path,
        controller: &ResidencyController,
        mut validate: F,
    ) -> Result<RangeAssetInfo> {
        self.load(controller, |t, m| validate(t, m))?;
        export_point!("range.export.source_validated");
        let end = self.inner.selected.frame_end()?;
        crate::create_layout(target)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .open(target.join("LOCK"))?;
        let _lock = WriterFence::try_lock(lock).map_err(|_| Error::Busy)?;
        let output = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(target.join(FILE))?;
        let mut buffer = [0u8; 65536];
        let mut offset = 0;
        while offset < end {
            let count = (end - offset).min(buffer.len() as u64) as usize;
            pack::io::read_exact_at(&*self.inner.source, &mut buffer[..count], offset)?;
            pack::io::write_all_at(&output, &buffer[..count], offset)?;
            offset += count as u64;
            export_point!("range.export.copy_chunk");
        }
        output.set_len(end)?;
        // Source heads may already describe later commits. Only the destination
        // receives canonical heads for this opaque selected receipt and parent.
        let older = self.inner.parent.unwrap_or(self.inner.selected);
        pack::io::write_all_at(
            &output,
            &pack::codec::encode_head(self.inner.file, self.inner.selected),
            pack::HEAD_OFFSETS[0],
        )?;
        pack::io::write_all_at(
            &output,
            &pack::codec::encode_head(self.inner.file, older),
            pack::HEAD_OFFSETS[1],
        )?;
        export_point!("range.export.before_data_sync");
        ulmdb_platform::sync_file(&output)?;
        export_point!("range.export.after_data_sync");
        let reader = File::open(target.join(FILE))?;
        if reader.metadata()?.len() != end {
            return Err(Error::Corrupt("exported range EOF mismatch"));
        }
        checked_global(&reader, self.inner.file)?;
        let (selected, parent) = pack::snapshot::select(&reader, self.inner.file)?;
        if selected != self.inner.selected || parent != self.inner.parent {
            return Err(Error::Corrupt("exported range selected identity"));
        }
        let state = direct::load_checkpoint(
            &reader,
            self.inner.file,
            self.inner.uuid,
            selected,
            controller,
        )?;
        if !self.matches_manifest(&state.manifest) {
            return Err(Error::Corrupt("exported range scalar mismatch"));
        }
        validate(&state.segments, &state.manifest)?;
        export_point!("range.export.destination_validated");
        let (path, mut marker) = crate::temporary::create(target, "BACKEND")?;
        marker.write_all(&direct::checkpoint_marker(self.inner.file, self.inner.uuid))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            marker.set_permissions(std::fs::Permissions::from_mode(0o400))?;
        }
        export_point!("range.export.before_marker_sync");
        ulmdb_platform::sync_file(&marker)?;
        export_point!("range.export.after_marker_sync");
        // Once selector installation starts, the target may already be visible.
        // Match ordinary snapshot export: errors here cannot prove nonpublication.
        (|| {
            export_point!("range.export.before_marker_publish");
            ulmdb_platform::rename_replace(&path, &target.join(MARKER))?;
            export_point!("range.export.after_marker_publish");
            export_point!("range.export.before_directory_sync");
            ulmdb_platform::sync_path(target)?;
            export_point!("range.export.after_directory_sync");
            Ok::<(), Error>(())
        })()
        .map_err(|_| Error::OutcomeUnknown)?;
        self.physical_asset()
    }
}
fn identity(directory: &Path) -> Result<([u8; 16], [u8; 16])> {
    let s = super::rotation::selection(directory)?;
    Ok((s.file, s.uuid))
}
fn checked_global(file: &File, id: [u8; 16]) -> Result<()> {
    let mut b = [0; pack::BLOCK];
    pack::io::read_exact_at(file, &mut b, 0)?;
    if pack::codec::decode_global(&b)?.file_id != id {
        return Err(Error::Corrupt("range snapshot file identity"));
    }
    Ok(())
}
/// Read-only format6 snapshot. A moving head can return Busy and malformed heads
/// fail closed; recovery/repair remains exclusive to the original writer open.
pub fn open_snapshot_checked<F: FnOnce(&SegmentTable, &Manifest) -> Result<()>>(
    directory: &Path,
    controller: &ResidencyController,
    validate: F,
) -> Result<RangeSnapshot> {
    let namespace = Namespace::capture(directory)?;
    let (file, uuid) = identity(directory)?;
    let reader = namespace.reader(directory)?;
    checked_global(&reader, file)?;
    let (receipt, _) = pack::snapshot::select(&reader, file)?;
    let mut state = direct::load_checkpoint(&reader, file, uuid, receipt, controller)?;
    state.checkpoint = Some(
        state
            .checkpoint
            .take()
            .ok_or(Error::Corrupt("missing range checkpoint"))?
            .bind_namespace(namespace),
    );
    validate(&state.segments, &state.manifest)?;
    Ok(state)
}
