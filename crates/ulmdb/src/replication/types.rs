use crate::ReadTxn;
use std::{
    fmt, io,
    path::{Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, ReplicationError>;
#[derive(Debug)]
pub enum ReplicationError {
    Engine(crate::Error),
    Io(io::Error),
    Authentication,
    UnauthorizedPublisher,
    EpochConflict,
    Gap,
    Conflict,
    LimitExceeded,
    InvalidInput(&'static str),
    Corrupt(&'static str),
}
impl From<crate::Error> for ReplicationError {
    fn from(value: crate::Error) -> Self {
        Self::Engine(value)
    }
}
impl From<io::Error> for ReplicationError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
impl fmt::Display for ReplicationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Engine(e) => e.fmt(f),
            Self::Io(e) => e.fmt(f),
            Self::Authentication => f.write_str("replication authentication failed"),
            Self::UnauthorizedPublisher => f.write_str("unauthorized replication publisher"),
            Self::EpochConflict => f.write_str("replication epoch conflict"),
            Self::Gap => f.write_str("replication checkpoint gap"),
            Self::Conflict => f.write_str("replication checkpoint conflict"),
            Self::LimitExceeded => f.write_str("replication transfer limit exceeded"),
            Self::InvalidInput(s) => write!(f, "invalid replication input: {s}"),
            Self::Corrupt(s) => write!(f, "corrupt replication transfer: {s}"),
        }
    }
}
impl std::error::Error for ReplicationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Engine(e) => Some(e),
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cursor {
    pub uuid: [u8; 16],
    pub epoch: u64,
    pub sequence: u64,
    pub commit: u128,
    pub checkpoint: [u8; 64],
}
impl Cursor {
    pub fn from_read(read: &ReadTxn) -> Result<Self> {
        if read.snapshot.range_backed {
            return Err(ReplicationError::InvalidInput(
                "range replication pending PACK-03",
            ));
        }
        Self::from_manifest(&read.snapshot.manifest)
    }
    pub(super) fn from_manifest(manifest: &ulmdb_storage::Manifest) -> Result<Self> {
        Ok(Self {
            uuid: manifest.uuid,
            epoch: manifest.epoch,
            sequence: manifest.sequence,
            commit: manifest.commit,
            checkpoint: ulmdb_storage::checkpoint_bytes(manifest)?,
        })
    }
    pub fn generation(&self) -> Result<u128> {
        Ok(ulmdb_storage::checkpoint_identity(&self.checkpoint)?.0)
    }
    pub(super) fn validate(&self) -> Result<()> {
        if self.uuid == [0; 16] || self.epoch == 0 || self.commit == 0 {
            return Err(ReplicationError::Corrupt("invalid cursor identity"));
        }
        self.generation()?;
        Ok(())
    }
}
/// Authority for one publisher, database realm and fencing epoch. No Debug or
/// public secret accessor is provided. Key distribution/rotation is external.
pub struct PublisherKey {
    pub(super) id: u64,
    pub(super) uuid: [u8; 16],
    pub(super) epoch: u64,
    secret: [u8; 32],
}
impl PublisherKey {
    pub fn new(id: u64, uuid: [u8; 16], epoch: u64, secret: [u8; 32]) -> Result<Self> {
        if id == 0 || uuid == [0; 16] || epoch == 0 || secret == [0; 32] {
            return Err(ReplicationError::InvalidInput(
                "publisher binding and secret must be nonzero",
            ));
        }
        Ok(Self {
            id,
            uuid,
            epoch,
            secret,
        })
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn uuid(&self) -> [u8; 16] {
        self.uuid
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub(super) fn secret(&self) -> &[u8; 32] {
        &self.secret
    }
    pub(super) fn authorize(&self, cursor: &Cursor) -> Result<()> {
        if cursor.uuid != self.uuid {
            return Err(ReplicationError::UnauthorizedPublisher);
        }
        if cursor.epoch != self.epoch {
            return Err(ReplicationError::EpochConflict);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct TransferLimits {
    pub max_files: u32,
    pub max_bytes: u64,
}
impl Default for TransferLimits {
    fn default() -> Self {
        Self {
            max_files: 65_536,
            max_bytes: 64 * 1024 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum AssetId {
    Segment([u8; 32]),
    Descriptor(u128),
    Cohort(u128),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssetDescription {
    pub id: AssetId,
    pub length: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferReport {
    pub base: Option<Cursor>,
    pub target: Cursor,
    pub files: u32,
    pub payload_bytes: u64,
    pub wire_bytes: u64,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct TransferHeader {
    pub base: Option<Cursor>,
    pub target: Cursor,
    pub key_id: u64,
    pub nonce: u128,
    pub file_count: u32,
    pub payload_bytes: u64,
}

pub(super) struct OwnedStage {
    path: PathBuf,
}
impl OwnedStage {
    pub(super) fn create(parent: &Path) -> Result<Self> {
        if !std::fs::symlink_metadata(parent)?.file_type().is_dir() {
            return Err(ReplicationError::InvalidInput(
                "staging parent must be a real directory",
            ));
        }
        for _ in 0..16 {
            let path = parent.join(format!(
                "ulmdb-transfer-{:032x}",
                ulmdb_storage::new_identity()?
            ));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => {
                    let owned = Self { path };
                    for name in ["segments", "commits", "cohorts"] {
                        std::fs::create_dir(owned.path.join(name))?;
                    }
                    return Ok(owned);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        Err(ReplicationError::LimitExceeded)
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for OwnedStage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
/// A MAC-authenticated, private immutable staging set. Keep it alive through
/// activation. Stage paths are deliberately internal; external mutation of
/// engine-owned staging files is outside the storage contract.
pub struct VerifiedTransfer {
    pub(super) header: TransferHeader,
    pub(super) stage: OwnedStage,
    pub(super) assets: Vec<AssetDescription>,
}
impl VerifiedTransfer {
    pub fn cursor(&self) -> Cursor {
        self.header.target
    }
    pub fn base(&self) -> Option<Cursor> {
        self.header.base
    }
    pub fn publisher_id(&self) -> u64 {
        self.header.key_id
    }
    pub fn assets(&self) -> &[AssetDescription] {
        &self.assets
    }
    pub fn payload_bytes(&self) -> u64 {
        self.header.payload_bytes
    }
    pub(super) fn staging_directory(&self) -> &Path {
        self.stage.path()
    }
}
impl fmt::Debug for VerifiedTransfer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedTransfer")
            .field("target", &self.header.target)
            .field("base", &self.header.base)
            .field("publisher_id", &self.header.key_id)
            .field("files", &self.assets.len())
            .finish()
    }
}
