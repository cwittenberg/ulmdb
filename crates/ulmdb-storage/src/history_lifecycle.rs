//! Explicit history availability, independent of business descriptors/selection.
//! Default absence/KeepAll never expires old bases. A finite policy is a caller
//! request; named and live pins still clamp each actual floor advancement.
use crate::{Error, Manifest, Result, manifest::number, read_mode::ReadMode};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
const BYTES: usize = 192;
const NAME: &str = "HISTORY_RETENTION";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryLifecycleBudget {
    pub generations: usize,
    pub mark_units: usize,
    pub directory_entries: usize,
    pub unlink_files: usize,
}
impl Default for HistoryLifecycleBudget {
    fn default() -> Self {
        Self {
            generations: 1,
            mark_units: 256,
            directory_entries: 64,
            unlink_files: 8,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoryPolicy {
    #[default]
    KeepAll,
    /// An unregistered copy/base older than the subsequently published floor
    /// expires explicitly. Keep a named snapshot pin for future exact deltas.
    Retain {
        keep_last_commits: u64,
        budget: HistoryLifecycleBudget,
    },
}
impl HistoryPolicy {
    pub fn validate(self) -> Result<()> {
        if let Self::Retain {
            keep_last_commits,
            budget: b,
        } = self
        {
            if keep_last_commits == 0
                || b.generations == 0
                || b.generations > 64
                || b.mark_units == 0
                || b.mark_units > 65_536
                || b.directory_entries == 0
                || b.directory_entries > 65_536
                || b.unlink_files == 0
                || b.unlink_files > 4096
            {
                return Err(Error::InvalidInput("invalid history retention policy"));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryAvailability {
    pub policy: HistoryPolicy,
    pub revision: u128,
    pub floor_sequence: u64,
    pub floor_checkpoint: Option<[u8; 64]>,
}
impl Default for HistoryAvailability {
    fn default() -> Self {
        Self {
            policy: HistoryPolicy::KeepAll,
            revision: 0,
            floor_sequence: 0,
            floor_checkpoint: None,
        }
    }
}
fn path(d: &Path) -> PathBuf {
    d.join(NAME)
}
fn decode(bytes: &[u8], uuid: [u8; 16], epoch: u64) -> Result<HistoryAvailability> {
    if bytes.len() != BYTES
        || &bytes[..8] != b"ULMDBHF1"
        || bytes[8..12] != (BYTES as u32).to_le_bytes()
        || bytes[12..16] != [0; 4]
        || bytes[184..188] != crate::publication::crc(&bytes[..184]).to_le_bytes()
        || bytes[188..] != [0; 4]
    {
        return Err(Error::Corrupt("invalid history availability record"));
    }
    let revision = u128::from_le_bytes(number(bytes, 40)?);
    let keep = u64::from_le_bytes(number(bytes, 56)?);
    let budget = HistoryLifecycleBudget {
        generations: u32::from_le_bytes(number(bytes, 64)?) as usize,
        mark_units: u32::from_le_bytes(number(bytes, 68)?) as usize,
        directory_entries: u32::from_le_bytes(number(bytes, 72)?) as usize,
        unlink_files: u32::from_le_bytes(number(bytes, 76)?) as usize,
    };
    let policy = if keep == 0 {
        if bytes[64..80] != [0; 16] {
            return Err(Error::Corrupt("KeepAll budget fields"));
        }
        HistoryPolicy::KeepAll
    } else {
        HistoryPolicy::Retain {
            keep_last_commits: keep,
            budget,
        }
    };
    policy
        .validate()
        .map_err(|_| Error::Corrupt("history policy fields"))?;
    let floor_sequence = u64::from_le_bytes(number(bytes, 80)?);
    let cp: [u8; 64] = number(bytes, 88)?;
    if revision == 0
        || bytes[152..184].iter().any(|x| *x != 0)
        || (floor_sequence == 0) != (cp == [0; 64])
    {
        return Err(Error::Corrupt("history floor identity"));
    }
    if floor_sequence != 0 {
        crate::checkpoint_identity(&cp)?;
    }
    // A legitimate realm/epoch transition does not inherit expiry authority.
    // KeepAll is conservative; malformed records were rejected above.
    if bytes[16..32] != uuid || bytes[32..40] != epoch.to_le_bytes() {
        return Ok(HistoryAvailability::default());
    }
    Ok(HistoryAvailability {
        policy,
        revision,
        floor_sequence,
        floor_checkpoint: (floor_sequence != 0).then_some(cp),
    })
}
pub fn read_history_availability(
    d: &Path,
    uuid: [u8; 16],
    epoch: u64,
    transform: Option<Arc<crate::transform::TransformContext>>,
) -> Result<HistoryAvailability> {
    let info = read_record(d, uuid, epoch)?;
    validate_floor(d, uuid, epoch, &info, transform)?;
    Ok(info)
}
fn read_record(d: &Path, uuid: [u8; 16], epoch: u64) -> Result<HistoryAvailability> {
    let named = path(d);
    let mode = ReadMode::Sync;
    let mut file = match mode.open(&named) {
        Ok(f) => f,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Default::default());
        }
        Err(e) => return Err(e),
    };
    if file.metadata()?.len() != BYTES as u64 {
        return Err(Error::Corrupt("history availability length"));
    }
    let mut bytes = [0; BYTES];
    file.read_exact(&mut bytes)?;
    mode.check(&named, &file)?;
    decode(&bytes, uuid, epoch)
}
fn validate_floor(
    d: &Path,
    uuid: [u8; 16],
    epoch: u64,
    info: &HistoryAvailability,
    transform: Option<Arc<crate::transform::TransformContext>>,
) -> Result<()> {
    if let Some(cp) = &info.floor_checkpoint {
        let floor = crate::read_checkpoint_with_transform(d, cp, transform)?;
        if floor.uuid != uuid || floor.epoch != epoch || floor.sequence != info.floor_sequence {
            return Err(Error::Corrupt("history floor checkpoint binding"));
        }
    }
    Ok(())
}

/// A checked history-policy observation, not serving or graph authority. The
/// private fields prevent a caller-supplied availability value from skipping
/// initial floor authentication. Reobserve the bounded 192-byte record each
/// step; an unchanged record in this exact namespace reuses its checked floor.
#[derive(Clone)]
pub struct HistoryAvailabilityObservation {
    directory: PathBuf,
    identity: ulmdb_platform::FileIdentity,
    uuid: [u8; 16],
    epoch: u64,
    transform: Option<Arc<crate::transform::TransformContext>>,
    info: HistoryAvailability,
}
impl HistoryAvailabilityObservation {
    pub fn read(
        d: &Path,
        uuid: [u8; 16],
        epoch: u64,
        transform: Option<Arc<crate::transform::TransformContext>>,
        previous: Option<&Self>,
    ) -> Result<Self> {
        let identity = ulmdb_platform::path_identity(d)?;
        if !identity.is_dir() {
            return Err(Error::Corrupt("history observation directory"));
        }
        let info = read_record(d, uuid, epoch)?;
        let same = previous.is_some_and(|old| {
            old.directory == d
                && old.identity.same_object(&identity)
                && old.uuid == uuid
                && old.epoch == epoch
                && old.info == info
                && match (&old.transform, &transform) {
                    (None, None) => true,
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    _ => false,
                }
        });
        if !same {
            validate_floor(d, uuid, epoch, &info, transform.clone())?;
        }
        if !identity.same_object(&ulmdb_platform::path_identity(d)?) {
            return Err(Error::Corrupt("history observation directory changed"));
        }
        Ok(Self {
            directory: d.to_path_buf(),
            identity,
            uuid,
            epoch,
            transform,
            info,
        })
    }
    pub fn availability(&self) -> &HistoryAvailability {
        &self.info
    }
}
pub(crate) fn write(
    d: &Path,
    uuid: [u8; 16],
    epoch: u64,
    info: &HistoryAvailability,
) -> Result<()> {
    info.policy.validate()?;
    let mut bytes = [0; BYTES];
    bytes[..8].copy_from_slice(b"ULMDBHF1");
    bytes[8..12].copy_from_slice(&(BYTES as u32).to_le_bytes());
    bytes[16..32].copy_from_slice(&uuid);
    bytes[32..40].copy_from_slice(&epoch.to_le_bytes());
    bytes[40..56].copy_from_slice(&info.revision.to_le_bytes());
    if let HistoryPolicy::Retain {
        keep_last_commits,
        budget: b,
    } = info.policy
    {
        bytes[56..64].copy_from_slice(&keep_last_commits.to_le_bytes());
        for (o, n) in [
            (64, b.generations),
            (68, b.mark_units),
            (72, b.directory_entries),
            (76, b.unlink_files),
        ] {
            bytes[o..o + 4].copy_from_slice(&(n as u32).to_le_bytes());
        }
    }
    bytes[80..88].copy_from_slice(&info.floor_sequence.to_le_bytes());
    if let Some(cp) = info.floor_checkpoint {
        bytes[88..152].copy_from_slice(&cp);
    }
    let crc = crate::publication::crc(&bytes[..184]);
    bytes[184..188].copy_from_slice(&crc.to_le_bytes());
    decode(&bytes, uuid, epoch)?;
    let (tmp, mut file) = crate::temporary::create(d, "history-floor")?;
    let result = (|| {
        file.write_all(&bytes)?;
        crate::fault_point!("history_floor.before_file_sync");
        ulmdb_platform::sync_file(&file)?;
        crate::fault_point!("history_floor.before_rename");
        ulmdb_platform::rename_replace(&tmp, &path(d))
            .map_err(|error| if ulmdb_platform::rename_outcome_unknown(&error) { Error::OutcomeUnknown } else { error.into() })?;
        crate::fault_point!("history_floor.before_directory_sync");
        ulmdb_platform::sync_directory(d)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryFileKind {
    LegacyDescriptor,
    Cohort,
    Archive,
}
/// Checked identity observation of exact bytes, NOT graph/serving authority.
#[derive(Clone, Debug)]
pub struct HistoryDescriptorIdentity {
    pub generation: u128,
    pub sequence: u64,
    pub uuid: [u8; 16],
    pub epoch: u64,
    pub minor: u16,
    pub checkpoint: [u8; 64],
}
pub fn inspect_history_descriptor(
    d: &Path,
    generation: u128,
    kind: HistoryFileKind,
    transform: Option<Arc<crate::transform::TransformContext>>,
) -> Result<HistoryDescriptorIdentity> {
    let mode = ReadMode::Sync.with_transform(transform);
    let bytes = match kind {
        HistoryFileKind::Archive => {
            let mut a = crate::cohort_history::open(d, generation)?;
            let bytes = a.descriptor(generation, &mode)?;
            let (_, hash) = crate::checkpoint_identity(&a.checkpoint)?;
            if <[u8; 32]>::from(Sha256::digest(&bytes)) != hash {
                return Err(Error::Corrupt("history archive body hash"));
            }
            bytes
        }
        HistoryFileKind::Cohort => {
            crate::cohort::read_descriptor(
                &mut mode.open(&crate::cohort::path(d, generation))?,
                generation,
                &mode,
            )?
            .0
        }
        HistoryFileKind::LegacyDescriptor => mode.read_legacy(
            &d.join("commits").join(format!("{generation:032x}.root")),
            67_109_056,
        )?,
    };
    let (minor, _, _, _) = Manifest::decode_prefix(&bytes)?;
    let uuid = number(&bytes, 16)?;
    let actual = u128::from_le_bytes(number(&bytes, 32)?);
    let epoch = u64::from_le_bytes(number(&bytes, 48)?);
    let sequence = u64::from_le_bytes(number(&bytes, 56)?);
    let commit = u128::from_le_bytes(number(&bytes, 64)?);
    let previous = u128::from_le_bytes(number(&bytes, 128)?);
    if actual != generation
        || uuid == [0; 16]
        || epoch == 0
        || commit == 0
        || actual == 0
        || actual == previous
        || u64::from_le_bytes(number(&bytes, 176)?) == 0
        || u64::from_le_bytes(number(&bytes, 184)?) == 0
        || ((kind == HistoryFileKind::LegacyDescriptor) != (minor < 3))
    {
        return Err(Error::Corrupt("history descriptor identity/type"));
    }
    let mut checkpoint = crate::publication::pointer(generation, Sha256::digest(&bytes).into());
    if minor >= 3 {
        checkpoint[..8].copy_from_slice(if minor == 4 { b"ULMDBT04" } else { b"ULMDBC03" });
        let crc = crate::publication::crc(&checkpoint[..60]);
        checkpoint[60..64].copy_from_slice(&crc.to_le_bytes());
    }
    Ok(HistoryDescriptorIdentity {
        generation,
        sequence,
        uuid,
        epoch,
        minor,
        checkpoint,
    })
}
