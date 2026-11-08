use super::*;
const MAGIC: &[u8; 8] = b"ULMDBRL1";
const DOMAIN: &[u8] = b"ULMDB-RESIDENT-LITERAL-RECORD-V1\0";

/// Header-only allocation bound for streaming scanners. This grants no record
/// authority: complete decode still binds Boot, placement, digest and recipes.
pub fn frame_len(header: &[u8]) -> Result<usize> {
    if header.len() != HEADER
        || take(header, 0, 8)? != MAGIC
        || u16_at(header, 8)? != 1
        || !matches!(header[10], 1 | 2)
        || header[11] != 0
        || u32_at(header, 12)? != HEADER as u32
        || header[208..].iter().any(|&v| v != 0)
    {
        return Err(Error::Corrupt("resident scanner header"));
    }
    let raw = u64_at(header, 16)?;
    if raw < (HEADER + DIGEST) as u64 || raw > MAX_RECORD_BYTES as u64 || !raw.is_multiple_of(8) {
        return Err(Error::Corrupt("resident scanner length"));
    }
    let n = usize::try_from(raw).map_err(|_| Error::Corrupt("resident scanner host length"))?;
    let kv = u32_at(header, 192)?;
    let total = kv
        .checked_add(u32_at(header, 196)?)
        .filter(|&n| kv > 0 && n <= MAX_SLOTS as u32)
        .ok_or(Error::Corrupt("resident scanner pool count"))? as usize;
    let a = u32_at(header, 184)? as usize;
    let r = u32_at(header, 188)? as usize;
    if a > total
        || r > total
        || a.checked_add(r).is_none_or(|v| v > total)
        || a.checked_mul(32 + 4 + PAGE + 4)
            .and_then(|bytes| {
                r.checked_mul(16)
                    .and_then(|retired| bytes.checked_add(retired))
            })
            .and_then(|bytes| bytes.checked_add(HEADER + DIGEST))
            .is_none_or(|minimum| minimum > n)
    {
        return Err(Error::Corrupt("resident scanner event counts"));
    }
    Ok(n)
}

