//! Immutable one-segment commit files. Origins dispatch only when opening maps.
use crate::{Error, Manifest, Result, SegmentEntry, manifest::number, mapping::hex};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
pub const MAX_COHORT_BYTES: u64 = (10u64 << 30) + 32;
const FOOTER: u64 = 32;
const MAGIC: &[u8; 8] = b"ULMDBCH3";
const INDEXED_MAGIC: &[u8; 8] = b"ULMDBCH4";

pub fn segment_path(directory: &Path, entry: &SegmentEntry) -> PathBuf {
    if entry.cohort == 0 {
        directory
            .join("segments")
            .join(format!("{}.seg", hex(&entry.hash)))
    } else {
        path(directory, entry.cohort)
    }
}
pub fn descriptor_path(directory: &Path, manifest: &Manifest) -> PathBuf {
    if manifest.format_minor >= 3 {
        path(directory, manifest.generation)
    } else {
        directory
            .join("commits")
            .join(format!("{:032x}.root", manifest.generation))
    }
}
pub(crate) fn reject_existing_generation(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(Error::InvalidInput(
            "physical generation is already claimed",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
pub(crate) fn path(directory: &Path, generation: u128) -> PathBuf {
    directory
        .join("cohorts")
        .join(format!("{generation:032x}.cohort"))
}
pub(crate) fn validate_layout(manifest: &Manifest, payload: u64) -> Result<Option<&SegmentEntry>> {
    if ![3, 4].contains(&manifest.format_minor)
        || manifest.generation == 0
        || manifest.generation == manifest.previous
    {
        return Err(Error::Corrupt("invalid cohort descriptor version/identity"));
    }
    if let Some(index) = &manifest.index
        && index.canonical_matches(manifest)
        && let Some(own) = &index.own_layout
    {
        return match (payload, own) {
            (0, None) => Ok(None),
            (4096.., Some(entry))
                if payload <= manifest.value_limit.segment_bytes() as u64
                    && entry.length == payload
                    && entry.cohort == manifest.generation =>
            {
                Ok(Some(entry))
            }
            _ => Err(Error::Corrupt("cohort prefix does not match its inventory")),
        };
    }
    let mut own = manifest
        .segments
        .iter()
        .filter(|entry| entry.cohort == manifest.generation);
    match (payload, own.next(), own.next()) {
        (0, None, None) => Ok(None),
        (4096.., Some(entry), None)
            if payload <= manifest.value_limit.segment_bytes() as u64
                && entry.length == payload =>
        {
            Ok(Some(entry))
        }
        _ => Err(Error::Corrupt("cohort prefix does not match its inventory")),
    }
}
/// Prepare a new immutable file. Caller supplies the already-hashed arena
/// prefix; only the arena invokes the payload-bearing path. No second data hash.
pub(crate) fn write(
    directory: &Path,
    manifest: &Manifest,
    segment: Option<&[u8]>,
) -> Result<[u8; 32]> {
    write_with_durability(directory, manifest, segment, crate::Durability::Full)
}
pub(crate) fn write_with_durability(
    directory: &Path,
    manifest: &Manifest,
    segment: Option<&[u8]>,
    policy: crate::Durability,
) -> Result<[u8; 32]> {
    Ok(write_open_with_durability(directory, manifest, segment, policy)?.hash)
}

/// A sealed cohort's existing descriptor can supply the new immutable map.
/// This descriptor never owns the environment writer lock and never escapes
/// storage; returning it avoids reopening/statting the same inode after seal.
pub(crate) struct WrittenCohort {
    pub hash: [u8; 32],
    pub file: File,
    pub physical: u64,
    pub segment: Option<SegmentEntry>,
}
#[cfg(test)]
pub(crate) fn write_open(
    directory: &Path,
    manifest: &Manifest,
    segment: Option<&[u8]>,
) -> Result<WrittenCohort> {
    write_open_with_durability(directory, manifest, segment, crate::Durability::Full)
}
pub(crate) fn write_open_with_durability(
    directory: &Path,
    manifest: &Manifest,
    segment: Option<&[u8]>,
    policy: crate::Durability,
) -> Result<WrittenCohort> {
    write_open_with_transform(
        directory,
        manifest,
        segment,
        policy,
        manifest.transform_context(),
    )
}
pub(crate) fn write_open_with_transform(
    directory: &Path,
    manifest: &Manifest,
    segment: Option<&[u8]>,
    policy: crate::Durability,
    context: Option<&std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<WrittenCohort> {
    crate::durability::check_policy(policy)?;
    let segment = segment.unwrap_or_default();
    let payload = segment.len() as u64;
    let entry = validate_layout(manifest, payload)?.cloned();
    let descriptor = manifest.encode()?;
    let descriptor_length = context
        .map(|c| c.encoded_length(descriptor.len() as u64))
        .transpose()?
        .unwrap_or(descriptor.len() as u64);
    let mut trailer = [0; FOOTER as usize];
    let encoded = if payload > 0 {
        context
            .map(|c| c.encoded_length(payload))
            .transpose()?
            .unwrap_or(payload)
    } else {
        0
    };
    trailer[..8].copy_from_slice(match (manifest.format_minor == 4, context.is_some()) {
        (true, true) => b"ULMDBCM4",
        (false, true) => b"ULMDBCM3",
        (true, false) => INDEXED_MAGIC,
        (false, false) => MAGIC,
    });
    trailer[8..16].copy_from_slice(&encoded.to_le_bytes());
    trailer[16..24].copy_from_slice(&descriptor_length.to_le_bytes());
    let legacy = directory
        .join("commits")
        .join(format!("{:032x}.root", manifest.generation));
    reject_existing_generation(&legacy)?;
    let final_path = path(directory, manifest.generation);
    // Never overwrite an inode that an old reader or retained root can own.
    // Partial files are unselected orphans; publication happens separately.
    let mut file = crate::access::options_for_entry(&final_path)?
        .read(true)
        .write(true)
        .create_new(true)
        .open(final_path)?;
    crate::fault_point!("cohort.before_write");
    if let Some(context) = context.filter(|_| payload > 0) {
        context.write_encoded(
            &mut file,
            crate::transform::ObjectKind::Segment,
            entry.as_ref().unwrap().id as u128,
            segment,
        )?;
    } else {
        file.write_all(segment)?;
    }
    if let Some(context) = context {
        context.write_encoded(
            &mut file,
            crate::transform::ObjectKind::Descriptor,
            manifest.generation,
            &descriptor,
        )?;
    } else {
        file.write_all(&descriptor)?;
    }
    file.write_all(&trailer)?;
    crate::fault_point!("cohort.after_write");
    let metadata = file.metadata()?;
    let physical = metadata.len();
    #[cfg(not(windows))]
    file.set_permissions(sealed_permissions(metadata.permissions()))?;
    #[cfg(windows)]
    ulmdb_platform::set_private_permissions(&file, false)?;
    if policy.sync_data() {
        crate::fault_point!("cohort.before_file_sync");
        ulmdb_platform::sync_file(&file)?;
        crate::fault_point!("cohort.after_file_sync");
        crate::fault_point!("cohort.before_directory_sync");
        ulmdb_platform::sync_directory(&directory.join("cohorts"))?;
        crate::fault_point!("cohort.after_directory_sync");
    }
    Ok(WrittenCohort {
        hash: Sha256::digest(&descriptor).into(),
        file,
        physical,
        segment: entry,
    })
}

pub(crate) fn sealed_permissions(mut permissions: std::fs::Permissions) -> std::fs::Permissions {
    permissions.set_readonly(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The open creator handle can read despite a restrictive creation
        // umask; the sealed pathname must also remain owner-readable on reopen.
        permissions.set_mode(permissions.mode() | 0o400);
    }
    permissions
}

#[cfg(all(test, unix))]
mod permission_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn sealed_data_is_owner_readable_and_has_no_write_bits_for_any_creation_mask() {
        for mode in 0..=0o777 {
            let permissions = sealed_permissions(std::fs::Permissions::from_mode(mode));
            assert_eq!(permissions.mode() & 0o222, 0);
            assert_eq!(permissions.mode() & 0o400, 0o400);
        }
    }
}
/// Bounded descriptor extraction. This does not follow the historical inventory
/// when the file is used only as a segment origin by a later selected root.
pub(crate) fn geometry(file: &mut File) -> Result<(u64, u64, u16)> {
    let (a, b, c, _) = geometry_details(file)?;
    Ok((a, b, c))
}
pub(crate) fn geometry_details(file: &mut File) -> Result<(u64, u64, u16, u64)> {
    let total = file.metadata()?.len();
    if !(192 + FOOTER..=MAX_COHORT_BYTES).contains(&total) {
        return Err(Error::Corrupt("invalid cohort file length"));
    }
    file.seek(SeekFrom::End(-(FOOTER as i64)))?;
    let mut trailer = [0; FOOTER as usize];
    file.read_exact(&mut trailer)?;
    let transformed = &trailer[..8] == b"ULMDBCT3"
        || &trailer[..8] == b"ULMDBCT4"
        || &trailer[..8] == b"ULMDBCM3"
        || &trailer[..8] == b"ULMDBCM4";
    let minor =
        if &trailer[..8] == MAGIC || &trailer[..8] == b"ULMDBCT3" || &trailer[..8] == b"ULMDBCM3" {
            3
        } else if &trailer[..8] == INDEXED_MAGIC
            || &trailer[..8] == b"ULMDBCT4"
            || &trailer[..8] == b"ULMDBCM4"
        {
            4
        } else {
            return Err(Error::Corrupt("invalid cohort trailer"));
        };
    if trailer[24..] != [0; 8] {
        return Err(Error::Corrupt("invalid cohort trailer reserved"));
    }
    let payload = u64::from_le_bytes(number(&trailer, 8)?);
    let length = u64::from_le_bytes(number(&trailer, 16)?);
    let (min, max) = if minor == 4 {
        (256, crate::metadata::MAX_DESCRIPTOR_BYTES)
    } else {
        (192, 67_109_056)
    };
    if (payload != 0
        && !(4096..=if transformed {
            128 + (ulmdb_core::ValueLimit::MAXIMUM.segment_limit() / 4096) * (4096 + 32 + 256 + 256)
        } else {
            ulmdb_core::ValueLimit::MAXIMUM.segment_limit()
        })
            .contains(&payload))
        || !(min..=if transformed {
            max + max / 4 + 8192
        } else {
            max
        })
            .contains(&length)
        || payload
            .checked_add(length)
            .and_then(|n| n.checked_add(FOOTER))
            != Some(total)
    {
        return Err(Error::Corrupt("invalid cohort extents"));
    }
    let logical = if transformed && payload > 0 {
        let (logical, encoded) = crate::transform::encoded_geometry(file)?;
        if encoded != payload {
            return Err(Error::Corrupt("transformed cohort prefix extent"));
        }
        logical
    } else {
        payload
    };
    Ok((payload, length, minor, logical))
}
pub(crate) fn read_descriptor(
    file: &mut File,
    generation: u128,
    mode: &crate::read_mode::ReadMode,
) -> Result<(Vec<u8>, u64)> {
    let (payload, physical, _minor, logical) = geometry_details(file)?;
    if let Some(context) = mode.context() {
        let (length, encoded) = crate::transform::envelope_geometry(file, payload)?;
        if encoded != physical {
            return Err(Error::Corrupt("transformed descriptor extent"));
        }
        let (bytes, _lease) = context.decode_mapping_at(
            file,
            payload,
            crate::transform::ObjectKind::Descriptor,
            generation,
            length,
        )?;
        Ok((bytes.to_vec(), logical))
    } else {
        file.seek(SeekFrom::Start(payload))?;
        let mut bytes = vec![0; physical as usize];
        file.read_exact(&mut bytes)?;
        if bytes.starts_with(b"ULMDBTX1") {
            return Err(Error::CryptoMismatch);
        }
        Ok((bytes, logical))
    }
}
pub(crate) fn read(directory: &Path, generation: u128) -> Result<(Vec<u8>, u64)> {
    let mut file = File::open(path(directory, generation))?;
    let (payload, length, _, logical) = geometry_details(&mut file)?;
    let mut descriptor = vec![0; usize::try_from(length).map_err(|_| Error::LimitExceeded)?];
    file.seek(SeekFrom::Start(payload))?;
    file.read_exact(&mut descriptor)?;
    Ok((descriptor, logical))
}
pub(crate) fn decode(directory: &Path, generation: u128, hash: [u8; 32]) -> Result<Manifest> {
    decode_with_fallback(directory, None, generation, hash)
}
pub(crate) fn decode_with_fallback(
    directory: &Path,
    fallback: Option<&Path>,
    generation: u128,
    hash: [u8; 32],
) -> Result<Manifest> {
    decode_with_mode(
        directory,
        fallback,
        generation,
        hash,
        crate::read_mode::ReadMode::Ordinary,
    )
}
pub(crate) fn decode_with_mode(
    directory: &Path,
    fallback: Option<&Path>,
    generation: u128,
    hash: [u8; 32],
    mode: crate::read_mode::ReadMode,
) -> Result<Manifest> {
    let named = path(directory, generation);
    let mut file = match mode.open(&named) {
        Ok(file) => file,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return crate::cohort_history::decode(directory, fallback, generation, hash, mode);
        }
        Err(error) => return Err(error),
    };
    let (_, _, minor, _) = geometry_details(&mut file)?;
    let (bytes, logical) = read_descriptor(&mut file, generation, &mode)?;
    if <[u8; 32]>::from(Sha256::digest(&bytes)) != hash {
        return Err(Error::Corrupt("cohort descriptor hash mismatch"));
    }
    let manifest = if minor == 4 {
        if mode == crate::read_mode::ReadMode::Ordinary {
            crate::metadata::decode_manifest(&bytes, directory, fallback)?
        } else {
            crate::metadata::decode_manifest_with_mode(&bytes, directory, fallback, mode.clone())?
        }
    } else {
        Manifest::decode(&bytes)?
    };
    if manifest.generation != generation || manifest.format_minor != minor {
        return Err(Error::Corrupt("cohort generation/version mismatch"));
    }
    validate_layout(&manifest, logical)?;
    mode.check(&named, &file)?;
    Ok(manifest)
}
/// Installing a verified copy preserves its immutable cohort bytes exactly.
pub(crate) fn install(directory: &Path, manifest: &Manifest) -> Result<[u8; 32]> {
    let bytes = manifest.encode()?;
    let hash = Sha256::digest(&bytes).into();
    if path(directory, manifest.generation).try_exists()? {
        let mode = crate::read_mode::ReadMode::Ordinary
            .with_transform(manifest.transform_context().cloned());
        let (existing, payload) = read_descriptor(
            &mut mode.open(&path(directory, manifest.generation))?,
            manifest.generation,
            &mode,
        )?;
        if existing != bytes {
            return Err(Error::Corrupt("copied cohort descriptor mismatch"));
        }
        validate_layout(manifest, payload)?;
        // Copy helpers sync files; also fence here for the public install API.
        ulmdb_platform::sync_file(&File::open(path(directory, manifest.generation))?)?;
        ulmdb_platform::sync_directory(&directory.join("cohorts"))?;
        Ok(hash)
    } else {
        write(directory, manifest, None)
    }
}

/// Canonical untransformed tail for the uniquely owned mapped allocator inode.
/// Layout checks are exactly the ordinary cohort sealer's; no arbitrary root or
/// payload reference is certified here. Arena owns and hashes the prefix.
pub(crate) fn mapped_tail(manifest: &Manifest, payload: u64) -> Result<(Vec<u8>, [u8; 32])> {
    if manifest.transform_context().is_some() {
        return Err(Error::InvalidInput(
            "transformed mapped cohort tail unsupported",
        ));
    }
    validate_layout(manifest, payload)?;
    let descriptor = manifest.encode()?;
    let mut trailer = [0; 32];
    trailer[..8].copy_from_slice(if manifest.format_minor == 4 {
        INDEXED_MAGIC
    } else {
        MAGIC
    });
    trailer[8..16].copy_from_slice(&payload.to_le_bytes());
    trailer[16..24].copy_from_slice(&(descriptor.len() as u64).to_le_bytes());
    Ok((descriptor, trailer))
}
