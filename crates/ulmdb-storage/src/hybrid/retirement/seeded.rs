//! Absolute-LSN read-only recovery with exact old-origin aliases. No repair,
//! floor installation, or prefix mutation is performed here.
use ulmdb_platform::FileExt;
use super::{CheckpointAliases, selection::Compound};
use crate::hybrid::{
    codec::Decoded,
    journal::{
        Bootstrap, WalLayout, Witness,
        wire::{self, BLOCK, DATA_START, MAX_WAL_BYTES, WITNESS_OFFSETS},
    },
    recovery::read_record,
    redo::{Authority, ObjectRedo, RootState, SourceReceipt},
};
use crate::{Error, Result};
use std::{fs::File};
#[derive(Clone, Copy, Debug)]
pub struct Boundary {
    pub base: Witness,
    pub root: RootState,
    pub predecessor: Witness,
    pub first_suffix_lsn: u64,
}
pub struct SeededPrefix {
    pub boundary: Boundary,
    pub header: Bootstrap,
    pub layout: WalLayout,
    pub selected: Witness,
    pub slot: usize,
    pub heal: bool,
    records: Vec<Decoded>,
    root: RootState,
}
impl SeededPrefix {
    pub fn records(&self) -> &[Decoded] {
        &self.records
    }
    pub fn root(&self) -> RootState {
        self.root
    }
    pub fn first_lsn(&self) -> u64 {
        self.boundary.first_suffix_lsn
    }
}
fn identity(aliases: &CheckpointAliases, h: Bootstrap) -> Result<()> {
    if aliases.uuid != h.uuid || aliases.epoch != h.epoch {
        return Err(Error::Corrupt("seeded checkpoint realm"));
    }
    Ok(())
}
fn sources(r: &Decoded, previous: &[Decoded], aliases: &CheckpointAliases) -> Result<()> {
    for object in &r.record.objects {
        if let ObjectRedo::BaseCopy {
            source, receipt, ..
        } = object
        {
            let Authority::Wal {
                lsn,
                sequence,
                digest,
            } = receipt.authority
            else {
                return Err(Error::Corrupt("seeded unsupported source authority"));
            };
            if lsn <= aliases.cut.receipt.lsn {
                if !aliases.authorizes(source, receipt) {
                    return Err(Error::Corrupt("seeded exact checkpoint alias absent"));
                }
            } else {
                let first = aliases
                    .cut
                    .receipt
                    .lsn
                    .checked_add(1)
                    .ok_or(Error::LimitExceeded)?;
                let index = usize::try_from(
                    lsn.checked_sub(first)
                        .ok_or(Error::Corrupt("seeded source below suffix"))?,
                )
                .map_err(|_| Error::LimitExceeded)?;
                let origin = previous
                    .get(index)
                    .ok_or(Error::Corrupt("seeded source outside prior suffix"))?;
                let expected = SourceReceipt {
                    uuid: origin.record.uuid,
                    epoch: origin.record.epoch,
                    creation_lsn: origin.record.lsn,
                    creation_sequence: origin.record.sequence,
                    creation_commit: origin.record.commit,
                    provenance_digest: origin.receipt.digest,
                    authority: Authority::Wal {
                        lsn: origin.record.lsn,
                        sequence: origin.record.sequence,
                        digest: origin.receipt.digest,
                    },
                };
                let at = origin
                    .record
                    .objects
                    .binary_search_by_key(&source.at, |o| o.dest().at)
                    .map_err(|_| Error::Corrupt("seeded source allocation absent"))?;
                if expected != *receipt
                    || origin.record.objects[at].dest() != *source
                    || sequence != receipt.creation_sequence
                    || digest != receipt.provenance_digest
                {
                    return Err(Error::Corrupt("seeded source identity/extent"));
                }
            }
        }
    }
    Ok(())
}
fn boundary(file: &File, h: Bootstrap, eof: u64, aliases: &CheckpointAliases) -> Result<Boundary> {
    identity(aliases, h)?;
    let cut = aliases.cut;
    let genesis = wire::genesis(h)?;
    let first_suffix_lsn = cut.receipt.lsn.checked_add(1).ok_or(Error::LimitExceeded)?;
    if cut.receipt.lsn == 0 {
        if cut.receipt != genesis.receipt
            || cut.commit != genesis.commit
            || cut.root != RootState::empty()
        {
            return Err(Error::Corrupt("seeded noncanonical genesis"));
        }
        return Ok(Boundary {
            base: genesis,
            root: cut.root,
            predecessor: genesis,
            first_suffix_lsn,
        });
    }
    let c = read_record(file, cut.receipt.offset, eof)?;
    if c.receipt != cut.receipt
        || c.record.commit != cut.commit
        || c.record.root != cut.root
        || c.record.uuid != h.uuid
        || c.record.epoch != h.epoch
    {
        return Err(Error::Corrupt("seeded C not authenticated by checkpoint"));
    }
    let predecessor = if c.record.parent.lsn == 0 {
        if c.record.parent != genesis.receipt || c.record.parent_commit != genesis.commit {
            return Err(Error::Corrupt("seeded C predecessor genesis"));
        }
        genesis
    } else {
        let p = read_record(file, c.record.parent.offset, cut.receipt.offset)?;
        if p.receipt != c.record.parent
            || p.record.commit != c.record.parent_commit
            || p.receipt.end()? != c.receipt.offset
            || p.record.uuid != h.uuid
            || p.record.epoch != h.epoch
            || p.record.root.next_chunk > cut.root.next_chunk
            || p.record.root.next_database > cut.root.next_database
        {
            return Err(Error::Corrupt("seeded C-1 not authenticated by C"));
        }
        sources(&p, &[], aliases)?;
        Witness {
            receipt: p.receipt,
            commit: p.record.commit,
        }
    };
    if predecessor.receipt.end()? != c.receipt.offset {
        return Err(Error::Corrupt("seeded boundary physical continuity"));
    }
    sources(&c, &[], aliases)?;
    for d in c.record.objects.iter() {
        let receipt = SourceReceipt {
            uuid: h.uuid,
            epoch: h.epoch,
            creation_lsn: c.record.lsn,
            creation_sequence: c.record.sequence,
            creation_commit: c.record.commit,
            provenance_digest: c.receipt.digest,
            authority: Authority::Wal {
                lsn: c.record.lsn,
                sequence: c.record.sequence,
                digest: c.receipt.digest,
            },
        };
        if !aliases.authorizes(&d.dest(), &receipt) {
            return Err(Error::Corrupt(
                "seeded C allocation missing from checkpoint",
            ));
        }
    }
    Ok(Boundary {
        base: Witness {
            receipt: cut.receipt,
            commit: cut.commit,
        },
        root: cut.root,
        predecessor,
        first_suffix_lsn,
    })
}
fn next(
    h: Bootstrap,
    parent: Witness,
    root: RootState,
    previous: &[Decoded],
    d: &Decoded,
    aliases: &CheckpointAliases,
) -> Result<()> {
    let r = &d.record;
    if r.uuid != h.uuid
        || r.epoch != h.epoch
        || r.parent != parent.receipt
        || r.parent_commit != parent.commit
        || d.receipt.offset != parent.receipt.end()?
        || r.root.next_chunk < root.next_chunk
        || r.root.next_database < root.next_database
    {
        return Err(Error::Corrupt("seeded absolute chain/counters"));
    }
    sources(d, previous, aliases)
}
fn append_to(
    file: &File,
    h: Bootstrap,
    b: Boundary,
    target: Witness,
    eof: u64,
    aliases: &CheckpointAliases,
) -> Result<(Vec<Decoded>, RootState)> {
    if target.receipt.lsn < b.base.receipt.lsn || target.receipt.end()? < b.base.receipt.end()? {
        return Err(Error::Corrupt("seeded head below selected base"));
    }
    let mut records = Vec::new();
    let mut last = b.base;
    let mut root = b.root;
    while last.receipt.end()? < target.receipt.end()? {
        let d = read_record(file, last.receipt.end()?, eof)?;
        next(h, last, root, &records, &d, aliases)?;
        last = Witness {
            receipt: d.receipt,
            commit: d.record.commit,
        };
        root = d.record.root;
        records.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
        records.push(d);
    }
    if last != target {
        return Err(Error::Corrupt("seeded selected receipt mismatch"));
    }
    Ok((records, root))
}
fn fixed(file: &File) -> Result<(Vec<u8>, Bootstrap, WalLayout, u64)> {
    let eof = file.metadata()?.len();
    if !(DATA_START..=MAX_WAL_BYTES).contains(&eof) {
        return Err(Error::Corrupt("seeded WAL physical extent"));
    }
    let mut p = vec![0u8; DATA_START as usize];
    file.read_exact_at(&mut p, 0)?;
    let (h, l) = wire::decode_layout_header(p[..BLOCK].try_into().unwrap())?;
    if let WalLayout::PreSized { capacity } = l
        && capacity != eof
    {
        return Err(Error::Corrupt("seeded WAL fixed capacity"));
    }
    for (at, byte) in p.iter().enumerate() {
        if at >= BLOCK
            && !WITNESS_OFFSETS
                .iter()
                .any(|o| (*o as usize..*o as usize + BLOCK).contains(&at))
            && *byte != 0
        {
            return Err(Error::Corrupt("seeded reserved prefix"));
        }
    }
    Ok((p, h, l, eof))
}
pub(crate) fn inspect_anchor(
    file: &File,
    h: Bootstrap,
    l: WalLayout,
    selected: Witness,
    expected_root: RootState,
    aliases: &CheckpointAliases,
) -> Result<SeededPrefix> {
    inspect_captured(file, h, l, selected, expected_root, aliases, None)
}
pub(crate) fn inspect_compound_anchor(
    file: &File,
    h: Bootstrap,
    l: WalLayout,
    selected: Witness,
    expected_root: RootState,
    aliases: &CheckpointAliases,
    compound: Compound,
) -> Result<SeededPrefix> {
    inspect_captured(file, h, l, selected, expected_root, aliases, Some(compound))
}
fn inspect_captured(
    file: &File,
    h: Bootstrap,
    l: WalLayout,
    selected: Witness,
    expected_root: RootState,
    aliases: &CheckpointAliases,
    compound: Option<Compound>,
) -> Result<SeededPrefix> {
    let (p, actual, layout, eof) = fixed(file)?;
    if h != actual || l != layout {
        return Err(Error::Corrupt("seeded captured header/layout"));
    }
    let b = boundary(file, h, eof, aliases)?;
    if let Some(c) = compound {
        check_compound(file, h, eof, &p, b, aliases, c)?;
    }
    let (records, root) = append_to(file, h, b, selected, selected.receipt.end()?, aliases)?;
    if root != expected_root {
        return Err(Error::Corrupt("seeded captured root"));
    }
    Ok(SeededPrefix {
        boundary: b,
        header: h,
        layout: l,
        selected,
        slot: 0,
        heal: false,
        records,
        root,
    })
}
/// Read-only kernel planner. The facade must certify the selected alias context;
/// this parser does not publish/heal/truncate or mint an installation permit.
pub fn inspect(file: &File, aliases: &CheckpointAliases) -> Result<SeededPrefix> {
    let (p, h, l, eof) = fixed(file)?;
    inspect_parts(file, aliases, p, h, l, eof, None)
}
pub fn inspect_compound(
    file: &File,
    aliases: &CheckpointAliases,
    compound: Compound,
) -> Result<SeededPrefix> {
    let (p, h, l, eof) = fixed(file)?;
    inspect_parts(file, aliases, p, h, l, eof, Some(compound))
}
fn inspect_parts(
    file: &File,
    aliases: &CheckpointAliases,
    p: Vec<u8>,
    h: Bootstrap,
    l: WalLayout,
    eof: u64,
    compound: Option<Compound>,
) -> Result<SeededPrefix> {
    let b = boundary(file, h, eof, aliases)?;
    if let Some(c) = compound {
        check_compound(file, h, eof, &p, b, aliases, c)?;
    }
    let genesis = wire::genesis(h)?;
    let heads = WITNESS_OFFSETS.map(|at| {
        wire::decode_layout_witness(
            h,
            p[at as usize..at as usize + BLOCK].try_into().unwrap(),
            l,
        )
    });
    let (anchor, slot, heal) = match heads {
        [Ok(a), Ok(c)] => {
            if a.receipt.lsn == c.receipt.lsn {
                if a != genesis || c != genesis || b.base != genesis {
                    return Err(Error::Corrupt("seeded equal heads"));
                }
                (a, 0, false)
            } else {
                let (new, old, index) = if a.receipt.lsn > c.receipt.lsn {
                    (a, c, 0)
                } else {
                    (c, a, 1)
                };
                if new.receipt.lsn != old.receipt.lsn.checked_add(1).ok_or(Error::LimitExceeded)?
                    || new.receipt.sequence
                        != old
                            .receipt
                            .sequence
                            .checked_add(1)
                            .ok_or(Error::LimitExceeded)?
                {
                    return Err(Error::Corrupt("seeded nonadjacent heads"));
                }
                (new, index, false)
            }
        }
        [Ok(a), Err(_)] => (a, 1, true),
        [Err(_), Ok(a)] => (a, 0, true),
        _ => return Err(Error::Corrupt("seeded both heads invalid")),
    };
    let (mut records, mut root, mut selected) =
        if heal && anchor == b.predecessor && anchor.receipt.lsn < b.base.receipt.lsn {
            (Vec::new(), b.root, b.base)
        } else {
            let (r, root) = append_to(file, h, b, anchor, eof, aliases)?;
            (r, root, anchor)
        };
    if !heal {
        let older = wire::decode_layout_witness(
            h,
            p[WITNESS_OFFSETS[1 - slot] as usize..WITNESS_OFFSETS[1 - slot] as usize + BLOCK]
                .try_into()
                .unwrap(),
            l,
        )?;
        let expected = if records.len() >= 2 {
            let d = &records[records.len() - 2];
            Witness {
                receipt: d.receipt,
                commit: d.record.commit,
            }
        } else if records.len() == 1 {
            b.base
        } else {
            b.predecessor
        };
        if older != expected {
            return Err(Error::Corrupt("seeded previous selected mismatch"));
        }
    } else {
        if selected == anchor {
            let d = read_record(file, anchor.receipt.end()?, eof)?;
            next(h, anchor, root, &records, &d, aliases)?;
            selected = Witness {
                receipt: d.receipt,
                commit: d.record.commit,
            };
            root = d.record.root;
            records.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
            records.push(d);
        }
        match l {
            WalLayout::Dense => {
                if selected.receipt.end()? != eof {
                    return Err(Error::Corrupt("seeded ambiguous successor tail"));
                }
            }
            WalLayout::PreSized { capacity } => {
                crate::hybrid::journal::tail::verify_zero(file, selected.receipt.end()?, capacity)?;
            }
        }
    }
    Ok(SeededPrefix {
        boundary: b,
        header: h,
        layout: l,
        selected,
        slot,
        heal,
        records,
        root,
    })
}