fn u16_at(b: &[u8], p: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(take(b, p, 2)?.try_into().unwrap()))
}
fn u32_at(b: &[u8], p: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(take(b, p, 4)?.try_into().unwrap()))
}
fn u64_at(b: &[u8], p: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(take(b, p, 8)?.try_into().unwrap()))
}
fn u128_at(b: &[u8], p: usize) -> Result<u128> {
    Ok(u128::from_le_bytes(take(b, p, 16)?.try_into().unwrap()))
}
fn take(b: &[u8], p: usize, n: usize) -> Result<&[u8]> {
    let end = p
        .checked_add(n)
        .ok_or(Error::Corrupt("resident wire range overflow"))?;
    b.get(p..end).ok_or(Error::Corrupt("short resident record"))
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(bytes);
    h.finalize().into()
}
fn put_receipt(b: &mut [u8], p: usize, r: WalReceipt) {
    b[p..p + 8].copy_from_slice(&r.lsn.to_le_bytes());
    b[p + 8..p + 16].copy_from_slice(&r.sequence.to_le_bytes());
    b[p + 16..p + 24].copy_from_slice(&r.offset.to_le_bytes());
    b[p + 24..p + 32].copy_from_slice(&r.length.to_le_bytes());
    b[p + 32..p + 64].copy_from_slice(&r.digest);
}
fn receipt(b: &[u8], p: usize) -> Result<WalReceipt> {
    Ok(WalReceipt {
        lsn: u64_at(b, p)?,
        sequence: u64_at(b, p + 8)?,
        offset: u64_at(b, p + 16)?,
        length: u64_at(b, p + 24)?,
        digest: take(b, p + 32, 32)?.try_into().unwrap(),
    })
}
pub fn encode(boot: Boot, record: &Record) -> Result<Encoded> {
    encode_limited(boot, record, MAX_RECORD_BYTES)
}
/// Encode once and retain the owned, immutable recipe for pre-IO state checks.
/// Only this module and decode can construct the receipt-bound token.
pub fn encode_owned(boot: Boot, record: Record) -> Result<(Encoded, Decoded)> {
    let encoded = encode(boot, &record)?;
    let receipt = WalReceipt {
        lsn: record.lsn,
        sequence: record.sequence,
        offset: record.parent.end()?,
        length: encoded.bytes.len() as u64,
        digest: encoded.digest,
    };
    receipt.end()?;
    Ok((encoded, Decoded { record, receipt }))
}
pub fn encode_limited(boot: Boot, r: &Record, limit: usize) -> Result<Encoded> {
    let length = r.encoded_length(boot)?;
    if length > limit {
        return Err(Error::LimitExceeded);
    }
    let mut b = Vec::new();
    b.try_reserve_exact(length)
        .map_err(|_| Error::LimitExceeded)?;
    b.resize(HEADER, 0);
    b[..8].copy_from_slice(MAGIC);
    b[8..10].copy_from_slice(&1u16.to_le_bytes());
    b[10] = match r.kind {
        Kind::Write => 1,
        Kind::Collect => 2,
    };
    b[12..16].copy_from_slice(&(HEADER as u32).to_le_bytes());
    b[16..24].copy_from_slice(&(length as u64).to_le_bytes());
    b[24..40].copy_from_slice(&r.uuid);
    b[40..48].copy_from_slice(&r.epoch.to_le_bytes());
    b[48..56].copy_from_slice(&r.lsn.to_le_bytes());
    b[56..64].copy_from_slice(&r.sequence.to_le_bytes());
    b[64..80].copy_from_slice(&r.commit.to_le_bytes());
    b[80..96].copy_from_slice(&r.parent_commit.to_le_bytes());
    put_receipt(&mut b, 96, r.parent);
    b[160..176].copy_from_slice(&r.root.encode());
    b[176..184].copy_from_slice(&r.next_database.to_le_bytes());
    b[184..188].copy_from_slice(&(r.allocations.len() as u32).to_le_bytes());
    b[188..192].copy_from_slice(&(r.retired.len() as u32).to_le_bytes());
    b[192..196].copy_from_slice(&boot.kv_pages.to_le_bytes());
    b[196..200].copy_from_slice(&boot.tiny_pages.to_le_bytes());
    b[200..208].copy_from_slice(&r.parent.end()?.to_le_bytes());
    for a in &r.allocations {
        let p = b.len();
        b.resize(p + 32, 0);
        b[p..p + 4].copy_from_slice(&a.key.slot.to_le_bytes());
        b[p + 4..p + 6].copy_from_slice(&(a.ranges.len() as u16).to_le_bytes());
        b[p + 8..p + 16].copy_from_slice(&a.key.generation.to_le_bytes());
        b[p + 16..p + 24].copy_from_slice(&a.expected_previous_generation.to_le_bytes());
        b[p + 24..p + 28].copy_from_slice(&(PAGE as u32).to_le_bytes());
        for &(start, n) in &a.ranges {
            b.extend_from_slice(&start.to_le_bytes());
            b.extend_from_slice(&n.to_le_bytes());
        }
        b.extend_from_slice(&a.bytes);
        b.resize(b.len().next_multiple_of(8), 0);
    }
    for key in &r.retired {
        b.extend_from_slice(&key.slot.to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&key.generation.to_le_bytes());
    }
    let digest = hash(&b);
    b.extend_from_slice(&digest);
    debug_assert_eq!(b.len(), length);
    Ok(Encoded { bytes: b, digest })
}
pub fn decode(boot: Boot, bytes: &[u8], offset: u64) -> Result<Decoded> {
    decode_limited(boot, bytes, offset, MAX_RECORD_BYTES)
}
pub fn decode_limited(boot: Boot, b: &[u8], offset: u64, limit: usize) -> Result<Decoded> {
    let slots = boot.slots()?;
    if b.len() > limit {
        return Err(Error::LimitExceeded);
    }
    if b.len() < HEADER + DIGEST
        || b.len() > MAX_RECORD_BYTES
        || !b.len().is_multiple_of(8)
        || take(b, 0, 8)? != MAGIC
        || u16_at(b, 8)? != 1
        || b[11] != 0
        || u32_at(b, 12)? != HEADER as u32
        || u64_at(b, 16)? != b.len() as u64
        || b[208..HEADER].iter().any(|&n| n != 0)
        || u32_at(b, 192)? != boot.kv_pages
        || u32_at(b, 196)? != boot.tiny_pages
        || offset < DATA_START
        || !offset.is_multiple_of(8)
        || u64_at(b, 200)? != offset
    {
        return Err(Error::Corrupt("resident record header/geometry"));
    }
    let kind = match b[10] {
        1 => Kind::Write,
        2 => Kind::Collect,
        _ => return Err(Error::Corrupt("resident record kind")),
    };
    let allocations = u32_at(b, 184)? as usize;
    let retired = u32_at(b, 188)? as usize;
    if allocations > slots
        || retired > slots
        || allocations.checked_add(retired).is_none_or(|n| n > slots)
    {
        return Err(Error::Corrupt("resident record count bound"));
    }
    let end = b.len() - DIGEST;
    let minimum = HEADER
        .checked_add(allocations * (32 + 4 + PAGE + 4))
        .and_then(|n| n.checked_add(retired * 16))
        .ok_or(Error::Corrupt("resident count size overflow"))?;
    if minimum > end {
        return Err(Error::Corrupt("resident counts exceed record"));
    }
    let digest: [u8; 32] = b[end..].try_into().unwrap();
    if hash(&b[..end]) != digest {
        return Err(Error::Corrupt("resident record digest"));
    }
    let mut r = Record {
        uuid: b[24..40].try_into().unwrap(),
        epoch: u64_at(b, 40)?,
        lsn: u64_at(b, 48)?,
        sequence: u64_at(b, 56)?,
        commit: u128_at(b, 64)?,
        parent_commit: u128_at(b, 80)?,
        parent: receipt(b, 96)?,
        root: Reference::decode(&b[160..176])?,
        next_database: u64_at(b, 176)?,
        kind,
        allocations: Vec::new(),
        retired: Vec::new(),
    };
    if r.parent.end()? != offset {
        return Err(Error::Corrupt("resident record placement/parent"));
    }
    r.allocations
        .try_reserve_exact(allocations)
        .map_err(|_| Error::LimitExceeded)?;
    r.retired
        .try_reserve_exact(retired)
        .map_err(|_| Error::LimitExceeded)?;
    let mut p = HEADER;
    for _ in 0..allocations {
        let h = take(&b[..end], p, 32)?;
        let n = usize::from(u16_at(h, 4)?);
        if n == 0
            || n > MAX_RANGES
            || h[6..8] != [0; 2]
            || h[28..32] != [0; 4]
            || u32_at(h, 24)? != PAGE as u32
        {
            return Err(Error::Corrupt("resident allocation header"));
        }
        let key = VersionKey {
            slot: u32_at(h, 0)?,
            generation: u64_at(h, 8)?,
        };
        let expected = u64_at(h, 16)?;
        p += 32;
        let raw = take(&b[..end], p, n * 4)?;
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(n)
            .map_err(|_| Error::LimitExceeded)?;
        for x in raw.chunks_exact(4) {
            ranges.push((
                u16::from_le_bytes(x[..2].try_into().unwrap()),
                u16::from_le_bytes(x[2..].try_into().unwrap()),
            ));
        }
        validate_ranges(boot, key.slot, &ranges)?;
        p += n * 4;
        let raw = take(&b[..end], p, PAGE)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(PAGE)
            .map_err(|_| Error::LimitExceeded)?;
        bytes.extend_from_slice(raw);
        p += PAGE;
        let aligned = p.next_multiple_of(8);
        if take(&b[..end], p, aligned - p)?.iter().any(|&n| n != 0) {
            return Err(Error::Corrupt("resident nonzero allocation padding"));
        }
        p = aligned;
        r.allocations.push(LiteralSlot {
            key,
            expected_previous_generation: expected,
            ranges,
            bytes,
        });
    }
    for _ in 0..retired {
        let raw = take(&b[..end], p, 16)?;
        if raw[4..8] != [0; 4] {
            return Err(Error::Corrupt("resident retired reserved"));
        }
        r.retired.push(VersionKey {
            slot: u32_at(raw, 0)?,
            generation: u64_at(raw, 8)?,
        });
        p += 16;
    }
    if p != end {
        return Err(Error::Corrupt("resident trailing record bytes"));
    }
    r.validate(boot)?;
    let receipt = WalReceipt {
        lsn: r.lsn,
        sequence: r.sequence,
        offset,
        length: b.len() as u64,
        digest,
    };
    receipt.end()?;
    Ok(Decoded { record: r, receipt })
}
