//! Read-only authoritative-cut validation. Image validation precedes repair.
use ulmdb_platform::FileExt;
use super::{
    codec::{self, Decoded},
    journal::wire::{
        self, BLOCK, Bootstrap, DATA_START, MAX_WAL_BYTES, WITNESS_OFFSETS, WalLayout, Witness,
    },
    redo::{Authority, ObjectRedo, RootState},
};
use crate::{Error, Result};
use std::{fs::File};

/// Parsed journal only. This does not certify an image, source checkpoint,
/// typed index graph, admission policy, or physical owner incarnation.
pub struct RecoveryPlan {
    pub header: Bootstrap,
    pub layout: WalLayout,
    pub selected: Witness,
    pub slot: usize,
    pub heal: bool,
    pub records: Vec<Decoded>,
}
impl RecoveryPlan {
    pub fn root(&self) -> RootState {
        self.records
            .last()
            .map_or_else(wire::empty_root, |r| r.record.root)
    }
}

/// Verified immutable prefix captured from a live Journal. It is not a repair
/// plan: current disk witness slots may already have advanced beyond this cut.
pub struct CommittedPrefix {
    header: Bootstrap,
    selected: Witness,
    root: RootState,
    layout: WalLayout,
    records: Vec<Decoded>,
}
impl CommittedPrefix {
    pub fn layout(&self) -> WalLayout {
        self.layout
    }
    pub fn header(&self) -> Bootstrap {
        self.header
    }
    pub fn selected(&self) -> Witness {
        self.selected
    }
    pub fn root(&self) -> RootState {
        self.root
    }
    pub fn records(&self) -> &[Decoded] {
        &self.records
    }
    pub fn at(&self, lsn: u64) -> Option<(Witness, RootState)> {
        if lsn == 0 {
            return wire::genesis(self.header)
                .ok()
                .map(|w| (w, wire::empty_root()));
        }
        let d = self.records.get(usize::try_from(lsn - 1).ok()?)?;
        Some((
            Witness {
                receipt: d.receipt,
                commit: d.record.commit,
            },
            d.record.root,
        ))
    }
}
pub(crate) fn inspect_prefix(
    file: &File,
    header: Bootstrap,
    layout: WalLayout,
    selected: Witness,
    expected_root: RootState,
) -> Result<CommittedPrefix> {
    let end = selected.receipt.end()?;
    if !(DATA_START..=MAX_WAL_BYTES).contains(&end) || file.metadata()?.len() < end {
        return Err(Error::Corrupt("hybrid captured prefix length"));
    }
    if let WalLayout::PreSized { capacity } = layout
        && file.metadata()?.len() != capacity
    {
        return Err(Error::Corrupt(
            "hybrid captured pre-sized physical capacity",
        ));
    }
    let mut fixed = vec![0u8; DATA_START as usize];
    file.read_exact_at(&mut fixed, 0)?;
    if wire::decode_layout_header(fixed[..BLOCK].try_into().unwrap())? != (header, layout) {
        return Err(Error::Corrupt("hybrid captured prefix header"));
    }
    for (at, byte) in fixed.iter().enumerate() {
        let mutable = WITNESS_OFFSETS
            .iter()
            .any(|o| (*o as usize..*o as usize + BLOCK).contains(&at));
        if at >= BLOCK && !mutable && *byte != 0 {
            return Err(Error::Corrupt("hybrid captured reserved prefix"));
        }
    }
    let mut records = Vec::new();
    let mut last = wire::genesis(header)?;
    let mut root = wire::empty_root();
    while last.receipt.end()? < end {
        let next = read_record(file, last.receipt.end()?, end)?;
        validate_next(header, &last, root, &records, &next)?;
        last = Witness {
            receipt: next.receipt,
            commit: next.record.commit,
        };
        root = next.record.root;
        records.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
        records.push(next);
    }
    if last != selected || root != expected_root {
        return Err(Error::Corrupt("hybrid captured prefix selected root"));
    }
    Ok(CommittedPrefix {
        header,
        selected,
        root,
        layout,
        records,
    })
}
/// Every acknowledged record and dependency is checked; an invalid chosen
/// record never causes fallback to an older root. The caller must validate the
/// reconstructed image/graph before healing/trimming or exposing readiness.
pub fn inspect(file: &File) -> Result<RecoveryPlan> {
    let eof = file.metadata()?.len();
    if !(DATA_START..=MAX_WAL_BYTES).contains(&eof) {
        return Err(Error::Corrupt("hybrid WAL size"));
    }
    let mut prefix = Vec::new();
    prefix
        .try_reserve_exact(DATA_START as usize)
        .map_err(|_| Error::LimitExceeded)?;
    prefix.resize(DATA_START as usize, 0);
    file.read_exact_at(&mut prefix, 0)?;
    let global = prefix[..BLOCK].try_into().unwrap();
    let (header, layout) = if &prefix[..8] == b"ULMDBHW1" {
        (wire::decode_header(global)?, WalLayout::Dense)
    } else {
        wire::decode_layout_header(global)?
    };
    if let WalLayout::PreSized { capacity } = layout
        && capacity != eof
    {
        return Err(Error::Corrupt(
            "hybrid pre-sized physical capacity mismatch",
        ));
    }
    for (index, byte) in prefix.iter().enumerate() {
        let active = index < BLOCK
            || WITNESS_OFFSETS
                .iter()
                .any(|o| (*o as usize..*o as usize + BLOCK).contains(&index));
        if !active && *byte != 0 {
            return Err(Error::Corrupt("hybrid fixed prefix reserved bytes"));
        }
    }
    let heads = WITNESS_OFFSETS.map(|o| {
        wire::decode_layout_witness(
            header,
            prefix[o as usize..o as usize + BLOCK].try_into().unwrap(),
            layout,
        )
    });
    let genesis = match layout {
        WalLayout::Dense => wire::genesis(header)?,
        _ => wire::genesis_layout(header, layout)?,
    };
    let (selected, slot, heal) = match heads {
        [Ok(a), Ok(b)] => {
            if a.receipt.lsn == b.receipt.lsn {
                if a != genesis || b != genesis {
                    return Err(Error::Corrupt("hybrid equal non-genesis heads"));
                }
                (a, 0, false)
            } else if a.receipt.lsn > b.receipt.lsn {
                adjacent(a, b)?;
                (a, 0, false)
            } else {
                adjacent(b, a)?;
                (b, 1, false)
            }
        }
        [Ok(a), Err(_)] => (a, 1, true),
        [Err(_), Ok(b)] => (b, 0, true),
        _ => return Err(Error::Corrupt("hybrid both selected witnesses invalid")),
    };
    let mut records = Vec::new();
    let mut last = genesis;
    let mut root = wire::empty_root();
    while last.receipt.end()? < selected.receipt.end()? {
        let decoded = read_record(file, last.receipt.end()?, eof)?;
        validate_next(header, &last, root, &records, &decoded)?;
        last = Witness {
            receipt: decoded.receipt,
            commit: decoded.record.commit,
        };
        root = decoded.record.root;
        records.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
        records.push(decoded);
    }
    if last != selected {
        return Err(Error::Corrupt("hybrid selected record mismatch"));
    }
    if !heal {
        let older = WITNESS_OFFSETS[1 - slot] as usize;
        let older = wire::decode_layout_witness(
            header,
            prefix[older..older + BLOCK].try_into().unwrap(),
            layout,
        )?;
        let previous = if records.len() >= 2 {
            let d = &records[records.len() - 2];
            Witness {
                receipt: d.receipt,
                commit: d.record.commit,
            }
        } else {
            genesis
        };
        if older != previous {
            return Err(Error::Corrupt("hybrid previous selected record mismatch"));
        }
    } else {
        // A torn selected witness can only describe ONE exact complete successor.
        // Multiple orphan attempts, missing/truncated tails, or corrupt bodies
        // cannot be interpreted as proof that an old acknowledged root is safe.
        let decoded = read_record(file, last.receipt.end()?, eof)?;
        validate_next(header, &last, root, &records, &decoded)?;
        match layout {
            WalLayout::Dense if decoded.receipt.end()? != eof => {
                return Err(Error::Corrupt("hybrid ambiguous successor tail"));
            }
            WalLayout::PreSized { capacity } => {
                super::journal::tail::verify_zero(file, decoded.receipt.end()?, capacity)?;
            }
            _ => {}
        }
        last = Witness {
            receipt: decoded.receipt,
            commit: decoded.record.commit,
        };
        records.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
        records.push(decoded);
    }
    Ok(RecoveryPlan {
        header,
        layout,
        selected: last,
        slot,
        heal,
        records,
    })
}
fn adjacent(new: Witness, old: Witness) -> Result<()> {
    if new.receipt.lsn != old.receipt.lsn.checked_add(1).ok_or(Error::LimitExceeded)?
        || new.receipt.sequence
            != old
                .receipt
                .sequence
                .checked_add(1)
                .ok_or(Error::LimitExceeded)?
    {
        return Err(Error::Corrupt("hybrid nonadjacent witnesses"));
    }
    Ok(())
}
pub(crate) fn read_record(file: &File, offset: u64, eof: u64) -> Result<Decoded> {
    let end = offset
        .checked_add(codec::HEADER_BYTES as u64)
        .ok_or(Error::LimitExceeded)?;
    if end > eof {
        return Err(Error::Corrupt("hybrid missing successor/selected record"));
    }
    let mut header = [0; codec::HEADER_BYTES];
    file.read_exact_at(&mut header, offset)?;
    let length = u64::from_le_bytes(header[192..200].try_into().unwrap());
    let length_usize = usize::try_from(length).map_err(|_| Error::LimitExceeded)?;
    if !(codec::HEADER_BYTES + codec::FOOTER_BYTES..=codec::MAX_RECORD_BYTES)
        .contains(&length_usize)
        || !length.is_multiple_of(8)
        || offset.checked_add(length).filter(|n| *n <= eof).is_none()
    {
        return Err(Error::Corrupt("hybrid record extent before allocation"));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length_usize)
        .map_err(|_| Error::LimitExceeded)?;
    bytes.resize(length_usize, 0);
    file.read_exact_at(&mut bytes, offset)?;
    codec::decode(&bytes, offset)
}
fn validate_next(
    header: Bootstrap,
    parent: &Witness,
    root: RootState,
    previous: &[Decoded],
    next: &Decoded,
) -> Result<()> {
    let r = &next.record;
    if r.uuid != header.uuid
        || r.epoch != header.epoch
        || r.parent != parent.receipt
        || r.parent_commit != parent.commit
        || next.receipt.offset != parent.receipt.end()?
        || r.root.next_chunk < root.next_chunk
        || r.root.next_database < root.next_database
    {
        return Err(Error::Corrupt("hybrid authenticated chain/base counters"));
    }
    for object in &r.objects {
        if let ObjectRedo::BaseCopy {
            source, receipt, ..
        } = object
            && let Authority::Wal {
                lsn,
                sequence,
                digest,
            } = receipt.authority
        {
            let index = usize::try_from(
                lsn.checked_sub(1)
                    .ok_or(Error::Corrupt("hybrid zero source LSN"))?,
            )
            .map_err(|_| Error::LimitExceeded)?;
            let origin = previous
                .get(index)
                .ok_or(Error::Corrupt("hybrid source authority absent"))?;
            if origin.receipt.lsn != lsn
                || origin.receipt.sequence != sequence
                || origin.receipt.digest != digest
                || origin.record.commit != receipt.creation_commit
                || receipt.creation_lsn != lsn
                || receipt.creation_sequence != sequence
                || receipt.provenance_digest != digest
            {
                return Err(Error::Corrupt("hybrid source authority identity"));
            }
            let at = origin
                .record
                .objects
                .binary_search_by_key(&source.at, |o| o.dest().at)
                .map_err(|_| Error::Corrupt("hybrid source not an authority allocation"))?;
            if origin.record.objects[at].dest() != *source {
                return Err(Error::Corrupt("hybrid source extent/generation mismatch"));
            }
        }
    }
    Ok(())
}
