//! Persistent ordered index for the opt-in snapshot log.
//!
//! The index owns immutable published nodes. A private prepare clones a node
//! only on its first edit; subsequent edits reuse that private node. Values and
//! keys up to 16 bytes stay inline. Larger retained bytes have one shared quota
//! charge, released with their final owner. Old roots never form version chains.
//!
//! Quota includes retained tree/byte capacity and caller-held metadata tickets.
//! It is requested retained capacity, not allocator usable size or RSS.
//! Caller-owned Change buffers, Index handles and iterator stack are separate.
//! Deletion prunes empty nodes and collapses roots; underfull nonempty pages are
//! retained. Their fragmentation is bounded by the same quota, not hidden debt.
use crate::{Error, Result};
use std::cell::Cell;
use std::cmp::Ordering;
use std::mem::size_of;
use std::ops::Bound;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering as AtomicOrdering},
};

const INLINE: usize = 16;
const LEAF: usize = 32;
const BRANCH: usize = 16;
const DEPTH: usize = 32;
const ARC_HEADER: usize = 2 * size_of::<usize>();

#[derive(Clone)]
pub struct Bytes(Repr);
#[derive(Clone)]
enum Repr {
    Inline { length: u8, bytes: [u8; INLINE] },
    Large(Arc<Large>),
    Staged(ulmdb_storage::snapshot_input::Slice),
}
struct Large {
    bytes: Box<[u8]>,
    charge: Option<Charge>,
}
impl std::fmt::Debug for Bytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Bytes").field(&self.as_slice()).finish()
    }
}
impl Bytes {
    /// Stage common medium values without a separate Box and Arc per row.
    /// This representation is never admitted directly into published pages.
    pub(super) fn staged(
        bytes: &[u8],
        arena: &mut ulmdb_storage::snapshot_input::Arena,
    ) -> Result<Self> {
        if bytes.len() > INLINE && bytes.len() <= 64 {
            match arena.copy(bytes) {
                Ok(slice) => return Ok(Self(Repr::Staged(slice))),
                Err(Error::LimitExceeded) => {}
                Err(error) => return Err(error),
            }
        }
        Self::new(bytes)
    }
    pub fn new(bytes: &[u8]) -> Result<Self> {
        if bytes.len() <= INLINE {
            let mut inline = [0; INLINE];
            inline[..bytes.len()].copy_from_slice(bytes);
            return Ok(Self(Repr::Inline {
                length: bytes.len() as u8,
                bytes: inline,
            }));
        }
        Ok(Self(Repr::Large(Arc::new(Large {
            bytes: copy_bytes(bytes)?,
            charge: None,
        }))))
    }
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        match &self.0 {
            Repr::Inline { length, bytes } => &bytes[..usize::from(*length)],
            Repr::Large(large) => &large.bytes,
            Repr::Staged(slice) => slice.as_slice(),
        }
    }
    fn admit(&self, quota: &Arc<Quota>) -> Result<Self> {
        match &self.0 {
            Repr::Inline { .. } => Ok(self.clone()),
            Repr::Large(large) => {
                if large
                    .charge
                    .as_ref()
                    .is_some_and(|c| Arc::ptr_eq(&c.quota, quota))
                {
                    return Ok(self.clone());
                }
                let bytes = size_of::<Large>()
                    .checked_add(ARC_HEADER)
                    .and_then(|n| n.checked_add(large.bytes.len()))
                    .ok_or(Error::LimitExceeded)?;
                let charge = Charge::new(quota, bytes, Kind::Large)?;
                let bytes = copy_bytes(&large.bytes)?;
                Ok(Self(Repr::Large(Arc::new(Large {
                    bytes,
                    charge: Some(charge),
                }))))
            }
            Repr::Staged(slice) => {
                let raw = slice.as_slice();
                let bytes = size_of::<Large>()
                    .checked_add(ARC_HEADER)
                    .and_then(|n| n.checked_add(raw.len()))
                    .ok_or(Error::LimitExceeded)?;
                let charge = Charge::new(quota, bytes, Kind::Large)?;
                let bytes = copy_bytes(raw)?;
                Ok(Self(Repr::Large(Arc::new(Large {
                    bytes,
                    charge: Some(charge),
                }))))
            }
        }
    }
}
fn copy_bytes(bytes: &[u8]) -> Result<Box<[u8]>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len())
        .map_err(|_| Error::LimitExceeded)?;
    copy.extend_from_slice(bytes);
    Ok(copy.into_boxed_slice())
}

#[derive(Clone, Debug)]
pub struct Change {
    pub database: u64,
    pub key: Bytes,
    pub value: Option<Bytes>,
}
#[derive(Clone)]
struct Key {
    database: u64,
    bytes: Bytes,
}
impl Key {
    fn compare(&self, database: u64, key: &[u8]) -> Ordering {
        self.database
            .cmp(&database)
            .then_with(|| self.bytes.as_slice().cmp(key))
    }
}
#[derive(Clone)]
struct Entry {
    key: Key,
    value: Bytes,
}
#[derive(Clone)]
struct Child {
    maximum: Key,
    node: Arc<Node>,
}
enum Body {
    Leaf(Vec<Entry>),
    Branch(Vec<Child>),
}
struct Node {
    body: Body,
    _charge: Charge,
}
impl Node {
    fn maximum(&self) -> &Key {
        match &self.body {
            Body::Leaf(entries) => &entries.last().expect("nonempty published leaf").key,
            Body::Branch(children) => &children.last().expect("nonempty published branch").maximum,
        }
    }
    fn is_empty(&self) -> bool {
        match &self.body {
            Body::Leaf(entries) => entries.is_empty(),
            Body::Branch(children) => children.is_empty(),
        }
    }
}

struct Quota {
    limit: usize,
    charged: AtomicUsize,
    peak: AtomicUsize,
    nodes: AtomicUsize,
    large_bytes: AtomicUsize,
    large_allocations: AtomicUsize,
}
#[derive(Clone, Copy)]
enum Kind {
    Node,
    Large,
    Metadata,
}
struct Charge {
    quota: Arc<Quota>,
    bytes: usize,
    kind: Kind,
}
impl Quota {
    fn reserve(&self, bytes: usize) -> Result<()> {
        let previous = self
            .charged
            .fetch_update(
                AtomicOrdering::Relaxed,
                AtomicOrdering::Relaxed,
                |current| {
                    current
                        .checked_add(bytes)
                        .filter(|next| *next <= self.limit)
                },
            )
            .map_err(|_| Error::LimitExceeded)?;
        self.peak
            .fetch_max(previous + bytes, AtomicOrdering::Relaxed);
        Ok(())
    }
}
impl Charge {
    fn new(quota: &Arc<Quota>, bytes: usize, kind: Kind) -> Result<Self> {
        quota.reserve(bytes)?;
        match kind {
            Kind::Node => {
                quota.nodes.fetch_add(1, AtomicOrdering::Relaxed);
            }
            Kind::Large => {
                quota
                    .large_allocations
                    .fetch_add(1, AtomicOrdering::Relaxed);
                quota.large_bytes.fetch_add(bytes, AtomicOrdering::Relaxed);
            }
            Kind::Metadata => {}
        }
        Ok(Self {
            quota: quota.clone(),
            bytes,
            kind,
        })
    }
}
impl Drop for Charge {
    fn drop(&mut self) {
        self.quota
            .charged
            .fetch_sub(self.bytes, AtomicOrdering::Relaxed);
        match self.kind {
            Kind::Node => {
                self.quota.nodes.fetch_sub(1, AtomicOrdering::Relaxed);
            }
            Kind::Large => {
                self.quota
                    .large_allocations
                    .fetch_sub(1, AtomicOrdering::Relaxed);
                self.quota
                    .large_bytes
                    .fetch_sub(self.bytes, AtomicOrdering::Relaxed);
            }
            Kind::Metadata => {}
        }
    }
}

/// Shared-quota retained metadata capacity. Own this ticket inside the exact
/// Snapshot/catalog allocation it covers so final-owner drop returns capacity.
/// The ticket is deliberately not Clone: sharing its containing Arc shares one
/// allocation; cloning that allocation requires a new reservation first.
pub(super) struct MetadataReservation {
    charge: Charge,
}
impl std::fmt::Debug for MetadataReservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetadataReservation")
            .field("bytes", &self.bytes())
            .finish()
    }
}
impl MetadataReservation {
    pub(super) fn bytes(&self) -> usize {
        self.charge.bytes
    }

