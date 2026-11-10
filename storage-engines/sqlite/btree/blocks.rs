//! Private grouped record-store prototype. Not wired into an adapter or format.
//! A ready view requires a complete, bounded audit of its actual ReadTxn.
#![forbid(unsafe_code)]
use std::{cell::RefCell, ops::Bound};
use ulmdb::snapshot_log::{Cursor as NativeCursor, KvRead, ReadTxn, WriteTxn};

pub(crate) const MAX_ROWS: usize = 32;
pub(crate) const MAX_GROUP: usize = 64 << 10;
pub(crate) const MAX_RECORD: usize = 16 << 20;
const HEADER: usize = 16;
const ENTRY: usize = 16;
const MAGIC: &[u8; 8] = b"USQLGR01";
const GROUP: u8 = 0;
const RAW: u8 = 1;
const FIXED_SCRATCH: usize = 16 << 10;
pub(crate) type LogicalKey = [u8; 16];
pub(crate) type PhysicalKey = [u8; 17];
#[derive(Debug)]
pub(crate) enum Error {
    Corrupt,
    Limit,
    Invalid,
    Native(ulmdb::Error),
}
impl From<ulmdb::Error> for Error {
    fn from(e: ulmdb::Error) -> Self {
        Self::Native(e)
    }
}
pub(crate) type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub(crate) audit_blocks: usize,
    pub(crate) audit_rows: usize,
    pub(crate) audit_bytes: usize,
    pub(crate) changes: usize,
    pub(crate) operations: usize,
    pub(crate) encoded_bytes: usize,
    /// Requested owned output and preallocated edit-vector bytes, plus fixed
    /// bounded working space. Allocator bookkeeping and mapped pages are separate.
    pub(crate) scratch_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            audit_blocks: 65_536,
            audit_rows: 65_536 * MAX_ROWS,
            audit_bytes: 64 << 20,
            changes: 65_536,
            operations: 65_536,
            encoded_bytes: 64 << 20,
            scratch_bytes: 64 << 20,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Row<'a> {
    pub(crate) logical_key: LogicalKey,
    pub(crate) visible_key: &'a [u8; 8],
    pub(crate) value: &'a [u8],
}
#[derive(Clone, Copy)]
struct Block<'a> {
    physical: PhysicalKey,
    rows: [Option<Row<'a>>; MAX_ROWS],
    count: usize,
}
impl<'a> Block<'a> {
    fn first(&self) -> Row<'a> {
        self.rows[0].unwrap()
    }
    fn last(&self) -> Row<'a> {
        self.rows[self.count - 1].unwrap()
    }
    fn find(&self, key: &[u8; 8]) -> Option<Row<'a>> {
        let mut lo = 0;
        let mut hi = self.count;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let row = self.rows[mid].unwrap();
            match row.visible_key.cmp(key) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(row),
            }
        }
        None
    }
}
pub(crate) fn rowid(key: i64) -> [u8; 8] {
    ((key as u64) ^ (1u64 << 63)).to_be_bytes()
}
fn logical(dbi: u32, key: &[u8; 8]) -> LogicalKey {
    let mut out = [0; 16];
    out[..8].copy_from_slice(&(dbi as u64).to_be_bytes());
    out[8..].copy_from_slice(key);
    out
}
fn physical(dbi: u32, key: &[u8; 8], kind: u8) -> PhysicalKey {
    let mut out = [0; 17];
    out[..16].copy_from_slice(&logical(dbi, key));
    out[16] = kind;
    out
}
fn prefix(dbi: u64) -> [u8; 8] {
    dbi.to_be_bytes()
}
fn decode<'a>(dbi: u32, key: &'a [u8], raw: &'a [u8]) -> Result<Block<'a>> {
    if key.len() != 17 || key[..8] != (dbi as u64).to_be_bytes() {
        return Err(Error::Corrupt);
    }
    let physical: PhysicalKey = key.try_into().map_err(|_| Error::Corrupt)?;
    let mut block = Block {
        physical,
        rows: [None; MAX_ROWS],
        count: 0,
    };
    if key[16] == RAW {
        if raw.len() > MAX_RECORD {
            return Err(Error::Corrupt);
        }
        let visible_key = key[8..16].try_into().map_err(|_| Error::Corrupt)?;
        block.rows[0] = Some(Row {
            logical_key: logical(dbi, visible_key),
            visible_key,
            value: raw,
        });
        block.count = 1;
        return Ok(block);
    }
    if key[16] != GROUP || raw.len() < HEADER + ENTRY || raw.len() > MAX_GROUP || &raw[..8] != MAGIC
    {
        return Err(Error::Corrupt);
    }
    let count = u16::from_be_bytes(raw[8..10].try_into().unwrap()) as usize;
    let table = u16::from_be_bytes(raw[10..12].try_into().unwrap()) as usize;
    let total = u32::from_be_bytes(raw[12..16].try_into().unwrap()) as usize;
    if !(2..=MAX_ROWS).contains(&count)
        || table != count * ENTRY
        || total != raw.len()
        || HEADER + table > raw.len()
    {
        return Err(Error::Corrupt);
    }
    let mut end = HEADER + table;
    let mut previous = None;
    for i in 0..count {
        let at = HEADER + i * ENTRY;
        let visible_key: &[u8; 8] = raw[at..at + 8].try_into().unwrap();
        let offset = u32::from_be_bytes(raw[at + 8..at + 12].try_into().unwrap()) as usize;
        let length = u32::from_be_bytes(raw[at + 12..at + 16].try_into().unwrap()) as usize;
        let next = offset.checked_add(length).ok_or(Error::Corrupt)?;
        if previous.is_some_and(|old: &[u8; 8]| old >= visible_key)
            || offset != end
            || next > raw.len()
        {
            return Err(Error::Corrupt);
        }
        block.rows[i] = Some(Row {
            logical_key: logical(dbi, visible_key),
            visible_key,
            value: &raw[offset..next],
        });
        previous = Some(visible_key);
        end = next;
    }
    block.count = count;
    if end != raw.len() || block.first().visible_key.as_slice() != &key[8..16] {
        return Err(Error::Corrupt);
    }
    Ok(block)
}
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Audit {
    pub(crate) blocks: usize,
    pub(crate) rows: usize,
    pub(crate) bytes: usize,
}
fn charge(current: &mut usize, add: usize, limit: usize) -> Result<()> {
    *current = current
        .checked_add(add)
        .filter(|&n| n <= limit)
        .ok_or(Error::Limit)?;
    Ok(())
}
/// The last decoded block belongs to this exact borrowed ReadTxn, never a
/// pathname/global generation. There is one bounded directory and no row copy.
pub(crate) struct SnapshotRows<'a> {
    source_read: &'a ReadTxn,
    view: KvRead<'a>,
    data: &'a [u8],
    dbi: u32,
    cache: RefCell<Option<Block<'a>>>,
    audit: Audit,
}
impl<'a> SnapshotRows<'a> {
    pub(crate) fn open_audited(
        read: &'a ReadTxn,
        data: &'a [u8],
        dbi: u32,
        limits: Limits,
    ) -> Result<Self> {
        if dbi == 0 {
            return Err(Error::Invalid);
        }
        let view = read.kv(data)?;
        let lo = prefix(dbi as u64);
        let hi = prefix(dbi as u64 + 1);
        let mut audit = Audit::default();
        let mut last: Option<[u8; 8]> = None;
        for item in view.cursor(Bound::Included(&lo), Bound::Excluded(&hi), false)? {
            let (k, v) = item?;
            charge(&mut audit.blocks, 1, limits.audit_blocks)?;
            charge(&mut audit.bytes, k.len(), limits.audit_bytes)?;
            charge(&mut audit.bytes, v.len(), limits.audit_bytes)?;
            let block = decode(dbi, k, v)?;
            charge(&mut audit.rows, block.count, limits.audit_rows)?;
            if last.is_some_and(|old| &old >= block.first().visible_key) {
                return Err(Error::Corrupt);
            }
            last = Some(*block.last().visible_key);
        }
        Ok(Self {
            source_read: read,
            view,
            data,
            dbi,
            cache: RefCell::new(None),
            audit,
        })
    }
    /// Reuse this completed audit only for the SAME live native ReadTxn and
    /// database. Both references are live during identity checking, preventing
    /// address reuse; returned views borrow the supplied actual owner directly.
    pub(crate) fn for_read<'b>(
        &self,
        read: &'b ReadTxn,
        data: &'b [u8],
    ) -> Result<SnapshotRows<'b>> {
        if !std::ptr::eq(self.source_read, read) || data != self.data {
            return Err(Error::Invalid);
        }
        Ok(SnapshotRows {
            source_read: read,
            view: read.kv(data)?,
            data,
            dbi: self.dbi,
            cache: RefCell::new(None),
            audit: self.audit,
        })
    }
    pub(crate) fn cursor_for<'b>(
        &self,
        read: &'b ReadTxn,
        data: &'b [u8],
        lower: Bound<[u8; 8]>,
        upper: Bound<[u8; 8]>,
        reverse: bool,
    ) -> Result<LogicalCursor<'b>> {
        self.for_read(read, data)?.cursor(lower, upper, reverse)
    }
    pub(crate) fn audit(&self) -> Audit {
        self.audit
    }
    pub(crate) fn cache_directory_bytes(&self) -> usize {
        std::mem::size_of::<Block<'a>>()
    }
    fn predecessor(&self, key: &[u8; 8]) -> Result<Option<Block<'a>>> {
        let lo = prefix(self.dbi as u64);
        let q = physical(self.dbi, key, u8::MAX);
        match self
            .view
            .cursor(Bound::Included(&lo), Bound::Included(&q), true)?
            .next()
        {
            Some(item) => {
                let (k, v) = item?;
                Ok(Some(decode(self.dbi, k, v)?))
            }
            None => Ok(None),
        }
    }
    pub(crate) fn exact(&self, key: &[u8; 8]) -> Result<Option<Row<'a>>> {
        {
            let cache = self.cache.borrow();
            if let Some(block) = cache.as_ref() {
                if block.first().visible_key <= key && key <= block.last().visible_key {
                    return Ok(block.find(key));
                }
            }
        }
        let block = self.predecessor(key)?;
        *self.cache.borrow_mut() = block;
        Ok(block.and_then(|b| b.find(key)))
    }
    pub(crate) fn cursor(
        &self,
        lower: Bound<[u8; 8]>,
        upper: Bound<[u8; 8]>,
        reverse: bool,
    ) -> Result<LogicalCursor<'a>> {
        let lo = prefix(self.dbi as u64);
        let hi = prefix(self.dbi as u64 + 1);
        let anchor = if reverse { &upper } else { &lower };
        let start = match anchor {
            Bound::Included(k) | Bound::Excluded(k) => self.predecessor(k)?.map(|b| b.physical),
            Bound::Unbounded => None,
        };
        let mut low = Bound::Included(lo.as_slice());
        let mut high = Bound::Excluded(hi.as_slice());
        if let Some(k) = start.as_ref() {
            if reverse {
                high = Bound::Included(k.as_slice())
            } else {
                low = Bound::Included(k.as_slice())
            }
        }
        // A reverse bound preceding the first physical record has no rows.
        let empty = reverse && !matches!(anchor, Bound::Unbounded) && start.is_none();
        Ok(LogicalCursor {
            native: self.view.cursor(low, high, reverse)?,
            dbi: self.dbi,
            block: None,
            at: 0,
            reverse,
            lower,
            upper,
            done: empty,
            current: None,
        })
    }
    fn next_start(&self, block: Option<&Block<'a>>) -> Result<Option<[u8; 8]>> {
        let lo = prefix(self.dbi as u64);
        let hi = prefix(self.dbi as u64 + 1);
        let low = match block {
            Some(b) => Bound::Excluded(b.physical.as_slice()),
            None => Bound::Included(lo.as_slice()),
        };
        match self.view.cursor(low, Bound::Excluded(&hi), false)?.next() {
            Some(item) => {
                let (k, v) = item?;
                let b = decode(self.dbi, k, v)?;
                Ok(Some(*b.first().visible_key))
            }
            None => Ok(None),
        }
    }
    /// Logical changes stay caller-owned. Each base block's interval is
    /// rewritten once; only affected intervals are read/re-encoded. Adjacent
    /// untouched groups are deliberately not rebalanced in this prototype.
    pub(crate) fn plan<'p>(
        &'p self,
        changes: &'p [Change<'p>],
        limits: Limits,
    ) -> Result<Plan<'p>> {
        if changes.len() > limits.changes {
            return Err(Error::Limit);
        }
        if changes.windows(2).any(|w| w[0].key >= w[1].key)
            || changes
                .iter()
                .any(|c| c.value.is_some_and(|v| v.len() > MAX_RECORD))
        {
            return Err(Error::Invalid);
        }
        let capacity = changes
            .len()
            .checked_mul(MAX_ROWS + 2)
            .ok_or(Error::Limit)?
            .min(limits.operations);
        let mut plan = Plan::new(self.data, limits, capacity)?;
        let mut at = 0;
        while at < changes.len() {
            let block = self.predecessor(&changes[at].key)?;
            let stop = self.next_start(block.as_ref())?;
            let mut end = at + 1;
            while end < changes.len() && stop.is_none_or(|k| changes[end].key < k) {
                end += 1
            }
            let mut pack = Pack::new(self.dbi);
            let mut old = 0;
            let mut delta = at;
            let count = block.as_ref().map_or(0, |b| b.count);
            let first_edit = plan.edits.len();
            while old < count || delta < end {
                let existing = if old < count {
                    block.as_ref().unwrap().rows[old]
                } else {
                    None
                };
                let change = changes.get(delta).filter(|_| delta < end);
                match (existing, change) {
                    (Some(row), Some(c)) => match row.visible_key.cmp(&c.key) {
                        std::cmp::Ordering::Less => {
                            pack.push(*row.visible_key, row.value, &mut plan)?;
                            old += 1
                        }
                        std::cmp::Ordering::Equal => {
                            if let Some(v) = c.value {
                                pack.push(c.key, v, &mut plan)?
                            }
                            old += 1;
                            delta += 1
                        }
                        std::cmp::Ordering::Greater => {
                            if let Some(v) = c.value {
                                pack.push(c.key, v, &mut plan)?
                            }
                            delta += 1
                        }
                    },
                    (Some(row), None) => {
                        pack.push(*row.visible_key, row.value, &mut plan)?;
                        old += 1
                    }
                    (None, Some(c)) => {
                        if let Some(v) = c.value {
                            pack.push(c.key, v, &mut plan)?
                        }
                        delta += 1
                    }
                    (None, None) => break,
                }
            }
            pack.flush(&mut plan)?;
            if let Some(old) = block {
                if !plan.edits[first_edit..]
                    .iter()
                    .any(|e| e.key == old.physical)
                {
                    plan.add(old.physical, None)?
                }
            }
            at = end;
        }
        Ok(plan)
    }
}
fn in_lower(key: &[u8; 8], bound: &Bound<[u8; 8]>) -> bool {
    match bound {
        Bound::Included(v) => key >= v,
        Bound::Excluded(v) => key > v,
        Bound::Unbounded => true,
    }
}
fn in_upper(key: &[u8; 8], bound: &Bound<[u8; 8]>) -> bool {
    match bound {
        Bound::Included(v) => key <= v,
        Bound::Excluded(v) => key < v,
        Bound::Unbounded => true,
    }
}
pub(crate) struct LogicalCursor<'a> {
    native: NativeCursor<'a>,
    dbi: u32,
    block: Option<Block<'a>>,
    at: usize,
    reverse: bool,
    lower: Bound<[u8; 8]>,
    upper: Bound<[u8; 8]>,
    done: bool,
    current: Option<Row<'a>>,
}
impl<'a> LogicalCursor<'a> {
    pub(crate) fn current(&self) -> Option<Row<'a>> {
        self.current
    }
}
impl<'a> Iterator for LogicalCursor<'a> {
    type Item = Result<Row<'a>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        loop {
            if let Some(block) = self.block.as_ref() {
                if self.at < block.count {
                    let i = if self.reverse {
                        block.count - 1 - self.at
                    } else {
                        self.at
                    };
                    self.at += 1;
                    let row = block.rows[i].unwrap();
                    if in_lower(row.visible_key, &self.lower)
                        && in_upper(row.visible_key, &self.upper)
                    {
                        self.current = Some(row);
                        return Some(Ok(row));
                    }
                    if (!self.reverse && !in_upper(row.visible_key, &self.upper))
                        || (self.reverse && !in_lower(row.visible_key, &self.lower))
                    {
                        self.done = true;
                        return None;
                    }
                    continue;
                }
            }
            match self.native.next() {
                Some(Ok((k, v))) => match decode(self.dbi, k, v) {
                    Ok(b) => {
                        self.block = Some(b);
                        self.at = 0
                    }
                    Err(e) => {
                        self.done = true;
                        return Some(Err(e));
                    }
                },
                Some(Err(e)) => {
                    self.done = true;
                    return Some(Err(e.into()));
                }
                None => {
                    self.done = true;
                    return None;
                }
            }
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Change<'a> {
    pub(crate) key: [u8; 8],
    pub(crate) value: Option<&'a [u8]>,
}
enum Bytes<'a> {
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
}
impl Bytes<'_> {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Borrowed(v) => v,
            Self::Owned(v) => v,
        }
    }
}
struct Edit<'a> {
    key: PhysicalKey,
    value: Option<Bytes<'a>>,
}
pub(crate) struct Plan<'a> {
    data: &'a [u8],
    edits: Vec<Edit<'a>>,
    limits: Limits,
    encoded: usize,
    owned: usize,
    metadata: usize,
}
impl<'a> Plan<'a> {
    fn new(data: &'a [u8], limits: Limits, capacity: usize) -> Result<Self> {
        let metadata = capacity
            .checked_mul(std::mem::size_of::<Edit<'a>>())
            .ok_or(Error::Limit)?;
        if metadata
            .checked_add(FIXED_SCRATCH)
            .is_none_or(|n| n > limits.scratch_bytes)
        {
            return Err(Error::Limit);
        }
        let mut edits = Vec::new();
        edits
            .try_reserve_exact(capacity)
            .map_err(|_| Error::Limit)?;
        let metadata = edits
            .capacity()
            .checked_mul(std::mem::size_of::<Edit<'a>>())
            .ok_or(Error::Limit)?;
        if metadata
            .checked_add(FIXED_SCRATCH)
            .is_none_or(|n| n > limits.scratch_bytes)
        {
            return Err(Error::Limit);
        }
        Ok(Self {
            data,
            edits,
            limits,
            encoded: 0,
            owned: 0,
            metadata,
        })
    }
    fn add(&mut self, key: PhysicalKey, value: Option<Bytes<'a>>) -> Result<()> {
        if self.edits.len() >= self.limits.operations || self.edits.len() == self.edits.capacity() {
            return Err(Error::Limit);
        }
        let bytes = value
            .as_ref()
            .map_or(0, |v| v.as_slice().len())
            .checked_add(17)
            .ok_or(Error::Limit)?;
        charge(&mut self.encoded, bytes, self.limits.encoded_bytes)?;
        self.edits.push(Edit { key, value });
        Ok(())
    }
    fn own_room(&self, size: usize) -> Result<()> {
        if self
            .owned
            .checked_add(self.metadata)
            .and_then(|n| n.checked_add(FIXED_SCRATCH))
            .and_then(|n| n.checked_add(size))
            .is_none_or(|n| n > self.limits.scratch_bytes)
        {
            Err(Error::Limit)
        } else {
            Ok(())
        }
    }
    pub(crate) fn operations(&self) -> usize {
        self.edits.len()
    }
    /// Native key plus value input bytes; delete keys are included.
    pub(crate) fn encoded_bytes(&self) -> usize {
        self.encoded
    }
    /// Stages into the caller's existing native transaction; NEVER commits.
    /// Caller must retain the exclusive writer reservation since this audited
    /// base was captured, and supply its SAME environment/base transaction.
    /// NativeWriteTxn exposes no public identity for this module to authenticate.
    /// Any error requires aborting that native transaction, not committing it.
    pub(crate) fn apply(&self, write: &mut WriteTxn<'_>) -> Result<()> {
        for e in &self.edits {
            match &e.value {
                Some(v) => write.put(self.data, &e.key, v.as_slice())?,
                None => write.delete(self.data, &e.key)?,
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
struct Input<'a> {
    key: [u8; 8],
    value: &'a [u8],
}
struct Pack<'a> {
    dbi: u32,
    rows: [Option<Input<'a>>; MAX_ROWS],
    count: usize,
    values: usize,
}
impl<'a> Pack<'a> {
    fn new(dbi: u32) -> Self {
        Self {
            dbi,
            rows: [None; MAX_ROWS],
            count: 0,
            values: 0,
        }
    }
    fn push(&mut self, key: [u8; 8], value: &'a [u8], plan: &mut Plan<'a>) -> Result<()> {
        if value.len() > MAX_RECORD {
            return Err(Error::Invalid);
        }
        if self.count > 0
            && (self.count == MAX_ROWS
                || HEADER + (self.count + 1) * ENTRY + self.values + value.len() > MAX_GROUP)
        {
            self.flush(plan)?
        }
        self.rows[self.count] = Some(Input { key, value });
        self.count += 1;
        self.values += value.len();
        if HEADER + ENTRY + value.len() > MAX_GROUP {
            self.flush(plan)?
        }
        Ok(())
    }
    fn flush(&mut self, plan: &mut Plan<'a>) -> Result<()> {
        if self.count == 0 {
            return Ok(());
        }
        let first = self.rows[0].unwrap();
        if self.count == 1 {
            plan.add(
                physical(self.dbi, &first.key, RAW),
                Some(Bytes::Borrowed(first.value)),
            )?
        } else {
            let size = HEADER + self.count * ENTRY + self.values;
            plan.own_room(size)?;
            let mut out = Vec::new();
            out.try_reserve_exact(size).map_err(|_| Error::Limit)?;
            plan.own_room(out.capacity())?;
            out.resize(HEADER + self.count * ENTRY, 0);
            out[..8].copy_from_slice(MAGIC);
            out[8..10].copy_from_slice(&(self.count as u16).to_be_bytes());
            out[10..12].copy_from_slice(&((self.count * ENTRY) as u16).to_be_bytes());
            out[12..16].copy_from_slice(&(size as u32).to_be_bytes());
            for i in 0..self.count {
                let row = self.rows[i].unwrap();
                let at = HEADER + i * ENTRY;
                let offset = out.len();
                out[at..at + 8].copy_from_slice(&row.key);
                out[at + 8..at + 12].copy_from_slice(&(offset as u32).to_be_bytes());
                out[at + 12..at + 16].copy_from_slice(&(row.value.len() as u32).to_be_bytes());
                out.extend_from_slice(row.value)
            }
            let capacity = out.capacity();
            plan.add(
                physical(self.dbi, &first.key, GROUP),
                Some(Bytes::Owned(out)),
            )?;
            plan.owned += capacity;
        }
        self.count = 0;
        self.values = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use ulmdb::{
        DatabaseKind,
        snapshot_log::{Environment, Options},
    };
    const DATA: &[u8] = b"blocks-private-test";
    struct Test {
        env: Environment,
        path: PathBuf,
        keep: bool,
    }
    impl Test {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "ulmdb-blocks-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let env = Environment::create(
                &path,
                Options {
                    index_bytes: 64 << 20,
                    journal_capacity: 64 << 20,
                    transaction_bytes: (64 << 20) - 4096,
                    max_operations: 65536,
                    max_databases: 1,
                    versioned_values: false,
                    ..Default::default()
                },
            )
            .unwrap();
            {
                let mut w = env.begin_write().unwrap();
                w.create_database(DATA, DatabaseKind::Kv).unwrap();
                w.commit().unwrap();
            }
            Self {
                env,
                path,
                keep: false,
            }
        }
        fn set(&self, changes: &[Change<'_>]) {
            let r = self.env.begin_read();
            let rows = SnapshotRows::open_audited(&r, DATA, 3, Limits::default()).unwrap();
            let p = rows.plan(changes, Limits::default()).unwrap();
            let mut w = self.env.begin_write().unwrap();
            p.apply(&mut w).unwrap();
            w.commit().unwrap();
        }
        fn all(&self) -> Vec<(i64, Vec<u8>)> {
            let r = self.env.begin_read();
            let rows = SnapshotRows::open_audited(&r, DATA, 3, Limits::default()).unwrap();
            rows.cursor(Bound::Unbounded, Bound::Unbounded, false)
                .unwrap()
                .map(|v| {
                    let v = v.unwrap();
                    (
                        ((u64::from_be_bytes(*v.visible_key)) ^ (1 << 63)) as i64,
                        v.value.to_vec(),
                    )
                })
                .collect()
        }
    }
    impl Drop for Test {
        fn drop(&mut self) {
            if !self.keep {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
    }
    #[test]
    fn sparse_signed_bounds_reverse_current_and_cursor_independent_views() {
        let t = Test::new();
        let keys = [i64::MIN, -1_000_000, -1, 0, 7, 1_000_000, i64::MAX];
        let changes: Vec<_> = keys
            .iter()
            .map(|&k| Change {
                key: rowid(k),
                value: Some(b"value".as_slice()),
            })
            .collect();
        t.set(&changes);
        let r = t.env.begin_read();
        let rows = SnapshotRows::open_audited(&r, DATA, 3, Limits::default()).unwrap();
        assert_eq!(rows.audit().blocks, 1);
        assert_eq!(rows.audit().rows, keys.len());
        let frame = r
            .kv(DATA)
            .unwrap()
            .get(&physical(3, &rowid(i64::MIN), GROUP))
            .unwrap()
            .unwrap();
        let mut cursor = rows
            .cursor(
                Bound::Excluded(rowid(-1_000_000)),
                Bound::Included(rowid(1_000_000)),
                false,
            )
            .unwrap();
        let first = cursor.next().unwrap().unwrap();
        for slice in [first.visible_key.as_slice(), first.value] {
            let p = slice.as_ptr() as usize;
            let b = frame.as_ptr() as usize;
            assert!(p >= b && p + slice.len() <= b + frame.len())
        }
        assert_eq!(*first.visible_key, rowid(-1));
        assert_eq!(cursor.current(), Some(first));
        let rest: Vec<_> = cursor.map(|v| *v.unwrap().visible_key).collect();
        assert_eq!(rest, vec![rowid(0), rowid(7), rowid(1_000_000)]);
        assert_eq!(first.value, b"value");
        assert_eq!(*first.visible_key, rowid(-1)); // actual read owns bytes after cursor close
        let reverse: Vec<_> = rows
            .cursor(
                Bound::Included(rowid(-1)),
                Bound::Excluded(rowid(i64::MAX)),
                true,
            )
            .unwrap()
            .map(|r| *r.unwrap().visible_key)
            .collect();
        assert_eq!(
            reverse,
            vec![rowid(1_000_000), rowid(7), rowid(0), rowid(-1)]
        );
        assert!(
            rows.cursor(Bound::Included(rowid(7)), Bound::Excluded(rowid(7)), false)
                .unwrap()
                .next()
                .is_none()
        );
        let visible = rows.exact(&rowid(7)).unwrap().unwrap();
        let key_ptr = visible.visible_key.as_ptr();
        let value_ptr = visible.value.as_ptr();
        rows.exact(&rowid(i64::MIN)).unwrap();
        assert_eq!(
            key_ptr,
            rows.exact(&rowid(7)).unwrap().unwrap().visible_key.as_ptr()
        );
        assert_eq!(value_ptr, visible.value.as_ptr());
    }
    #[test]
    fn sparse_rewrite_split_delete_old_start_and_raw_transitions() {
        let t = Test::new();
        let small = vec![5; 100];
        let keys: Vec<i64> = (0..64).map(|k| k * 1_000_000 - 30_000_000).collect();
        let changes: Vec<_> = keys
            .iter()
            .map(|&k| Change {
                key: rowid(k),
                value: Some(&small),
            })
            .collect();
        t.set(&changes);
        {
            let r = t.env.begin_read();
            let rows = SnapshotRows::open_audited(&r, DATA, 3, Limits::default()).unwrap();
            assert_eq!(rows.audit().blocks, 2);
        }
        let old = physical(3, &rowid(keys[0]), GROUP);
        let large = vec![9; 40_000];
        t.set(&[
            Change {
                key: rowid(keys[0]),
                value: None,
            },
            Change {
                key: rowid(keys[1]),
                value: Some(&large),
            },
            Change {
                key: rowid(keys[2]),
                value: Some(&large),
            },
        ]);
        let r = t.env.begin_read();
        let rows = SnapshotRows::open_audited(&r, DATA, 3, Limits::default()).unwrap();
        assert_eq!(rows.audit().rows, 63);
        assert!(r.kv(DATA).unwrap().get(&old).unwrap().is_none());
        assert_eq!(rows.exact(&rowid(keys[1])).unwrap().unwrap().value, large);
        assert!(rows.exact(&rowid(keys[0])).unwrap().is_none());
        drop(rows);
        drop(r);
        t.set(&[
            Change {
                key: rowid(keys[1]),
                value: Some(&small),
            },
            Change {
                key: rowid(keys[2]),
                value: None,
            },
        ]);
        assert_eq!(t.all().len(), 62);
        let r = t.env.begin_read();
        let rows = SnapshotRows::open_audited(&r, DATA, 3, Limits::default()).unwrap();
        assert_eq!(rows.exact(&rowid(keys[1])).unwrap().unwrap().value, small);
    }
    #[test]
    fn exact_maximum_raw_and_old_snapshot_are_not_copied_into_cache() {
        let t = Test::new();
        let huge = vec![0xab; MAX_RECORD];
        t.set(&[Change {
            key: rowid(0),
            value: Some(&huge),
        }]);
        let r = t.env.begin_read();
        let rows = SnapshotRows::open_audited(&r, DATA, 3, Limits::default()).unwrap();
        let held = rows.exact(&rowid(0)).unwrap().unwrap();
        assert_eq!(held.value.len(), MAX_RECORD);
        assert_eq!(
            held.value.as_ptr(),
            r.kv(DATA)
                .unwrap()
                .get(&physical(3, &rowid(0), RAW))
                .unwrap()
                .unwrap()
                .as_ptr()
        );
        assert!(rows.cache_directory_bytes() < 4096);
        assert_eq!(
            r.kv(DATA)
                .unwrap()
                .get(&physical(3, &rowid(0), RAW))
                .unwrap()
                .unwrap()
                .len(),
            MAX_RECORD
        );
        let small = b"new";
        let changes = [
            Change {
                key: rowid(0),
                value: Some(small),
            },
            Change {
                key: rowid(1),
                value: Some(small),
            },
        ];
        let p = rows.plan(&changes, Limits::default()).unwrap();
        let mut w = t.env.begin_write().unwrap();
        p.apply(&mut w).unwrap();
        w.commit().unwrap();
        assert_eq!(held.value, huge);
        assert_eq!(*held.visible_key, rowid(0));
        drop(p);
        drop(rows);
        drop(r);
        let r = t.env.begin_read();
        assert!(
            r.kv(DATA)
                .unwrap()
                .get(&physical(3, &rowid(0), RAW))
                .unwrap()
                .is_none()
        );
        let rows = SnapshotRows::open_audited(&r, DATA, 3, Limits::default()).unwrap();
        assert_eq!(rows.audit().blocks, 1);
        assert_eq!(rows.audit().rows, 2);
        let above = vec![0; MAX_RECORD + 1];
        assert!(matches!(
            rows.plan(
                &[Change {
                    key: rowid(0),
                    value: Some(&above)
                }],
                Limits::default()
            ),
            Err(Error::Invalid)
        ));
    }
    #[test]
    fn full_audit_rejects_non_neighbor_hidden_overlap_and_limits() {
        let t = Test::new();
        t.set(&[
            Change {
                key: rowid(0),
                value: Some(b"a"),
            },
            Change {
                key: rowid(100),
                value: Some(b"b"),
            },
        ]);
        {
            let mut w = t.env.begin_write().unwrap();
            w.put(DATA, &physical(3, &rowid(1), RAW), b"hidden")
                .unwrap();
            w.commit().unwrap();
        }
        let r = t.env.begin_read();
        assert!(matches!(
            SnapshotRows::open_audited(&r, DATA, 3, Limits::default()),
            Err(Error::Corrupt)
        ));
        let t = Test::new();
        t.set(&[Change {
            key: rowid(0),
            value: Some(b"a"),
        }]);
        let r = t.env.begin_read();
        for limit in [
            Limits {
                audit_blocks: 0,
                ..Limits::default()
            },
            Limits {
                audit_rows: 0,
                ..Limits::default()
            },
            Limits {
                audit_bytes: 1,
                ..Limits::default()
            },
        ] {
            assert!(matches!(
                SnapshotRows::open_audited(&r, DATA, 3, limit),
                Err(Error::Limit)
            ))
        }
    }
    #[test]
    fn malformed_frame_and_limits_never_stage_partial_plan() {
        let t = Test::new();
        let r = t.env.begin_read();
        let rows = SnapshotRows::open_audited(&r, DATA, 3, Limits::default()).unwrap();
        let changes = [Change {
            key: rowid(0),
            value: Some(b"payload"),
        }];
        for limit in [
            Limits {
                changes: 0,
                ..Limits::default()
            },
            Limits {
                operations: 0,
                ..Limits::default()
            },
            Limits {
                encoded_bytes: 1,
                ..Limits::default()
            },
            Limits {
                scratch_bytes: FIXED_SCRATCH - 1,
                ..Limits::default()
            },
        ] {
            assert!(matches!(rows.plan(&changes, limit), Err(Error::Limit)))
        }
        let noop = rows
            .plan(
                &[],
                Limits {
                    operations: 0,
                    ..Limits::default()
                },
            )
            .unwrap();
        assert_eq!(noop.operations(), 0);
        assert_eq!(rows.audit().rows, 0);
        drop(noop);
        drop(rows);
        drop(r);
        {
            let mut w = t.env.begin_write().unwrap();
            w.put(DATA, &physical(3, &rowid(0), 2), b"bad tag").unwrap();
            w.commit().unwrap();
        }
        let r = t.env.begin_read();
        assert!(matches!(
            SnapshotRows::open_audited(&r, DATA, 3, Limits::default()),
            Err(Error::Corrupt)
        ));
    }
    #[test]
    fn sparse_mutations_match_reference_and_apply_does_not_commit() {
        let t = Test::new();
        let mut reference = BTreeMap::new();
        for round in 0..8 {
            let owned: Vec<_> = (0..12)
                .map(|i| {
                    let key = (i * 7 + round * 3) % 41 - 20;
                    let value = if (i + round) % 4 == 0 {
                        None
                    } else {
                        Some(vec![round as u8; (i as usize * 13 + round as usize) % 97])
                    };
                    (key, value)
                })
                .collect();
            let changes_map: BTreeMap<_, _> = owned.into_iter().collect();
            let changes: Vec<_> = changes_map
                .iter()
                .map(|(&key, value)| Change {
                    key: rowid(key),
                    value: value.as_deref(),
                })
                .collect();
            t.set(&changes);
            for (key, value) in changes_map {
                if let Some(value) = value {
                    reference.insert(key, value);
                } else {
                    reference.remove(&key);
                }
            }
            assert_eq!(
                t.all(),
                reference
                    .iter()
                    .map(|(&k, v)| (k, v.clone()))
                    .collect::<Vec<_>>()
            );
        }
        let r = t.env.begin_read();
        let rows = SnapshotRows::open_audited(&r, DATA, 3, Limits::default()).unwrap();
        let changes = [Change {
            key: rowid(99),
            value: Some(b"uncommitted"),
        }];
        let p = rows.plan(&changes, Limits::default()).unwrap();
        {
            let mut w = t.env.begin_write().unwrap();
            p.apply(&mut w).unwrap();
        }
        assert!(rows.exact(&rowid(99)).unwrap().is_none());
        assert_eq!(t.all().len(), reference.len());
    }
    #[test]
    fn physical_byte_budget_group_directory_corruption_and_durable_reopen() {
        let mut t = Test::new();
        let r = t.env.begin_read();
        let rows = SnapshotRows::open_audited(&r, DATA, 3, Limits::default()).unwrap();
        let value = vec![7; 100];
        let changes = [
            Change {
                key: rowid(-5),
                value: Some(&value),
            },
            Change {
                key: rowid(900_000),
                value: Some(&value),
            },
        ];
        // Two rows: 16-byte header, 32-byte directory, 200-byte payload,
        // and the 17-byte physical key all count against native input bytes.
        assert!(matches!(
            rows.plan(
                &changes,
                Limits {
                    encoded_bytes: 264,
                    ..Limits::default()
                }
            ),
            Err(Error::Limit)
        ));
        let plan = rows
            .plan(
                &changes,
                Limits {
                    encoded_bytes: 265,
                    ..Limits::default()
                },
            )
            .unwrap();
        assert_eq!(plan.encoded_bytes(), 265);
        assert_eq!(plan.operations(), 1);
        let mut w = t.env.begin_write().unwrap();
        plan.apply(&mut w).unwrap();
        w.commit().unwrap();
        drop(plan);
        drop(rows);
        drop(r);
        let r = t.env.begin_read();
        let raw = r
            .kv(DATA)
            .unwrap()
            .get(&physical(3, &rowid(-5), GROUP))
            .unwrap()
            .unwrap();
        let mut bad = raw.to_vec();
        bad[HEADER + 8..HEADER + 12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(
            decode(3, &physical(3, &rowid(-5), GROUP), &bad),
            Err(Error::Corrupt)
        ));
        assert!(matches!(
            decode(3, &physical(3, &rowid(-4), GROUP), raw),
            Err(Error::Corrupt)
        ));
        let mut bad = raw.to_vec();
        bad[HEADER + ENTRY..HEADER + ENTRY + 8].copy_from_slice(&rowid(-5));
        assert!(matches!(
            decode(3, &physical(3, &rowid(-5), GROUP), &bad),
            Err(Error::Corrupt)
        ));
        drop(r);
        let path = t.path.clone();
        t.keep = true;
        drop(t);
        let env = Environment::open(
            &path,
            Options {
                index_bytes: 64 << 20,
                journal_capacity: 64 << 20,
                transaction_bytes: (64 << 20) - 4096,
                max_operations: 65536,
                max_databases: 1,
                versioned_values: false,
                ..Default::default()
            },
        )
        .unwrap();
        {
            let r = env.begin_read();
            let rows = SnapshotRows::open_audited(&r, DATA, 3, Limits::default()).unwrap();
            assert_eq!(rows.audit().rows, 2);
            assert_eq!(rows.exact(&rowid(-5)).unwrap().unwrap().value, value);
        }
        drop(env);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn multi_group_ranges_match_signed_order_reference_in_both_directions() {
        use std::ops::RangeBounds;
        let t = Test::new();
        let mut keys: Vec<i64> = (-55..55).map(|k| k * 19).collect();
        keys.extend([i64::MIN, i64::MAX]);
        keys.sort();
        let values: Vec<_> = keys.iter().map(|k| k.to_be_bytes().repeat(8)).collect();
        let changes: Vec<_> = keys
            .iter()
            .zip(&values)
            .map(|(&k, v)| Change {
                key: rowid(k),
                value: Some(v),
            })
            .collect();
        t.set(&changes);
        let reference: BTreeMap<_, _> = keys.iter().copied().zip(values).collect();
        let r = t.env.begin_read();
        let rows = SnapshotRows::open_audited(&r, DATA, 3, Limits::default()).unwrap();
        assert_eq!(rows.audit().blocks, 4);
        let bounds = [
            Bound::Unbounded,
            Bound::Included(i64::MIN),
            Bound::Excluded(i64::MIN),
            Bound::Included(-551),
            Bound::Excluded(0),
            Bound::Included(380),
            Bound::Excluded(1000),
            Bound::Included(i64::MAX),
            Bound::Excluded(i64::MAX),
        ];
        let encode = |b: Bound<i64>| match b {
            Bound::Included(k) => Bound::Included(rowid(k)),
            Bound::Excluded(k) => Bound::Excluded(rowid(k)),
            Bound::Unbounded => Bound::Unbounded,
        };
        for &lo in &bounds {
            for &hi in &bounds {
                let expected: Vec<_> = reference
                    .iter()
                    .filter(|(k, _)| (lo, hi).contains(k))
                    .map(|(&k, v)| (k, v.clone()))
                    .collect();
                for reverse in [false, true] {
                    let mut wanted = expected.clone();
                    if reverse {
                        wanted.reverse()
                    }
                    let actual: Vec<_> = rows
                        .cursor(encode(lo), encode(hi), reverse)
                        .unwrap()
                        .map(|row| {
                            let row = row.unwrap();
                            (
                                ((u64::from_be_bytes(*row.visible_key)) ^ (1 << 63)) as i64,
                                row.value.to_vec(),
                            )
                        })
                        .collect();
                    assert_eq!(actual, wanted, "lo={lo:?} hi={hi:?} reverse={reverse}");
                }
            }
        }
    }
    #[test]
    fn audited_bridge_requires_same_live_read_and_borrows_external_owner() {
        let t = Test::new();
        t.set(&[Change {
            key: rowid(7),
            value: Some(b"snapshot"),
        }]);
        let first = t.env.begin_read();
        let other = t.env.begin_read();
        let rows = SnapshotRows::open_audited(&first, DATA, 3, Limits::default()).unwrap();
        assert!(matches!(rows.for_read(&other, DATA), Err(Error::Invalid)));
        assert!(matches!(
            rows.for_read(&first, b"foreign database"),
            Err(Error::Invalid)
        ));
        let retained = rows
            .for_read(&first, DATA)
            .unwrap()
            .exact(&rowid(7))
            .unwrap()
            .unwrap();
        let mut cursor = rows
            .cursor_for(&first, DATA, Bound::Unbounded, Bound::Unbounded, false)
            .unwrap();
        assert_eq!(cursor.next().unwrap().unwrap(), retained);
        drop(cursor);
        drop(rows);
        assert_eq!(retained.value, b"snapshot");
        assert_eq!(*retained.visible_key, rowid(7));
    }
}
