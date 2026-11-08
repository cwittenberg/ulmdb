//! Versioned resident-slot literal records. This is a distinct format: old
//! append-only records, span bounds and replay validators remain unchanged.
mod state;
mod wire;
use super::VersionKey;
use crate::{Error, Reference, Result, hybrid::redo::WalReceipt};
use sha2::{Digest, Sha256};
pub use state::{ReplayState, SlotState, Transition};
pub use wire::{decode, decode_limited, encode, encode_limited, encode_owned, frame_len};

pub const PAGE: usize = 4096;
/// The low28 bits carry `(slot+1)*PAGE+within`; retain a representable
/// one-past page boundary without carrying into the36-bit generation tag.
/// This also keeps writer index+1 values representable in u16.
pub const MAX_SLOTS: usize = (PHYSICAL_MASK / PAGE as u64) as usize - 1;
const _: () = assert!(MAX_SLOTS == 65_534 && MAX_SLOTS < u16::MAX as usize);
pub const MAX_RANGES: usize = 256;
pub const MAX_GENERATION: u64 = (1u64 << 36) - 1;
pub const PHYSICAL_MASK: u64 = (1u64 << 28) - 1;
pub const DATA_START: u64 = 196_608;
pub const HEADER: usize = 256;
pub const DIGEST: usize = 32;
pub const FOOTER: usize = DIGEST;
pub const MAX_RECORD_BYTES: usize = 32 * 1024 * 1024;

