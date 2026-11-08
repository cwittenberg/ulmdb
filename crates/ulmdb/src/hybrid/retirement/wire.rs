//! Private compound producer: checked bytes, no public installation adapter.
use super::RetirementReport;
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use ulmdb_storage::hybrid::{
    checkpoint::selection::Selector,
    journal::Witness,
    retirement::{
        Boundary,
        selection::{Compound, decode_compound},
    },
};
const DOMAIN: &[u8] = b"ULMDB/hybrid/compound-floor/v2\0";
pub(in crate::hybrid) struct Draft {
    bytes: [u8; 512],
    context: Compound,
}
fn p64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}
fn witness(b: &mut [u8], at: usize, w: Witness) {
    p64(b, at, w.receipt.lsn);
    p64(b, at + 8, w.receipt.sequence);
    p64(b, at + 16, w.receipt.offset);
    p64(b, at + 24, w.receipt.length);
    b[at + 32..at + 64].copy_from_slice(&w.receipt.digest);
    b[at + 64..at + 80].copy_from_slice(&w.commit.to_le_bytes());
}
fn hash(b: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(b);
    h.finalize().into()
}
impl Draft {
    pub fn new(
        s: Selector,
        b: Boundary,
        r: RetirementReport,
        header: [u8; 32],
        prior: Option<Compound>,
    ) -> Result<Self> {
        Self::build(s, b, r, b.predecessor, header, prior)
    }
    pub fn carry(s: Selector, b: Boundary, prior: Compound) -> Result<Self> {
        let r = RetirementReport {
            checkpoint_sequence: s.cut.receipt.sequence,
            validated_through: s.cut.receipt.sequence,
            first_required_lsn: prior.first_required().receipt.lsn,
            first_required_offset: prior.first_required().receipt.offset,
            proposed_prefix_end: prior.prefix_end(),
            declared_failure_granule: prior.granule(),
            alias_digest: s.digest,
        };
        Self::build(
            s,
            b,
            r,
            prior.first_required(),
            prior.header_digest(),
            Some(prior),
        )
    }
    fn build(
        s: Selector,
        b: Boundary,
        r: RetirementReport,
        first: Witness,
        header: [u8; 32],
        prior: Option<Compound>,
    ) -> Result<Self> {
        if b.base.receipt != s.cut.receipt
            || b.base.commit != s.cut.commit
            || b.root != s.cut.root
            || b.first_suffix_lsn
                != s.cut
                    .receipt
                    .lsn
                    .checked_add(1)
                    .ok_or(Error::LimitExceeded)?
            || r.checkpoint_sequence != s.cut.receipt.sequence
            || r.validated_through < r.checkpoint_sequence
            || r.alias_digest != s.digest
            || r.first_required_lsn != first.receipt.lsn
            || r.first_required_offset != first.receipt.offset
        {
            return Err(Error::Corrupt("compound producer boundary/report binding"));
        }
        let serial = if let Some(old) = prior {
            let previous = old.selector();
            if s.uuid != previous.uuid
                || s.epoch != previous.epoch
                || header != old.header_digest()
                || s.cut.receipt.lsn < previous.cut.receipt.lsn
                || s.cut.receipt.sequence < previous.cut.receipt.sequence
                || (s.cut.receipt.lsn == previous.cut.receipt.lsn && s != previous)
                || first.receipt.lsn < old.first_required().receipt.lsn
                || first.receipt.offset < old.first_required().receipt.offset
                || r.proposed_prefix_end < old.prefix_end()
                || r.declared_failure_granule != old.granule()
                || s.cut.root.next_chunk < previous.cut.root.next_chunk
                || s.cut.root.next_database < previous.cut.root.next_database
            {
                return Err(Error::InvalidInput(
                    "compound producer regresses authority/floor",
                ));
            }
            old.serial().checked_add(1).ok_or(Error::LimitExceeded)?
        } else {
            1
        };
        let mut bytes = [0u8; 512];
        bytes[..8].copy_from_slice(b"ULMDBHB2");
        bytes[8..12].copy_from_slice(&2u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&512u32.to_le_bytes());
        bytes[16..32].copy_from_slice(&s.uuid);
        p64(&mut bytes, 32, s.epoch);
        bytes[40..56].copy_from_slice(&s.generation.to_le_bytes());
        witness(&mut bytes, 56, b.base);
        bytes[136..152].copy_from_slice(&s.cut.root.catalog.encode());
        p64(&mut bytes, 152, s.cut.root.next_chunk);
        p64(&mut bytes, 160, s.cut.root.next_database);
        bytes[168..200].copy_from_slice(&s.digest);
        if let Some(old) = prior {
            bytes[200..232].copy_from_slice(&old.digest());
        }
        p64(&mut bytes, 232, first.receipt.lsn);
        p64(&mut bytes, 240, first.receipt.offset);
        p64(&mut bytes, 248, r.proposed_prefix_end);
        p64(&mut bytes, 256, r.declared_failure_granule);
        witness(&mut bytes, 264, b.predecessor);
        witness(&mut bytes, 344, first);
        p64(&mut bytes, 424, serial);
        bytes[432..464].copy_from_slice(&header);
        let digest = hash(&bytes[..480]);
        bytes[480..].copy_from_slice(&digest);
        let context = decode_compound(&bytes)?;
        Ok(Self { bytes, context })
    }
    pub fn digest(&self) -> [u8; 32] {
        self.context.digest()
    }
    pub fn context(&self) -> Compound {
        self.context
    }
    pub fn bytes(&self) -> &[u8; 512] {
        &self.bytes
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use ulmdb_storage::hybrid::{
        checkpoint::SnapshotSelection,
        redo::{RootState, WalReceipt},
    };
    fn fixture(lsn: u64) -> (Selector, Boundary, RetirementReport) {
        let offset = 196608 + lsn * 65536;
        let p = Witness {
            receipt: WalReceipt {
                lsn: lsn - 1,
                sequence: lsn - 1,
                offset: offset - 65536,
                length: 65536,
                digest: [1; 32],
            },
            commit: 1,
        };
        let c = Witness {
            receipt: WalReceipt {
                lsn,
                sequence: lsn,
                offset,
                length: 65536,
                digest: [2; 32],
            },
            commit: 2,
        };
        let root = RootState::empty();
        let s = Selector {
            uuid: [1; 16],
            epoch: 1,
            generation: lsn as u128,
            cut: SnapshotSelection {
                receipt: c.receipt,
                commit: c.commit,
                root,
            },
            digest: [3; 32],
        };
        let b = Boundary {
            base: c,
            root,
            predecessor: p,
            first_suffix_lsn: lsn + 1,
        };
        let r = RetirementReport {
            checkpoint_sequence: lsn,
            validated_through: lsn,
            first_required_lsn: lsn - 1,
            first_required_offset: p.receipt.offset,
            proposed_prefix_end: p.receipt.offset,
            declared_failure_granule: 65536,
            alias_digest: s.digest,
        };
        (s, b, r)
    }
    #[test]
    fn compound_draft_carries_forward_monotone_context_without_publication() {
        let (s, b, r) = fixture(3);
        let old = Draft::new(s, b, r, [7; 32], None).unwrap();
        let (s, b, r) = fixture(5);
        let new = Draft::new(s, b, r, [7; 32], Some(old.context())).unwrap();
        assert_eq!(new.bytes[200..232], old.digest());
        assert_eq!(new.context.serial(), 2);
        let (s, b, r) = fixture(3);
        assert!(Draft::new(s, b, r, [7; 32], Some(new.context())).is_err());
        let (mut s, b, r) = fixture(6);
        s.epoch += 1;
        assert!(Draft::new(s, b, r, [7; 32], Some(new.context())).is_err());
        let (s, b, mut r) = fixture(6);
        r.proposed_prefix_end = 196608;
        assert!(Draft::new(s, b, r, [7; 32], Some(new.context())).is_err());
    }
    #[test]
    fn every_byte_is_integrity_bound_and_rehashed_reserved_data_rejected() {
        let (s, b, r) = fixture(3);
        let original = Draft::new(s, b, r, [7; 32], None).unwrap();
        for i in 0..512 {
            let mut bytes = original.bytes;
            bytes[i] ^= 1;
            assert!(decode_compound(&bytes).is_err(), "byte {i}");
        }
        let mut bytes = original.bytes;
        bytes[464] = 1;
        let digest = hash(&bytes[..480]);
        bytes[480..].copy_from_slice(&digest);
        assert!(decode_compound(&bytes).is_err());
    }
}
