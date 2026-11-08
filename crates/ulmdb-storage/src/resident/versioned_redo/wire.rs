use super::*;
use sha2::{Digest, Sha256};
const HEADER: usize = 160;
const MAX: usize = wal_codec::MAX_RECORD_BYTES;
const DOMAIN: &[u8] = b"ULMDB-RESIDENT-RECIPE-BLOCK-V2\0";
fn hash(b: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(b);
    h.finalize().into()
}
fn put(b: &mut [u8], p: usize, v: u64) {
    b[p..p + 8].copy_from_slice(&v.to_le_bytes());
}
fn take(b: &[u8], p: usize, n: usize) -> Result<&[u8]> {
    b.get(
        p..p.checked_add(n)
            .ok_or(Error::Corrupt("RP2 wire overflow"))?,
    )
    .ok_or(Error::Corrupt("RP2 short wire"))
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
fn aligned(n: usize) -> Result<usize> {
    n.checked_add(7).map(|n| n & !7).ok_or(Error::LimitExceeded)
}
fn body_len(a: &Allocation) -> Result<usize> {
    match &a.body {
        Body::Literal(_) => Ok(PAGE),
        Body::Copy { patches, .. } => patches.iter().try_fold(16usize, |n, p| {
            n.checked_add(8)
                .and_then(|n| n.checked_add(aligned(p.bytes.len()).ok()?))
                .ok_or(Error::LimitExceeded)
        }),
    }
}
pub fn encode(r: &Recipes) -> Result<Vec<u8>> {
    r.validate()?;
    let length = r.allocations.iter().try_fold(HEADER + 32, |n, a| {
        n.checked_add(aligned(40 + a.ranges.len() * 4)?)
            .and_then(|n| n.checked_add(body_len(a).ok()?))
            .ok_or(Error::LimitExceeded)
    })?;
    if length > MAX {
        return Err(Error::LimitExceeded);
    }
    let mut b = Vec::new();
    b.try_reserve_exact(length)
        .map_err(|_| Error::LimitExceeded)?;
    b.resize(HEADER, 0);
    b[..8].copy_from_slice(b"ULMDBRP2");
    b[8..10].copy_from_slice(&2u16.to_le_bytes());
    b[12..16].copy_from_slice(&(HEADER as u32).to_le_bytes());
    put(&mut b, 16, length as u64);
    let a = r.authority;
    b[24..40].copy_from_slice(&a.boot.uuid);
    put(&mut b, 40, a.boot.epoch);
    b[48..64].copy_from_slice(&a.boot.commit.to_le_bytes());
    b[64..68].copy_from_slice(&a.boot.kv_pages.to_le_bytes());
    b[68..72].copy_from_slice(&a.boot.tiny_pages.to_le_bytes());
    for (p, v) in [
        (72, a.parent.lsn),
        (80, a.parent.sequence),
        (88, a.parent.offset),
        (96, a.parent.length),
    ] {
        put(&mut b, p, v);
    }
    b[104..136].copy_from_slice(&a.parent.digest);
    b[136..152].copy_from_slice(&a.parent_commit.to_le_bytes());
    b[152..156].copy_from_slice(&(r.allocations.len() as u32).to_le_bytes());
    for a in &r.allocations {
        let p = b.len();
        b.resize(p + 40, 0);
        b[p..p + 4].copy_from_slice(&a.destination.slot.to_le_bytes());
        b[p + 4..p + 6].copy_from_slice(&(a.ranges.len() as u16).to_le_bytes());
        b[p + 6] = if matches!(a.body, Body::Copy { .. }) {
            2
        } else {
            1
        };
        put(&mut b, p + 8, a.destination.generation);
        put(&mut b, p + 16, a.previous_generation);
        let patches = match &a.body {
            Body::Copy { patches, .. } => patches.len(),
            _ => 0,
        };
        b[p + 24..p + 28].copy_from_slice(&(patches as u32).to_le_bytes());
        b[p + 28..p + 32].copy_from_slice(&(body_len(a)? as u32).to_le_bytes());
        for &(start, len) in &a.ranges {
            b.extend_from_slice(&start.to_le_bytes());
            b.extend_from_slice(&len.to_le_bytes());
        }
        b.resize(aligned(b.len())?, 0);
        match &a.body {
            Body::Literal(bytes) => b.extend_from_slice(bytes),
            Body::Copy { source, patches } => {
                b.extend_from_slice(&source.slot.to_le_bytes());
                b.extend_from_slice(&[0; 4]);
                b.extend_from_slice(&source.generation.to_le_bytes());
                for p in patches {
                    b.extend_from_slice(&p.offset.to_le_bytes());
                    b.extend_from_slice(&(p.bytes.len() as u32).to_le_bytes());
                    b.extend_from_slice(&p.bytes);
                    b.resize(aligned(b.len())?, 0);
                }
            }
        }
    }
    let digest = hash(&b);
    b.extend_from_slice(&digest);
    debug_assert_eq!(b.len(), length);
    Ok(b)
}
pub fn decode(b: &[u8]) -> Result<Recipes> {
    if b.len() < HEADER + 32
        || b.len() > MAX
        || !b.len().is_multiple_of(8)
        || take(b, 0, 8)? != b"ULMDBRP2"
        || u16at(b, 8)? != 2
        || take(b, 10, 2)? != [0; 2]
        || u32at(b, 12)? as usize != HEADER
        || u64at(b, 16)? != b.len() as u64
        || take(b, 156, 4)? != [0; 4]
    {
        return Err(Error::Corrupt("RP2 header"));
    }
    let end = b.len() - 32;
    if hash(&b[..end]) != b[end..] {
        return Err(Error::Corrupt("RP2 digest"));
    }
    let authority = Authority {
        boot: Boot {
            uuid: take(b, 24, 16)?.try_into().unwrap(),
            epoch: u64at(b, 40)?,
            commit: u128::from_le_bytes(take(b, 48, 16)?.try_into().unwrap()),
            kv_pages: u32at(b, 64)?,
            tiny_pages: u32at(b, 68)?,
        },
        parent: WalReceipt {
            lsn: u64at(b, 72)?,
            sequence: u64at(b, 80)?,
            offset: u64at(b, 88)?,
            length: u64at(b, 96)?,
            digest: take(b, 104, 32)?.try_into().unwrap(),
        },
        parent_commit: u128::from_le_bytes(take(b, 136, 16)?.try_into().unwrap()),
    };
    authority.validate()?;
    let count = u32at(b, 152)? as usize;
    if count > authority.boot.slots()? || HEADER + count * 64 > end {
        return Err(Error::Corrupt("RP2 count"));
    }
    let mut allocations = Vec::new();
    allocations
        .try_reserve_exact(count)
        .map_err(|_| Error::LimitExceeded)?;
    let mut p = HEADER;
    for _ in 0..count {
        let h = take(b, p, 40)?;
        if h[7] != 0 || h[32..] != [0; 8] {
            return Err(Error::Corrupt("RP2 allocation reserved"));
        }
        let destination = VersionKey {
            slot: u32at(h, 0)?,
            generation: u64at(h, 8)?,
        };
        let previous_generation = u64at(h, 16)?;
        let nr = u16at(h, 4)? as usize;
        let np = u32at(h, 24)? as usize;
        let size = u32at(h, 28)? as usize;
        let kind = h[6];
        if nr == 0 || nr > 256 || np > PAGE / 2 {
            return Err(Error::Corrupt("RP2 bounded counts"));
        }
        p += 40;
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(nr)
            .map_err(|_| Error::LimitExceeded)?;
        for _ in 0..nr {
            ranges.push((u16at(b, p)?, u16at(b, p + 2)?));
            p += 4;
        }
        let next = aligned(p)?;
        if take(b, p, next - p)?.iter().any(|x| *x != 0) {
            return Err(Error::Corrupt("RP2 range padding"));
        }
        p = next;
        let body_end = p
            .checked_add(size)
            .filter(|n| *n <= end)
            .ok_or(Error::Corrupt("RP2 body extent"))?;
        let body = match kind {
            1 => {
                if np != 0 || size != PAGE {
                    return Err(Error::Corrupt("RP2 literal counts"));
                }
                Body::Literal(take(b, p, PAGE)?.to_vec())
            }
            2 => {
                if size < 16 + np * 16 || take(b, p + 4, 4)? != [0; 4] {
                    return Err(Error::Corrupt("RP2 copy counts"));
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
                    let offset = u32at(b, p)?;
                    let len = u32at(b, p + 4)? as usize;
                    p += 8;
                    if len == 0 || len > PAGE || p.checked_add(len).is_none_or(|n| n > body_end) {
                        return Err(Error::Corrupt("RP2 patch extent"));
                    }
                    let bytes = take(b, p, len)?.to_vec();
                    p += len;
                    let next = aligned(p)?;
                    if next > body_end || take(b, p, next - p)?.iter().any(|x| *x != 0) {
                        return Err(Error::Corrupt("RP2 patch padding"));
                    }
                    p = next;
                    patches.push(Patch { offset, bytes });
                }
                if p != body_end {
                    return Err(Error::Corrupt("RP2 copy trailing"));
                }
                Body::Copy { source, patches }
            }
            _ => return Err(Error::Corrupt("RP2 body tag")),
        };
        p = body_end;
        allocations.push(Allocation {
            destination,
            previous_generation,
            ranges,
            body,
        });
    }
    if p != end {
        return Err(Error::Corrupt("RP2 trailing records"));
    }
    let r = Recipes {
        authority,
        allocations,
    };
    r.validate()?;
    Ok(r)
}
