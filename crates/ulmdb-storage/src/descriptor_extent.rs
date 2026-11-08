//! Bounded physical descriptor location, without inventory or graph decoding.
use crate::{Error, Result};
use std::{
    fs::File,
    path::Path,
};
use ulmdb_platform::{FileExt, FileIdentity};

pub struct DescriptorSourceExtent {
    pub file: File,
    pub start: u64,
    /// Decoded descriptor length, even for a transformed physical extent.
    pub length: usize,
    pub transformed: bool,
    pub generation: u128,
    pub format_minor: u16,
}
/// Locate an exact typed checkpoint's source using fixed headers/trailers only.
/// This does not verify its content hash or grant a lease. Callers keep genuine
/// source protection and check their namespace authority before and after IO.
pub fn open_descriptor_source(
    directory: &Path,
    checkpoint: &[u8; 64],
) -> Result<DescriptorSourceExtent> {
    let (generation, _) = crate::checkpoint_identity(checkpoint)?;
    let minor = match &checkpoint[..8] {
        b"ULMDBT04" => 4,
        b"ULMDBC03" => 3,
        _ => 2,
    };
    open(directory, generation, minor, Some(checkpoint))
}
/// Locate a linked generation before its content hash has been observed.
/// Generation/minor alone authenticate no descriptor or logical identity.
pub fn open_descriptor_generation(
    directory: &Path,
    generation: u128,
    minor: u16,
) -> Result<DescriptorSourceExtent> {
    open(directory, generation, minor, None)
}
fn open(
    directory: &Path,
    generation: u128,
    minor: u16,
    expected: Option<&[u8; 64]>,
) -> Result<DescriptorSourceExtent> {
    if generation == 0 || ![1, 2, 3, 4].contains(&minor) {
        return Err(Error::InvalidInput("descriptor source generation/version"));
    }
    if minor < 3 {
        let path = directory
            .join("commits")
            .join(format!("{generation:032x}.root"));
        let file = ulmdb_platform::open_readonly(&path)?;
        let before = checked(&path, &file)?;
        let mut magic = [0; 8];
        file.read_exact_at(&mut magic, 0)?;
        let transformed = &magic == b"ULMDBTX1";
        let length = if transformed {
            let (logical, encoded) = crate::transform::envelope_geometry(&file, 0)?;
            if encoded != before.length {
                return Err(Error::Corrupt("legacy descriptor encoded extent"));
            }
            logical
        } else {
            before.length
        };
        if !(192..=67_109_056).contains(&length) {
            return Err(Error::Corrupt("legacy descriptor length"));
        }
        unchanged(&path, &file, before)?;
        return Ok(DescriptorSourceExtent {
            file,
            start: 0,
            length: usize::try_from(length).map_err(|_| Error::LimitExceeded)?,
            transformed,
            generation,
            format_minor: minor,
        });
    }
    let path = crate::cohort::path(directory, generation);
    match ulmdb_platform::open_readonly(&path) {
        Ok(mut file) => {
            let before = checked(&path, &file)?;
            let (start, physical, actual_minor, _) = crate::cohort::geometry_details(&mut file)?;
            if expected.is_some() && actual_minor != minor {
                return Err(Error::Corrupt("descriptor cohort version"));
            }
            let mut trailer = [0; 32];
            file.read_exact_at(&mut trailer, before.length - 32)?;
            let transformed = matches!(
                &trailer[..8],
                b"ULMDBCT3" | b"ULMDBCT4" | b"ULMDBCM3" | b"ULMDBCM4"
            );
            let length = descriptor_length(&file, start, physical, transformed, actual_minor)?;
            unchanged(&path, &file, before)?;
            Ok(DescriptorSourceExtent {
                file,
                start,
                length,
                transformed,
                generation,
                format_minor: actual_minor,
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let archive = match crate::cohort_history::open(directory, generation) {
                Ok(a)=>a,
                Err(Error::Io(e)) if expected.is_none() && e.kind()==std::io::ErrorKind::NotFound => return open(directory,generation,2,None),
                Err(e)=>return Err(e),
            };
            if (expected.is_some() && archive.minor != minor) || expected.is_some_and(|cp| *cp != archive.checkpoint) {
                return Err(Error::Corrupt("descriptor history binding"));
            }
            let path = crate::cohort_history::path(directory, generation);
            let before = checked(&path, &archive.file)?;
            let length = descriptor_length(
                &archive.file,
                archive.body,
                archive.physical,
                archive.transformed,
                archive.minor,
            )?;
            unchanged(&path, &archive.file, before)?;
            Ok(DescriptorSourceExtent {
                file: archive.file,
                start: archive.body,
                length,
                transformed: archive.transformed,
                generation,
                format_minor: archive.minor,
            })
        }
        Err(e) => Err(e.into()),
    }
}
fn descriptor_length(
    file: &File,
    start: u64,
    physical: u64,
    transformed: bool,
    minor: u16,
) -> Result<usize> {
    let length = if transformed {
        let (logical, encoded) = crate::transform::envelope_geometry(file, start)?;
        if encoded != physical {
            return Err(Error::Corrupt("descriptor envelope extent"));
        }
        logical
    } else {
        physical
    };
    let (min, max) = if minor == 4 {
        (256, crate::MAX_DESCRIPTOR_BYTES)
    } else {
        (192, 67_109_056)
    };
    if !(min..=max).contains(&length) {
        return Err(Error::Corrupt("descriptor decoded extent"));
    }
    usize::try_from(length).map_err(|_| Error::LimitExceeded)
}
fn checked(path: &Path, file: &File) -> Result<FileIdentity> {
    let held = ulmdb_platform::file_identity(file)?;
    let named = ulmdb_platform::path_identity(path)?;
    if !held.is_file()
        || !named.is_file()
        || held.volume != named.volume
        || held.file_id != named.file_id
        || held.length != named.length
    {
        return Err(Error::Corrupt("descriptor source replaced"));
    }
    Ok(held)
}
fn unchanged(path: &Path, file: &File, before: FileIdentity) -> Result<()> {
    if checked(path, file)? != before {
        return Err(Error::Corrupt("descriptor source changed"));
    }
    Ok(())
}
pub(crate) fn observed_checkpoint(generation: u128, minor: u16, hash: [u8; 32]) -> [u8; 64] {
    let mut cp = crate::publication::pointer(generation, hash);
    if minor >= 3 {
        cp[..8].copy_from_slice(if minor == 4 { b"ULMDBT04" } else { b"ULMDBC03" });
        let crc = crate::publication::crc(&cp[..60]);
        cp[60..64].copy_from_slice(&crc.to_le_bytes());
    }
    cp
}