fn check_compound(
    file: &File,
    h: Bootstrap,
    eof: u64,
    fixed: &[u8],
    b: Boundary,
    aliases: &CheckpointAliases,
    c: Compound,
) -> Result<()> {
    let selected = c.selector();
    if fixed[BLOCK - 32..BLOCK] != c.header_digest()
        || selected.uuid != h.uuid
        || selected.epoch != h.epoch
        || selected.digest != aliases.descriptor_digest()
        || selected.cut != aliases.selection()
        || b.base.receipt != selected.cut.receipt
        || b.base.commit != selected.cut.commit
        || b.root != selected.cut.root
        || b.predecessor != c.predecessor()
    {
        return Err(Error::Corrupt(
            "compound selected header/alias/boundary binding",
        ));
    }
    let first = c.first_required();
    if first.receipt.lsn == 0 {
        if first != wire::genesis(h)? {
            return Err(Error::Corrupt(
                "compound canonical genesis retained witness",
            ));
        }
    } else if first != b.predecessor {
        let record = read_record(file, first.receipt.offset, eof)?;
        if record.receipt != first.receipt
            || record.record.commit != first.commit
            || record.record.uuid != h.uuid
            || record.record.epoch != h.epoch
        {
            return Err(Error::Corrupt("compound first-retained witness binding"));
        }
        sources(&record, &[], aliases)?;
    }
    Ok(())
}
