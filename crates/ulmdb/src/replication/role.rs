//! Durable receiver role. A visible replicated root must never reopen writable.
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
use ulmdb_storage::Manifest;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReplicaIdentity {
    pub key_id: u64,
    pub uuid: [u8; 16],
    pub epoch: u64,
}
const NAME: &str = "REPLICA";
fn encode(role: ReplicaIdentity) -> [u8; 64] {
    let mut bytes = [0; 64];
    bytes[..8].copy_from_slice(b"ULMDBREP");
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&64u16.to_le_bytes());
    bytes[16..32].copy_from_slice(&role.uuid);
    bytes[32..40].copy_from_slice(&role.key_id.to_le_bytes());
    bytes[40..48].copy_from_slice(&role.epoch.to_le_bytes());
    let hash = Sha256::digest(&bytes[..48]);
    bytes[48..].copy_from_slice(&hash[..16]);
    bytes
}
fn decode(bytes: &[u8]) -> Result<ReplicaIdentity> {
    if bytes.len() != 64
        || &bytes[..8] != b"ULMDBREP"
        || bytes[8..10] != 1u16.to_le_bytes()
        || bytes[10..12] != 64u16.to_le_bytes()
        || bytes[12..16] != [0; 4]
        || bytes[48..] != Sha256::digest(&bytes[..48])[..16]
    {
        return Err(Error::Corrupt("invalid replica role marker"));
    }
    let role = ReplicaIdentity {
        uuid: bytes[16..32]
            .try_into()
            .map_err(|_| Error::Corrupt("replica uuid"))?,
        key_id: u64::from_le_bytes(
            bytes[32..40]
                .try_into()
                .map_err(|_| Error::Corrupt("replica key id"))?,
        ),
        epoch: u64::from_le_bytes(
            bytes[40..48]
                .try_into()
                .map_err(|_| Error::Corrupt("replica epoch"))?,
        ),
    };
    if role.uuid == [0; 16] || role.key_id == 0 || role.epoch == 0 {
        return Err(Error::Corrupt("invalid replica authority"));
    }
    Ok(role)
}
pub(crate) fn load_role(directory: &Path, manifest: &Manifest) -> Result<Option<ReplicaIdentity>> {
    let path = directory.join(NAME);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_file() || metadata.len() != 64 {
        return Err(Error::Corrupt("nonregular replica role"));
    }
    let mut bytes = Vec::with_capacity(65);
    File::open(path)?.take(65).read_to_end(&mut bytes)?;
    let role = decode(&bytes)?;
    if role.uuid != manifest.uuid || role.epoch != manifest.epoch {
        return Err(Error::Corrupt("replica role/root mismatch"));
    }
    Ok(Some(role))
}
fn point(name: &str) -> Result<()> {
    #[cfg(feature = "fault-injection")]
    ulmdb_storage::injected_fault(name)?;
    #[cfg(not(feature = "fault-injection"))]
    let _ = name;
    Ok(())
}
pub(crate) fn write_role(directory: &Path, role: ReplicaIdentity) -> Result<()> {
    let bytes = encode(role);
    decode(&bytes)?;
    let mut file = ulmdb_storage::access::options_for_entry(&directory.join(NAME))?
        .write(true)
        .create_new(true)
        .open(directory.join(NAME))?;
    point("replica.role.before_write")?;
    file.write_all(&bytes)?;
    point("replica.role.after_write")?;
    point("replica.role.before_file_sync")?;
    ulmdb_platform::sync_file(&file)?;
    point("replica.role.after_file_sync")?;
    point("replica.role.before_directory_sync")?;
    ulmdb_platform::sync_path(directory)?;
    point("replica.role.after_directory_sync")?;
    Ok(())
}
