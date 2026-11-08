use super::*;
use crate::hybrid::redo::Patch;
const MAGIC: &[u8; 8] = b"ULMDBRL2";
const DOMAIN: &[u8] = b"ULMDB-RESIDENT-COMPACT-RECORD-V2\0";
fn take(b: &[u8], p: usize, n: usize) -> Result<&[u8]> {
    b.get(
        p..p.checked_add(n)
            .ok_or(Error::Corrupt("RL2 wire overflow"))?,
    )
    .ok_or(Error::Corrupt("RL2 short record"))
}
fn u16at(b: &[u8], p: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(take(b, p, 2)?.try_into().unwrap()))
}
fn u32at(b: &[u8], p: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(take(b, p, 4)?.try_into().unwrap()))
}
fn u64at(b: &[u8], p: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(take(b, p, 8)?.try_into().unwrap()))
}
fn u128at(b: &[u8], p: usize) -> Result<u128> {
    Ok(u128::from_le_bytes(take(b, p, 16)?.try_into().unwrap()))
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(bytes);
    h.finalize().into()
}
fn put64(b: &mut [u8], p: usize, n: u64) {
    b[p..p + 8].copy_from_slice(&n.to_le_bytes());
}
fn put_receipt(b: &mut [u8], p: usize, r: WalReceipt) {
    for (i, n) in [r.lsn, r.sequence, r.offset, r.length]
        .into_iter()
        .enumerate()
    {
        put64(b, p + i * 8, n);
    }
    b[p + 32..p + 64].copy_from_slice(&r.digest);
}
fn receipt(b: &[u8], p: usize) -> Result<WalReceipt> {
    Ok(WalReceipt {
        lsn: u64at(b, p)?,
        sequence: u64at(b, p + 8)?,
        offset: u64at(b, p + 16)?,
        length: u64at(b, p + 24)?,
        digest: take(b, p + 32, 32)?.try_into().unwrap(),
    })
}
/// Scanner preflight bounds allocation; full decode still binds the whole frame.
pub fn frame_len(b: &[u8]) -> Result<usize> {
    if b.len() != HEADER
        || take(b, 0, 8)? != MAGIC
        || u16at(b, 8)? != 2
        || !matches!(b[10], 1 | 2)
        || b[11] != 0
        || u32at(b, 12)? != HEADER as u32
        || b[208..].iter().any(|&x| x != 0)
    {
        return Err(Error::Corrupt("RL2 scanner header"));
    }
    let length = u64at(b, 16)?;
    if length < (HEADER + FOOTER) as u64
        || length > MAX_RECORD_BYTES as u64
        || !length.is_multiple_of(8)
    {
        return Err(Error::Corrupt("RL2 scanner length"));
    }
    let kv = u32at(b, 192)?;
    let slots = kv
        .checked_add(u32at(b, 196)?)
        .filter(|&n| kv > 0 && n <= MAX_SLOTS as u32)
        .ok_or(Error::Corrupt("RL2 scanner pool"))? as usize;
    let a = u32at(b, 184)? as usize;
    let r = u32at(b, 188)? as usize;
    if a > slots
        || r > slots
        || a.checked_add(r).is_none_or(|n| n > slots)
        || a.checked_mul(64)
            .and_then(|bytes| {
                r.checked_mul(16)
                    .and_then(|retired| bytes.checked_add(retired))
            })
            .and_then(|bytes| bytes.checked_add(HEADER + FOOTER))
            .is_none_or(|minimum| minimum as u64 > length)
    {
        return Err(Error::Corrupt("RL2 scanner counts"));
    }
    usize::try_from(length).map_err(|_| Error::Corrupt("RL2 scanner host length"))
}
pub fn encode(boot: Boot, record: &Record) -> Result<Encoded> {
    encode_limited(boot, record, MAX_RECORD_BYTES)
}
pub fn encode_owned(boot: Boot, record: Record) -> Result<(Encoded, Decoded)> {
    encode_owned_limited(boot, record, MAX_RECORD_BYTES)
}
/// Validate once and reject the caller's byte budget before allocating a frame.
/// The owned token retains the same canonical validation as `decode_limited`.
pub fn encode_owned_limited(
    boot: Boot,
    record: Record,
    limit: usize,
) -> Result<(Encoded, Decoded)> {
    let encoded = encode_limited(boot, &record, limit)?;
    let receipt = WalReceipt {
        lsn: record.lsn,
        sequence: record.sequence,
        offset: record.parent.end()?,
        length: encoded.bytes.len() as u64,
        digest: encoded.digest,
    };
    receipt.end()?;
    Ok((
        encoded,
        Decoded {
            boot,
            record,
            receipt,
        },
    ))
}
pub fn encode_limited(boot: Boot, r: &Record, limit: usize) -> Result<Encoded> {
    let mut bytes = Vec::new();
    let digest = encode_view_into(boot, r, limit, &mut bytes)?;
    Ok(Encoded { bytes, digest })
}
pub(crate) fn encode_view_into(
    boot: Boot,
    v: &impl view::RecordView,
    limit: usize,
    b: &mut Vec<u8>,
) -> Result<[u8; 32]> {
    let length = view::encoded_length(v, boot)?;
    if length > limit {
        return Err(Error::LimitExceeded);
    }
    let r = v.header();
    b.clear();
    b.try_reserve_exact(length)
        .map_err(|_| Error::LimitExceeded)?;
    if b.capacity() > MAX_RECORD_BYTES {
        *b = Vec::new();
        return Err(Error::LimitExceeded);
    }
    b.resize(HEADER, 0);
    b[..8].copy_from_slice(MAGIC);
    b[8..10].copy_from_slice(&2u16.to_le_bytes());
    b[10] = match r.kind {
        Kind::Write => 1,
        Kind::Collect => 2,
    };
    b[12..16].copy_from_slice(&(HEADER as u32).to_le_bytes());
    put64(b, 16, length as u64);
    b[24..40].copy_from_slice(&r.uuid);
    for (p, n) in [(40, r.epoch), (48, r.lsn), (56, r.sequence)] {
        put64(b, p, n);
    }
    b[64..80].copy_from_slice(&r.commit.to_le_bytes());
    b[80..96].copy_from_slice(&r.parent_commit.to_le_bytes());
    put_receipt(b, 96, r.parent);
    b[160..176].copy_from_slice(&r.root.encode());
    put64(b, 176, r.next_database);
    for (p, n) in [
        (184, v.allocation_count() as u32),
        (188, r.retired.len() as u32),
        (192, boot.kv_pages),
        (196, boot.tiny_pages),
    ] {
        b[p..p + 4].copy_from_slice(&n.to_le_bytes());
    }
    put64(b, 200, r.parent.end()?);
    for i in 0..v.allocation_count() {
        let a = v.allocation(i);
        let p = b.len();
        b.resize(p + 40, 0);
        b[p..p + 4].copy_from_slice(&a.destination.slot.to_le_bytes());
        b[p + 4..p + 6].copy_from_slice(&(a.ranges.len() as u16).to_le_bytes());
        b[p + 6] = match a.body {
            view::BodyRef::Literal(_) => 1,
            view::BodyRef::Copy { .. } => 2,
        };
        put64(b, p + 8, a.destination.generation);
        put64(b, p + 16, a.previous_generation);
        let patches = match a.body {
            view::BodyRef::Literal(_) => 0,
            view::BodyRef::Copy { patches, .. } => patches.len(),
        };
        b[p + 24..p + 28].copy_from_slice(&(patches as u32).to_le_bytes());
        b[p + 28..p + 32].copy_from_slice(&(view::body_len(a.body)? as u32).to_le_bytes());
        for &(start, n) in a.ranges {
            b.extend_from_slice(&start.to_le_bytes());
            b.extend_from_slice(&n.to_le_bytes());
        }
        b.resize(aligned(b.len())?, 0);
        match a.body {
            view::BodyRef::Literal(bytes) => b.extend_from_slice(bytes),
            view::BodyRef::Copy { source, patches } => {
                b.extend_from_slice(&source.slot.to_le_bytes());
                b.extend_from_slice(&[0; 4]);
                b.extend_from_slice(&source.generation.to_le_bytes());
                for i in 0..patches.len() {
                    let p = patches.get(i);
                    b.extend_from_slice(&p.offset.to_le_bytes());
                    b.extend_from_slice(&(p.bytes.len() as u32).to_le_bytes());
                    b.extend_from_slice(p.bytes);
                    b.resize(aligned(b.len())?, 0);
                }
            }
        }
    }
    for key in &r.retired {
        b.extend_from_slice(&key.slot.to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&key.generation.to_le_bytes());
    }
    let digest = hash(b);
    b.extend_from_slice(&digest);
    debug_assert_eq!(b.len(), length);
    Ok(digest)
}

