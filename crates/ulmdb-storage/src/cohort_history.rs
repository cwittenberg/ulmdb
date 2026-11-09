//! Immutable exact descriptor tails for cohorts whose payload is no longer live.
//! Archives never certify availability of user data. Full graph admission is
//! unchanged; only historical metadata/NodeRef discovery can outlive a payload.
use crate::{Error, Manifest, Result, manifest::number, read_mode::ReadMode};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
const HEADER: usize = 128;
const MAGIC: &[u8; 8] = b"ULMDBHI1";
pub(crate) fn path(directory: &Path, generation: u128) -> PathBuf {
    directory
        .join("commits")
        .join(format!("{generation:032x}.history"))
}
pub(crate) struct Archive {
    pub file: File,
    pub body: u64,
    pub physical: u64,
    pub logical_payload: u64,
    pub minor: u16,
    pub checkpoint: [u8; 64],
    pub transformed: bool,
}
impl Archive {
    pub(crate) fn descriptor(&mut self, generation: u128, mode: &ReadMode) -> Result<Vec<u8>> {
        if self.transformed {
            let context = mode.context().ok_or(Error::CryptoMismatch)?;
            let (logical, encoded) = crate::transform::envelope_geometry(&self.file, self.body)?;
            if encoded != self.physical || logical > crate::metadata::MAX_DESCRIPTOR_BYTES {
                return Err(Error::Corrupt("history descriptor envelope"));
            }
            let (bytes, _lease) = context.decode_mapping_at(
                &self.file,
                self.body,
                crate::transform::ObjectKind::Descriptor,
                generation,
                logical,
            )?;
            Ok(bytes.to_vec())
        } else {
            if mode.context().is_some() {
                return Err(Error::CryptoMismatch);
            }
            let mut bytes =
                vec![0; usize::try_from(self.physical).map_err(|_| Error::LimitExceeded)?];
            self.file.seek(SeekFrom::Start(self.body))?;
            crate::work::read_exact(&mut self.file, &mut bytes)?;
            Ok(bytes)
        }
    }
}
pub(crate) fn open(directory: &Path, generation: u128) -> Result<Archive> {
    let named = path(directory, generation);
    // A new format has no legacy permissive-open contract to preserve.
    let mode = ReadMode::Sync;
    let mut file = mode.open(&named)?;
    let mut header = [0; HEADER];
    crate::work::read_exact(&mut file, &mut header)?;
    let minor = u16::from_le_bytes(number(&header, 12)?);
    let transformed = u16::from_le_bytes(number(&header, 14)?);
    let physical = u64::from_le_bytes(number(&header, 32)?);
    let logical_payload = u64::from_le_bytes(number(&header, 40)?);
    let checkpoint: [u8; 64] = number(&header, 48)?;
    let (id, _) = crate::checkpoint_identity(&checkpoint)?;
    let expected = if minor == 4 { b"ULMDBT04" } else { b"ULMDBC03" };
    if &header[..8] != MAGIC
        || header[8..12] != (HEADER as u32).to_le_bytes()
        || ![3, 4].contains(&minor)
        || transformed > 1
        || id != generation
        || header[16..32] != generation.to_le_bytes()
        || &checkpoint[..8] != expected
        || u32::from_le_bytes(number(&header, 112)?) != crate::publication::crc(&header[..112])
        || header[116..].iter().any(|x| *x != 0)
        || physical < 192
        || physical
            > crate::metadata::MAX_DESCRIPTOR_BYTES
                + crate::metadata::MAX_DESCRIPTOR_BYTES / 4
                + 8192
        || logical_payload > ulmdb_core::ValueLimit::MAXIMUM.segment_limit()
        || file.metadata()?.len()
            != physical
                .checked_add(HEADER as u64)
                .ok_or(Error::LimitExceeded)?
    {
        return Err(Error::Corrupt("invalid cohort history header/extent"));
    }
    mode.check(&named, &file)?;
    Ok(Archive {
        file,
        body: HEADER as u64,
        physical,
        logical_payload,
        minor,
        checkpoint,
        transformed: transformed != 0,
    })
}
pub(crate) fn checkpoint(directory: &Path, generation: u128, minor: u16) -> Result<[u8; 64]> {
    let archive = open(directory, generation)?;
    if archive.minor != minor {
        return Err(Error::Corrupt("history format mismatch"));
    }
    Ok(archive.checkpoint)
}
pub(crate) fn decode(
    directory: &Path,
    fallback: Option<&Path>,
    generation: u128,
    hash: [u8; 32],
    mode: ReadMode,
) -> Result<Manifest> {
    let mut archive = open(directory, generation)?;
    let (_, claimed) = crate::checkpoint_identity(&archive.checkpoint)?;
    if claimed != hash {
        return Err(Error::Corrupt("history checkpoint mismatch"));
    }
    let bytes = archive.descriptor(generation, &mode)?;
    if crate::work::digest(&bytes)? != hash {
        return Err(Error::Corrupt("history descriptor hash"));
    }
    let manifest = if archive.minor == 4 {
        crate::metadata::decode_manifest_with_mode(&bytes, directory, fallback, mode)?
    } else {
        Manifest::decode(&bytes)?
    };
    if manifest.generation != generation || manifest.format_minor != archive.minor {
        return Err(Error::Corrupt("history descriptor identity"));
    }
    crate::cohort::validate_layout(&manifest, archive.logical_payload)?;
    Ok(manifest)
}
#[derive(Clone, Debug)]
pub struct HistoryArchiveReport {
    pub generation: u128,
    pub checkpoint: [u8; 64],
    pub metadata_bytes: u64,
    pub created: bool,
    pub inventory_entries: usize,
}
/// Caller owns genuine Capture EX and native writer exclusion. No serving or
/// arbitrary publication authority is created; exact original bytes are saved.
pub(crate) fn archive(
    directory: &Path,
    uuid: [u8; 16],
    generation: u128,
    max_bytes: u64,
    max_inventory_entries: usize,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<HistoryArchiveReport> {
    let mode = ReadMode::Sync.with_transform(transform);
    let named = crate::cohort::path(directory, generation);
    let mut source = mode.open(&named)?;
    let (payload, physical, minor, logical_payload) = crate::cohort::geometry_details(&mut source)?;
    let metadata_bytes = physical
        .checked_add(HEADER as u64)
        .ok_or(Error::LimitExceeded)?;
    if metadata_bytes > max_bytes {
        return Err(Error::LimitExceeded);
    }
    let (bytes, _) = crate::cohort::read_descriptor(&mut source, generation, &mode)?;
    // Separately admit historical index traversal before the shared codec
    // follows old NodeRefs. This is not charged as one cheap mark unit.
    let inventory_entries = u32::from_le_bytes(number(&bytes, 112)?) as usize;
    if inventory_entries > max_inventory_entries {
        return Err(Error::LimitExceeded);
    }
    let hash = crate::work::digest(&bytes)?;
    let manifest =
        crate::cohort::decode_with_mode(directory, None, generation, hash, mode.clone())?;
    if manifest.uuid != uuid {
        return Err(Error::Corrupt("foreign history descriptor"));
    }
    let checkpoint = crate::publication::record(&manifest, hash);
    let destination = path(directory, generation);
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            let existing = open(directory, generation)?;
            if existing.checkpoint != checkpoint
                || existing.logical_payload != logical_payload
                || existing.physical != physical
            {
                return Err(Error::Corrupt("existing history differs"));
            }
            let archived = decode(directory, None, generation, hash, mode.clone())?;
            if archived.uuid != uuid {
                return Err(Error::Corrupt("foreign existing history"));
            }
            ulmdb_platform::sync_file(&existing.file)?;
            ulmdb_platform::sync_directory(&directory.join("commits"))?;
            mode.check(&named, &source)?;
            return Ok(HistoryArchiveReport {
                generation,
                checkpoint,
                metadata_bytes,
                created: false,
                inventory_entries,
            });
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    let mut header = [0; HEADER];
    header[..8].copy_from_slice(MAGIC);
    header[8..12].copy_from_slice(&(HEADER as u32).to_le_bytes());
    header[12..14].copy_from_slice(&minor.to_le_bytes());
    header[14..16].copy_from_slice(&u16::from(mode.context().is_some()).to_le_bytes());
    header[16..32].copy_from_slice(&generation.to_le_bytes());
    header[32..40].copy_from_slice(&physical.to_le_bytes());
    header[40..48].copy_from_slice(&logical_payload.to_le_bytes());
    header[48..112].copy_from_slice(&checkpoint);
    let crc = crate::publication::crc(&header[..112]);
    header[112..116].copy_from_slice(&crc.to_le_bytes());
    let (temporary, mut output) = crate::temporary::create(&directory.join("commits"), "history")?;
    let result = (|| {
        output.write_all(&header)?;
        source.seek(SeekFrom::Start(payload))?;
        let copied = std::io::copy(&mut (&mut source).take(physical), &mut output)?;
        if copied != physical {
            return Err(Error::Corrupt("truncated cohort metadata"));
        }
        mode.check(&named, &source)?;
        #[cfg(not(windows))]
        output.set_permissions(crate::cohort::sealed_permissions(
            output.metadata()?.permissions(),
        ))?;
        #[cfg(windows)]
        ulmdb_platform::set_private_permissions(&output, false)?;
        crate::fault_point!("history_archive.before_file_sync");
        ulmdb_platform::sync_file(&output)?;
        crate::fault_point!("history_archive.before_rename");
        ulmdb_platform::rename_replace(&temporary, &destination)
            .map_err(|error| if ulmdb_platform::rename_outcome_unknown(&error) { Error::OutcomeUnknown } else { error.into() })?;
        crate::fault_point!("history_archive.before_directory_sync");
        ulmdb_platform::sync_directory(&directory.join("commits"))?;
        let restored = decode(directory, None, generation, hash, mode.clone())?;
        if restored.uuid != uuid {
            return Err(Error::Corrupt("history archive roundtrip"));
        }
        mode.check(&named, &source)?;
        Ok(HistoryArchiveReport {
            generation,
            checkpoint,
            metadata_bytes,
            created: true,
            inventory_entries,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