    /// Grow before allocating/mutating the corresponding retained object.
    /// Refusal changes neither the ticket nor the live quota charge. Shrink
    /// only after the covered capacity has actually been relinquished.
    pub(super) fn resize(&mut self, new_bytes: usize) -> Result<()> {
        let previous = self.charge.bytes;
        if new_bytes > previous {
            self.charge.quota.reserve(new_bytes - previous)?;
        } else if new_bytes < previous {
            self.charge
                .quota
                .charged
                .fetch_sub(previous - new_bytes, AtomicOrdering::Relaxed);
        }
        self.charge.bytes = new_bytes;
        Ok(())
    }
}

#[derive(Clone)]
pub struct Index {
    root: Option<Arc<Node>>,
    quota: Arc<Quota>,
    entries: usize,
    encoded_rows: usize,
}
#[derive(Debug, Clone, Copy)]
pub struct Stats {
    pub limit_bytes: usize,
    pub charged_bytes: usize,
    pub peak_bytes: usize,
    pub nodes: usize,
    /// Includes each large allocation's header, Arc control and byte payload.
    pub large_bytes: usize,
    pub large_allocations: usize,
    pub entries: usize,
    pub height: usize,
    pub index_handle_bytes: usize,
    pub iterator_scratch_bytes: usize,
}
impl std::fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Index")
            .field("stats", &self.stats())
            .finish()
    }
}
impl Index {
    pub fn new(max_bytes: usize) -> Result<Self> {
        let shared = size_of::<Quota>() + ARC_HEADER;
        if max_bytes < shared {
            return Err(Error::LimitExceeded);
        }
        Ok(Self {
            root: None,
            entries: 0,
            encoded_rows: 0,
            quota: Arc::new(Quota {
                limit: max_bytes,
                charged: AtomicUsize::new(shared),
                peak: AtomicUsize::new(shared),
                nodes: AtomicUsize::new(0),
                large_bytes: AtomicUsize::new(0),
                large_allocations: AtomicUsize::new(0),
            }),
        })
    }
    /// Charge retained snapshot/catalog metadata to the same limit as data.
    pub(super) fn reserve_metadata(&self, bytes: usize) -> Result<MetadataReservation> {
        Ok(MetadataReservation {
            charge: Charge::new(&self.quota, bytes, Kind::Metadata)?,
        })
    }
    fn find(&self, database: u64, key: &[u8]) -> Option<&Entry> {
        let mut node = self.root.as_deref()?;
        loop {
            match &node.body {
                Body::Leaf(entries) => {
                    return entries
                        .binary_search_by(|entry| entry.key.compare(database, key))
                        .ok()
                        .map(|i| &entries[i]);
                }
                Body::Branch(children) => {
                    let index = children.partition_point(|child| {
                        child.maximum.compare(database, key) == Ordering::Less
                    });
                    node = children.get(index)?.node.as_ref();
                }
            }
        }
    }
    pub fn get(&self, database: u64, key: &[u8]) -> Option<&[u8]> {
        self.find(database, key).map(|entry| entry.value.as_slice())
    }
    pub fn len(&self) -> usize {
        self.entries
    }
    pub fn is_empty(&self) -> bool {
        self.entries == 0
    }
    /// Exact serialized row bytes: database/length fields plus keys/values.
    pub fn encoded_rows_bytes(&self) -> usize {
        self.encoded_rows
    }

