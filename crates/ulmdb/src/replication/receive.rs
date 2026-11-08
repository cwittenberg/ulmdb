use super::{types::*, wire};
use hmac::Mac;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

/// Consume exactly one frame without requiring transport EOF. The caller must
/// discard/resynchronize a transport after an error. No environment is changed;
/// successful authentication returns a private immutable staging owner.
pub fn receive<R: Read>(
    input: &mut R,
    key: &PublisherKey,
    limits: TransferLimits,
    staging_parent: impl AsRef<Path>,
) -> Result<VerifiedTransfer> {
    let mut bytes = [0; wire::HEADER_BYTES];
    input.read_exact(&mut bytes)?;
    let header = wire::decode(&bytes, key, limits)?;
    let mut mac = wire::authenticator(key)?;
    mac.update(&bytes);
    let stage = OwnedStage::create(staging_parent.as_ref())?;
    let mut assets = Vec::with_capacity((header.file_count as usize).min(4096));
    let mut previous = None;
    let mut consumed = 0u64;
    let mut buffer = [0u8; 131_072];
    for _ in 0..header.file_count {
        let mut metadata = [0; wire::ASSET_BYTES];
        input.read_exact(&mut metadata)?;
        let asset = wire::decode_asset(&metadata)?;
        if previous.is_some_and(|id| id >= asset.id) {
            return Err(ReplicationError::Corrupt(
                "duplicate or noncanonical asset ordering",
            ));
        }
        previous = Some(asset.id);
        consumed = consumed
            .checked_add(asset.length)
            .ok_or(ReplicationError::LimitExceeded)?;
        if consumed > header.payload_bytes || consumed > limits.max_bytes {
            return Err(ReplicationError::LimitExceeded);
        }
        mac.update(&metadata);
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(asset.id.path(stage.path()))?;
        let mut remaining = asset.length;
        while remaining != 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            input.read_exact(&mut buffer[..count])?;
            mac.update(&buffer[..count]);
            file.write_all(&buffer[..count])?;
            remaining -= count as u64;
        }
        let mut permissions = file.metadata()?.permissions();
        permissions.set_readonly(true);
        file.set_permissions(permissions)?;
        assets.push(asset);
    }
    if consumed != header.payload_bytes {
        return Err(ReplicationError::Corrupt(
            "declared transfer byte total mismatch",
        ));
    }
    let mut tag = [0u8; wire::TAG_BYTES];
    input.read_exact(&mut tag)?;
    mac.verify_slice(&tag)
        .map_err(|_| ReplicationError::Authentication)?;
    // Synchronize only authenticated data; invalid MACs cannot demand a file
    // flush for every unauthenticated asset. Target activation syncs again.
    for asset in &assets {
        ulmdb_platform::sync_path(asset.id.path(stage.path()))?;
    }
    for name in ["segments", "commits", "cohorts"] {
        ulmdb_platform::sync_path(stage.path().join(name))?;
    }
    ulmdb_platform::sync_path(stage.path())?;
    Ok(VerifiedTransfer {
        header,
        stage,
        assets,
    })
}