pub fn genesis(boot: &Boot) -> Result<WalReceipt> {
    boot.genesis()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Boot {
    pub uuid: [u8; 16],
    pub epoch: u64,
    pub commit: u128,
    pub kv_pages: u32,
    pub tiny_pages: u32,
}
impl Boot {
    pub fn slots(self) -> Result<usize> {
        let slots = self
            .kv_pages
            .checked_add(self.tiny_pages)
            .filter(|&n| n > 0 && n <= MAX_SLOTS as u32)
            .ok_or(Error::Corrupt("resident boot slot count"))?;
        if self.uuid == [0; 16] || self.epoch == 0 || self.commit == 0 || self.kv_pages == 0 {
            return Err(Error::Corrupt("resident boot identity"));
        }
        Ok(slots as usize)
    }
    pub fn genesis(self) -> Result<WalReceipt> {
        self.slots()?;
        let mut hash = Sha256::new();
        hash.update(b"ULMDB-RESIDENT-GENESIS-V1\0");
        hash.update(self.uuid);
        hash.update(self.epoch.to_le_bytes());
        hash.update(self.commit.to_le_bytes());
        hash.update(self.kv_pages.to_le_bytes());
        hash.update(self.tiny_pages.to_le_bytes());
        Ok(WalReceipt {
            lsn: 0,
            sequence: 0,
            offset: DATA_START,
            length: 0,
            digest: hash.finalize().into(),
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Write,
    Collect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiteralSlot {
    pub key: VersionKey,
    pub expected_previous_generation: u64,
    pub ranges: Vec<(u16, u16)>,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    pub uuid: [u8; 16],
    pub epoch: u64,
    pub lsn: u64,
    pub sequence: u64,
    pub commit: u128,
    pub parent_commit: u128,
    pub parent: WalReceipt,
    pub root: Reference,
    pub next_database: u64,
    pub kind: Kind,
    pub allocations: Vec<LiteralSlot>,
    pub retired: Vec<VersionKey>,
}
pub struct Encoded {
    pub bytes: Vec<u8>,
    pub digest: [u8; 32],
}
/// Canonical codec result. Read-only access preserves correspondence between the
/// record and its receipt without rehashing or cloning literal pages at prepare.
/// This is not a graph certificate or filesystem publication authority.
pub struct Decoded {
    record: Record,
    receipt: WalReceipt,
}
impl Decoded {
    pub fn record(&self) -> &Record {
        &self.record
    }
    pub fn receipt(&self) -> WalReceipt {
        self.receipt
    }
    pub fn into_parts(self) -> (Record, WalReceipt) {
        (self.record, self.receipt)
    }
}

pub fn catalog_key(boot: Boot, root: Reference) -> Result<Option<VersionKey>> {
    boot.slots()?;
    if root.is_null() {
        return Ok(None);
    }
    let generation = root.offset >> 28;
    let physical = root.offset & PHYSICAL_MASK;
    if root.segment != 1
        || generation == 0
        || physical < PAGE as u64
        || !physical.is_multiple_of(PAGE as u64)
    {
        return Err(Error::Corrupt("resident catalog reference"));
    }
    let slot = physical / PAGE as u64 - 1;
    if slot >= u64::from(boot.kv_pages) {
        return Err(Error::Corrupt("resident catalog is not a KV page"));
    }
    Ok(Some(VersionKey {
        slot: slot as u32,
        generation,
    }))
}
pub(super) fn validate_ranges(boot: Boot, slot: u32, ranges: &[(u16, u16)]) -> Result<()> {
    if ranges.is_empty() || ranges.len() > MAX_RANGES {
        return Err(Error::Corrupt("resident literal range count"));
    }
    if slot < boot.kv_pages && ranges != [(0, PAGE as u16)] {
        return Err(Error::Corrupt("resident KV page ranges"));
    }
    let mut end = 0usize;
    for &(start, length) in ranges {
        let start = usize::from(start);
        let length = usize::from(length);
        let next = start
            .checked_add(length)
            .ok_or(Error::Corrupt("resident range overflow"))?;
        if length == 0 || start < end || next > PAGE {
            return Err(Error::Corrupt("resident literal range bounds/order"));
        }
        end = next;
    }
    Ok(())
}
impl Record {
    pub fn validate(&self, boot: Boot) -> Result<()> {
        let count = boot.slots()?;
        let sequence = match self.kind {
            Kind::Write => self
                .parent
                .sequence
                .checked_add(1)
                .ok_or(Error::LimitExceeded)?,
            Kind::Collect => self.parent.sequence,
        };
        if self.uuid != boot.uuid
            || self.epoch != boot.epoch
            || self.lsn != self.parent.lsn.checked_add(1).ok_or(Error::LimitExceeded)?
            || self.sequence != sequence
            || self.commit
                != boot
                    .commit
                    .checked_add(u128::from(sequence))
                    .ok_or(Error::LimitExceeded)?
            || self.parent_commit
                != boot
                    .commit
                    .checked_add(u128::from(self.parent.sequence))
                    .ok_or(Error::LimitExceeded)?
            || self.next_database == 0
            || self.allocations.len() > count
            || self.retired.len() > count
        {
            return Err(Error::Corrupt("resident record identity/counters"));
        }
        catalog_key(boot, self.root)?;
        if self.parent.lsn == 0 {
            if self.parent != boot.genesis()? {
                return Err(Error::Corrupt("resident genesis parent"));
            }
        } else if self.parent.sequence > self.parent.lsn
            || self.parent.offset < DATA_START
            || !self.parent.offset.is_multiple_of(8)
            || (self.parent.length < (HEADER + DIGEST) as u64
                && !(self.parent.length == 0
                    && self.parent.offset == DATA_START
                    && self.parent.digest != [0; 32]))
            || self.parent.length > MAX_RECORD_BYTES as u64
            || !self.parent.length.is_multiple_of(8)
        {
            return Err(Error::Corrupt("resident parent receipt geometry"));
        }
        self.parent.end()?;
        if self.kind == Kind::Collect && !self.allocations.is_empty() {
            return Err(Error::Corrupt("resident collect allocates"));
        }
        let mut last = None;
        for a in &self.allocations {
            if a.key.slot as usize >= count
                || last.is_some_and(|n| n >= a.key.slot)
                || a.key.generation == 0
                || a.key.generation > MAX_GENERATION
                || a.expected_previous_generation >= a.key.generation
                || (a.key.slot >= boot.kv_pages && a.expected_previous_generation != 0)
                || a.bytes.len() != PAGE
            {
                return Err(Error::Corrupt("resident allocation order/generation"));
            }
            validate_ranges(boot, a.key.slot, &a.ranges)?;
            last = Some(a.key.slot);
        }
        last = None;
        for key in &self.retired {
            if key.slot as usize >= count
                || last.is_some_and(|n| n >= key.slot)
                || key.generation == 0
                || key.generation > MAX_GENERATION
            {
                return Err(Error::Corrupt("resident retired order/generation"));
            }
            last = Some(key.slot);
            if self
                .allocations
                .binary_search_by_key(&key.slot, |a| a.key.slot)
                .is_ok()
            {
                return Err(Error::Corrupt("resident same-record retirement/reuse"));
            }
        }
        Ok(())
    }
    pub fn encoded_length(&self, boot: Boot) -> Result<usize> {
        self.validate(boot)?;
        let mut length = HEADER + DIGEST;
        for a in &self.allocations {
            let n = 32 + a.ranges.len() * 4 + PAGE;
            length = length
                .checked_add(n + (8 - n % 8) % 8)
                .ok_or(Error::LimitExceeded)?;
        }
        length
            .checked_add(self.retired.len() * 16)
            .filter(|&n| n <= MAX_RECORD_BYTES)
            .ok_or(Error::LimitExceeded)
    }
}

#[cfg(test)]
mod tests;
