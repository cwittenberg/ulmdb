//! Read-only recovery-base parser. Authorized encoding/publication is private
//! to the facade PreparedCheckpoint path; this module cannot install authority.
use crate::{Error, Result};
use crate::{
    Reference,
    hybrid::{
        checkpoint::SnapshotSelection,
        redo::{RootState, WalReceipt},
    },
};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::{
    fs::OpenOptions,
    io::Read,
    path::{Path, PathBuf},
};
const NAME: &str = "HYBRID.base";
const DOMAIN: &[u8] = b"ULMDB/hybrid/recovery-base/v1\0";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Selector {
    pub uuid: [u8; 16],
    pub epoch: u64,
    pub generation: u128,
    pub cut: SnapshotSelection,
    pub digest: [u8; 32],
}
impl Selector {
    pub fn directory(&self, engine: &Path) -> PathBuf {
        engine
            .join("HYBRID.checkpoints")
            .join(format!("{:032x}", self.generation))
    }
    fn encode(self) -> [u8; 256] {
        let mut b = [0; 256];
        b[..8].copy_from_slice(b"ULMDBHB1");
        b[8..12].copy_from_slice(&1u32.to_le_bytes());
        b[12..16].copy_from_slice(&256u32.to_le_bytes());
        b[16..32].copy_from_slice(&self.uuid);
        p64(&mut b, 32, self.epoch);
        b[40..56].copy_from_slice(&self.generation.to_le_bytes());
        p64(&mut b, 56, self.cut.receipt.lsn);
        p64(&mut b, 64, self.cut.receipt.sequence);
        p64(&mut b, 72, self.cut.receipt.offset);
        p64(&mut b, 80, self.cut.receipt.length);
        b[88..120].copy_from_slice(&self.cut.receipt.digest);
        b[120..136].copy_from_slice(&self.cut.commit.to_le_bytes());
        b[136..152].copy_from_slice(&self.cut.root.catalog.encode());
        p64(&mut b, 152, self.cut.root.next_chunk);
        p64(&mut b, 160, self.cut.root.next_database);
        b[168..200].copy_from_slice(&self.digest);
        let mut h = Sha256::new();
        h.update(DOMAIN);
        h.update(&b[..224]);
        b[224..].copy_from_slice(&h.finalize());
        b
    }
    fn decode(b: &[u8; 256]) -> Result<Self> {
        let s = Self {
            uuid: b[16..32].try_into().unwrap(),
            epoch: g64(b, 32),
            generation: u128::from_le_bytes(b[40..56].try_into().unwrap()),
            cut: SnapshotSelection {
                receipt: WalReceipt {
                    lsn: g64(b, 56),
                    sequence: g64(b, 64),
                    offset: g64(b, 72),
                    length: g64(b, 80),
                    digest: b[88..120].try_into().unwrap(),
                },
                commit: u128::from_le_bytes(b[120..136].try_into().unwrap()),
                root: RootState {
                    catalog: Reference::decode(&b[136..152])?,
                    next_chunk: g64(b, 152),
                    next_database: g64(b, 160),
                },
            },
            digest: b[168..200].try_into().unwrap(),
        };
        let r = s.cut.receipt;
        if r.lsn == 0 {
            if s.cut.root != RootState::empty()
                || r != crate::hybrid::journal::genesis(crate::hybrid::journal::Bootstrap {
                    uuid: s.uuid,
                    epoch: s.epoch,
                    commit: s.cut.commit,
                })?
                .receipt
            {
                return Err(Error::Corrupt("hybrid recovery-base genesis"));
            }
        } else if r.offset < 196608
            || !r.offset.is_multiple_of(8)
            || r.length < 320
            || !r.length.is_multiple_of(8)
            || r.end()? > 268435456
        {
            return Err(Error::Corrupt("hybrid recovery-base WAL extent"));
        }
        if !s.cut.root.catalog.is_null()
            && (s.cut.root.catalog.segment >= s.cut.root.next_chunk
                || s.cut.root.catalog.offset < 4096
                || s.cut.root.catalog.offset >= 268435456)
        {
            return Err(Error::Corrupt("hybrid recovery-base root bounds"));
        }
        if s.uuid == [0; 16]
            || s.epoch == 0
            || s.generation == 0
            || s.cut.commit == 0
            || s.cut.root.next_chunk == 0
            || s.cut.root.next_database == 0
            || s.cut.receipt.lsn != s.cut.receipt.sequence
            || s.encode() != *b
        {
            return Err(Error::Corrupt(
                "hybrid recovery-base selector identity/version/hash",
            ));
        }
        Ok(s)
    }
}
fn p64(b: &mut [u8], at: usize, n: u64) {
    b[at..at + 8].copy_from_slice(&n.to_le_bytes());
}
fn g64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
/// Read-only decoder. No authorized publication/encoding API is exposed.
pub fn decode_bytes(bytes: &[u8; 256]) -> Result<Selector> {
    Selector::decode(bytes)
}
pub fn read(engine: &Path) -> Result<Option<Selector>> {
    let mut f = match OpenOptions::new()
        .read(true)
        .custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS)
        .open(engine.join(NAME))
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !f.metadata()?.is_file() || f.metadata()?.len() != 256 {
        return Err(Error::Corrupt("hybrid recovery-base selector length/type"));
    }
    let mut b = [0; 256];
    f.read_exact(&mut b)?;
    Ok(Some(Selector::decode(&b)?))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Selector {
        let h = crate::hybrid::journal::Bootstrap {
            uuid: [7; 16],
            epoch: 1,
            commit: 9,
        };
        Selector {
            uuid: h.uuid,
            epoch: h.epoch,
            generation: 3,
            cut: SnapshotSelection {
                receipt: crate::hybrid::journal::genesis(h).unwrap().receipt,
                commit: h.commit,
                root: RootState::empty(),
            },
            digest: [8; 32],
        }
    }
    #[test]
    fn selector_binds_every_bit_and_rejects_rehashed_extent_reserved_and_version_damage() {
        let selector = sample();
        let original = selector.encode();
        assert_eq!(Selector::decode(&original).unwrap(), selector);
        for offset in 0..256 {
            for bit in 0..8 {
                let mut bad = original;
                bad[offset] ^= 1 << bit;
                assert!(Selector::decode(&bad).is_err());
            }
        }
        for offset in [8, 12, 200, 223] {
            let mut bad = original;
            bad[offset] ^= 1;
            let mut h = Sha256::new();
            h.update(DOMAIN);
            h.update(&bad[..224]);
            bad[224..].copy_from_slice(&h.finalize());
            assert!(Selector::decode(&bad).is_err());
        }
        let mut bad = selector;
        bad.cut.receipt.lsn = 1;
        bad.cut.receipt.sequence = 1;
        bad.cut.receipt.offset = u64::MAX - 7;
        bad.cut.receipt.length = 320;
        assert!(Selector::decode(&bad.encode()).is_err());
    }
}