pub fn decode(boot: Boot, bytes: &[u8], offset: u64) -> Result<Decoded> {
    decode_limited(boot, bytes, offset, MAX_RECORD_BYTES)
}
pub fn decode_limited(boot: Boot, b: &[u8], offset: u64, limit: usize) -> Result<Decoded> {
    boot.slots()?;
    if b.len() > limit {
        return Err(Error::LimitExceeded);
    }
    if b.len() < HEADER + FOOTER
        || frame_len(take(b, 0, HEADER)?)? != b.len()
        || u32at(b, 192)? != boot.kv_pages
        || u32at(b, 196)? != boot.tiny_pages
        || offset < DATA_START
        || !offset.is_multiple_of(8)
        || u64at(b, 200)? != offset
    {
        return Err(Error::Corrupt("RL2 header/geometry"));
    }
    let end = b.len() - FOOTER;
    let digest: [u8; 32] = b[end..].try_into().unwrap();
    if hash(&b[..end]) != digest {
        return Err(Error::Corrupt("RL2 record digest"));
    }
    let allocations = u32at(b, 184)? as usize;
    let retired = u32at(b, 188)? as usize;
    let mut r = Record {
        uuid: b[24..40].try_into().unwrap(),
        epoch: u64at(b, 40)?,
        lsn: u64at(b, 48)?,
        sequence: u64at(b, 56)?,
        commit: u128at(b, 64)?,
        parent_commit: u128at(b, 80)?,
        parent: receipt(b, 96)?,
        root: Reference::decode(&b[160..176])?,
        next_database: u64at(b, 176)?,
        kind: if b[10] == 1 {
            Kind::Write
        } else {
            Kind::Collect
        },
        allocations: Vec::new(),
        retired: Vec::new(),
    };
    if r.parent.end()? != offset {
        return Err(Error::Corrupt("RL2 parent placement"));
    }
    r.allocations
        .try_reserve_exact(allocations)
        .map_err(|_| Error::LimitExceeded)?;
    r.retired
        .try_reserve_exact(retired)
        .map_err(|_| Error::LimitExceeded)?;
    let mut p = HEADER;
    for _ in 0..allocations {
        let h = take(&b[..end], p, 40)?;
        let nr = usize::from(u16at(h, 4)?);
        let np = u32at(h, 24)? as usize;
        let size = u32at(h, 28)? as usize;
        let kind = h[6];
        if nr == 0 || nr > MAX_RANGES || np > PAGE / 2 || h[7] != 0 || h[32..] != [0; 8] {
            return Err(Error::Corrupt("RL2 allocation header"));
        }
        let destination = VersionKey {
            slot: u32at(h, 0)?,
            generation: u64at(h, 8)?,
        };
        let previous_generation = u64at(h, 16)?;
        p += 40;
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(nr)
            .map_err(|_| Error::LimitExceeded)?;
        for raw in take(&b[..end], p, nr * 4)?.chunks_exact(4) {
            ranges.push((
                u16::from_le_bytes(raw[..2].try_into().unwrap()),
                u16::from_le_bytes(raw[2..].try_into().unwrap()),
            ));
        }
        p += nr * 4;
        super::super::wal_codec::validate_ranges(boot, destination.slot, &ranges)?;
        let next = aligned(p)?;
        if take(&b[..end], p, next - p)?.iter().any(|&x| x != 0) {
            return Err(Error::Corrupt("RL2 range padding"));
        }
        p = next;
        let body_end = p
            .checked_add(size)
            .filter(|&n| n <= end)
            .ok_or(Error::Corrupt("RL2 body extent"))?;
        let body = match kind {
            1 => {
                if np != 0 || size != PAGE {
                    return Err(Error::Corrupt("RL2 literal count/width"));
                }
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(PAGE)
                    .map_err(|_| Error::LimitExceeded)?;
                bytes.extend_from_slice(take(&b[..end], p, PAGE)?);
                p = body_end;
                Body::Literal(bytes)
            }
            2 => {
                if size >= PAGE || size < 16 + np * 16 || take(&b[..body_end], p + 4, 4)? != [0; 4]
                {
                    return Err(Error::Corrupt("RL2 Copy size/count"));
                }
                let source = VersionKey {
                    slot: u32at(b, p)?,
                    generation: u64at(b, p + 8)?,
                };
                p += 16;
                let mut patches = Vec::new();
                patches
                    .try_reserve_exact(np)
                    .map_err(|_| Error::LimitExceeded)?;
                for _ in 0..np {
                    let offset = u32at(take(&b[..body_end], p, 8)?, 0)?;
                    let n = u32at(b, p + 4)? as usize;
                    p += 8;
                    if n == 0 || n > PAGE {
                        return Err(Error::Corrupt("RL2 patch length"));
                    }
                    let raw = take(&b[..body_end], p, n)?;
                    let mut bytes = Vec::new();
                    bytes
                        .try_reserve_exact(n)
                        .map_err(|_| Error::LimitExceeded)?;
                    bytes.extend_from_slice(raw);
                    p += n;
                    let next = aligned(p)?;
                    if take(&b[..body_end], p, next - p)?.iter().any(|&x| x != 0) {
                        return Err(Error::Corrupt("RL2 patch padding"));
                    }
                    p = next;
                    patches.push(Patch { offset, bytes });
                }
                if p != body_end {
                    return Err(Error::Corrupt("RL2 Copy trailing bytes"));
                }
                Body::Copy { source, patches }
            }
            _ => return Err(Error::Corrupt("RL2 allocation kind")),
        };
        r.allocations.push(Allocation {
            destination,
            previous_generation,
            ranges,
            body,
        });
    }
    for _ in 0..retired {
        let raw = take(&b[..end], p, 16)?;
        if raw[4..8] != [0; 4] {
            return Err(Error::Corrupt("RL2 retired reserved"));
        }
        r.retired.push(VersionKey {
            slot: u32at(raw, 0)?,
            generation: u64at(raw, 8)?,
        });
        p += 16;
    }
    if p != end {
        return Err(Error::Corrupt("RL2 trailing bytes"));
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
    Ok(Decoded {
        boot,
        record: r,
        receipt,
    })
}
