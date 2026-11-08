//! Sealed-to-crate event view. Public callers cannot implement a new issuer.
//! Both owned/cold and borrowed/live forms share canonical structure rules.
use super::*;
use crate::hybrid::redo::Patch;
#[derive(Clone, Copy)]
pub(crate) struct PatchSpan {
    pub offset: u32,
    pub length: u32,
}
#[derive(Clone, Copy)]
pub(crate) enum Patches<'a> {
    Owned(&'a [Patch]),
    Borrowed {
        spans: &'a [PatchSpan],
        after: &'a [u8],
    },
}
impl<'a> Patches<'a> {
    pub fn len(self) -> usize {
        match self {
            Self::Owned(p) => p.len(),
            Self::Borrowed { spans, .. } => spans.len(),
        }
    }
    pub fn get(self, i: usize) -> PatchRef<'a> {
        match self {
            Self::Owned(p) => PatchRef {
                offset: p[i].offset,
                bytes: &p[i].bytes,
            },
            Self::Borrowed { spans, after } => {
                let p = spans[i];
                PatchRef {
                    offset: p.offset,
                    bytes: &after[p.offset as usize..p.offset as usize + p.length as usize],
                }
            }
        }
    }
}
pub(crate) struct PatchRef<'a> {
    pub offset: u32,
    pub bytes: &'a [u8],
}
#[derive(Clone, Copy)]
pub(crate) enum BodyRef<'a> {
    Literal(&'a [u8]),
    Copy {
        source: VersionKey,
        patches: Patches<'a>,
    },
}
pub(crate) struct AllocationRef<'a> {
    pub destination: VersionKey,
    pub previous_generation: u64,
    pub ranges: &'a [(u16, u16)],
    pub body: BodyRef<'a>,
}
pub(crate) trait RecordView {
    fn header(&self) -> &Record;
    fn allocation_count(&self) -> usize;
    fn allocation(&self, i: usize) -> AllocationRef<'_>;
}
impl RecordView for Record {
    fn header(&self) -> &Record {
        self
    }
    fn allocation_count(&self) -> usize {
        self.allocations.len()
    }
    fn allocation(&self, i: usize) -> AllocationRef<'_> {
        let a = &self.allocations[i];
        AllocationRef {
            destination: a.destination,
            previous_generation: a.previous_generation,
            ranges: &a.ranges,
            body: match &a.body {
                Body::Literal(b) => BodyRef::Literal(b),
                Body::Copy { source, patches } => BodyRef::Copy {
                    source: *source,
                    patches: Patches::Owned(patches),
                },
            },
        }
    }
}
pub(crate) fn find(v: &impl RecordView, slot: u32) -> Option<usize> {
    let mut low = 0;
    let mut high = v.allocation_count();
    while low < high {
        let m = low + (high - low) / 2;
        match v.allocation(m).destination.slot.cmp(&slot) {
            std::cmp::Ordering::Less => low = m + 1,
            std::cmp::Ordering::Greater => high = m,
            std::cmp::Ordering::Equal => return Some(m),
        }
    }
    None
}
pub(crate) fn body_len(b: BodyRef<'_>) -> Result<usize> {
    match b {
        BodyRef::Literal(_) => Ok(PAGE),
        BodyRef::Copy { patches, .. } => {
            let mut n = 16usize;
            for i in 0..patches.len() {
                n = n
                    .checked_add(8)
                    .and_then(|n| n.checked_add(aligned(patches.get(i).bytes.len()).ok()?))
                    .ok_or(Error::LimitExceeded)?;
            }
            Ok(n)
        }
    }
}
pub(crate) fn encoded_length(v: &impl RecordView, boot: Boot) -> Result<usize> {
    validate(v, boot)?;
    let mut n = HEADER + FOOTER;
    for i in 0..v.allocation_count() {
        let a = v.allocation(i);
        n = n
            .checked_add(aligned(40 + a.ranges.len() * 4)?)
            .and_then(|n| n.checked_add(body_len(a.body).ok()?))
            .ok_or(Error::LimitExceeded)?;
    }
    n.checked_add(v.header().retired.len() * 16)
        .filter(|n| *n <= MAX_RECORD_BYTES)
        .ok_or(Error::LimitExceeded)
}
pub(crate) fn validate(v: &impl RecordView, boot: Boot) -> Result<()> {
    let r = v.header();

    let count = boot.slots()?;
    let sequence = match r.kind {
        Kind::Write => r
            .parent
            .sequence
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?,
        Kind::Collect => r.parent.sequence,
    };
    if r.uuid != boot.uuid
        || r.epoch != boot.epoch
        || r.lsn != r.parent.lsn.checked_add(1).ok_or(Error::LimitExceeded)?
        || r.sequence != sequence
        || r.commit
            != boot
                .commit
                .checked_add(u128::from(sequence))
                .ok_or(Error::LimitExceeded)?
        || r.parent_commit
            != boot
                .commit
                .checked_add(u128::from(r.parent.sequence))
                .ok_or(Error::LimitExceeded)?
        || r.next_database == 0
        || v.allocation_count() > count
        || r.retired.len() > count
    {
        return Err(Error::Corrupt("RL2 record identity/counters"));
    }
    catalog_key(boot, r.root)?;
    if r.parent.lsn == 0 {
        if r.parent != genesis(&boot)? {
            return Err(Error::Corrupt("RL2 genesis parent"));
        }
    } else if r.parent.sequence > r.parent.lsn
        || r.parent.offset < DATA_START
        || !r.parent.offset.is_multiple_of(8)
        || (r.parent.length < (HEADER + FOOTER) as u64
            && !(r.parent.length == 0
                && r.parent.offset == DATA_START
                && r.parent.digest != [0; 32]))
        || r.parent.length > MAX_RECORD_BYTES as u64
        || !r.parent.length.is_multiple_of(8)
    {
        return Err(Error::Corrupt("RL2 parent geometry"));
    }
    r.parent.end()?;
    if r.kind == Kind::Collect && v.allocation_count() != 0 {
        return Err(Error::Corrupt("RL2 Collect allocates"));
    }
    let mut last = None;
    for index in 0..v.allocation_count() {
        let a = v.allocation(index);
        if a.destination.slot as usize >= count
            || last.is_some_and(|n| n >= a.destination.slot)
            || a.destination.generation == 0
            || a.destination.generation > MAX_GENERATION
            || a.previous_generation >= a.destination.generation
            || (a.destination.slot >= boot.kv_pages && a.previous_generation != 0)
        {
            return Err(Error::Corrupt("RL2 allocation order/generation"));
        }
        crate::resident::wal_codec::validate_ranges(boot, a.destination.slot, a.ranges)?;
        match a.body {
            BodyRef::Literal(bytes) if bytes.len() == PAGE => {}
            BodyRef::Copy { source, patches } => {
                if a.destination.slot >= boot.kv_pages
                    || source.slot >= boot.kv_pages
                    || source.slot == a.destination.slot
                    || source.generation == 0
                    || source.generation > MAX_GENERATION
                    || patches.len() > PAGE / 2
                {
                    return Err(Error::Corrupt("RL2 Copy source/count"));
                }
                let mut end = 0;
                for i in 0..patches.len() {
                    let p = patches.get(i);
                    let start = usize::try_from(p.offset)
                        .map_err(|_| Error::Corrupt("RL2 patch offset"))?;
                    let next = start
                        .checked_add(p.bytes.len())
                        .ok_or(Error::Corrupt("RL2 patch overflow"))?;
                    if p.bytes.is_empty() || next > PAGE || (i > 0 && start <= end) {
                        return Err(Error::Corrupt("RL2 patch order/nonminimal bounds"));
                    }
                    end = next;
                }
                if body_len(a.body)? >= PAGE {
                    return Err(Error::Corrupt("RL2 Copy must be smaller than literal"));
                }
                if r.retired
                    .binary_search_by_key(&source.slot, |k| k.slot)
                    .is_ok()
                    || find(v, source.slot).is_some()
                {
                    return Err(Error::Corrupt("RL2 Copy source changes in same frame"));
                }
            }
            _ => return Err(Error::Corrupt("RL2 literal width")),
        }
        last = Some(a.destination.slot);
    }
    last = None;
    for key in &r.retired {
        if key.slot as usize >= count
            || last.is_some_and(|n| n >= key.slot)
            || key.generation == 0
            || key.generation > MAX_GENERATION
            || find(v, key.slot).is_some()
        {
            return Err(Error::Corrupt("RL2 retired order/generation/overlap"));
        }
        last = Some(key.slot);
    }
    Ok(())
}
