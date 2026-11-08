use super::{assets::MAX_ASSETS, types::*};
use hmac::{Hmac, Mac};
use sha2::Sha256;
pub(super) const HEADER_BYTES: usize = 320;
pub(super) const ASSET_BYTES: usize = 64;
pub(super) const TAG_BYTES: usize = 32;
pub(super) const DOMAIN: &[u8] = b"ULMDB/replication/frame/v1\0";
pub(super) type Authenticator = Hmac<Sha256>;
pub(super) fn authenticator(key: &PublisherKey) -> Result<Authenticator> {
    let mut mac = Authenticator::new_from_slice(key.secret())
        .map_err(|_| ReplicationError::InvalidInput("invalid authentication key"))?;
    mac.update(DOMAIN);
    Ok(mac)
}
fn cursor_encode(cursor: Cursor) -> [u8; 112] {
    let mut b = [0; 112];
    b[..16].copy_from_slice(&cursor.uuid);
    b[16..24].copy_from_slice(&cursor.epoch.to_le_bytes());
    b[24..32].copy_from_slice(&cursor.sequence.to_le_bytes());
    b[32..48].copy_from_slice(&cursor.commit.to_le_bytes());
    b[48..].copy_from_slice(&cursor.checkpoint);
    b
}
fn cursor_decode(b: &[u8]) -> Result<Cursor> {
    let cursor = Cursor {
        uuid: b[..16].try_into().unwrap(),
        epoch: u64::from_le_bytes(b[16..24].try_into().unwrap()),
        sequence: u64::from_le_bytes(b[24..32].try_into().unwrap()),
        commit: u128::from_le_bytes(b[32..48].try_into().unwrap()),
        checkpoint: b[48..112].try_into().unwrap(),
    };
    cursor.validate()?;
    Ok(cursor)
}
pub(super) fn encode(header: &TransferHeader) -> [u8; HEADER_BYTES] {
    let mut b = [0; HEADER_BYTES];
    b[..8].copy_from_slice(b"ULMDBR01");
    b[8..10].copy_from_slice(&1u16.to_le_bytes());
    b[10..12].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
    b[12..16].copy_from_slice(&u32::from(header.base.is_some()).to_le_bytes());
    b[16..24].copy_from_slice(&header.key_id.to_le_bytes());
    b[24..40].copy_from_slice(&header.nonce.to_le_bytes());
    if let Some(base) = header.base {
        b[40..152].copy_from_slice(&cursor_encode(base));
    }
    b[152..264].copy_from_slice(&cursor_encode(header.target));
    b[264..268].copy_from_slice(&header.file_count.to_le_bytes());
    b[272..280].copy_from_slice(&header.payload_bytes.to_le_bytes());
    b
}
pub(super) fn decode(
    b: &[u8; HEADER_BYTES],
    key: &PublisherKey,
    limits: TransferLimits,
) -> Result<TransferHeader> {
    if &b[..8] != b"ULMDBR01"
        || b[8..10] != 1u16.to_le_bytes()
        || b[10..12] != (HEADER_BYTES as u16).to_le_bytes()
        || b[268..272] != [0; 4]
        || b[280..].iter().any(|v| *v != 0)
    {
        return Err(ReplicationError::Corrupt("invalid transfer header"));
    }
    let flags = u32::from_le_bytes(b[12..16].try_into().unwrap());
    let base = match flags {
        0 if b[40..152].iter().all(|v| *v == 0) => None,
        1 => Some(cursor_decode(&b[40..152])?),
        _ => return Err(ReplicationError::Corrupt("invalid transfer base presence")),
    };
    let key_id = u64::from_le_bytes(b[16..24].try_into().unwrap());
    if key_id != key.id {
        return Err(ReplicationError::UnauthorizedPublisher);
    }
    let target = cursor_decode(&b[152..264])?;
    key.authorize(&target)?;
    if let Some(base) = &base {
        key.authorize(base)?;
    }
    let nonce = u128::from_le_bytes(b[24..40].try_into().unwrap());
    if nonce == 0 {
        return Err(ReplicationError::Corrupt("zero transfer nonce"));
    }
    let file_count = u32::from_le_bytes(b[264..268].try_into().unwrap());
    let payload_bytes = u64::from_le_bytes(b[272..280].try_into().unwrap());
    if file_count > MAX_ASSETS || file_count > limits.max_files || payload_bytes > limits.max_bytes
    {
        return Err(ReplicationError::LimitExceeded);
    }
    if u64::from(file_count)
        .checked_mul(192)
        .ok_or(ReplicationError::LimitExceeded)?
        > payload_bytes
    {
        return Err(ReplicationError::Corrupt(
            "asset count exceeds declared payload",
        ));
    }
    if file_count == 0 && payload_bytes != 0 {
        return Err(ReplicationError::Corrupt("payload without assets"));
    }
    if let Some(base) = base {
        if base == target {
            if file_count != 0 {
                return Err(ReplicationError::Conflict);
            }
        } else {
            if target.generation()? == base.generation()? {
                return Err(ReplicationError::Conflict);
            }
            if target.sequence != base.sequence
                && base.sequence.checked_add(1) != Some(target.sequence)
            {
                return Err(ReplicationError::Gap);
            }
            if target.sequence == base.sequence && target.commit != base.commit {
                return Err(ReplicationError::Conflict);
            }
        }
    }
    Ok(TransferHeader {
        base,
        target,
        key_id,
        nonce,
        file_count,
        payload_bytes,
    })
}
pub(super) fn encode_asset(asset: AssetDescription) -> [u8; ASSET_BYTES] {
    let mut b = [0; ASSET_BYTES];
    match asset.id {
        AssetId::Segment(hash) => {
            b[0] = 1;
            b[8..40].copy_from_slice(&hash);
        }
        AssetId::Descriptor(id) => {
            b[0] = 2;
            b[8..24].copy_from_slice(&id.to_le_bytes());
        }
        AssetId::Cohort(id) => {
            b[0] = 3;
            b[8..24].copy_from_slice(&id.to_le_bytes());
        }
    }
    b[40..48].copy_from_slice(&asset.length.to_le_bytes());
    b
}
pub(super) fn decode_asset(b: &[u8; ASSET_BYTES]) -> Result<AssetDescription> {
    if b[1..8].iter().chain(&b[48..64]).any(|v| *v != 0) {
        return Err(ReplicationError::Corrupt("reserved asset metadata"));
    }
    let id = match b[0] {
        1 => AssetId::Segment(b[8..40].try_into().unwrap()),
        2 | 3 if b[24..40] == [0; 16] => {
            let id = u128::from_le_bytes(b[8..24].try_into().unwrap());
            if id == 0 {
                return Err(ReplicationError::Corrupt("zero asset generation"));
            }
            if b[0] == 2 {
                AssetId::Descriptor(id)
            } else {
                AssetId::Cohort(id)
            }
        }
        _ => return Err(ReplicationError::Corrupt("invalid asset type/identity")),
    };
    let length = u64::from_le_bytes(b[40..48].try_into().unwrap());
    id.check_length(length)?;
    Ok(AssetDescription { id, length })
}
pub(super) fn wire_bytes(files: u32, payload: u64) -> Result<u64> {
    u64::from(files)
        .checked_mul(ASSET_BYTES as u64)
        .and_then(|n| n.checked_add(payload))
        .and_then(|n| n.checked_add((HEADER_BYTES + TAG_BYTES) as u64))
        .ok_or(ReplicationError::LimitExceeded)
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
