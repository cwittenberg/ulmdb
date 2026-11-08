use super::{assets, types::*, wire};
use crate::{Environment, ReadTxn};
use hmac::Mac;
use std::{
    fs::File,
    io::{Read, Write},
};

/// Stream exactly one authenticated immutable snapshot frame. Both readers must
/// belong to this open environment; their snapshot pins remain live throughout.
pub fn send_snapshot<W: Write>(
    environment: &Environment,
    read: &ReadTxn,
    base: Option<&ReadTxn>,
    key: &PublisherKey,
    output: &mut W,
) -> Result<TransferReport> {
    let owner = environment.current.load().owner;
    if read.snapshot.owner != owner || base.is_some_and(|r| r.snapshot.owner != owner) {
        return Err(ReplicationError::InvalidInput(
            "replication reader belongs to another environment",
        ));
    }
    let target = Cursor::from_read(read)?;
    key.authorize(&target)?;
    let base_cursor = base.map(Cursor::from_read).transpose()?;
    if let (Some(base_read), Some(base_cursor)) = (base, base_cursor) {
        key.authorize(&base_cursor)?;
        if base_cursor != target {
            super::activation::lineage(&base_read.snapshot.manifest, &read.snapshot.manifest)?;
        }
    }
    let files = if let Some(base) = base {
        let target_ids = assets::ids(&read.snapshot.manifest);
        let base_ids = assets::ids(&base.snapshot.manifest);
        assets::describe_ids(
            &environment.directory,
            target_ids.difference(&base_ids).copied(),
        )?
    } else {
        assets::describe_manifest(&environment.directory, &read.snapshot.manifest)?
    };
    let count = u32::try_from(files.len()).map_err(|_| ReplicationError::LimitExceeded)?;
    let payload = files.iter().try_fold(0u64, |sum, a| {
        sum.checked_add(a.length)
            .ok_or(ReplicationError::LimitExceeded)
    })?;
    let header = TransferHeader {
        base: base_cursor,
        target,
        key_id: key.id,
        nonce: ulmdb_storage::new_identity()?,
        file_count: count,
        payload_bytes: payload,
    };
    let encoded = wire::encode(&header);
    let mut mac = wire::authenticator(key)?;
    // Keep sender/receiver structural policy identical before any output bytes.
    wire::decode(
        &encoded,
        key,
        TransferLimits {
            max_files: assets::MAX_ASSETS,
            max_bytes: u64::MAX,
        },
    )?;
    mac.update(&encoded);
    output.write_all(&encoded)?;
    let mut buffer = [0u8; 131_072];
    for asset in files {
        let encoded = wire::encode_asset(asset);
        mac.update(&encoded);
        output.write_all(&encoded)?;
        let mut input = File::open(asset.id.path(&environment.directory))?;
        let mut remaining = asset.length;
        while remaining != 0 {
            let length = remaining.min(buffer.len() as u64) as usize;
            input.read_exact(&mut buffer[..length])?;
            mac.update(&buffer[..length]);
            output.write_all(&buffer[..length])?;
            remaining -= length as u64;
        }
        if input.read(&mut buffer[..1])? != 0 {
            return Err(ReplicationError::Corrupt(
                "source asset grew during transfer",
            ));
        }
    }
    let tag = mac.finalize().into_bytes();
    output.write_all(&tag)?;
    Ok(TransferReport {
        base: base_cursor,
        target,
        files: count,
        payload_bytes: payload,
        wire_bytes: wire::wire_bytes(count, payload)?,
    })
}
