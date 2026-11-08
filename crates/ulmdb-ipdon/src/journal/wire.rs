use super::{Cursor, Event, Limits, Versions};
use sha2::{Digest, Sha256};
use ulmdb::{Error, Result};

pub(super) const META: &[u8] = b"ipdon.journal.meta.v1";
pub(super) const EVENTS: &[u8] = b"ipdon.journal.events.v1";
pub(super) const CURSORS: &[u8] = b"ipdon.journal.cursors.v1";
pub(super) const HEADS: &[u8] = b"ipdon.journal.heads.v1";
pub(super) const FLOORS: &[u8] = b"ipdon.journal.floors.v1";
pub(super) const REALM: &[u8] = b"realm";
pub(super) const VERSIONS: &[u8] = b"versions";
pub(super) const FORMAT: &[u8] = b"format";
pub(super) const MAX_ORIGINS: usize = 4096;
const PAYLOAD_OVERHEAD: usize = 1 + 8 + 8 + 8 + 4 + 32;
pub(super) const EVENT_OVERHEAD: usize = PAYLOAD_OVERHEAD + 32;

pub(super) fn limit(limits: Limits) -> Result<()> {
    if limits.max_events == 0 || limits.max_bytes < EVENT_OVERHEAD {
        return Err(Error::InvalidInput(
            "journal limits must admit one encoded event",
        ));
    }
    Ok(())
}
pub(super) fn key(origin: u64, sequence: u64) -> [u8; 16] {
    let mut key = [0; 16];
    key[..8].copy_from_slice(&origin.to_be_bytes());
    key[8..].copy_from_slice(&sequence.to_be_bytes());
    key
}
pub(super) fn number(bytes: &[u8]) -> Result<u64> {
    Ok(u64::from_be_bytes(
        bytes
            .try_into()
            .map_err(|_| Error::Corrupt("journal integer length"))?,
    ))
}
pub(super) fn encode_cursor(cursor: Cursor) -> [u8; 24] {
    let mut out = [0; 24];
    out[..8].copy_from_slice(&cursor.applied.to_be_bytes());
    out[8..16].copy_from_slice(&cursor.serving.to_be_bytes());
    out[16..].copy_from_slice(&cursor.floor.to_be_bytes());
    out
}
pub(super) fn decode_cursor(bytes: &[u8]) -> Result<Cursor> {
    if bytes.len() != 24 {
        return Err(Error::Corrupt("journal cursor length"));
    }
    let c = Cursor {
        applied: number(&bytes[..8])?,
        serving: number(&bytes[8..16])?,
        floor: number(&bytes[16..])?,
    };
    if c.serving > c.applied || c.floor > c.serving {
        return Err(Error::Corrupt("journal cursor ordering"));
    }
    Ok(c)
}
pub(super) fn encode_versions(v: Versions) -> Result<[u8; 16]> {
    if v.schema == 0 || v.policy == 0 {
        return Err(Error::InvalidInput("journal versions must be nonzero"));
    }
    let mut out = [0; 16];
    out[..8].copy_from_slice(&v.schema.to_be_bytes());
    out[8..].copy_from_slice(&v.policy.to_be_bytes());
    Ok(out)
}
pub(super) fn decode_versions(bytes: &[u8]) -> Result<Versions> {
    if bytes.len() != 16 {
        return Err(Error::Corrupt("journal version length"));
    }
    let v = Versions {
        schema: number(&bytes[..8])?,
        policy: number(&bytes[8..])?,
    };
    if v.schema == 0 || v.policy == 0 {
        return Err(Error::Corrupt("zero journal version"));
    }
    Ok(v)
}
pub(super) fn encoded_len(event: &Event) -> Result<usize> {
    if event.origin == 0 || event.sequence == 0 || event.payload.len() > u32::MAX as usize {
        return Err(Error::InvalidInput(
            "invalid journal event identity or payload length",
        ));
    }
    event
        .payload
        .len()
        .checked_add(EVENT_OVERHEAD)
        .ok_or(Error::LimitExceeded)
}
pub(super) fn encode(event: &Event) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(encoded_len(event)?);
    out.push(1);
    out.extend_from_slice(&event.origin.to_be_bytes());
    out.extend_from_slice(&event.sequence.to_be_bytes());
    out.extend_from_slice(&event.recorded_at.to_be_bytes());
    out.extend_from_slice(&(event.payload.len() as u32).to_be_bytes());
    out.extend_from_slice(&event.payload);
    let hash = Sha256::digest(&out);
    out.extend_from_slice(&hash);
    Ok(out)
}
pub(super) fn decode(bytes: &[u8]) -> Result<Event> {
    if bytes.len() < PAYLOAD_OVERHEAD || bytes[0] != 1 {
        return Err(Error::Corrupt("journal event header"));
    }
    let length = u32::from_be_bytes(bytes[25..29].try_into().unwrap()) as usize;
    if length.checked_add(PAYLOAD_OVERHEAD) != Some(bytes.len())
        || Sha256::digest(&bytes[..bytes.len() - 32])[..] != bytes[bytes.len() - 32..]
    {
        return Err(Error::Corrupt("journal event length or digest"));
    }
    let e = Event {
        origin: number(&bytes[1..9])?,
        sequence: number(&bytes[9..17])?,
        recorded_at: number(&bytes[17..25])?,
        payload: bytes[29..bytes.len() - 32].to_vec(),
    };
    if e.origin == 0 || e.sequence == 0 {
        return Err(Error::Corrupt("zero journal event identity"));
    }
    Ok(e)
}
pub(super) fn chain(previous: &[u8; 32], event_bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"IPDon/origin-chain/v1\0");
    hash.update(previous);
    hash.update(&event_bytes[event_bytes.len() - 32..]);
    hash.finalize().into()
}
pub(super) fn encode_record(event: &Event, previous: &[u8; 32]) -> Result<Vec<u8>> {
    let mut bytes = encode(event)?;
    let hash = chain(previous, &bytes);
    bytes.extend_from_slice(&hash);
    Ok(bytes)
}
pub(super) fn decode_record(bytes: &[u8]) -> Result<(Event, [u8; 32])> {
    if bytes.len() < EVENT_OVERHEAD {
        return Err(Error::Corrupt("journal record length"));
    }
    let at = bytes.len() - 32;
    Ok((decode(&bytes[..at])?, bytes[at..].try_into().unwrap()))
}
