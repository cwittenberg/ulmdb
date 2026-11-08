use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
use ulmdb::replication::{Cursor, PublisherKey};
const LENGTH: usize = 96;
pub fn generate(path: &Path, cursor: Cursor, id: u64) -> Result<(), Box<dyn Error>> {
    let mut secret = [0; 32];
    File::open("/dev/urandom")?.read_exact(&mut secret)?;
    let key = PublisherKey::new(id, cursor.uuid, cursor.epoch, secret)?;
    let mut bytes = [0; LENGTH];
    bytes[..8].copy_from_slice(b"ULMDBKEY");
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&(LENGTH as u16).to_le_bytes());
    bytes[16..24].copy_from_slice(&key.id().to_le_bytes());
    bytes[24..40].copy_from_slice(&key.uuid());
    bytes[40..48].copy_from_slice(&key.epoch().to_le_bytes());
    bytes[48..80].copy_from_slice(&secret);
    let digest = Sha256::digest(&bytes[..80]);
    bytes[80..].copy_from_slice(&digest[..16]);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(&bytes)?;
    ulmdb_platform::sync_file(&file)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    ulmdb_platform::sync_path(parent)?;
    Ok(())
}
pub fn load(path: &Path) -> Result<PublisherKey, Box<dyn Error>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.len() != LENGTH as u64 {
        return Err("invalid key-file size/type".into());
    }
    let mut bytes = Vec::with_capacity(LENGTH + 1);
    File::open(path)?
        .take((LENGTH + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() != LENGTH
        || &bytes[..8] != b"ULMDBKEY"
        || bytes[8..10] != 1u16.to_le_bytes()
        || bytes[10..12] != (LENGTH as u16).to_le_bytes()
        || bytes[12..16] != [0; 4]
        || bytes[80..] != Sha256::digest(&bytes[..80])[..16]
    {
        return Err("invalid key-file header/checksum".into());
    }
    Ok(PublisherKey::new(
        u64::from_le_bytes(bytes[16..24].try_into()?),
        bytes[24..40].try_into()?,
        u64::from_le_bytes(bytes[40..48].try_into()?),
        bytes[48..80].try_into()?,
    )?)
}