    /// Logical no-op identity without walking the tree.
    pub fn same_state(&self, other: &Self) -> bool {
        match (&self.root, &other.root) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Existing point-edit route, retained as the small-transaction control.
    pub fn prepare(&self, changes: &[Change]) -> Result<Self> {
        let mut next = self.clone();
        for change in changes {
            if let Some(value) = &change.value {
                let prior = next.find(change.database, change.key.as_slice());
                if prior.is_some_and(|entry| entry.value.as_slice() == value.as_slice()) {
                    continue;
                }
                let existed = prior.is_some();
                let old_bytes = prior.map_or(Ok(0), entry_bytes)?;
                let key = match prior {
                    Some(entry) => entry.key.clone(),
                    None => Key {
                        database: change.database,
                        bytes: change.key.admit(&next.quota)?,
                    },
                };
                let entry = Entry {
                    key,
                    value: value.admit(&next.quota)?,
                };
                let row_bytes = entry_bytes(&entry)?;
                if let Some(root) = next.root.as_mut() {
                    if let Some(right) = insert(root, entry, &next.quota, 1)? {
                        let mut parent = branch_node(&next.quota)?;
                        let Body::Branch(children) = &mut parent.body else {
                            unreachable!()
                        };
                        children.push(Child {
                            maximum: root.maximum().clone(),
                            node: root.clone(),
                        });
                        children.push(Child {
                            maximum: right.maximum().clone(),
                            node: right,
                        });
                        next.root = Some(Arc::new(parent));
                    }
                } else {
                    let mut node = leaf_node(&next.quota)?;
                    let Body::Leaf(entries) = &mut node.body else {
                        unreachable!()
                    };
                    entries.push(entry);
                    next.root = Some(Arc::new(node));
                }
                if !existed {
                    next.entries = next.entries.checked_add(1).ok_or(Error::LimitExceeded)?;
                }
                next.encoded_rows = next
                    .encoded_rows
                    .checked_sub(old_bytes)
                    .and_then(|n| n.checked_add(row_bytes))
                    .ok_or(Error::LimitExceeded)?;
            } else if let Some(prior) = next.find(change.database, change.key.as_slice()) {
                let old_bytes = entry_bytes(prior)?;
                let root = next.root.as_mut().expect("present key has root");
                remove(root, change.database, change.key.as_slice(), &next.quota, 1)?;
                next.entries -= 1;
                next.encoded_rows = next
                    .encoded_rows
                    .checked_sub(old_bytes)
                    .ok_or(Error::LimitExceeded)?;
                if root.is_empty() {
                    next.root = None;
                }
                collapse_root(&mut next.root);
            }
        }
        Ok(next)
    }

    /// Select grouped preparation only at the caller's declared batch boundary.
    /// usize::MAX disables it; zero/one force it for every nonempty change set.
    pub fn prepare_hybrid(&self, changes: &[Change], threshold: usize) -> Result<Self> {
        if !changes.is_empty() && threshold != usize::MAX && changes.len() >= threshold {
            self.prepare_grouped(changes)
        } else {
            self.prepare(changes)
        }
    }

    /// Sort/coalesce effects, visit each affected old branch/leaf once, merge
    /// each affected leaf once, then rebuild its changed ancestors once. This
    /// changes only private construction; published snapshots keep exact bytes.
    pub fn prepare_grouped(&self, changes: &[Change]) -> Result<Self> {
        if changes.is_empty() {
            return Ok(self.clone());
        }
        if changes.len() > 1_048_576 {
            return Err(Error::LimitExceeded);
        }
        // Upfront conservative frontier envelope. Every actual scratch Vec
        // also meters requested capacity against it before allocation, so a
        // more fragmented shape refuses rather than exceeding this bound.
        let per_change = size_of::<usize>()
            + 2 * size_of::<Entry>()
            + 8 * size_of::<Child>()
            + 8 * size_of::<Arc<Node>>();
        let frontier = (self.stats().height + 2)
            .checked_mul(
                8 * BRANCH * size_of::<Child>()
                    + 4 * LEAF * size_of::<Entry>()
                    + 16 * size_of::<Arc<Node>>(),
            )
            .ok_or(Error::LimitExceeded)?;
        let scratch_bytes = changes
            .len()
            .checked_mul(per_change)
            .and_then(|n| n.checked_add(frontier))
            .ok_or(Error::LimitExceeded)?;
        let scratch = Scratch {
            maximum: scratch_bytes,
            used: Cell::new(0),
            _reservation: self.reserve_metadata(scratch_bytes)?,
        };
        let mut order = ScratchVec::with_capacity(changes.len(), &scratch)?;
        for index in 0..changes.len() {
            order.push(index)?;
        }
        order.values.sort_unstable_by(|a, b| {
            change_compare(&changes[*a], &changes[*b]).then_with(|| b.cmp(a))
        });
        // Descending input position within a key keeps its final effect first.
        order
            .values
            .dedup_by(|a, b| change_compare(&changes[*a], &changes[*b]) == Ordering::Equal);
        let mut accounting = GroupAccounting::default();
        let mut roots = ScratchVec::with_capacity(2, &scratch)?;
        grouped_edit(
            self.root.as_ref(),
            &order.values,
            changes,
            &self.quota,
            &scratch,
            &mut accounting,
            1,
            &mut roots,
        )?;
        let mut height = self.stats().height.max(1);
        while roots.values.len() > 1 {
            height = height.checked_add(1).ok_or(Error::LimitExceeded)?;
            if height >= DEPTH {
                return Err(Error::LimitExceeded);
            }
            let mut parents =
                ScratchVec::with_capacity(roots.values.len().div_ceil(BRANCH), &scratch)?;
            pack_branches(&mut roots, &self.quota, &mut parents)?;
            roots = parents;
        }
        let mut root = roots.values.pop().map(|child| child.node);
        collapse_root(&mut root);
        Ok(Self {
            root,
            quota: self.quota.clone(),
            entries: self
                .entries
                .checked_sub(accounting.before_entries)
                .and_then(|n| n.checked_add(accounting.after_entries))
                .ok_or(Error::LimitExceeded)?,
            encoded_rows: self
                .encoded_rows
                .checked_sub(accounting.before_bytes)
                .and_then(|n| n.checked_add(accounting.after_bytes))
                .ok_or(Error::LimitExceeded)?,
        })
    }
    pub fn iter(&self) -> Iter<'_> {
        Iter {
            front: Cursor::new(self.root.as_deref(), false),
            back: Cursor::new(self.root.as_deref(), true),
            remaining: self.entries,
        }
    }
    /// Seeks both ends of one database's key range without retaining the bounds.
    ///
    /// Construction visits only the search paths. Iteration borrows this root's
    /// immutable entries, so advancing either end requires no bound comparison
    /// or allocation. Reversed or empty bounds produce an empty iterator.
    pub fn range(&self, database: u64, lower: Bound<&[u8]>, upper: Bound<&[u8]>) -> RangeIter<'_> {
        let mut front = Cursor::seek(self.root.as_deref(), database, lower, false);
        let mut back = Cursor::seek(self.root.as_deref(), database, upper, true);
        let first = front.next();
        let last = back.next();
        let (first, last) = match (first, last) {
            (Some(first), Some(last))
                if first
                    .key
                    .compare(last.key.database, last.key.bytes.as_slice())
                    != Ordering::Greater =>
            {
                (Some(first), Some(last))
            }
            _ => (None, None),
        };
        RangeIter {
            front,
            back,
            first,
            last,
        }
    }
    /// A cursor for one direction. An unbounded terminal end needs no opposite
    /// search path: crossing the database namespace terminates iteration.
    pub fn scan(
        &self,
        database: u64,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> ScanIter<'_> {
        let (start, terminal) = if reverse {
            (upper, lower)
        } else {
            (lower, upper)
        };
        let mut cursor = Cursor::seek(self.root.as_deref(), database, start, reverse);
        let first = cursor.next().filter(|entry| entry.key.database == database);
        let (first, terminal) = match terminal {
            Bound::Unbounded => (first, None),
            bound => {
                let mut end = Cursor::seek(self.root.as_deref(), database, bound, !reverse);
                match (first, end.next()) {
                    (Some(first), Some(last)) if last.key.database == database => {
                        let order = first
                            .key
                            .compare(last.key.database, last.key.bytes.as_slice());
                        if (!reverse && order != Ordering::Greater)
                            || (reverse && order != Ordering::Less)
                        {
                            (Some(first), Some(last))
                        } else {
                            (None, None)
                        }
                    }
                    _ => (None, None),
                }
            }
        };
        ScanIter {
            cursor,
            first,
            terminal,
            database,
        }
    }
    pub fn stats(&self) -> Stats {
        let mut height = 0;
        let mut node = self.root.as_deref();
        while let Some(current) = node {
            height += 1;
            node = match &current.body {
                Body::Leaf(_) => None,
                Body::Branch(children) => children.first().map(|child| child.node.as_ref()),
            };
        }
        Stats {
            limit_bytes: self.quota.limit,
            charged_bytes: self.quota.charged.load(AtomicOrdering::Relaxed),
            peak_bytes: self.quota.peak.load(AtomicOrdering::Relaxed),
            nodes: self.quota.nodes.load(AtomicOrdering::Relaxed),
            large_bytes: self.quota.large_bytes.load(AtomicOrdering::Relaxed),
            large_allocations: self.quota.large_allocations.load(AtomicOrdering::Relaxed),
            entries: self.entries,
            height,
            index_handle_bytes: size_of::<Self>(),
            iterator_scratch_bytes: size_of::<Iter<'static>>(),
        }
    }
}
fn entry_bytes(entry: &Entry) -> Result<usize> {
    16usize
        .checked_add(entry.key.bytes.as_slice().len())
        .and_then(|n| n.checked_add(entry.value.as_slice().len()))
        .ok_or(Error::LimitExceeded)
}
fn collapse_root(root: &mut Option<Arc<Node>>) {
    loop {
        let replacement = match root.as_deref().map(|node| &node.body) {
            Some(Body::Branch(children)) if children.len() == 1 => Some(children[0].node.clone()),
            _ => None,
        };
        match replacement {
            Some(next) => *root = Some(next),
            None => break,
        }
    }
}
fn change_compare(a: &Change, b: &Change) -> Ordering {
    a.database
        .cmp(&b.database)
        .then_with(|| a.key.as_slice().cmp(b.key.as_slice()))
}
struct Scratch {
    maximum: usize,
    used: Cell<usize>,
    _reservation: MetadataReservation,
}
impl Scratch {
    fn reserve(&self, bytes: usize) -> Result<()> {
        let next = self
            .used
            .get()
            .checked_add(bytes)
            .filter(|n| *n <= self.maximum)
            .ok_or(Error::LimitExceeded)?;
        self.used.set(next);
        Ok(())
    }
    fn release(&self, bytes: usize) {
        self.used.set(self.used.get() - bytes);
    }
}
struct ScratchVec<'a, T> {
    values: Vec<T>,
    scratch: &'a Scratch,
    charged: usize,
}
impl<'a, T> ScratchVec<'a, T> {
    fn with_capacity(capacity: usize, scratch: &'a Scratch) -> Result<Self> {
        let charged = capacity
            .checked_mul(size_of::<T>())
            .ok_or(Error::LimitExceeded)?;
        scratch.reserve(charged)?;
        let mut values = Vec::new();
        if values.try_reserve_exact(capacity).is_err() {
            scratch.release(charged);
            return Err(Error::LimitExceeded);
        }
        Ok(Self {
            values,
            scratch,
            charged,
        })
    }
    fn push(&mut self, value: T) -> Result<()> {
        if self.values.len() == self.values.capacity() {
            let capacity = self
                .values
                .capacity()
                .checked_mul(2)
                .ok_or(Error::LimitExceeded)?
                .max(4);
            let additional = capacity
                .checked_sub(self.values.len())
                .ok_or(Error::LimitExceeded)?;
            let bytes = additional
                .checked_mul(size_of::<T>())
                .ok_or(Error::LimitExceeded)?;
            self.scratch.reserve(bytes)?;
            if self.values.try_reserve_exact(additional).is_err() {
                self.scratch.release(bytes);
                return Err(Error::LimitExceeded);
            }
            self.charged += bytes;
        }
        self.values.push(value);
        Ok(())
    }
}
impl<T> Drop for ScratchVec<'_, T> {
    fn drop(&mut self) {
        self.scratch.release(self.charged);
    }
}
#[derive(Default)]
struct GroupAccounting {
    before_entries: usize,
    after_entries: usize,
    before_bytes: usize,
    after_bytes: usize,
}
impl GroupAccounting {
    fn add(
        &mut self,
        before: usize,
        after: usize,
        before_bytes: usize,
        after_bytes: usize,
    ) -> Result<()> {
        self.before_entries = self
            .before_entries
            .checked_add(before)
            .ok_or(Error::LimitExceeded)?;
        self.after_entries = self
            .after_entries
            .checked_add(after)
            .ok_or(Error::LimitExceeded)?;
        self.before_bytes = self
            .before_bytes
            .checked_add(before_bytes)
            .ok_or(Error::LimitExceeded)?;
        self.after_bytes = self
            .after_bytes
            .checked_add(after_bytes)
            .ok_or(Error::LimitExceeded)?;
        Ok(())
    }
}
/// Merge descriptors borrow their inputs. The first pass counts and detects
/// real changes without cloning bytes or allocating a complete merged leaf.
/// The second pass admits each retained entry directly into its final node.
enum MergeStep<'a> {
    Keep(&'a Entry),
    Insert(&'a Change),
    Match(&'a Entry, &'a Change),
}
struct LeafMerge<'a> {
    old: &'a [Entry],
    order: &'a [usize],
    changes: &'a [Change],
    old_at: usize,
    change_at: usize,
}
impl<'a> LeafMerge<'a> {
    fn new(old: &'a [Entry], order: &'a [usize], changes: &'a [Change]) -> Self {
        Self {
            old,
            order,
            changes,
            old_at: 0,
            change_at: 0,
        }
    }
    fn step(&mut self) -> Option<MergeStep<'a>> {
        let old = self.old.get(self.old_at);
        let change = self
            .order
            .get(self.change_at)
            .map(|offset| &self.changes[*offset]);
        let comparison = match (old, change) {
            (Some(old), Some(change)) => old.key.compare(change.database, change.key.as_slice()),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => return None,
        };
        Some(match comparison {
            Ordering::Less => {
                self.old_at += 1;
                MergeStep::Keep(old.expect("pending old entry"))
            }
            Ordering::Greater => {
                self.change_at += 1;
                MergeStep::Insert(change.expect("pending change"))
            }
            Ordering::Equal => {
                self.old_at += 1;
                self.change_at += 1;
                MergeStep::Match(
                    old.expect("matching old entry"),
                    change.expect("matching change"),
                )
            }
        })
    }
    fn entry(&mut self, quota: &Arc<Quota>) -> Result<Option<Entry>> {
        while let Some(step) = self.step() {
            let entry = match step {
                MergeStep::Keep(old) => old.clone(),
                MergeStep::Insert(change) => {
                    let Some(value) = &change.value else { continue };
                    Entry {
                        key: Key {
                            database: change.database,
                            bytes: change.key.admit(quota)?,
                        },
                        value: value.admit(quota)?,
                    }
                }
                MergeStep::Match(old, change) => {
                    let Some(value) = &change.value else { continue };
                    if value.as_slice() == old.value.as_slice() {
                        old.clone()
                    } else {
                        Entry {
                            key: old.key.clone(),
                            value: value.admit(quota)?,
                        }
                    }
                }
            };
            return Ok(Some(entry));
        }
        Ok(None)
    }
}
fn emit_node(output: &mut ScratchVec<'_, Child>, node: Arc<Node>) -> Result<()> {
    output.push(Child {
        maximum: node.maximum().clone(),
        node,
    })
}
fn pack_branches(
    children: &mut ScratchVec<'_, Child>,
    quota: &Arc<Quota>,
    output: &mut ScratchVec<'_, Child>,
) -> Result<()> {
    let count = children.values.len().div_ceil(BRANCH);
    if count == 0 {
        return Ok(());
    }
    let quotient = children.values.len() / count;
    let remainder = children.values.len() % count;
    let mut source = std::mem::take(&mut children.values).into_iter();
    for group in 0..count {
        let mut node = branch_node(quota)?;
        let Body::Branch(target) = &mut node.body else {
            unreachable!()
        };
        for _ in 0..quotient + usize::from(group < remainder) {
            target.push(source.next().expect("partitioned children"));
        }
        emit_node(output, Arc::new(node))?;
    }
    drop(source);
    Ok(())
}
fn grouped_edit<'a>(
    node: Option<&Arc<Node>>,
    order: &[usize],
    changes: &[Change],
    quota: &Arc<Quota>,
    scratch: &'a Scratch,
    accounting: &mut GroupAccounting,
    depth: usize,
    output: &mut ScratchVec<'_, Child>,
) -> Result<bool> {
    if depth >= DEPTH {
        return Err(Error::LimitExceeded);
    }
    if order.is_empty() {
        if let Some(node) = node {
            emit_node(output, node.clone())?;
        }
        return Ok(false);
    }
    if let Some(node) = node {
        if let Body::Branch(children) = &node.body {
            let mut replacement = ScratchVec::with_capacity(children.len(), scratch)?;
            let mut start = 0;
            let mut changed = false;
            for (index, child) in children.iter().enumerate() {
                let end = if index + 1 == children.len() {
                    order.len()
                } else {
                    start
                        + order[start..].partition_point(|offset| {
                            child
                                .maximum
                                .compare(changes[*offset].database, changes[*offset].key.as_slice())
                                != Ordering::Less
                        })
                };
                if start == end {
                    replacement.push(child.clone())?;
                } else {
                    changed |= grouped_edit(
                        Some(&child.node),
                        &order[start..end],
                        changes,
                        quota,
                        scratch,
                        accounting,
                        depth + 1,
                        &mut replacement,
                    )?;
                }
                start = end;
            }
            if !changed {
                emit_node(output, node.clone())?;
                return Ok(false);
            }
            pack_branches(&mut replacement, quota, output)?;
            return Ok(true);
        }
    }
    let old = match node.map(|node| &node.body) {
        Some(Body::Leaf(entries)) => entries.as_slice(),
        None => &[],
        _ => unreachable!(),
    };
    let (mut before_bytes, mut after_bytes) = (0usize, 0usize);
    let mut after_entries = 0usize;
    let mut changed = false;
    let mut summary = LeafMerge::new(old, order, changes);
    while let Some(step) = summary.step() {
        let (before, after) = match step {
            MergeStep::Keep(entry) => (Some(entry), Some((&entry.key.bytes, &entry.value))),
            MergeStep::Insert(change) => {
                changed |= change.value.is_some();
                (
                    None,
                    change.value.as_ref().map(|value| (&change.key, value)),
                )
            }
            MergeStep::Match(entry, change) => {
                changed |=
                    change.value.as_ref().map(Bytes::as_slice) != Some(entry.value.as_slice());
                (
                    Some(entry),
                    change.value.as_ref().map(|value| (&entry.key.bytes, value)),
                )
            }
        };
        if let Some(entry) = before {
            before_bytes = before_bytes
                .checked_add(entry_bytes(entry)?)
                .ok_or(Error::LimitExceeded)?;
        }
        if let Some((key, value)) = after {
            let bytes = 16usize
                .checked_add(key.as_slice().len())
                .and_then(|n| n.checked_add(value.as_slice().len()))
                .ok_or(Error::LimitExceeded)?;
            after_bytes = after_bytes.checked_add(bytes).ok_or(Error::LimitExceeded)?;
            after_entries = after_entries.checked_add(1).ok_or(Error::LimitExceeded)?;
        }
    }
    if !changed {
        if let Some(node) = node {
            emit_node(output, node.clone())?;
        }
        return Ok(false);
    }
    accounting.add(old.len(), after_entries, before_bytes, after_bytes)?;
    let count = after_entries.div_ceil(LEAF);
    if count != 0 {
        let quotient = after_entries / count;
        let remainder = after_entries % count;
        let mut source = LeafMerge::new(old, order, changes);
        for group in 0..count {
            let mut node = leaf_node(quota)?;
            let Body::Leaf(target) = &mut node.body else {
                unreachable!()
            };
            for _ in 0..quotient + usize::from(group < remainder) {
                target.push(source.entry(quota)?.expect("counted merged entry"));
            }
            emit_node(output, Arc::new(node))?;
        }
        debug_assert!(source.entry(quota)?.is_none());
    }
    Ok(true)
}
fn leaf_node(quota: &Arc<Quota>) -> Result<Node> {
    let charge = Charge::new(
        quota,
        size_of::<Node>() + ARC_HEADER + (LEAF + 1) * size_of::<Entry>(),
        Kind::Node,
    )?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(LEAF + 1)
        .map_err(|_| Error::LimitExceeded)?;
    Ok(Node {
        body: Body::Leaf(entries),
        _charge: charge,
    })
}
fn branch_node(quota: &Arc<Quota>) -> Result<Node> {
    let charge = Charge::new(
        quota,
        size_of::<Node>() + ARC_HEADER + (BRANCH + 1) * size_of::<Child>(),
        Kind::Node,
    )?;
    let mut children = Vec::new();
    children
        .try_reserve_exact(BRANCH + 1)
        .map_err(|_| Error::LimitExceeded)?;
    Ok(Node {
        body: Body::Branch(children),
        _charge: charge,
    })
}
fn private<'a>(node: &'a mut Arc<Node>, quota: &Arc<Quota>) -> Result<&'a mut Node> {
    if Arc::get_mut(node).is_none() {
        let copy = match &node.body {
            Body::Leaf(entries) => {
                let mut copy = leaf_node(quota)?;
                let Body::Leaf(output) = &mut copy.body else {
                    unreachable!()
                };
                output.extend(entries.iter().cloned());
                copy
            }
            Body::Branch(children) => {
                let mut copy = branch_node(quota)?;
                let Body::Branch(output) = &mut copy.body else {
                    unreachable!()
                };
                output.extend(children.iter().cloned());
                copy
            }
        };
        *node = Arc::new(copy);
    }
    Ok(Arc::get_mut(node).expect("private node"))
}
fn insert(
    node: &mut Arc<Node>,
    entry: Entry,
    quota: &Arc<Quota>,
    depth: usize,
) -> Result<Option<Arc<Node>>> {
    if depth >= DEPTH {
        return Err(Error::LimitExceeded);
    }
    let node = private(node, quota)?;
    match &mut node.body {
        Body::Leaf(entries) => {
            match entries.binary_search_by(|old| {
                old.key
                    .compare(entry.key.database, entry.key.bytes.as_slice())
            }) {
                Ok(index) => entries[index].value = entry.value,
                Err(index) => entries.insert(index, entry),
            }
            if entries.len() <= LEAF {
                return Ok(None);
            }
            let mut sibling = leaf_node(quota)?;
            let Body::Leaf(right) = &mut sibling.body else {
                unreachable!()
            };
            right.extend(entries.drain(entries.len() / 2..));
            Ok(Some(Arc::new(sibling)))
        }
        Body::Branch(children) => {
            let index = children
                .partition_point(|child| {
                    child
                        .maximum
                        .compare(entry.key.database, entry.key.bytes.as_slice())
                        == Ordering::Less
                })
                .min(children.len() - 1);
            let right = insert(&mut children[index].node, entry, quota, depth + 1)?;
            children[index].maximum = children[index].node.maximum().clone();
            if let Some(right) = right {
                children.insert(
                    index + 1,
                    Child {
                        maximum: right.maximum().clone(),
                        node: right,
                    },
                );
            }
            if children.len() <= BRANCH {
                return Ok(None);
            }
            let mut sibling = branch_node(quota)?;
            let Body::Branch(right) = &mut sibling.body else {
                unreachable!()
            };
            right.extend(children.drain(children.len() / 2..));
            Ok(Some(Arc::new(sibling)))
        }
    }
}
fn remove(
    node: &mut Arc<Node>,
    database: u64,
    key: &[u8],
    quota: &Arc<Quota>,
    depth: usize,
) -> Result<()> {
    if depth >= DEPTH {
        return Err(Error::LimitExceeded);
    }
    let node = private(node, quota)?;
    match &mut node.body {
        Body::Leaf(entries) => {
            if let Ok(index) = entries.binary_search_by(|entry| entry.key.compare(database, key)) {
                entries.remove(index);
            }
        }
        Body::Branch(children) => {
            let index = children
                .partition_point(|child| child.maximum.compare(database, key) == Ordering::Less);
            if index >= children.len() {
                return Ok(());
            }
            remove(&mut children[index].node, database, key, quota, depth + 1)?;
            if children[index].node.is_empty() {
                children.remove(index);
            } else {
                children[index].maximum = children[index].node.maximum().clone();
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Frame<'a> {
    node: &'a Node,
    next: usize,
}
struct Cursor<'a> {
    stack: [Option<Frame<'a>>; DEPTH],
    depth: usize,
    leaf: &'a [Entry],
    reverse: bool,
}
impl<'a> Cursor<'a> {
    fn new(root: Option<&'a Node>, reverse: bool) -> Self {
        let mut cursor = Self {
            stack: [None; DEPTH],
            depth: 0,
            leaf: &[],
            reverse,
        };
        if let Some(root) = root {
            cursor.push(root);
        }
        cursor
    }
    fn seek(root: Option<&'a Node>, database: u64, bound: Bound<&[u8]>, reverse: bool) -> Self {
        let mut cursor = Self::new(None, reverse);
        let Some(mut node) = root else {
            return cursor;
        };
        loop {
            match &node.body {
                Body::Leaf(entries) => {
                    let next = entries.partition_point(|entry| match (reverse, bound) {
                        (false, Bound::Unbounded) => entry.key.database < database,
                        (true, Bound::Unbounded) => entry.key.database <= database,
                        (false, Bound::Included(key)) | (true, Bound::Excluded(key)) => {
                            entry.key.compare(database, key) == Ordering::Less
                        }
                        (false, Bound::Excluded(key)) | (true, Bound::Included(key)) => {
                            entry.key.compare(database, key) != Ordering::Greater
                        }
                    });
                    cursor.leaf = if reverse {
                        &entries[..next]
                    } else {
                        &entries[next..]
                    };
                    return cursor;
                }
                Body::Branch(children) => {
                    let index = children.partition_point(|child| match (reverse, bound) {
                        (false, Bound::Unbounded) => child.maximum.database < database,
                        (true, Bound::Unbounded) => child.maximum.database <= database,
                        (false, Bound::Excluded(key)) => {
                            child.maximum.compare(database, key) != Ordering::Greater
                        }
                        (_, Bound::Included(key)) | (true, Bound::Excluded(key)) => {
                            child.maximum.compare(database, key) == Ordering::Less
                        }
                    });
                    // An upper seek can start just beyond the requested key:
                    // its leaf search then falls back through the saved path.
                    let index = if reverse {
                        index.min(children.len() - 1)
                    } else if index == children.len() {
                        cursor.depth = 0;
                        return cursor;
                    } else {
                        index
                    };
                    cursor.push_branch(node, if reverse { index } else { index + 1 });
                    node = children[index].node.as_ref();
                }
            }
        }
    }
    fn push(&mut self, node: &'a Node) {
        match &node.body {
            Body::Leaf(entries) => self.leaf = entries,
            Body::Branch(children) => {
                self.push_branch(node, if self.reverse { children.len() } else { 0 });
            }
        }
    }
    fn push_branch(&mut self, node: &'a Node, next: usize) {
        assert!(self.depth < DEPTH, "tree depth admitted before publication");
        self.stack[self.depth] = Some(Frame { node, next });
        self.depth += 1;
    }
    #[inline(always)]
    fn next_leaf(&mut self) -> Option<&'a Entry> {
        let (entry, rest) = if self.reverse {
            self.leaf.split_last()?
        } else {
            self.leaf.split_first()?
        };
        self.leaf = rest;
        Some(entry)
    }
    // Keep the ordinary leaf advance tiny so public iterator wrappers can
    // inline it into callers without importing the branch-stack walker.
    #[inline(always)]
    fn next(&mut self) -> Option<&'a Entry> {
        if let Some(entry) = self.next_leaf() {
            return Some(entry);
        }
        self.refill()
    }
    #[inline(never)]
    fn refill(&mut self) -> Option<&'a Entry> {
        while self.depth > 0 {
            let frame = self.stack[self.depth - 1].as_mut().unwrap();
            let Body::Branch(children) = &frame.node.body else {
                unreachable!("cursor stack contains branches only");
            };
            let index = if self.reverse {
                if frame.next == 0 {
                    self.depth -= 1;
                    continue;
                }
                frame.next -= 1;
                frame.next
            } else {
                if frame.next == children.len() {
                    self.depth -= 1;
                    continue;
                }
                let index = frame.next;
                frame.next += 1;
                index
            };
            let child = children[index].node.as_ref();
            self.push(child);
            if let Some(entry) = self.next_leaf() {
                return Some(entry);
            }
        }
        None
    }
}
pub struct Iter<'a> {
    front: Cursor<'a>,
    back: Cursor<'a>,
    remaining: usize,
}
impl<'a> Iterator for Iter<'a> {
    type Item = (u64, &'a [u8], &'a [u8]);
    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let entry = self.front.next()?;
        self.remaining -= 1;
        Some((
            entry.key.database,
            entry.key.bytes.as_slice(),
            entry.value.as_slice(),
        ))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}
