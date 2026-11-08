//! Bounded streaming recovery; no full-history object/receipt dictionary.
use super::{
    tail,
    wire::{self, Witness},
};
use crate::resident::wal_codec_v2::{self, Boot, Decoded, ReplayState};
use crate::transform::ResidentFile;
use crate::{Error, Result};
pub(super) fn read_record(
    file: &ResidentFile,
    boot: Boot,
    offset: u64,
    limit: u64,
) -> Result<Decoded> {
    let mut header = [0; wal_codec_v2::HEADER];
    if offset
        .checked_add(header.len() as u64)
        .is_none_or(|n| n > limit)
    {
        return Err(Error::Corrupt("resident truncated record header"));
    }
    file.read_exact_at(&mut header, offset)?;
    let length = wal_codec_v2::frame_len(&header)?;
    if offset.checked_add(length as u64).is_none_or(|n| n > limit) {
        return Err(Error::Corrupt("resident record crosses selected boundary"));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| Error::LimitExceeded)?;
    bytes.resize(length, 0);
    file.read_exact_at(&mut bytes, offset)?;
    wal_codec_v2::decode(boot, &bytes, offset)
}
pub struct Recovery<'a> {
    file: &'a ResidentFile,
    boot: Boot,
    capacity: u64,
    pub(super) state: ReplayState,
    target: Witness,
    older: Witness,
    offset: u64,
    older_seen: bool,
    checkpoint: Option<crate::resident::checkpoint::Reader>,
    checkpoint_restored: bool,
    pub(super) complete: bool,
}
impl<'a> Recovery<'a> {
    pub(super) fn new(
        file: &'a ResidentFile,
        boot: Boot,
        target: Witness,
        older: Witness,
        capacity: u64,
        seed: Option<crate::resident::checkpoint::Seed>,
        parent: &std::path::Path,
    ) -> Result<Self> {
        let checkpoint = seed
            .map(|s| {
                crate::resident::checkpoint::Reader::open(parent, boot, s, file.context().cloned())
            })
            .transpose()?;
        let state = match checkpoint.as_ref() {
            Some(c) => ReplayState::from_checkpoint(boot, c.seed(), c.slots.clone())?,
            None => ReplayState::new(boot)?,
        };
        let older_seen = older.receipt.lsn == state.selected().lsn;
        if older.receipt.lsn < state.selected().lsn || target.receipt.lsn < state.selected().lsn {
            return Err(Error::Corrupt("resident witness predates checkpoint"));
        }
        if older_seen && (older.receipt != state.selected() || older.commit != state.commit()) {
            return Err(Error::Corrupt("resident older genesis identity"));
        }
        Ok(Self {
            file,
            boot,
            capacity,
            state,
            target,
            older,
            offset: wire::DATA_START,
            older_seen,
            checkpoint,
            checkpoint_restored: false,
            complete: false,
        })
    }
    pub fn transform_context(&self) -> Option<&std::sync::Arc<crate::transform::TransformContext>> {
        self.file.context()
    }
    pub fn capacity(&self) -> u64 {
        self.capacity
    }
    pub fn boot(&self) -> Boot {
        self.boot
    }
    pub fn selected(&self) -> crate::hybrid::redo::WalReceipt {
        self.target.receipt
    }
    /// Materialize the authenticated checkpoint with fresh physical owners.
    /// Must run before any tail replay or recovery side effect.
    pub fn restore_checkpoint(
        &mut self,
        pool: &std::sync::Arc<crate::resident::Pool>,
    ) -> Result<crate::resident::View> {
        if self.checkpoint_restored {
            return Err(Error::InvalidInput("resident checkpoint restored twice"));
        }
        let view = match self.checkpoint.as_mut() {
            Some(c) => c.restore(pool)?,
            None => pool.empty()?,
        };
        self.checkpoint_restored = true;
        Ok(view)
    }
    pub fn next_record(&mut self) -> Result<Option<Decoded>> {
        if self.checkpoint.is_some() && !self.checkpoint_restored {
            return Err(Error::InvalidInput("resident checkpoint not restored"));
        }
        let end = self.target.receipt.end()?;
        if self.offset == end {
            if self.state.selected() != self.target.receipt
                || self.state.commit() != self.target.commit
                || !self.older_seen
            {
                return Err(Error::Corrupt(
                    "resident selected/older witness not in validated prefix",
                ));
            }
            self.complete = true;
            return Ok(None);
        }
        let record = read_record(self.file, self.boot, self.offset, end)?;
        self.state.apply(&record)?;
        if record.receipt().lsn == self.older.receipt.lsn {
            if record.receipt() != self.older.receipt || record.record().commit != self.older.commit
            {
                return Err(Error::Corrupt("resident older witness frame mismatch"));
            }
            self.older_seen = true;
        }
        self.offset = record.receipt().end()?;
        Ok(Some(record))
    }
    pub fn state(&self) -> &ReplayState {
        &self.state
    }
}
pub(super) struct Selection {
    pub boot: Boot,
    pub capacity: u64,
    pub selected: Witness,
    pub older: Witness,
    pub slot: usize,
    pub heal: bool,
    pub seed: Option<crate::resident::checkpoint::Seed>,
}
pub(super) fn select(file: &ResidentFile) -> Result<Selection> {
    select_limited(file, u64::MAX)
}
pub(super) fn select_limited(file: &ResidentFile, maximum_end: u64) -> Result<Selection> {
    let mut header = [0; wire::BLOCK];
    file.read_exact_at(&mut header, 0)?;
    let (boot, capacity) = wire::decode_header(&header)?;
    let seed = wire::seed(&header);
    if file.metadata()?.len() != capacity {
        return Err(Error::Corrupt("resident WAL capacity differs from header"));
    }
    for (start, end) in [
        (wire::BLOCK as u64, wire::HEADS[0]),
        (wire::HEADS[0] + wire::BLOCK as u64, wire::HEADS[1]),
        (wire::HEADS[1] + wire::BLOCK as u64, wire::DATA_START),
    ] {
        tail::verify_zero(file, start, end)?;
    }
    let mut bytes = [[0; wire::BLOCK]; 2];
    for (b, at) in bytes.iter_mut().zip(wire::HEADS) {
        file.read_exact_at(b, at)?;
    }
    let heads = bytes.map(|b| wire::decode_head(&b, boot, capacity));
    match heads {
        [Ok(a), Ok(b)] => {
            if a.receipt.lsn == b.receipt.lsn {
                if (a.receipt.lsn != 0 && seed.map(|s| s.receipt()) != Some(a.receipt)) || a != b {
                    return Err(Error::Corrupt("resident invalid equal witness ordinals"));
                }
                return Ok(Selection {
                    boot,
                    capacity,
                    selected: a,
                    older: b,
                    slot: 0,
                    heal: false,
                    seed,
                });
            }
            let (selected, older, slot) = if a.receipt.lsn > b.receipt.lsn {
                (a, b, 0)
            } else {
                (b, a, 1)
            };
            if older.receipt.lsn.checked_add(1) != Some(selected.receipt.lsn) {
                return Err(Error::Corrupt("resident witness ordinals are not adjacent"));
            }
            Ok(Selection {
                boot,
                capacity,
                selected,
                older,
                slot,
                heal: false,
                seed,
            })
        }
        [Ok(anchor), Err(_)] | [Err(_), Ok(anchor)] => {
            let slot = if wire::decode_head(&bytes[0], boot, capacity).is_err() {
                0
            } else {
                1
            };
            let start = anchor.receipt.end()?;
            if maximum_end < capacity {
                if start.checked_add(256).is_none_or(|n| n > maximum_end) {
                    return Err(Error::LimitExceeded);
                }
                let mut header = [0; 256];
                file.read_exact_at(&mut header, start)?;
                let bytes = wal_codec_v2::frame_len(&header)? as u64;
                if start.checked_add(bytes).is_none_or(|n| n > maximum_end) {
                    return Err(Error::LimitExceeded);
                }
            }
            let candidate = read_record(file, boot, start, capacity)?;
            if candidate.record().parent != anchor.receipt
                || candidate.record().parent_commit != anchor.commit
                || anchor.receipt.lsn.checked_add(1) != Some(candidate.record().lsn)
            {
                return Err(Error::Corrupt(
                    "resident invalid witness has no exact successor",
                ));
            }
            tail::verify_zero(file, candidate.receipt().end()?, capacity)?;
            Ok(Selection {
                boot,
                capacity,
                selected: Witness {
                    receipt: candidate.receipt(),
                    commit: candidate.record().commit,
                },
                older: anchor,
                slot,
                heal: true,
                seed,
            })
        }
        _ => Err(Error::Corrupt("resident neither witness is valid")),
    }
}
