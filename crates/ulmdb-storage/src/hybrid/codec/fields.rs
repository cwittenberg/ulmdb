use crate::hybrid::redo::{Authority, Extent, SourceReceipt, WalReceipt};
use crate::{Error, Reference, Result};
pub(super) struct Cursor<'a> {
    pub bytes: &'a [u8],
    pub at: usize,
}
impl<'a> Cursor<'a> {
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or(Error::LimitExceeded)?;
        let out = self
            .bytes
            .get(self.at..end)
            .ok_or(Error::Corrupt("short hybrid field"))?;
        self.at = end;
        Ok(out)
    }
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(ulmdb_core::endian::decode_u64_le(
            self.take(8)?.try_into().unwrap(),
        ))
    }
    pub fn u128(&mut self) -> Result<u128> {
        Ok(u128::from_le_bytes(self.take(16)?.try_into().unwrap()))
    }
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    pub fn zero(&mut self, n: usize) -> Result<()> {
        if self.take(n)?.iter().any(|b| *b != 0) {
            return Err(Error::Corrupt("hybrid reserved bytes"));
        }
        Ok(())
    }
    pub fn reference(&mut self) -> Result<Reference> {
        Reference::decode(self.take(16)?)
    }
    pub fn extent(&mut self) -> Result<Extent> {
        let e = Extent {
            at: self.reference()?,
            logical_generation: self.u128()?,
            length: self.u32()?,
        };
        self.zero(4)?;
        Ok(e)
    }
    pub fn receipt(&mut self) -> Result<WalReceipt> {
        Ok(WalReceipt {
            lsn: self.u64()?,
            sequence: self.u64()?,
            offset: self.u64()?,
            length: self.u64()?,
            digest: self.array()?,
        })
    }
    pub fn source(&mut self) -> Result<SourceReceipt> {
        let uuid = self.array()?;
        let epoch = self.u64()?;
        let creation_lsn = self.u64()?;
        let creation_sequence = self.u64()?;
        let creation_commit = self.u128()?;
        let provenance_digest = self.array()?;
        let tag = self.u64()?;
        let generation = self.u128()?;
        let lsn = self.u64()?;
        let sequence = self.u64()?;
        let digest = self.array()?;
        let authority = match tag {
            0 if lsn == 0 => Authority::Checkpoint {
                generation,
                sequence,
                digest,
            },
            1 if generation == 0 => Authority::Wal {
                lsn,
                sequence,
                digest,
            },
            _ => return Err(Error::Corrupt("hybrid authority tag/padding")),
        };
        Ok(SourceReceipt {
            uuid,
            epoch,
            creation_lsn,
            creation_sequence,
            creation_commit,
            provenance_digest,
            authority,
        })
    }
}
pub(super) fn u32v(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
pub(super) fn u64v(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&ulmdb_core::endian::encode_u64_le(v));
}
pub(super) fn u128v(out: &mut Vec<u8>, v: u128) {
    out.extend_from_slice(&v.to_le_bytes());
}
pub(super) fn extent(out: &mut Vec<u8>, e: Extent) {
    out.extend_from_slice(&e.at.encode());
    u128v(out, e.logical_generation);
    u32v(out, e.length);
    u32v(out, 0);
}
pub(super) fn receipt(out: &mut Vec<u8>, r: WalReceipt) {
    u64v(out, r.lsn);
    u64v(out, r.sequence);
    u64v(out, r.offset);
    u64v(out, r.length);
    out.extend_from_slice(&r.digest);
}
pub(super) fn source(out: &mut Vec<u8>, s: SourceReceipt) {
    out.extend_from_slice(&s.uuid);
    u64v(out, s.epoch);
    u64v(out, s.creation_lsn);
    u64v(out, s.creation_sequence);
    u128v(out, s.creation_commit);
    out.extend_from_slice(&s.provenance_digest);
    match s.authority {
        Authority::Checkpoint {
            generation,
            sequence,
            digest,
        } => {
            u64v(out, 0);
            u128v(out, generation);
            u64v(out, 0);
            u64v(out, sequence);
            out.extend_from_slice(&digest);
        }
        Authority::Wal {
            lsn,
            sequence,
            digest,
        } => {
            u64v(out, 1);
            u128v(out, 0);
            u64v(out, lsn);
            u64v(out, sequence);
            out.extend_from_slice(&digest);
        }
    }
}