impl<'a> DoubleEndedIterator for Iter<'a> {
    #[inline(always)]
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let entry = self.back.next()?;
        self.remaining -= 1;
        Some((
            entry.key.database,
            entry.key.bytes.as_slice(),
            entry.value.as_slice(),
        ))
    }
}
impl ExactSizeIterator for Iter<'_> {}

/// A borrowed range of one immutable index root. Bounds are resolved at
/// construction, and advancing either end never searches the other database
/// namespaces or compares keys outside the requested range.
pub struct RangeIter<'a> {
    front: Cursor<'a>,
    back: Cursor<'a>,
    first: Option<&'a Entry>,
    last: Option<&'a Entry>,
}
impl<'a> Iterator for RangeIter<'a> {
    type Item = (u64, &'a [u8], &'a [u8]);
    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.first?;
        if std::ptr::eq(entry, self.last.expect("range endpoints coexist")) {
            self.first = None;
            self.last = None;
        } else {
            self.first = self.front.next();
        }
        Some((
            entry.key.database,
            entry.key.bytes.as_slice(),
            entry.value.as_slice(),
        ))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.first.is_some() {
            (1, None)
        } else {
            (0, Some(0))
        }
    }
}
impl DoubleEndedIterator for RangeIter<'_> {
    #[inline(always)]
    fn next_back(&mut self) -> Option<Self::Item> {
        let entry = self.last?;
        if std::ptr::eq(entry, self.first.expect("range endpoints coexist")) {
            self.first = None;
            self.last = None;
        } else {
            self.last = self.back.next();
        }
        Some((
            entry.key.database,
            entry.key.bytes.as_slice(),
            entry.value.as_slice(),
        ))
    }
}
impl std::iter::FusedIterator for RangeIter<'_> {}

