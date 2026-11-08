//! Read-only compound recovery-base decoder. Decoding grants no Environment
//! publication capability; the engine-owned selector remains trusted authority.
use crate::hybrid::{
    checkpoint::selection::{self, Selector},
    journal::{Bootstrap, WalLayout, Witness},
    redo::WalReceipt,
};
use crate::{Error, Result};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::{fs::OpenOptions, io::Read, path::Path};
const DOMAIN: &[u8] = b"ULMDB/hybrid/compound-floor/v2\0";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Compound {
    selector: Selector,
    predecessor: Witness,
    first: Witness,
    prefix_end: u64,
    granule: u64,
    serial: u64,
    previous: [u8; 32],
    digest: [u8; 32],
    header_digest: [u8; 32],
}
impl Compound {
    pub fn selector(self) -> Selector {
        self.selector
    }
    pub fn predecessor(self) -> Witness {
        self.predecessor
    }
    pub fn first_required(self) -> Witness {
        self.first
    }
    pub fn prefix_end(self) -> u64 {
        self.prefix_end
    }
    pub fn granule(self) -> u64 {
        self.granule
    }
    pub fn serial(self) -> u64 {
        self.serial
    }
    pub fn previous_digest(self) -> [u8; 32] {
        self.previous
    }
    pub fn digest(self) -> [u8; 32] {
        self.digest
    }
    pub fn header_digest(self) -> [u8; 32] {
        self.header_digest
    }
}
// Cold fixed-size authority records stay Copy: equality/rechecks must not
// allocate after preparing a durable decision. The largest payload is 480B.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryBase {
    Legacy(Selector),
    Compound(Compound),
}
impl RecoveryBase {
    pub fn selector(self) -> Selector {
        match self {
            Self::Legacy(s) => s,
            Self::Compound(c) => c.selector(),
        }
    }
}
fn g64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn hash(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(domain);
    h.update(bytes);
    h.finalize().into()
}
fn witness(b: &[u8], at: usize) -> Result<Witness> {
    let w = Witness {
        receipt: WalReceipt {
            lsn: g64(b, at),
            sequence: g64(b, at + 8),
            offset: g64(b, at + 16),
            length: g64(b, at + 24),
            digest: b[at + 32..at + 64].try_into().unwrap(),
        },
        commit: u128::from_le_bytes(b[at + 64..at + 80].try_into().unwrap()),
    };
    let invalid = if w.receipt.lsn == 0 {
        w.receipt.sequence != 0 || w.receipt.offset != 196608 || w.receipt.length != 0
    } else {
        w.receipt.sequence == 0
            || w.receipt.offset < 196608
            || !w.receipt.offset.is_multiple_of(8)
            || w.receipt.length < 320
            || !w.receipt.length.is_multiple_of(8)
            || w.receipt.end()? > 268435456
    };
    if invalid || w.commit == 0 || w.receipt.digest == [0; 32] {
        return Err(Error::Corrupt("compound retained witness geometry"));
    }
    Ok(w)
}
pub fn decode_compound(b: &[u8; 512]) -> Result<Compound> {
    if &b[..8] != b"ULMDBHB2"
        || b[8..12] != 2u32.to_le_bytes()
        || b[12..16] != 512u32.to_le_bytes()
        || b[464..480] != [0; 16]
        || b[432..464] == [0; 32]
        || hash(DOMAIN, &b[..480]) != b[480..]
    {
        return Err(Error::Corrupt("compound recovery-base header/hash"));
    }
    // Reuse the complete V1 scalar/cut checks. This temporary canonical record
    // is only a decoder adapter, never an on-disk authority/publication.
    let mut legacy = [0u8; 256];
    legacy[..200].copy_from_slice(&b[..200]);
    legacy[..8].copy_from_slice(b"ULMDBHB1");
    legacy[8..12].copy_from_slice(&1u32.to_le_bytes());
    legacy[12..16].copy_from_slice(&256u32.to_le_bytes());
    let digest = hash(b"ULMDB/hybrid/recovery-base/v1\0", &legacy[..224]);
    legacy[224..].copy_from_slice(&digest);
    let selector = selection::decode_bytes(&legacy)?;
    let predecessor = witness(b, 264)?;
    let first = witness(b, 344)?;
    let c = Compound {
        selector,
        predecessor,
        first,
        prefix_end: g64(b, 248),
        granule: g64(b, 256),
        serial: g64(b, 424),
        previous: b[200..232].try_into().unwrap(),
        digest: b[480..].try_into().unwrap(),
        header_digest: b[432..464].try_into().unwrap(),
    };
    if selector.cut.receipt.lsn == 0
        || predecessor.receipt.lsn.checked_add(1) != Some(selector.cut.receipt.lsn)
        || predecessor.receipt.sequence.checked_add(1) != Some(selector.cut.receipt.sequence)
        || predecessor.receipt.end()? != selector.cut.receipt.offset
        || first.receipt.lsn > predecessor.receipt.lsn
        || first.receipt.sequence > predecessor.receipt.sequence
        || first.receipt.offset > predecessor.receipt.offset
        || first.receipt.end()? > predecessor.receipt.end()?
        || (first.receipt.lsn == predecessor.receipt.lsn && first != predecessor)
        || (first.receipt.lsn < predecessor.receipt.lsn
            && first.receipt.end()? > predecessor.receipt.offset)
        || g64(b, 232) != first.receipt.lsn
        || g64(b, 240) != first.receipt.offset
        || c.granule != 65536
        || c.prefix_end < 196608
        || !c.prefix_end.is_multiple_of(c.granule)
        || c.prefix_end > first.receipt.offset
        || c.serial == 0
        || (c.serial == 1) != (c.previous == [0; 32])
    {
        return Err(Error::Corrupt("compound recovery-base floor/cut binding"));
    }
    Ok(c)
}
/// Strict length/tag dispatch. A malformed selected V2 never falls back to V1.
pub fn read_typed(engine: &Path) -> Result<Option<RecoveryBase>> {
    let mut f = match OpenOptions::new()
        .read(true)
        .custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS)
        .open(engine.join("HYBRID.base"))
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let meta = f.metadata()?;
    if !meta.is_file() {
        return Err(Error::Corrupt("recovery-base file type"));
    }
    match meta.len() {
        256 => {
            let mut b = [0; 256];
            f.read_exact(&mut b)?;
            Ok(Some(RecoveryBase::Legacy(selection::decode_bytes(&b)?)))
        }
        512 => {
            let mut b = [0; 512];
            f.read_exact(&mut b)?;
            Ok(Some(RecoveryBase::Compound(decode_compound(&b)?)))
        }
        _ => Err(Error::Corrupt("recovery-base typed length")),
    }
}

/// Canonical immutable header binding, including physical layout/capacity and
/// original bootstrap commit. It does not read or modify either mutable head.
pub fn header_digest(header: Bootstrap, layout: WalLayout) -> Result<[u8; 32]> {
    let bytes = crate::hybrid::journal::wire::encode_layout_header(header, layout)?;
    Ok(bytes[bytes.len() - 32..].try_into().unwrap())
}
