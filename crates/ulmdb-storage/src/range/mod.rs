//! Explicit v5 flat / v6 persistent physical locator Environment protocols.
#[cfg(feature = "range-lifecycle")]
mod checkpoint;
mod codec;
#[cfg(feature = "range-lifecycle")]
pub use checkpoint::{RangeAssetInfo, RangeCheckpoint, open_snapshot_checked};
mod direct;
mod origin;
#[cfg(feature = "range-lifecycle")]
mod rotation;
#[cfg(feature = "range-lifecycle")]
pub use rotation::RangeReclaimReport;
mod pack;
mod v5;
use crate::{
    Arena, Error, Manifest, ResidencyAdmission, ResidencyController, Result, SegmentTable,
};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};
const MARKER: &str = "BACKEND";
const FILE: &str = "RANGE.pack";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Seal {
    Frame {
        start: u64,
        end: u64,
        ordinal: u64,
        hash: [u8; 32],
    },
    Prefix {
        committed_end: u64,
        data_hash: [u8; 32],
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RangeIdentity {
    pub file_id: [u8; 16],
    pub seal: Seal,
    pub data_offset: u64,
    pub length: u64,
}
impl RangeIdentity {
    fn new(file_id: [u8; 16], o: codec::Origin, length: u64) -> Result<Self> {
        Ok(Self {
            file_id,
            seal: Seal::Frame {
                start: o.receipt.offset,
                end: o.receipt.frame_end()?,
                ordinal: o.receipt.ordinal,
                hash: o.receipt.hash,
            },
            data_offset: codec::data_offset(o)?,
            length,
        })
    }
    pub(crate) fn bounds(self) -> (u64, u64) {
        match self.seal {
            Seal::Frame { start, end, .. } => (start, end),
            Seal::Prefix { committed_end, .. } => (pack::DATA_START, committed_end),
        }
    }
}
pub struct RangeSnapshot {
    #[cfg(feature = "range-lifecycle")]
    pub checkpoint: Option<RangeCheckpoint>,
    pub manifest: Manifest,
    pub segments: SegmentTable,
    pub admission: ResidencyAdmission,
}
/// Explicit immutable locator work, not a latency claim or hot-query counter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LocatorWork {
    pub nodes_visited: u64,
    pub nodes_written: u64,
    pub node_bytes: u64,
    pub data_bytes: u64,
    pub inventory_materialized: bool,
}
/// Borrowed optimization context. It never authorizes mapping ownership itself:
/// the controller rechecks token, table lineage and lock-health epoch.
#[derive(Clone, Copy)]
pub struct PriorAdmission<'a> {
    base: &'a SegmentTable,
    ready: &'a ResidencyAdmission,
}
impl<'a> PriorAdmission<'a> {
    pub fn new(base: &'a SegmentTable, ready: &'a ResidencyAdmission) -> Self {
        Self { base, ready }
    }
}
enum Backend {
    Flat(v5::Store),
    Direct(direct::Store),
}
pub struct RangeStore {
    backend: Backend,
    #[cfg(feature = "range-lifecycle")]
    namespace: Option<std::sync::Arc<checkpoint::Namespace>>,
    #[cfg(feature = "range-lifecycle")]
    _rotation_writer: Option<crate::WriterFence>,
}
pub fn is_selected(directory: &Path) -> Result<bool> {
    let exists = directory.join(MARKER).try_exists()?;
    if !exists && directory.join(FILE).try_exists()? {
        return Err(Error::Corrupt("range file without explicit backend marker"));
    }
    if exists
        && (directory.join("CURRENT").try_exists()? || directory.join("PREVIOUS").try_exists()?)
    {
        return Err(Error::Corrupt("ambiguous file/range backend selectors"));
    }
    Ok(exists)
}
fn version(directory: &Path) -> Result<u16> {
    let mut bytes = Vec::new();
    File::open(directory.join(MARKER))?
        .take(97)
        .read_to_end(&mut bytes)?;
    if bytes.len() != 96 {
        return Err(Error::Corrupt("range marker length"));
    }
    let minor = u16::from_le_bytes(bytes[10..12].try_into().unwrap());
    #[cfg(feature = "range-lifecycle")]
    if minor == 6 {
        rotation::selection(directory)?;
        return Ok(6);
    }
    let magic = match minor {
        5 => b"ULMDBB05",
        6 => b"ULMDBB06",
        _ => return Err(Error::Corrupt("unsupported range marker version")),
    };
    if &bytes[..8] != magic
        || bytes[8..10] != [0; 2]
        || bytes[12..16] != 96u32.to_le_bytes()
        || bytes[16..32] == [0; 16]
        || bytes[32..48] == [0; 16]
        || bytes[48..64] != [0; 16]
        || <[u8; 32]>::from(Sha256::digest(&bytes[..64])) != bytes[64..]
    {
        return Err(Error::Corrupt("range marker version/hash"));
    }
    Ok(minor)
}
impl RangeStore {
    pub fn create_v5_compatibility(
        directory: &Path,
        controller: &ResidencyController,
    ) -> Result<(Self, RangeSnapshot)> {
        let (s, r) = v5::Store::create(directory, controller)?;
        Ok((
            Self {
                backend: Backend::Flat(s),
                #[cfg(feature = "range-lifecycle")]
                namespace: None,
                #[cfg(feature = "range-lifecycle")]
                _rotation_writer: None,
            },
            r,
        ))
    }
    pub fn create(
        directory: &Path,
        controller: &ResidencyController,
    ) -> Result<(Self, RangeSnapshot)> {
        #[cfg(feature = "range-lifecycle")]
        let namespace = checkpoint::Namespace::capture(directory)?;
        #[cfg(feature = "range-lifecycle")]
        let rotation_writer = rotation::stable_writer(directory)?;
        let (s, r) = direct::Store::create(directory, controller)?;
        let store = Self {
            backend: Backend::Direct(s),
            #[cfg(feature = "range-lifecycle")]
            namespace: Some(namespace),
            #[cfg(feature = "range-lifecycle")]
            _rotation_writer: Some(rotation_writer),
        };
        #[cfg(feature = "range-lifecycle")]
        let r = store.bind_snapshot(r);
        Ok((store, r))
    }
    pub fn open_checked<F: FnOnce(&dyn crate::ReadStore, &Manifest) -> Result<()>>(
        directory: &Path,
        controller: &ResidencyController,
        validate: F,
    ) -> Result<(Self, RangeSnapshot)> {
        if !is_selected(directory)? {
            return Err(Error::InvalidInput("range backend not selected"));
        }
        let version = version(directory)?;
        #[cfg(feature = "range-lifecycle")]
        let namespace = if version == 6 {
            Some(checkpoint::Namespace::capture(directory)?)
        } else {
            None
        };
        #[cfg(feature = "range-lifecycle")]
        let _capture = namespace
            .as_ref()
            .map(|n| n.reader(directory))
            .transpose()?;
        #[cfg(feature = "range-lifecycle")]
        let rotation_writer = if version == 6 {
            Some(rotation::stable_writer(directory)?)
        } else {
            None
        };
        let (backend, state) = match version {
            5 => {
                let (s, r) = v5::Store::open_checked(directory, controller, validate)?;
                (Backend::Flat(s), r)
            }
            6 => {
                let (s, r) = direct::Store::open_checked(directory, controller, validate)?;
                (Backend::Direct(s), r)
            }
            _ => unreachable!(),
        };
        let store = Self {
            backend,
            #[cfg(feature = "range-lifecycle")]
            namespace,
            #[cfg(feature = "range-lifecycle")]
            _rotation_writer: rotation_writer,
        };
        #[cfg(feature = "range-lifecycle")]
        let state = store.bind_snapshot(state);
        Ok((store, state))
    }
    #[cfg(feature = "range-lifecycle")]
    fn bind_snapshot(&self, mut state: RangeSnapshot) -> RangeSnapshot {
        if let Some(namespace) = &self.namespace {
            state.checkpoint = Some(
                state
                    .checkpoint
                    .take()
                    .expect("private format6 checkpoint")
                    .bind_namespace(namespace.clone()),
            );
        }
        state
    }
    #[cfg(feature = "range-lifecycle")]
    pub fn check_lifecycle_owner(&self) -> Result<()> {
        self.namespace
            .as_ref()
            .ok_or(Error::InvalidInput("range retention requires format6"))?
            .check_creator()
    }
    pub fn is_poisoned(&self) -> bool {
        match &self.backend {
            Backend::Flat(s) => s.is_poisoned(),
            Backend::Direct(s) => s.is_poisoned(),
        }
    }
    pub fn commit_arena(
        &mut self,
        arena: Arena,
        manifest: Manifest,
        base: &Manifest,
        controller: &ResidencyController,
    ) -> Result<RangeSnapshot> {
        self.commit_arena_with_admission(arena, manifest, base, controller, None)
    }
    /// Optional prior context affects only healthy RequiredLocked single-insert
    /// admission. Generic callers and all other policies retain full semantics.
    pub fn commit_arena_with_admission(
        &mut self,
        arena: Arena,
        manifest: Manifest,
        base: &Manifest,
        controller: &ResidencyController,
        prior: Option<PriorAdmission<'_>>,
    ) -> Result<RangeSnapshot> {
        #[cfg(feature = "range-lifecycle")]
        if let Some(namespace) = &self.namespace {
            namespace.check_creator()?;
        }
        let state = match &mut self.backend {
            Backend::Flat(s) => s.commit_arena(arena, manifest, base, controller)?,
            Backend::Direct(s) => s.commit_arena(arena, manifest, base, controller, prior)?,
        };
        #[cfg(feature = "range-lifecycle")]
        let state = self.bind_snapshot(state);
        Ok(state)
    }
    pub fn locator_work(&self) -> Option<LocatorWork> {
        match &self.backend {
            Backend::Flat(_) => None,
            Backend::Direct(s) => Some(s.work()),
        }
    }
}

#[cfg(feature = "range-chunks")]
pub mod chunks;
