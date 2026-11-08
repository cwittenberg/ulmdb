//! RL2 outer frames: exact parent, scalar state and compact recipes share one hash.
//! RL1 formats, validators and genesis remain unchanged.
mod borrowed;
mod state;
mod view;
pub(crate) use borrowed::encode_into as encode_borrowed_into;
pub use borrowed::{BorrowedRecord, RecipeBuilder, RecipeWorkspace};
mod wire;
pub use super::VersionKey;
pub use super::versioned_redo::{Allocation, Body};
pub use super::wal_codec::{
    Boot, DATA_START, DIGEST, FOOTER, HEADER, Kind, MAX_GENERATION, MAX_RANGES, MAX_RECORD_BYTES,
    MAX_SLOTS, PAGE, PHYSICAL_MASK, SlotState,
};
use crate::{Error, Reference, Result, hybrid::redo::WalReceipt};
use sha2::{Digest, Sha256};
pub use state::{ReplayState, Transition};
pub use wire::{
    decode, decode_limited, encode, encode_limited, encode_owned, encode_owned_limited, frame_len,
};

pub fn genesis(boot: &Boot) -> Result<WalReceipt> {
    boot.slots()?;
    let mut h = Sha256::new();
    h.update(b"ULMDB-RESIDENT-GENESIS-V2\0");
    h.update(boot.uuid);
    h.update(boot.epoch.to_le_bytes());
    h.update(boot.commit.to_le_bytes());
    h.update(boot.kv_pages.to_le_bytes());
    h.update(boot.tiny_pages.to_le_bytes());
    Ok(WalReceipt {
        lsn: 0,
        sequence: 0,
        offset: DATA_START,
        length: 0,
        digest: h.finalize().into(),
    })
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
    pub allocations: Vec<Allocation>,
    pub retired: Vec<VersionKey>,
}
pub struct Encoded {
    pub bytes: Vec<u8>,
    pub digest: [u8; 32],
}
/// Immutable receipt/recipe correspondence issued only by this canonical codec.
/// Physical source ownership and graph retirement remain the Env's obligations.
/// Every constructor validates the complete record for the retained exact Boot;
/// immutable accessors preserve that validation for state-transition preparation.
pub struct Decoded {
    boot: Boot,
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
    super::wal_codec::catalog_key(boot, root)
}
fn aligned(n: usize) -> Result<usize> {
    n.checked_add(7).map(|n| n & !7).ok_or(Error::LimitExceeded)
}
impl Record {
    pub fn validate(&self, boot: Boot) -> Result<()> {
        view::validate(self, boot)
    }
    pub fn encoded_length(&self, boot: Boot) -> Result<usize> {
        view::encoded_length(self, boot)
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod borrowed_tests;
