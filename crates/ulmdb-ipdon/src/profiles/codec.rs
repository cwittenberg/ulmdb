use super::*;

pub(super) fn encode_prefix(prefix: Prefix) -> [u8; 18] {
    let mut b = [0; 18];
    b[1] = prefix.length();
    match prefix.network() {
        Address::V4(value) => {
            b[0] = 4;
            b[2..6].copy_from_slice(&value.to_be_bytes());
        }
        Address::V6(value) => {
            b[0] = 6;
            b[2..].copy_from_slice(&value.to_be_bytes());
        }
    }
    b
}
fn decode_prefix(bytes: &[u8]) -> Result<Prefix> {
    if bytes.len() != 18 {
        return Err(Error::Corrupt("field prefix extent"));
    }
    let address = match bytes[0] {
        4 if bytes[6..].iter().all(|b| *b == 0) => {
            Address::V4(u32::from_be_bytes(bytes[2..6].try_into().unwrap()))
        }
        6 => Address::V6(u128::from_be_bytes(bytes[2..18].try_into().unwrap())),
        _ => return Err(Error::Corrupt("field prefix family")),
    };
    Prefix::new(address, bytes[1]).map_err(|_| Error::Corrupt("noncanonical field prefix"))
}
pub(super) fn encode(claim: &Claim) -> Result<Vec<u8>> {
    if claim.id.0 == [0; 32] || claim.field.0 == 0 || claim.source == 0 || claim.generation == 0 {
        return Err(Error::InvalidInput("zero field claim identity/source"));
    }
    if claim.value.len() > MAX_VALUE_BYTES {
        return Err(Error::LimitExceeded);
    }
    validate_freshness(claim.freshness())?;
    let mut bytes = vec![1];
    bytes.extend_from_slice(&claim.field.0.to_be_bytes());
    bytes.extend_from_slice(&claim.generation.to_be_bytes());
    bytes.extend_from_slice(&encode_prefix(claim.prefix));
    bytes.extend_from_slice(&claim.source.to_be_bytes());
    bytes.extend_from_slice(&claim.priority.to_be_bytes());
    bytes.extend_from_slice(&encode_fresh(claim.freshness()));
    bytes.extend_from_slice(&claim.evidence);
    bytes.extend_from_slice(&(claim.value.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&claim.value);
    Ok(bytes)
}
pub(super) struct ClaimRef<'a> {
    pub id: ClaimId,
    pub generation: u64,
    pub field: FieldId,
    pub prefix: Prefix,
    pub source: u64,
    pub priority: u32,
    pub freshness: Freshness,
    pub value: &'a [u8],
    pub evidence: [u8; 32],
}
pub(super) fn decode(id: ClaimId, bytes: &[u8]) -> Result<ClaimRef<'_>> {
    const FIXED: usize = 107;
    if bytes.len() < FIXED || bytes[0] != 1 {
        return Err(Error::Corrupt("field claim header/extent"));
    }
    let field = FieldId(number(&bytes[1..9])?);
    let generation = number(&bytes[9..17])?;
    let prefix = decode_prefix(&bytes[17..35])?;
    let source = number(&bytes[35..43])?;
    let priority = u32::from_be_bytes(bytes[43..47].try_into().unwrap());
    let freshness = decode_fresh(&bytes[47..71])?;
    let evidence = bytes[71..103].try_into().unwrap();
    let length = u32::from_be_bytes(bytes[103..107].try_into().unwrap()) as usize;
    if id.0 == [0; 32]
        || field.0 == 0
        || source == 0
        || generation == 0
        || length > MAX_VALUE_BYTES
        || bytes.len() != FIXED + length
    {
        return Err(Error::Corrupt("field claim identity/value extent"));
    }
    Ok(ClaimRef {
        id,
        generation,
        field,
        prefix,
        source,
        priority,
        freshness,
        value: &bytes[FIXED..],
        evidence,
    })
}
pub(super) fn encode_fresh(freshness: Freshness) -> [u8; 24] {
    let mut bytes = [0; 24];
    bytes[..8].copy_from_slice(&freshness.observed_at.to_be_bytes());
    bytes[8..16].copy_from_slice(&freshness.recorded_at.to_be_bytes());
    bytes[16..].copy_from_slice(&freshness.expires_at.to_be_bytes());
    bytes
}
pub(super) fn decode_fresh(bytes: &[u8]) -> Result<Freshness> {
    if bytes.len() != 24 {
        return Err(Error::Corrupt("field freshness extent"));
    }
    let fresh = Freshness {
        observed_at: number(&bytes[..8])?,
        recorded_at: number(&bytes[8..16])?,
        expires_at: number(&bytes[16..])?,
    };
    validate_freshness(fresh).map_err(|_| Error::Corrupt("invalid field freshness"))?;
    Ok(fresh)
}
pub(super) fn encode_bucket(ids: &[ClaimId]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + ids.len() * 32);
    bytes.push(1);
    for id in ids {
        bytes.extend_from_slice(&id.0);
    }
    bytes
}
pub(super) fn decode_bucket(bytes: &[u8]) -> Result<Vec<ClaimId>> {
    if bytes.len() < 33
        || bytes[0] != 1
        || !(bytes.len() - 1).is_multiple_of(32)
        || (bytes.len() - 1) / 32 > MAX_CLAIMS_PER_PREFIX
    {
        return Err(Error::Corrupt("field bucket extent"));
    }
    let mut ids = Vec::with_capacity((bytes.len() - 1) / 32);
    let mut seen = std::collections::BTreeSet::new();
    for bytes in bytes[1..].chunks_exact(32) {
        let id = ClaimId(bytes.try_into().unwrap());
        if id.0 == [0; 32] || !seen.insert(id) {
            return Err(Error::Corrupt("invalid/duplicate field bucket identity"));
        }
        ids.push(id);
    }
    Ok(ids)
}