/// A bounded iterator that advances in its construction-time direction.
/// Unlike RangeIter it retains only one traversal stack.
pub struct ScanIter<'a> {
    cursor: Cursor<'a>,
    first: Option<&'a Entry>,
    terminal: Option<&'a Entry>,
    database: u64,
}
impl<'a> Iterator for ScanIter<'a> {
    type Item = (u64, &'a [u8], &'a [u8]);
    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.first?;
        self.first = match self.terminal {
            Some(last) if std::ptr::eq(entry, last) => None,
            Some(_) => self.cursor.next(),
            None => self
                .cursor
                .next()
                .filter(|next| next.key.database == self.database),
        };
        Some((
            entry.key.database,
            entry.key.bytes.as_slice(),
            entry.value.as_slice(),
        ))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.first.is_some() {
            (1, None)
        } else {
            (0, Some(0))
        }
    }
}
impl std::iter::FusedIterator for ScanIter<'_> {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn staging_chunks_are_copied_before_tree_admission_and_reused_safely() {
        assert!(size_of::<Bytes>() <= 24);
        assert!(size_of::<Change>() <= 56);
        let mut arena = ulmdb_storage::snapshot_input::Arena::new(16 << 10);
        let staged = Bytes::staged(&[7; 64], &mut arena).unwrap();
        let source = staged.as_slice().as_ptr();
        let index = Index::new(1 << 20).unwrap();
        let next = index
            .prepare(&[Change {
                database: 1,
                key: Bytes::new(b"key").unwrap(),
                value: Some(staged.clone()),
            }])
            .unwrap();
        let borrowed = next.get(1, b"key").unwrap();
        assert_ne!(borrowed.as_ptr(), source);
        assert_eq!(borrowed, &[7; 64]);
        drop(staged);
        let allocated = arena.stats().allocated_bytes;
        arena.reset();
        let overwritten = Bytes::staged(&[9; 64], &mut arena).unwrap();
        assert_eq!(overwritten.as_slice(), &[9; 64]);
        assert_eq!(arena.stats().allocated_bytes, allocated);
        assert_eq!(borrowed, &[7; 64]);
        assert_eq!(next.stats().large_allocations, 1);
        // An optional staging allocation refusal keeps ordinary owned input.
        let mut tiny = ulmdb_storage::snapshot_input::Arena::new(0);
        assert_eq!(
            Bytes::staged(&[11; 64], &mut tiny).unwrap().as_slice(),
            &[11; 64]
        );
    }
    fn put(database: u64, key: &[u8], value: &[u8]) -> Change {
        Change {
            database,
            key: Bytes::new(key).unwrap(),
            value: Some(Bytes::new(value).unwrap()),
        }
    }
    fn delete(database: u64, key: &[u8]) -> Change {
        Change {
            database,
            key: Bytes::new(key).unwrap(),
            value: None,
        }
    }
    fn borrowed_bound(bound: &Bound<Vec<u8>>) -> Bound<&[u8]> {
        match bound {
            Bound::Included(key) => Bound::Included(key.as_slice()),
            Bound::Excluded(key) => Bound::Excluded(key.as_slice()),
            Bound::Unbounded => Bound::Unbounded,
        }
    }
    fn within_bounds(key: &[u8], lower: &Bound<Vec<u8>>, upper: &Bound<Vec<u8>>) -> bool {
        let after_lower = match lower {
            Bound::Included(bound) => key >= bound.as_slice(),
            Bound::Excluded(bound) => key > bound.as_slice(),
            Bound::Unbounded => true,
        };
        let before_upper = match upper {
            Bound::Included(bound) => key <= bound.as_slice(),
            Bound::Excluded(bound) => key < bound.as_slice(),
            Bound::Unbounded => true,
        };
        after_lower && before_upper
    }
    #[test]
    fn range_and_scan_match_full_iteration_across_bounds_namespaces_and_mixed_ends() {
        let mut changes = Vec::new();
        for database in [0, 4, u64::MAX] {
            for number in 0u64..1536 {
                changes.push(put(database, &number.to_be_bytes(), &number.to_le_bytes()));
            }
            for key in [b"".as_slice(), &[0], &[255], &[255; 33]] {
                changes.push(put(database, key, key));
            }
        }
        let index = Index::new(32 << 20)
            .unwrap()
            .prepare_grouped(&changes)
            .unwrap();
        assert!(index.stats().height >= 3);
        // Prune some leaves and retain underfull ones, so seeks also cross
        // changed branch fences rather than relying on a packed fixture.
        let deletes: Vec<_> = (0u64..1536)
            .filter(|number| number % 5 != 0)
            .map(|number| delete(4, &number.to_be_bytes()))
            .collect();
        let index = index.prepare_grouped(&deletes).unwrap();
        let all: Vec<_> = index.iter().collect();
        let mut bounds = vec![Bound::Unbounded];
        for key in [
            vec![],
            vec![0],
            0u64.to_be_bytes().to_vec(),
            511u64.to_be_bytes().to_vec(),
            1535u64.to_be_bytes().to_vec(),
            vec![255],
            vec![255; 33],
        ] {
            bounds.push(Bound::Included(key.clone()));
            bounds.push(Bound::Excluded(key));
        }
        let charged = index.stats().charged_bytes;
        let peak = index.stats().peak_bytes;
        for database in [0, 1, 4, 5, u64::MAX] {
            for lower in &bounds {
                for upper in &bounds {
                    let expected: Vec<_> = all
                        .iter()
                        .copied()
                        .filter(|(db, key, _)| *db == database && within_bounds(key, lower, upper))
                        .collect();
                    let range =
                        || index.range(database, borrowed_bound(lower), borrowed_bound(upper));
                    assert_eq!(
                        range().collect::<Vec<_>>(),
                        expected,
                        "forward database={database} lower={lower:?} upper={upper:?}"
                    );
                    assert_eq!(
                        range().rev().collect::<Vec<_>>(),
                        expected.iter().copied().rev().collect::<Vec<_>>(),
                        "reverse database={database} lower={lower:?} upper={upper:?}"
                    );
                    assert_eq!(
                        index
                            .scan(
                                database,
                                borrowed_bound(lower),
                                borrowed_bound(upper),
                                false
                            )
                            .collect::<Vec<_>>(),
                        expected,
                        "forward scan database={database} lower={lower:?} upper={upper:?}"
                    );
                    assert_eq!(
                        index
                            .scan(database, borrowed_bound(lower), borrowed_bound(upper), true)
                            .collect::<Vec<_>>(),
                        expected.iter().copied().rev().collect::<Vec<_>>(),
                        "reverse scan database={database} lower={lower:?} upper={upper:?}"
                    );
                    let mut mixed = range();
                    let (mut first, mut last) = (0, expected.len());
                    while first < last {
                        if (last - first) % 3 == 0 {
                            last -= 1;
                            assert_eq!(mixed.next_back(), Some(expected[last]));
                        } else {
                            assert_eq!(mixed.next(), Some(expected[first]));
                            first += 1;
                        }
                    }
                    assert_eq!(mixed.next(), None);
                    assert_eq!(mixed.next_back(), None);
                    assert_eq!(mixed.size_hint(), (0, Some(0)));
                }
            }
        }
        assert_eq!(index.stats().charged_bytes, charged);
        assert_eq!(index.stats().peak_bytes, peak);
    }
    #[test]
    fn range_consumes_temporary_bounds_and_keeps_old_snapshot_values() {
        let old = Index::new(4 << 20)
            .unwrap()
            .prepare(&[
                put(7, b"a", &[1; 40]),
                put(7, b"b", &[2; 40]),
                put(7, b"c", &[3; 40]),
            ])
            .unwrap();
        let mut range = {
            let lower = b"a".to_vec();
            let upper = b"c".to_vec();
            old.range(7, Bound::Included(&lower), Bound::Excluded(&upper))
        };
        let mut scan = {
            let lower = b"a".to_vec();
            let upper = b"c".to_vec();
            old.scan(7, Bound::Excluded(&lower), Bound::Included(&upper), true)
        };
        let (_, key, borrowed) = range.next().unwrap();
        let address = borrowed.as_ptr();
        let next = old
            .prepare(&[put(7, b"a", &[9; 40]), delete(7, b"b")])
            .unwrap();
        assert_eq!(key, b"a");
        assert_eq!(borrowed, &[1; 40]);
        assert_eq!(old.get(7, b"a").unwrap().as_ptr(), address);
        assert_eq!(
            range.next_back(),
            Some((7, b"b".as_slice(), [2; 40].as_slice()))
        );
        assert!(range.next().is_none());
        assert_eq!(next.get(7, b"a"), Some([9; 40].as_slice()));
        assert!(next.get(7, b"b").is_none());
        assert_eq!(scan.next(), Some((7, b"c".as_slice(), [3; 40].as_slice())));
        assert_eq!(scan.next(), Some((7, b"b".as_slice(), [2; 40].as_slice())));
        assert!(scan.next().is_none());
    }
    #[test]
    fn empty_index_ranges_are_fused_in_both_directions() {
        let index = Index::new(4096).unwrap();
        for database in [0, u64::MAX] {
            let mut range = index.range(database, Bound::Unbounded, Bound::Unbounded);
            assert_eq!(range.next(), None);
            assert_eq!(range.next_back(), None);
            assert_eq!(range.next(), None);
            for reverse in [false, true] {
                let mut scan = index.scan(database, Bound::Unbounded, Bound::Unbounded, reverse);
                assert_eq!(scan.next(), None);
                assert_eq!(scan.next(), None);
            }
        }
    }
    #[test]
    fn snapshots_keep_borrowed_values_through_insert_delete_and_replacement() {
        let empty = Index::new(4 << 20).unwrap();
        let changes: Vec<_> = (0u64..1000)
            .rev()
            .map(|n| put(1, &n.to_be_bytes(), &n.to_le_bytes()))
            .collect();
        let old = empty.prepare(&changes).unwrap();
        let borrowed = old.get(1, &500u64.to_be_bytes()).unwrap();
        let address = borrowed.as_ptr();
        let next = old
            .prepare(&[
                put(1, &500u64.to_be_bytes(), b"replacement"),
                delete(1, &700u64.to_be_bytes()),
                put(2, b"", b"empty-key"),
            ])
            .unwrap();
        assert_eq!(borrowed, &500u64.to_le_bytes());
        assert_eq!(old.get(1, &500u64.to_be_bytes()).unwrap().as_ptr(), address);
        assert_eq!(
            next.get(1, &500u64.to_be_bytes()),
            Some(b"replacement".as_slice())
        );
        assert!(next.get(1, &700u64.to_be_bytes()).is_none());
        assert_eq!(old.len(), 1000);
        assert_eq!(next.len(), 1000);
        let forward: Vec<_> = next
            .iter()
            .map(|(d, k, v)| (d, k.to_vec(), v.to_vec()))
            .collect();
        assert!(
            forward
                .windows(2)
                .all(|pair| (&pair[0].0, &pair[0].1) < (&pair[1].0, &pair[1].1))
        );
        assert_eq!(
            next.iter()
                .rev()
                .map(|(d, k, v)| (d, k.to_vec(), v.to_vec()))
                .collect::<Vec<_>>(),
            forward.into_iter().rev().collect::<Vec<_>>()
        );
    }
    #[test]
    fn quota_refusal_is_atomic_and_old_roots_release_large_values() {
        let empty = Index::new(64 << 10).unwrap();
        let baseline = empty.stats().charged_bytes;
        let first = empty.prepare(&[put(1, b"key", &[1; 8000])]).unwrap();
        let borrowed = first.get(1, b"key").unwrap();
        let second = first.prepare(&[put(1, b"key", &[2; 8000])]).unwrap();
        assert_eq!(borrowed, &[1; 8000]);
        assert_eq!(second.stats().large_allocations, 2);
        let before = second.stats().charged_bytes;
        assert!(second.prepare(&[put(1, b"key", &[3; 60000])]).is_err());
        assert_eq!(second.stats().charged_bytes, before);
        drop(first);
        assert_eq!(second.stats().large_allocations, 1);
        drop(second);
        assert_eq!(empty.stats().charged_bytes, baseline);
    }
    #[test]
    fn arbitrary_binary_keys_and_mixed_ends_remain_ordered() {
        let index = Index::new(4 << 20)
            .unwrap()
            .prepare(&[
                put(0, b"", b""),
                put(0, &[0], &[255]),
                put(0, &[0, 255], &[0]),
                put(0, &[255; 100], &[42; 200]),
                put(u64::MAX, b"", b"last"),
            ])
            .unwrap();
        let mut iter = index.iter();
        assert_eq!(iter.next().unwrap().1, b"");
        assert_eq!(iter.next_back().unwrap().0, u64::MAX);
        assert_eq!(iter.next_back().unwrap().1, &[255; 100]);
        assert_eq!(iter.next().unwrap().1, &[0]);
        assert_eq!(iter.next().unwrap().1, &[0, 255]);
        assert!(iter.next().is_none());
        assert!(iter.next_back().is_none());
    }
    #[test]
    fn mixed_transactions_match_independent_btree_oracle() {
        let mut index = Index::new(16 << 20).unwrap();
        let mut expected = std::collections::BTreeMap::new();
        let mut random = 0x9183_da71_4082_153fu64;
        let mut held = Vec::new();
        for turn in 0..80 {
            let mut changes = Vec::new();
            for _ in 0..32 {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                let database = random % 5;
                let key = (random % 300).to_be_bytes().to_vec();
                if random % 4 == 0 {
                    changes.push(delete(database, &key));
                    expected.remove(&(database, key));
                } else {
                    let value = random.to_be_bytes().to_vec();
                    changes.push(put(database, &key, &value));
                    expected.insert((database, key), value);
                }
            }
            index = index
                .prepare_hybrid(&changes, if turn % 3 == 0 { 1 } else { usize::MAX })
                .unwrap();
            assert_cached_bytes(&index);
            let observed: std::collections::BTreeMap<_, _> = index
                .iter()
                .map(|(database, key, value)| ((database, key.to_vec()), value.to_vec()))
                .collect();
            assert_eq!(observed, expected);
            if turn % 20 == 0 {
                held.push(index.clone());
            }
        }
        for old in held {
            let before = old
                .iter()
                .map(|(d, k, v)| (d, k.to_vec(), v.to_vec()))
                .collect::<Vec<_>>();
            assert!(
                before
                    .windows(2)
                    .all(|p| (&p[0].0, &p[0].1) < (&p[1].0, &p[1].1))
            );
        }
    }

    #[test]
    fn later_quota_failure_rolls_back_earlier_private_changes() {
        let base = Index::new(32 << 10)
            .unwrap()
            .prepare(&[put(1, b"base", b"original")])
            .unwrap();
        let before = base.stats().charged_bytes;
        assert!(
            base.prepare(&[put(1, b"base", b"changed"), put(1, b"other", &[7; 40000])])
                .is_err()
        );
        assert_eq!(base.get(1, b"base"), Some(b"original".as_slice()));
        assert!(base.get(1, b"other").is_none());
        assert_eq!(base.stats().charged_bytes, before);
    }

    fn assert_cached_bytes(index: &Index) {
        assert_eq!(
            index.encoded_rows_bytes(),
            index
                .iter()
                .map(|(_, k, v)| 16 + k.len() + v.len())
                .sum::<usize>()
        );
    }
    fn owned_rows(index: &Index) -> Vec<(u64, Vec<u8>, Vec<u8>)> {
        index
            .iter()
            .map(|(d, k, v)| (d, k.to_vec(), v.to_vec()))
            .collect()
    }
    #[test]
    fn grouped_and_point_routes_match_unordered_last_write_delete_effects() {
        let empty = Index::new(32 << 20).unwrap();
        let seed: Vec<_> = (0u64..1000)
            .map(|n| put(n % 3, &n.to_be_bytes(), &n.to_le_bytes()))
            .collect();
        let base = empty.prepare(&seed).unwrap();
        let borrowed = base.get(2, &500u64.to_be_bytes()).unwrap();
        let address = borrowed.as_ptr();
        let mut random = 0x5416_2190_8592_3741u64;
        let mut changes = Vec::new();
        let mut oracle: std::collections::BTreeMap<_, _> = base
            .iter()
            .map(|(d, k, v)| ((d, k.to_vec()), v.to_vec()))
            .collect();
        for position in 0u64..3000 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let database = random % 4;
            let key = (random % 1300).to_be_bytes().to_vec();
            if random % 4 == 0 {
                changes.push(delete(database, &key));
                oracle.remove(&(database, key));
            } else {
                let value = if random % 11 == 0 {
                    vec![(random & 255) as u8; 73]
                } else {
                    position.to_le_bytes().to_vec()
                };
                changes.push(put(database, &key, &value));
                oracle.insert((database, key), value);
            }
        }
        // Coalescing must honor the final position, including empty binary data.
        changes.push(put(9, b"", b"temporary"));
        changes.push(delete(9, b""));
        changes.push(put(9, b"", b""));
        oracle.insert((9, Vec::new()), Vec::new());
        let point = base.prepare(&changes).unwrap();
        let grouped = base.prepare_grouped(&changes).unwrap();
        assert_eq!(owned_rows(&point), owned_rows(&grouped));
        assert_eq!(
            grouped
                .iter()
                .map(|(d, k, v)| ((d, k.to_vec()), v.to_vec()))
                .collect::<std::collections::BTreeMap<_, _>>(),
            oracle
        );
        assert_cached_bytes(&base);
        assert_cached_bytes(&point);
        assert_cached_bytes(&grouped);
        assert_eq!(borrowed, &500u64.to_le_bytes());
        assert_eq!(
            base.get(2, &500u64.to_be_bytes()).unwrap().as_ptr(),
            address
        );
    }
    #[test]
    fn grouped_bulk_split_sparse_path_and_delete_all_preserve_shared_subtrees() {
        let empty = Index::new(32 << 20).unwrap();
        let changes: Vec<_> = (0u64..8192)
            .rev()
            .map(|n| put(1, &n.to_be_bytes(), &n.to_le_bytes()))
            .collect();
        let base = empty.prepare_hybrid(&changes, 16).unwrap();
        assert_eq!(base.len(), 8192);
        assert_cached_bytes(&base);
        let before_nodes = base.stats().nodes;
        let one: Vec<_> = (0u64..64)
            .map(|n| put(1, &4000u64.to_be_bytes(), &n.to_le_bytes()))
            .collect();
        let next = base.prepare_grouped(&one).unwrap();
        assert_eq!(next.stats().nodes - before_nodes, base.stats().height);
        assert_eq!(
            next.get(1, &4000u64.to_be_bytes()),
            Some(63u64.to_le_bytes().as_slice())
        );
        assert_eq!(
            base.get(1, &4000u64.to_be_bytes()),
            Some(4000u64.to_le_bytes().as_slice())
        );
        assert_cached_bytes(&next);
        let deletes: Vec<_> = (0u64..8192).map(|n| delete(1, &n.to_be_bytes())).collect();
        let cleared = next.prepare_grouped(&deletes).unwrap();
        assert!(cleared.is_empty());
        assert_eq!(cleared.encoded_rows_bytes(), 0);
        drop(next);
        drop(base);
        assert_eq!(cleared.stats().nodes, 0);
    }
    #[test]
    fn grouped_noop_identity_hybrid_dispatch_and_scratch_refusal() {
        let base = Index::new(2 << 20)
            .unwrap()
            .prepare(&[put(1, b"a", b"original")])
            .unwrap();
        let changes = [
            put(1, b"a", b"temporary"),
            delete(1, b"missing"),
            put(1, b"a", b"original"),
        ];
        assert!(base.same_state(&base.prepare_grouped(&changes).unwrap()));
        assert!(!base.same_state(&base.prepare_hybrid(&changes, usize::MAX).unwrap()));
        assert!(base.same_state(&base.prepare_hybrid(&changes, 1).unwrap()));
        assert_cached_bytes(&base.prepare(&changes).unwrap());
        let tiny = Index::new(8192)
            .unwrap()
            .prepare(&[put(1, b"a", b"old")])
            .unwrap();
        let before = tiny.stats().charged_bytes;
        assert!(tiny.prepare_grouped(&[put(1, b"a", b"new")]).is_err());
        assert_eq!(tiny.stats().charged_bytes, before);
        assert_eq!(tiny.get(1, b"a"), Some(b"old".as_slice()));
    }
    #[test]
    fn grouped_late_large_value_failure_returns_all_temporary_charges() {
        let base = Index::new(256 << 10)
            .unwrap()
            .prepare(&[put(1, b"large", &[1; 40000])])
            .unwrap();
        let before = base.stats().charged_bytes;
        assert!(
            base.prepare_grouped(&[put(1, b"a", b"private"), put(1, b"large", &[2; 220000])])
                .is_err()
        );
        assert_eq!(base.stats().charged_bytes, before);
        assert_eq!(base.get(1, b"large"), Some([1; 40000].as_slice()));
        assert!(base.get(1, b"a").is_none());
        assert_cached_bytes(&base);
    }

    #[test]
    fn metadata_reservation_refusal_shrink_and_drop_are_exact() {
        let index = Index::new(8192).unwrap();
        let initial = index.stats().charged_bytes;
        let mut ticket = index.reserve_metadata(128).unwrap();
        assert_eq!(ticket.bytes(), 128);
        assert_eq!(index.stats().charged_bytes, initial + 128);
        ticket.resize(4096).unwrap();
        assert_eq!(index.stats().charged_bytes, initial + 4096);
        let peak = index.stats().peak_bytes;
        assert!(ticket.resize(8192).is_err());
        assert_eq!(ticket.bytes(), 4096);
        assert_eq!(index.stats().charged_bytes, initial + 4096);
        assert_eq!(index.stats().peak_bytes, peak);
        ticket.resize(64).unwrap();
        assert_eq!(index.stats().charged_bytes, initial + 64);
        ticket.resize(0).unwrap();
        assert_eq!(index.stats().charged_bytes, initial);
        ticket.resize(512).unwrap();
        drop(ticket);
        assert_eq!(index.stats().charged_bytes, initial);
        assert_eq!(index.stats().nodes, 0);
        assert_eq!(index.stats().large_allocations, 0);
    }

    #[test]
    fn retained_metadata_owners_share_the_same_budget_as_tree_pages() {
        let index = Index::new(8192).unwrap();
        let observer = index.clone();
        let initial = index.stats().charged_bytes;
        let owner = index.reserve_metadata(7000).unwrap();
        drop(index);
        assert_eq!(observer.stats().charged_bytes, initial + 7000);
        assert!(observer.prepare(&[put(1, b"key", b"value")]).is_err());
        assert_eq!(observer.stats().charged_bytes, initial + 7000);
        drop(owner);
        let populated = observer.prepare(&[put(1, b"key", b"value")]).unwrap();
        assert_eq!(populated.get(1, b"key"), Some(b"value".as_slice()));
        drop(populated);
        assert_eq!(observer.stats().charged_bytes, initial);
    }

    #[test]
    fn repeated_key_reuses_private_page_and_deleting_all_releases_every_node() {
        let empty = Index::new(4 << 20).unwrap();
        let initial = empty.stats().charged_bytes;
        let changes: Vec<_> = (0u64..512)
            .map(|n| put(3, &n.to_be_bytes(), &[1]))
            .collect();
        let index = empty.prepare(&changes).unwrap();
        let repeated: Vec<_> = (0u64..1000)
            .map(|n| put(3, &17u64.to_be_bytes(), &n.to_be_bytes()))
            .collect();
        let next = index.prepare(&repeated).unwrap();
        assert_eq!(
            next.get(3, &17u64.to_be_bytes()),
            Some(999u64.to_be_bytes().as_slice())
        );
        drop(index);
        let deletes: Vec<_> = (0u64..512).map(|n| delete(3, &n.to_be_bytes())).collect();
        let cleared = next.prepare(&deletes).unwrap();
        assert!(cleared.is_empty());
        assert_eq!(cleared.stats().height, 0);
        drop(next);
        assert_eq!(cleared.stats().nodes, 0);
        assert_eq!(cleared.stats().charged_bytes, initial);
    }
}
