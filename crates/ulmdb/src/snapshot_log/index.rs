//! Persistent ordered index for the opt-in snapshot log.
//!
//! The index owns immutable published nodes. A private prepare clones a node
//! only on its first edit; subsequent edits reuse that private node. Values and
//! keys up to 16 bytes stay inline. Larger retained bytes have one shared quota
//! charge, released with their final owner. Old roots never form version chains.
//! Uniform single-database leaves with 8/8 or 16/8 byte key/value shapes store
//! raw fixed-size rows; arbitrary other shapes use the generic representation.
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
use std::num::NonZeroUsize;
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
    charge: Option<Charge<CHARGE_LARGE>>,
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
                let charge = Charge::<CHARGE_LARGE>::new(quota, bytes)?;
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
                let charge = Charge::<CHARGE_LARGE>::new(quota, bytes)?;
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
#[derive(Clone, Copy)]
struct SliceRef<'a>(&'a [u8]);
impl<'a> SliceRef<'a> {
    #[inline(always)]
    fn as_slice(self) -> &'a [u8] {
        self.0
    }
}
#[derive(Clone, Copy)]
struct KeyRef<'a> {
    database: u64,
    bytes: SliceRef<'a>,
}
impl KeyRef<'_> {
    fn compare(self, database: u64, key: &[u8]) -> Ordering {
        self.database
            .cmp(&database)
            .then_with(|| self.bytes.as_slice().cmp(key))
    }
}
#[derive(Clone, Copy)]
enum EntryRef<'a> {
    Generic(&'a Entry),
    Packed8(u64, &'a Packed8),
    Packed16(u64, &'a Packed16),
}
impl<'a> EntryRef<'a> {
    fn identity(self) -> NonZeroUsize {
        let address = match self {
            Self::Generic(entry) => entry as *const Entry as usize,
            Self::Packed8(_, entry) => entry as *const Packed8 as usize,
            Self::Packed16(_, entry) => entry as *const Packed16 as usize,
        };
        NonZeroUsize::new(address).expect("borrowed row addresses are nonnull")
    }
    #[inline(always)]
    fn row(self) -> (u64, &'a [u8], &'a [u8]) {
        match self {
            Self::Generic(entry) => (
                entry.key.database,
                entry.key.bytes.as_slice(),
                entry.value.as_slice(),
            ),
            Self::Packed8(database, entry) => (database, &entry.key, &entry.value),
            Self::Packed16(database, entry) => (database, &entry.key, &entry.value),
        }
    }
    #[inline(always)]
    fn database(self) -> u64 {
        match self {
            Self::Generic(entry) => entry.key.database,
            Self::Packed8(database, _) | Self::Packed16(database, _) => database,
        }
    }
    #[inline(always)]
    fn key(self) -> KeyRef<'a> {
        match self {
            Self::Generic(entry) => KeyRef {
                database: entry.key.database,
                bytes: SliceRef(entry.key.bytes.as_slice()),
            },
            Self::Packed8(database, entry) => KeyRef {
                database,
                bytes: SliceRef(&entry.key),
            },
            Self::Packed16(database, entry) => KeyRef {
                database,
                bytes: SliceRef(&entry.key),
            },
        }
    }
    fn value(self) -> &'a [u8] {
        self.row().2
    }
    fn owned(self) -> Entry {
        if let Self::Generic(entry) = self {
            return entry.clone();
        }
        let (database, key, value) = self.row();
        Entry {
            key: Key {
                database,
                bytes: Bytes::new(key).expect("packed key is inline"),
            },
            value: Bytes::new(value).expect("packed value is inline"),
        }
    }
    fn owned_key(self) -> Key {
        match self {
            Self::Generic(entry) => entry.key.clone(),
            _ => {
                let key = self.key();
                Key {
                    database: key.database,
                    bytes: Bytes::new(key.bytes.as_slice()).expect("packed key is inline"),
                }
            }
        }
    }
    fn encoded_bytes(self) -> Result<usize> {
        let (_, key, value) = self.row();
        16usize
            .checked_add(key.len())
            .and_then(|n| n.checked_add(value.len()))
            .ok_or(Error::LimitExceeded)
    }
    #[inline(always)]
    fn same_entry(self, other: Self) -> bool {
        match (self, other) {
            (Self::Generic(a), Self::Generic(b)) => std::ptr::eq(a, b),
            (Self::Packed8(_, a), Self::Packed8(_, b)) => std::ptr::eq(a, b),
            (Self::Packed16(_, a), Self::Packed16(_, b)) => std::ptr::eq(a, b),
            _ => false,
        }
    }
}
#[derive(Clone, Copy)]
struct Packed8 {
    key: [u8; 8],
    value: [u8; 8],
}
#[derive(Clone, Copy)]
struct Packed16 {
    key: [u8; 16],
    value: [u8; 8],
}
#[derive(Clone, Copy)]
enum LeafShape {
    Generic,
    Packed8(u64),
    Packed16(u64),
}
impl LeafShape {
    fn entry(entry: &Entry) -> Self {
        match (
            entry.key.bytes.as_slice().len(),
            entry.value.as_slice().len(),
        ) {
            (8, 8) => Self::Packed8(entry.key.database),
            (16, 8) => Self::Packed16(entry.key.database),
            _ => Self::Generic,
        }
    }
    fn partition(entries: &[Entry]) -> Self {
        let Some(first) = entries.first() else {
            return Self::Generic;
        };
        let shape = Self::entry(first);
        if entries.iter().all(|entry| shape.accepts(entry)) {
            shape
        } else {
            Self::Generic
        }
    }
    fn accepts(self, entry: &Entry) -> bool {
        match self {
            Self::Generic => true,
            Self::Packed8(database) => {
                database == entry.key.database
                    && entry.key.bytes.as_slice().len() == 8
                    && entry.value.as_slice().len() == 8
            }
            Self::Packed16(database) => {
                database == entry.key.database
                    && entry.key.bytes.as_slice().len() == 16
                    && entry.value.as_slice().len() == 8
            }
        }
    }
    fn entry_size(self) -> usize {
        match self {
            Self::Generic => size_of::<Entry>(),
            Self::Packed8(_) => size_of::<Packed8>(),
            Self::Packed16(_) => size_of::<Packed16>(),
        }
    }
}
enum Leaf {
    Generic(Vec<Entry>),
    Packed8 {
        database: u64,
        entries: Vec<Packed8>,
    },
    Packed16 {
        database: u64,
        entries: Vec<Packed16>,
    },
}
// Typed slices keep the cursor on a leaf until its next branch refill. No
// temporary Entry/Bytes object is required to return packed borrowed bytes.
#[derive(Clone, Copy)]
enum LeafSlice<'a> {
    Generic(&'a [Entry]),
    Packed8(u64, &'a [Packed8]),
    Packed16(u64, &'a [Packed16]),
}
impl<'a> LeafSlice<'a> {
    fn len(self) -> usize {
        match self {
            Self::Generic(entries) => entries.len(),
            Self::Packed8(_, entries) => entries.len(),
            Self::Packed16(_, entries) => entries.len(),
        }
    }
    #[inline(always)]
    fn get(self, index: usize) -> Option<EntryRef<'a>> {
        match self {
            Self::Generic(entries) => entries.get(index).map(EntryRef::Generic),
            Self::Packed8(database, entries) => entries
                .get(index)
                .map(|entry| EntryRef::Packed8(database, entry)),
            Self::Packed16(database, entries) => entries
                .get(index)
                .map(|entry| EntryRef::Packed16(database, entry)),
        }
    }
    fn slice(self, start: usize, end: usize) -> Self {
        match self {
            Self::Generic(entries) => Self::Generic(&entries[start..end]),
            Self::Packed8(database, entries) => Self::Packed8(database, &entries[start..end]),
            Self::Packed16(database, entries) => Self::Packed16(database, &entries[start..end]),
        }
    }
    fn partition_point(self, mut predicate: impl FnMut(EntryRef<'a>) -> bool) -> usize {
        let (mut left, mut right) = (0, self.len());
        while left < right {
            let middle = left + (right - left) / 2;
            if predicate(self.get(middle).expect("leaf position")) {
                left = middle + 1;
            } else {
                right = middle;
            }
        }
        left
    }
}
impl Leaf {
    fn new(shape: LeafShape) -> Result<Self> {
        fn buffer<T>() -> Result<Vec<T>> {
            let mut entries = Vec::new();
            entries
                .try_reserve_exact(LEAF + 1)
                .map_err(|_| Error::LimitExceeded)?;
            Ok(entries)
        }
        Ok(match shape {
            LeafShape::Generic => Self::Generic(buffer()?),
            LeafShape::Packed8(database) => Self::Packed8 {
                database,
                entries: buffer()?,
            },
            LeafShape::Packed16(database) => Self::Packed16 {
                database,
                entries: buffer()?,
            },
        })
    }
    fn shape(&self) -> LeafShape {
        match self {
            Self::Generic(_) => LeafShape::Generic,
            Self::Packed8 { database, .. } => LeafShape::Packed8(*database),
            Self::Packed16 { database, .. } => LeafShape::Packed16(*database),
        }
    }
    fn view(&self) -> LeafSlice<'_> {
        match self {
            Self::Generic(entries) => LeafSlice::Generic(entries),
            Self::Packed8 { database, entries } => LeafSlice::Packed8(*database, entries),
            Self::Packed16 { database, entries } => LeafSlice::Packed16(*database, entries),
        }
    }
    fn len(&self) -> usize {
        self.view().len()
    }
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn maximum(&self) -> Key {
        self.view()
            .get(self.len() - 1)
            .expect("nonempty published leaf")
            .owned_key()
    }
    fn push(&mut self, entry: Entry) {
        assert!(self.shape().accepts(&entry));
        match self {
            Self::Generic(entries) => entries.push(entry),
            Self::Packed8 { entries, .. } => entries.push(Packed8 {
                key: entry.key.bytes.as_slice().try_into().expect("packed shape"),
                value: entry.value.as_slice().try_into().expect("packed shape"),
            }),
            Self::Packed16 { entries, .. } => entries.push(Packed16 {
                key: entry.key.bytes.as_slice().try_into().expect("packed shape"),
                value: entry.value.as_slice().try_into().expect("packed shape"),
            }),
        }
    }
    fn clone_into(&self, target: &mut Self) {
        match (self, &mut *target) {
            (Self::Generic(from), Self::Generic(to)) => to.extend(from.iter().cloned()),
            (Self::Packed8 { entries: from, .. }, Self::Packed8 { entries: to, .. }) => {
                to.extend_from_slice(from)
            }
            (Self::Packed16 { entries: from, .. }, Self::Packed16 { entries: to, .. }) => {
                to.extend_from_slice(from)
            }
            _ => unreachable!("private clone retains leaf shape"),
        }
    }
    fn insert(&mut self, entry: Entry) {
        let position = self.view().partition_point(|old| {
            old.key()
                .compare(entry.key.database, entry.key.bytes.as_slice())
                == Ordering::Less
        });
        let exists = self.view().get(position).is_some_and(|old| {
            old.key()
                .compare(entry.key.database, entry.key.bytes.as_slice())
                == Ordering::Equal
        });
        match self {
            Self::Generic(entries) => {
                if exists {
                    entries[position].value = entry.value;
                } else {
                    entries.insert(position, entry);
                }
            }
            Self::Packed8 { entries, .. } => {
                let packed = Packed8 {
                    key: entry.key.bytes.as_slice().try_into().expect("packed shape"),
                    value: entry.value.as_slice().try_into().expect("packed shape"),
                };
                if exists {
                    entries[position].value = packed.value;
                } else {
                    entries.insert(position, packed);
                }
            }
            Self::Packed16 { entries, .. } => {
                let packed = Packed16 {
                    key: entry.key.bytes.as_slice().try_into().expect("packed shape"),
                    value: entry.value.as_slice().try_into().expect("packed shape"),
                };
                if exists {
                    entries[position].value = packed.value;
                } else {
                    entries.insert(position, packed);
                }
            }
        }
    }
    fn split_into(&mut self, target: &mut Self) {
        let middle = self.len() / 2;
        match (&mut *self, &mut *target) {
            (Self::Generic(from), Self::Generic(to)) => to.extend(from.drain(middle..)),
            (Self::Packed8 { entries: from, .. }, Self::Packed8 { entries: to, .. }) => {
                to.extend(from.drain(middle..))
            }
            (Self::Packed16 { entries: from, .. }, Self::Packed16 { entries: to, .. }) => {
                to.extend(from.drain(middle..))
            }
            _ => unreachable!("split retains leaf shape"),
        }
    }
    fn remove(&mut self, database: u64, key: &[u8]) {
        let position = self
            .view()
            .partition_point(|entry| entry.key().compare(database, key) == Ordering::Less);
        if !self
            .view()
            .get(position)
            .is_some_and(|entry| entry.key().compare(database, key) == Ordering::Equal)
        {
            return;
        }
        match self {
            Self::Generic(entries) => {
                entries.remove(position);
            }
            Self::Packed8 { entries, .. } => {
                entries.remove(position);
            }
            Self::Packed16 { entries, .. } => {
                entries.remove(position);
            }
        }
    }
}
#[derive(Clone)]
struct Child {
    maximum: Key,
    node: Arc<Node>,
}
enum Body {
    Leaf(Leaf),
    Branch(Vec<Child>),
}
struct Node {
    body: Body,
    _charge: Charge<CHARGE_NODE>,
}
impl Node {
    fn maximum(&self) -> Key {
        match &self.body {
            Body::Leaf(entries) => entries.maximum(),
            Body::Branch(children) => children
                .last()
                .expect("nonempty published branch")
                .maximum
                .clone(),
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
const CHARGE_NODE: u8 = 0;
const CHARGE_LARGE: u8 = 1;
const CHARGE_METADATA: u8 = 2;
struct Charge<const KIND: u8> {
    quota: Arc<Quota>,
    bytes: usize,
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
impl<const KIND: u8> Charge<KIND> {
    fn new(quota: &Arc<Quota>, bytes: usize) -> Result<Self> {
        quota.reserve(bytes)?;
        match KIND {
            CHARGE_NODE => {
                quota.nodes.fetch_add(1, AtomicOrdering::Relaxed);
            }
            CHARGE_LARGE => {
                quota
                    .large_allocations
                    .fetch_add(1, AtomicOrdering::Relaxed);
                quota.large_bytes.fetch_add(bytes, AtomicOrdering::Relaxed);
            }
            CHARGE_METADATA => {}
            _ => unreachable!("private charge kind"),
        }
        Ok(Self {
            quota: quota.clone(),
            bytes,
        })
    }
}
impl<const KIND: u8> Drop for Charge<KIND> {
    fn drop(&mut self) {
        self.quota
            .charged
            .fetch_sub(self.bytes, AtomicOrdering::Relaxed);
        match KIND {
            CHARGE_NODE => {
                self.quota.nodes.fetch_sub(1, AtomicOrdering::Relaxed);
            }
            CHARGE_LARGE => {
                self.quota
                    .large_allocations
                    .fetch_sub(1, AtomicOrdering::Relaxed);
                self.quota
                    .large_bytes
                    .fetch_sub(self.bytes, AtomicOrdering::Relaxed);
            }
            CHARGE_METADATA => {}
            _ => unreachable!("private charge kind"),
        }
    }
}

/// Shared-quota retained metadata capacity. Own this ticket inside the exact
/// Snapshot/catalog allocation it covers so final-owner drop returns capacity.
/// The ticket is deliberately not Clone: sharing its containing Arc shares one
/// allocation; cloning that allocation requires a new reservation first.
pub(super) struct MetadataReservation {
    charge: Charge<CHARGE_METADATA>,
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
            charge: Charge::<CHARGE_METADATA>::new(&self.quota, bytes)?,
        })
    }
    fn find(&self, database: u64, key: &[u8]) -> Option<EntryRef<'_>> {
        let mut node = self.root.as_deref()?;
        loop {
            match &node.body {
                Body::Leaf(entries) => {
                    let view = entries.view();
                    let index = view.partition_point(|entry| {
                        entry.key().compare(database, key) == Ordering::Less
                    });
                    return view
                        .get(index)
                        .filter(|entry| entry.key().compare(database, key) == Ordering::Equal);
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
        self.find(database, key).map(EntryRef::value)
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
                if prior.is_some_and(|entry| entry.value() == value.as_slice()) {
                    continue;
                }
                let existed = prior.is_some();
                let old_bytes = prior.map_or(Ok(0), EntryRef::encoded_bytes)?;
                let key = match prior {
                    Some(entry) => entry.owned_key(),
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
                            maximum: root.maximum(),
                            node: root.clone(),
                        });
                        children.push(Child {
                            maximum: right.maximum(),
                            node: right,
                        });
                        next.root = Some(Arc::new(parent));
                    }
                } else {
                    let mut node = leaf_node(&next.quota, LeafShape::entry(&entry))?;
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
                let old_bytes = prior.encoded_bytes()?;
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
                    .key()
                    .compare(last.database(), last.key().bytes.as_slice())
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
        let first = cursor.next().filter(|entry| entry.database() == database);
        let (first, terminal) = match terminal {
            Bound::Unbounded => (first, None),
            bound => {
                let mut end = Cursor::seek(self.root.as_deref(), database, bound, !reverse);
                match (first, end.next()) {
                    (Some(first), Some(last)) if last.database() == database => {
                        let order = first
                            .key()
                            .compare(last.database(), last.key().bytes.as_slice());
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
            first: first.map(ScanFirst::new),
            terminal: terminal.map(EntryRef::identity),
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
// Borrow the selected source row until its final packed/generic destination
// is known. Packed rows never reconstruct temporary generic Bytes/Entry.
#[derive(Clone, Copy)]
enum MergeStep<'a> {
    Keep(EntryRef<'a>),
    Insert(&'a Change),
    Match(EntryRef<'a>, &'a Change),
}
impl<'a> MergeStep<'a> {
    fn before(self) -> Option<EntryRef<'a>> {
        match self {
            Self::Keep(old) | Self::Match(old, _) => Some(old),
            Self::Insert(_) => None,
        }
    }
    fn row(self) -> Option<(u64, &'a [u8], &'a [u8])> {
        match self {
            Self::Keep(old) => Some(old.row()),
            Self::Insert(change) => change
                .value
                .as_ref()
                .map(|value| (change.database, change.key.as_slice(), value.as_slice())),
            Self::Match(old, change) => change.value.as_ref().map(|value| {
                let (db, key, _) = old.row();
                (db, key, value.as_slice())
            }),
        }
    }
    fn changed(self) -> bool {
        match self {
            Self::Keep(_) => false,
            Self::Insert(change) => change.value.is_some(),
            Self::Match(old, change) => {
                change.value.as_ref().map(Bytes::as_slice) != Some(old.value())
            }
        }
    }
    fn emits(self) -> bool {
        match self {
            Self::Keep(_) => true,
            Self::Insert(change) | Self::Match(_, change) => change.value.is_some(),
        }
    }
    fn entry(self, quota: &Arc<Quota>) -> Result<Option<Entry>> {
        Ok(Some(match self {
            Self::Keep(old) => old.owned(),
            Self::Insert(change) => {
                let Some(value) = &change.value else {
                    return Ok(None);
                };
                Entry {
                    key: Key {
                        database: change.database,
                        bytes: change.key.admit(quota)?,
                    },
                    value: value.admit(quota)?,
                }
            }
            Self::Match(old, change) => {
                let Some(value) = &change.value else {
                    return Ok(None);
                };
                if value.as_slice() == old.value() {
                    old.owned()
                } else {
                    Entry {
                        key: old.owned_key(),
                        value: value.admit(quota)?,
                    }
                }
            }
        }))
    }
    fn push(self, target: &mut Leaf, quota: &Arc<Quota>) -> Result<()> {
        match target {
            Leaf::Generic(entries) => {
                if let Some(entry) = self.entry(quota)? {
                    entries.push(entry)
                }
            }
            Leaf::Packed8 { database, entries } => {
                let row = match self {
                    Self::Keep(EntryRef::Packed8(db, entry)) => {
                        debug_assert_eq!(db, *database);
                        *entry
                    }
                    _ => {
                        let (db, key, value) = self.row().expect("selected packed row");
                        debug_assert_eq!(db, *database);
                        Packed8 {
                            key: key.try_into().expect("checked packed key"),
                            value: value.try_into().expect("checked packed value"),
                        }
                    }
                };
                entries.push(row);
            }
            Leaf::Packed16 { database, entries } => {
                let row = match self {
                    Self::Keep(EntryRef::Packed16(db, entry)) => {
                        debug_assert_eq!(db, *database);
                        *entry
                    }
                    _ => {
                        let (db, key, value) = self.row().expect("selected packed row");
                        debug_assert_eq!(db, *database);
                        Packed16 {
                            key: key.try_into().expect("checked packed key"),
                            value: value.try_into().expect("checked packed value"),
                        }
                    }
                };
                entries.push(row);
            }
        }
        Ok(())
    }
}
struct LeafMerge<'a> {
    old: LeafSlice<'a>,
    order: &'a [usize],
    changes: &'a [Change],
    old_at: usize,
    change_at: usize,
}
impl<'a> LeafMerge<'a> {
    fn new(old: LeafSlice<'a>, order: &'a [usize], changes: &'a [Change]) -> Self {
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
        let cmp = match (old, change) {
            (Some(old), Some(change)) => old.key().compare(change.database, change.key.as_slice()),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => return None,
        };
        Some(match cmp {
            Ordering::Less => {
                self.old_at += 1;
                MergeStep::Keep(old.unwrap())
            }
            Ordering::Greater => {
                self.change_at += 1;
                MergeStep::Insert(change.unwrap())
            }
            Ordering::Equal => {
                self.old_at += 1;
                self.change_at += 1;
                MergeStep::Match(old.unwrap(), change.unwrap())
            }
        })
    }
    fn push_next(&mut self, target: &mut Leaf, quota: &Arc<Quota>) -> Result<bool> {
        while let Some(step) = self.step() {
            if step.emits() {
                step.push(target, quota)?;
                return Ok(true);
            }
        }
        Ok(false)
    }
}
fn emit_node(output: &mut ScratchVec<'_, Child>, node: Arc<Node>) -> Result<()> {
    output.push(Child {
        maximum: node.maximum(),
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
// A typed old leaf already proves the complete old-row shape. Validate the
// final changes once, then move unchanged Copy spans rather than rebuilding
// a generic descriptor for each old row.
trait CopyRow: Copy {
    const KEY_BYTES: usize;
    fn key(&self) -> &[u8];
    fn value(&self) -> &[u8; 8];
    fn changed(key: &[u8], value: &[u8]) -> Self;
    fn shape(database: u64) -> LeafShape;
    fn target(leaf: &mut Leaf) -> &mut Vec<Self>;
}
impl CopyRow for Packed8 {
    const KEY_BYTES: usize = 8;
    fn key(&self) -> &[u8] {
        &self.key
    }
    fn value(&self) -> &[u8; 8] {
        &self.value
    }
    fn changed(key: &[u8], value: &[u8]) -> Self {
        Self {
            key: key.try_into().expect("validated key width"),
            value: value.try_into().expect("validated value width"),
        }
    }
    fn shape(database: u64) -> LeafShape {
        LeafShape::Packed8(database)
    }
    fn target(leaf: &mut Leaf) -> &mut Vec<Self> {
        let Leaf::Packed8 { entries, .. } = leaf else {
            unreachable!("typed target")
        };
        entries
    }
}
impl CopyRow for Packed16 {
    const KEY_BYTES: usize = 16;
    fn key(&self) -> &[u8] {
        &self.key
    }
    fn value(&self) -> &[u8; 8] {
        &self.value
    }
    fn changed(key: &[u8], value: &[u8]) -> Self {
        Self {
            key: key.try_into().expect("validated key width"),
            value: value.try_into().expect("validated value width"),
        }
    }
    fn shape(database: u64) -> LeafShape {
        LeafShape::Packed16(database)
    }
    fn target(leaf: &mut Leaf) -> &mut Vec<Self> {
        let Leaf::Packed16 { entries, .. } = leaf else {
            unreachable!("typed target")
        };
        entries
    }
}
struct CopyOutput<'o, 's, 'q, T: CopyRow> {
    output: &'o mut ScratchVec<'s, Child>,
    quota: &'q Arc<Quota>,
    database: u64,
    count: usize,
    quotient: usize,
    remainder: usize,
    group: usize,
    node: Option<Node>,
    _row: std::marker::PhantomData<T>,
}
impl<'o, 's, 'q, T: CopyRow> CopyOutput<'o, 's, 'q, T> {
    fn new(
        output: &'o mut ScratchVec<'s, Child>,
        quota: &'q Arc<Quota>,
        database: u64,
        entries: usize,
    ) -> Self {
        let count = entries.div_ceil(LEAF);
        Self {
            output,
            quota,
            database,
            count,
            quotient: if count == 0 { 0 } else { entries / count },
            remainder: if count == 0 { 0 } else { entries % count },
            group: 0,
            node: None,
            _row: std::marker::PhantomData,
        }
    }
    fn copy(&mut self, mut rows: &[T]) -> Result<()> {
        while !rows.is_empty() {
            if self.group >= self.count {
                return Err(Error::InvalidInput("packed copy count"));
            }
            if self.node.is_none() {
                self.node = Some(leaf_node(self.quota, T::shape(self.database))?)
            }
            let node = self.node.as_mut().expect("private copy target");
            let Body::Leaf(leaf) = &mut node.body else {
                unreachable!()
            };
            let target = T::target(leaf);
            let group_len = self.quotient + usize::from(self.group < self.remainder);
            let take = (group_len - target.len()).min(rows.len());
            target.extend_from_slice(&rows[..take]);
            rows = &rows[take..];
            if target.len() == group_len {
                emit_node(
                    self.output,
                    Arc::new(self.node.take().expect("full copy target")),
                )?;
                self.group += 1;
            }
        }
        Ok(())
    }
    fn finish(self) -> Result<()> {
        if self.group != self.count || self.node.is_some() {
            return Err(Error::InvalidInput("packed copy incomplete"));
        }
        Ok(())
    }
}
fn copy_merge<T: CopyRow>(
    node: &Arc<Node>,
    database: u64,
    old: &[T],
    order: &[usize],
    changes: &[Change],
    quota: &Arc<Quota>,
    accounting: &mut GroupAccounting,
    output: &mut ScratchVec<'_, Child>,
) -> Result<Option<bool>> {
    let mut after = old.len();
    let mut dirty = false;
    for &offset in order {
        let change = &changes[offset];
        let key = change.key.as_slice();
        let value = change.value.as_ref().map(Bytes::as_slice);
        if change.database != database
            || key.len() != T::KEY_BYTES
            || value.is_some_and(|v| v.len() != 8)
        {
            return Ok(None);
        }
        match (old.binary_search_by(|entry| entry.key().cmp(key)), value) {
            (Ok(position), Some(value)) => dirty |= old[position].value().as_slice() != value,
            (Ok(_), None) => {
                after = after.checked_sub(1).ok_or(Error::LimitExceeded)?;
                dirty = true;
            }
            (Err(_), Some(_)) => {
                after = after.checked_add(1).ok_or(Error::LimitExceeded)?;
                dirty = true;
            }
            (Err(_), None) => {}
        }
    }
    if !dirty {
        emit_node(output, node.clone())?;
        return Ok(Some(false));
    }
    let row_bytes = 16 + T::KEY_BYTES + 8;
    accounting.add(
        old.len(),
        after,
        old.len()
            .checked_mul(row_bytes)
            .ok_or(Error::LimitExceeded)?,
        after.checked_mul(row_bytes).ok_or(Error::LimitExceeded)?,
    )?;
    let mut target = CopyOutput::<T>::new(output, quota, database, after);
    let mut at = 0;
    for &offset in order {
        let change = &changes[offset];
        let key = change.key.as_slice();
        let found = old.binary_search_by(|entry| entry.key().cmp(key));
        let position = match found {
            Ok(i) | Err(i) => i,
        };
        if position < at {
            return Err(Error::InvalidInput("packed copy input order"));
        }
        target.copy(&old[at..position])?;
        if let Some(value) = &change.value {
            if found.is_ok() && old[position].value().as_slice() == value.as_slice() {
                target.copy(&old[position..position + 1])?;
            } else {
                target.copy(std::slice::from_ref(&T::changed(key, value.as_slice())))?;
            }
        }
        at = position + usize::from(found.is_ok());
    }
    target.copy(&old[at..])?;
    target.finish()?;
    Ok(Some(true))
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
        Some(Body::Leaf(entries)) => entries.view(),
        None => LeafSlice::Generic(&[]),
        _ => unreachable!(),
    };
    let copied = match old {
        LeafSlice::Packed8(database, entries) => copy_merge(
            node.expect("old typed leaf"),
            database,
            entries,
            order,
            changes,
            quota,
            accounting,
            output,
        )?,
        LeafSlice::Packed16(database, entries) => copy_merge(
            node.expect("old typed leaf"),
            database,
            entries,
            order,
            changes,
            quota,
            accounting,
            output,
        )?,
        LeafSlice::Generic(_) => None,
    };
    if let Some(changed) = copied {
        return Ok(changed);
    }
    let (mut before_bytes, mut after_bytes, mut after_entries) = (0usize, 0usize, 0usize);
    let mut changed = false;
    let mut shape = None;
    let mut summary = LeafMerge::new(old, order, changes);
    while let Some(step) = summary.step() {
        changed |= step.changed();
        if let Some(entry) = step.before() {
            before_bytes = before_bytes
                .checked_add(entry.encoded_bytes()?)
                .ok_or(Error::LimitExceeded)?;
        }
        if let Some((database, key, value)) = step.row() {
            let bytes = 16usize
                .checked_add(key.len())
                .and_then(|n| n.checked_add(value.len()))
                .ok_or(Error::LimitExceeded)?;
            after_bytes = after_bytes.checked_add(bytes).ok_or(Error::LimitExceeded)?;
            after_entries = after_entries.checked_add(1).ok_or(Error::LimitExceeded)?;
            let current = match (key.len(), value.len()) {
                (8, 8) => LeafShape::Packed8(database),
                (16, 8) => LeafShape::Packed16(database),
                _ => LeafShape::Generic,
            };
            shape = Some(match (shape, current) {
                (None, s) => s,
                (Some(LeafShape::Packed8(a)), LeafShape::Packed8(b)) if a == b => {
                    LeafShape::Packed8(a)
                }
                (Some(LeafShape::Packed16(a)), LeafShape::Packed16(b)) if a == b => {
                    LeafShape::Packed16(a)
                }
                _ => LeafShape::Generic,
            });
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
        // Mixed shapes use the unchanged generic representation. Choosing a
        // single verified shape for this complete leaf merge removes per-row
        // shape probes and never admits mutable staged bytes into a packed leaf.
        let shape = shape.expect("nonempty merged shape");
        for group in 0..count {
            let mut node = leaf_node(quota, shape)?;
            let Body::Leaf(target) = &mut node.body else {
                unreachable!()
            };
            for _ in 0..quotient + usize::from(group < remainder) {
                assert!(source.push_next(target, quota)?, "counted merged entry");
            }
            emit_node(output, Arc::new(node))?;
        }
        debug_assert!(!std::iter::from_fn(|| source.step()).any(MergeStep::emits));
    }
    Ok(true)
}
fn leaf_node(quota: &Arc<Quota>, shape: LeafShape) -> Result<Node> {
    let charge = Charge::<CHARGE_NODE>::new(
        quota,
        size_of::<Node>() + ARC_HEADER + (LEAF + 1) * shape.entry_size(),
    )?;
    let entries = Leaf::new(shape)?;
    Ok(Node {
        body: Body::Leaf(entries),
        _charge: charge,
    })
}
fn branch_node(quota: &Arc<Quota>) -> Result<Node> {
    let charge = Charge::<CHARGE_NODE>::new(
        quota,
        size_of::<Node>() + ARC_HEADER + (BRANCH + 1) * size_of::<Child>(),
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
                let mut copy = leaf_node(quota, entries.shape())?;
                let Body::Leaf(output) = &mut copy.body else {
                    unreachable!()
                };
                entries.clone_into(output);
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
fn admit_leaf_shape(node: &mut Node, entry: &Entry, quota: &Arc<Quota>) -> Result<()> {
    let Body::Leaf(leaf) = &node.body else {
        return Ok(());
    };
    if leaf.shape().accepts(entry) {
        return Ok(());
    }
    // The old packed capacity remains charged until the replacement actually
    // owns all rows and the old Vec has dropped. Allocation refusal leaves the
    // private page unchanged; the outer prepare still owns rollback.
    let old_bytes = (LEAF + 1) * leaf.shape().entry_size();
    let new_bytes = (LEAF + 1) * size_of::<Entry>();
    let mut replacement_charge = Charge::<CHARGE_METADATA>::new(quota, new_bytes)?;
    let mut replacement = Leaf::new(LeafShape::Generic)?;
    for position in 0..leaf.len() {
        replacement.push(leaf.view().get(position).expect("packed row").owned());
    }
    let old = std::mem::replace(&mut node.body, Body::Leaf(replacement));
    drop(old);
    node._charge.bytes = node._charge.bytes + replacement_charge.bytes - old_bytes;
    replacement_charge.bytes = 0;
    quota.charged.fetch_sub(old_bytes, AtomicOrdering::Relaxed);
    Ok(())
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
    admit_leaf_shape(node, &entry, quota)?;
    match &mut node.body {
        Body::Leaf(entries) => {
            entries.insert(entry);
            if entries.len() <= LEAF {
                return Ok(None);
            }
            let mut sibling = leaf_node(quota, entries.shape())?;
            let Body::Leaf(right) = &mut sibling.body else {
                unreachable!()
            };
            entries.split_into(right);
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
            children[index].maximum = children[index].node.maximum();
            if let Some(right) = right {
                children.insert(
                    index + 1,
                    Child {
                        maximum: right.maximum(),
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
            entries.remove(database, key);
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
                children[index].maximum = children[index].node.maximum();
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
    leaf: LeafSlice<'a>,
    reverse: bool,
}
impl<'a> Cursor<'a> {
    fn new(root: Option<&'a Node>, reverse: bool) -> Self {
        let mut cursor = Self {
            stack: [None; DEPTH],
            depth: 0,
            leaf: LeafSlice::Generic(&[]),
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
                    let entries = entries.view();
                    let next = entries.partition_point(|entry| match (reverse, bound) {
                        (false, Bound::Unbounded) => entry.database() < database,
                        (true, Bound::Unbounded) => entry.database() <= database,
                        (false, Bound::Included(key)) | (true, Bound::Excluded(key)) => {
                            entry.key().compare(database, key) == Ordering::Less
                        }
                        (false, Bound::Excluded(key)) | (true, Bound::Included(key)) => {
                            entry.key().compare(database, key) != Ordering::Greater
                        }
                    });
                    cursor.leaf = if reverse {
                        entries.slice(0, next)
                    } else {
                        entries.slice(next, entries.len())
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
            Body::Leaf(entries) => self.leaf = entries.view(),
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
    fn next_leaf(&mut self) -> Option<EntryRef<'a>> {
        // Match once per row, then advance a typed packed or generic slice.
        // This remains independent of the branch-stack refill path.
        let reverse = self.reverse;
        macro_rules! advance {
            ($entries:ident, $make:expr) => {{
                // Keep the original lifetime while mutating only the slice
                // cursor. The leaf's tag and database remain unchanged.
                let entries = *$entries;
                let (entry, rest) = if reverse {
                    entries.split_last()?
                } else {
                    entries.split_first()?
                };
                *$entries = rest;
                Some(($make)(entry))
            }};
        }
        match &mut self.leaf {
            LeafSlice::Generic(entries) => advance!(entries, EntryRef::Generic),
            LeafSlice::Packed8(database, entries) => {
                let database = *database;
                advance!(entries, |entry| EntryRef::Packed8(database, entry))
            }
            LeafSlice::Packed16(database, entries) => {
                let database = *database;
                advance!(entries, |entry| EntryRef::Packed16(database, entry))
            }
        }
    }
    // Keep the ordinary leaf advance tiny so public iterator wrappers can
    // inline it into callers without importing the branch-stack walker.
    #[inline(always)]
    fn next(&mut self) -> Option<EntryRef<'a>> {
        if let Some(entry) = self.next_leaf() {
            return Some(entry);
        }
        self.refill()
    }
    #[inline(never)]
    fn refill(&mut self) -> Option<EntryRef<'a>> {
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
        Some(entry.row())
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
        Some(entry.row())
    }
}
impl ExactSizeIterator for Iter<'_> {}

/// A borrowed range of one immutable index root. Bounds are resolved at
/// construction, and advancing either end never searches the other database
/// namespaces or compares keys outside the requested range.
pub struct RangeIter<'a> {
    front: Cursor<'a>,
    back: Cursor<'a>,
    first: Option<EntryRef<'a>>,
    last: Option<EntryRef<'a>>,
}
impl<'a> Iterator for RangeIter<'a> {
    type Item = (u64, &'a [u8], &'a [u8]);
    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.first?;
        if entry.same_entry(self.last.expect("range endpoints coexist")) {
            self.first = None;
            self.last = None;
        } else {
            self.first = self.front.next();
        }
        Some(entry.row())
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
        if entry.same_entry(self.first.expect("range endpoints coexist")) {
            self.first = None;
            self.last = None;
        } else {
            self.last = self.back.next();
        }
        Some(entry.row())
    }
}
impl std::iter::FusedIterator for RangeIter<'_> {}

/// A bounded iterator that advances in its construction-time direction.
/// Unlike RangeIter it retains only one traversal stack.
#[derive(Clone, Copy)]
enum ScanFirst<'a> {
    Generic(&'a Entry),
    Packed8(&'a Packed8),
    Packed16(&'a Packed16),
}
impl<'a> ScanFirst<'a> {
    fn new(entry: EntryRef<'a>) -> Self {
        match entry {
            EntryRef::Generic(entry) => Self::Generic(entry),
            EntryRef::Packed8(_, entry) => Self::Packed8(entry),
            EntryRef::Packed16(_, entry) => Self::Packed16(entry),
        }
    }
    #[inline(always)]
    fn identity(self) -> NonZeroUsize {
        let address = match self {
            Self::Generic(entry) => entry as *const Entry as usize,
            Self::Packed8(entry) => entry as *const Packed8 as usize,
            Self::Packed16(entry) => entry as *const Packed16 as usize,
        };
        NonZeroUsize::new(address).expect("borrowed row addresses are nonnull")
    }
    #[inline(always)]
    fn row(self, database: u64) -> (u64, &'a [u8], &'a [u8]) {
        match self {
            Self::Generic(entry) => (
                entry.key.database,
                entry.key.bytes.as_slice(),
                entry.value.as_slice(),
            ),
            Self::Packed8(entry) => (database, &entry.key, &entry.value),
            Self::Packed16(entry) => (database, &entry.key, &entry.value),
        }
    }
}
pub struct ScanIter<'a> {
    cursor: Cursor<'a>,
    first: Option<ScanFirst<'a>>,
    terminal: Option<NonZeroUsize>,
    database: u64,
}
impl<'a> Iterator for ScanIter<'a> {
    type Item = (u64, &'a [u8], &'a [u8]);
    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.first?;
        self.first = match self.terminal {
            Some(last) if entry.identity() == last => None,
            Some(_) => self.cursor.next(),
            None => self
                .cursor
                .next()
                .filter(|next| next.database() == self.database),
        }
        .map(ScanFirst::new);
        Some(entry.row(self.database))
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
    fn compact_scan_preserves_empty_key_identity_old_borrows_and_auto_traits() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<Index>();
        send_sync::<ScanIter<'static>>();
        send_sync::<RangeIter<'static>>();
        let base = Index::new(4 << 20)
            .unwrap()
            .prepare_grouped(&[
                put(1, b"", b""),
                put(1, b"a", b"old"),
                put(1, b"b", b"last"),
                put(2, b"", b"other database"),
                put(2, b"a", b"other value"),
            ])
            .unwrap();
        let mut empty = base.scan(1, Bound::Included(b""), Bound::Included(b""), false);
        assert_eq!(empty.next(), Some((1, b"".as_slice(), b"".as_slice())));
        assert_eq!(empty.next(), None);
        assert_eq!(empty.next(), None);
        let mut backwards = base.scan(1, Bound::Included(b""), Bound::Excluded(b"b"), true);
        let (_, key, borrowed) = backwards.next().unwrap();
        assert_eq!(key, b"a");
        let address = borrowed.as_ptr();
        let next = base
            .prepare(&[put(1, b"a", &[7; 64]), delete(1, b"")])
            .unwrap();
        assert_eq!(borrowed, b"old");
        assert_eq!(base.get(1, b"a").unwrap().as_ptr(), address);
        assert_eq!(backwards.next(), Some((1, b"".as_slice(), b"".as_slice())));
        assert_eq!(backwards.next(), None);
        assert_eq!(backwards.next(), None);
        assert!(next.get(1, b"").is_none());
        assert_eq!(next.get(2, b""), Some(b"other database".as_slice()));
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(size_of::<ScanFirst<'_>>(), 16);
            assert_eq!(size_of::<Option<NonZeroUsize>>(), 8);
            assert_eq!(size_of::<ScanIter<'_>>(), 592);
            assert_eq!(size_of::<Node>(), 56);
        }
    }
    #[test]
    fn database_and_key_accessors_do_not_read_value_bytes() {
        let entry = Entry {
            key: Key {
                database: 42,
                bytes: Bytes::new(b"key").unwrap(),
            },
            // This private malformed value is never inserted into an Index.
            // Accessing row() would panic; namespace/key access must not.
            value: Bytes(Repr::Inline {
                length: 255,
                bytes: [0; INLINE],
            }),
        };
        let borrowed = EntryRef::Generic(&entry);
        assert_eq!(borrowed.database(), 42);
        assert_eq!(borrowed.key().database, 42);
        assert_eq!(borrowed.key().bytes.as_slice(), b"key");
        let invalid_key = Entry {
            key: Key {
                database: u64::MAX,
                bytes: Bytes(Repr::Inline {
                    length: 255,
                    bytes: [0; INLINE],
                }),
            },
            value: Bytes::new(b"value").unwrap(),
        };
        assert_eq!(EntryRef::Generic(&invalid_key).database(), u64::MAX);
    }
    fn leaf_counts(node: &Node) -> (usize, usize, usize) {
        match &node.body {
            Body::Leaf(Leaf::Packed8 { .. }) => (1, 0, 0),
            Body::Leaf(Leaf::Packed16 { .. }) => (0, 1, 0),
            Body::Leaf(Leaf::Generic(_)) => (0, 0, 1),
            Body::Branch(children) => children.iter().fold((0, 0, 0), |count, child| {
                let next = leaf_counts(&child.node);
                (count.0 + next.0, count.1 + next.1, count.2 + next.2)
            }),
        }
    }
    fn assert_node_capacity_charges(node: &Node) -> usize {
        let payload = match &node.body {
            Body::Leaf(leaf) => {
                let capacity = match leaf {
                    Leaf::Generic(entries) => entries.capacity(),
                    Leaf::Packed8 { entries, .. } => entries.capacity(),
                    Leaf::Packed16 { entries, .. } => entries.capacity(),
                };
                assert_eq!(capacity, LEAF + 1);
                capacity * leaf.shape().entry_size()
            }
            Body::Branch(children) => {
                assert_eq!(children.capacity(), BRANCH + 1);
                children.capacity() * size_of::<Child>()
            }
        };
        assert_eq!(node._charge.bytes, size_of::<Node>() + ARC_HEADER + payload);
        node._charge.bytes
            + match &node.body {
                Body::Leaf(_) => 0,
                Body::Branch(children) => children
                    .iter()
                    .map(|child| assert_node_capacity_charges(&child.node))
                    .sum(),
            }
    }
    #[test]
    fn packed_layout_and_charges_match_actual_selected_capacity() {
        #[cfg(target_pointer_width = "64")]
        assert_eq!(size_of::<Entry>(), 56);
        assert_eq!(size_of::<Packed8>(), 16);
        assert_eq!(size_of::<Packed16>(), 24);
        assert!(size_of::<EntryRef<'_>>() <= 24);
        let empty = Index::new(4 << 20).unwrap();
        let baseline = empty.stats().charged_bytes;
        for key_len in [8, 16] {
            let changes: Vec<_> = (0u64..512)
                .map(|n| {
                    let mut key = vec![0; key_len];
                    key[..8].copy_from_slice(&n.to_be_bytes());
                    put(37, &key, &n.to_le_bytes())
                })
                .collect();
            for grouped in [false, true] {
                let index = if grouped {
                    empty.prepare_grouped(&changes)
                } else {
                    empty.prepare(&changes)
                }
                .unwrap();
                let root = index.root.as_deref().unwrap();
                let counts = leaf_counts(root);
                assert_eq!(counts.2, 0);
                assert!(if key_len == 8 {
                    counts.0 > 0 && counts.1 == 0
                } else {
                    counts.1 > 0 && counts.0 == 0
                });
                assert_eq!(
                    index.stats().charged_bytes,
                    baseline + assert_node_capacity_charges(root)
                );
                assert_cached_bytes(&index);
                assert_eq!(
                    owned_rows(&index),
                    changes
                        .iter()
                        .map(|change| (
                            change.database,
                            change.key.as_slice().to_vec(),
                            change.value.as_ref().unwrap().as_slice().to_vec()
                        ))
                        .collect::<Vec<_>>()
                );
                drop(index);
                assert_eq!(empty.stats().charged_bytes, baseline);
            }
        }
    }
    #[test]
    fn packed_point_shape_upgrades_preserve_old_borrows_and_release_overlap() {
        for key_len in [8, 16] {
            let empty = Index::new(4 << 20).unwrap();
            let baseline = empty.stats().charged_bytes;
            let key = vec![0x81; key_len];
            let base = empty.prepare(&[put(7, &key, &[3; 8])]).unwrap();
            let borrowed = base.get(7, &key).unwrap();
            let address = borrowed.as_ptr();
            for change in [
                put(7, &key, &[9; 40]),
                put(8, &key, &[5; 8]),
                put(7, b"shape-change", &[4; 8]),
            ] {
                let next = base.prepare(&[change]).unwrap();
                assert_eq!(leaf_counts(next.root.as_deref().unwrap()), (0, 0, 1));
                assert_node_capacity_charges(next.root.as_deref().unwrap());
                assert_eq!(borrowed, &[3; 8]);
                assert_eq!(base.get(7, &key).unwrap().as_ptr(), address);
                assert_cached_bytes(&next);
                drop(next);
                assert_eq!(empty.stats().large_allocations, 0);
            }
            drop(base);
            assert_eq!(empty.stats().charged_bytes, baseline);
        }
    }
    #[test]
    fn packed_upgrade_refusal_counts_old_and_new_capacity_and_rolls_back() {
        let empty = Index::new(32 << 10).unwrap();
        let base = empty.prepare(&[put(3, &[1; 8], &[2; 8])]).unwrap();
        let before = base.stats();
        let private_copy = base.root.as_ref().unwrap()._charge.bytes;
        let generic_buffer = (LEAF + 1) * size_of::<Entry>();
        let held = base
            .reserve_metadata(
                before.limit_bytes - before.charged_bytes - private_copy - generic_buffer + 1,
            )
            .unwrap();
        let reserved = base.stats().charged_bytes;
        assert!(matches!(
            base.prepare(&[put(3, b"different", &[4; 8])]),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(base.stats().charged_bytes, reserved);
        assert_eq!(base.stats().nodes, before.nodes);
        assert_eq!(base.get(3, &[1; 8]), Some([2; 8].as_slice()));
        drop(held);
        let next = base.prepare(&[put(3, b"different", &[4; 8])]).unwrap();
        assert_eq!(leaf_counts(next.root.as_deref().unwrap()), (0, 0, 1));
        assert_node_capacity_charges(next.root.as_deref().unwrap());
        drop(next);
        assert_eq!(base.stats().charged_bytes, before.charged_bytes);
    }
    #[test]
    fn packed_splits_merges_bounds_and_mixed_ends_match_generic_oracle() {
        let empty = Index::new(16 << 20).unwrap();
        let mut seed = Vec::new();
        for database in [7, 8] {
            for n in 0u64..256 {
                let mut key = n.to_be_bytes().to_vec();
                if database == 8 {
                    key.extend_from_slice(&(!n).to_le_bytes());
                }
                seed.push(put(database, &key, &n.to_le_bytes()));
            }
        }
        let base = empty.prepare_grouped(&seed).unwrap();
        let old_borrow = base.get(8, seed[300].key.as_slice()).unwrap();
        let mut changes = Vec::new();
        for (position, change) in seed.iter().enumerate() {
            if position % 7 == 0 {
                changes.push(delete(change.database, change.key.as_slice()));
            }
            if position % 9 == 0 {
                changes.push(put(change.database, change.key.as_slice(), &[0xff; 8]));
            }
        }
        changes.push(put(7, &[0; 8], &[6; 8]));
        changes.push(delete(7, &[0; 8]));
        changes.push(put(7, &[0; 8], &[7; 8]));
        let point = base.prepare(&changes).unwrap();
        let grouped = base.prepare_grouped(&changes).unwrap();
        assert_eq!(owned_rows(&point), owned_rows(&grouped));
        assert_eq!(old_borrow, seed[300].value.as_ref().unwrap().as_slice());
        let lower_key = 50u64.to_be_bytes();
        let upper_key = 200u64.to_be_bytes();
        for database in [6, 7, 8, 9] {
            let rows: Vec<_> = grouped
                .iter()
                .filter(|(db, _, _)| *db == database)
                .collect();
            let bounds = [
                Bound::Unbounded,
                Bound::Included(lower_key.as_slice()),
                Bound::Excluded(upper_key.as_slice()),
            ];
            for lower in bounds {
                for upper in bounds {
                    let expected: Vec<_> = rows
                        .iter()
                        .copied()
                        .filter(|(_, key, _)| {
                            (match lower {
                                Bound::Unbounded => true,
                                Bound::Included(bound) => *key >= bound,
                                Bound::Excluded(bound) => *key > bound,
                            }) && (match upper {
                                Bound::Unbounded => true,
                                Bound::Included(bound) => *key <= bound,
                                Bound::Excluded(bound) => *key < bound,
                            })
                        })
                        .collect();
                    let mut range = grouped.range(database, lower, upper);
                    let (mut front, mut back) = (0, expected.len());
                    while front < back {
                        if front % 2 == 0 {
                            assert_eq!(range.next(), Some(expected[front]));
                            front += 1;
                        } else {
                            back -= 1;
                            assert_eq!(range.next_back(), Some(expected[back]));
                        }
                    }
                    assert!(range.next().is_none());
                    assert!(range.next_back().is_none());
                    assert_eq!(
                        grouped
                            .scan(database, lower, upper, false)
                            .collect::<Vec<_>>(),
                        expected
                    );
                    assert_eq!(
                        grouped
                            .scan(database, lower, upper, true)
                            .collect::<Vec<_>>(),
                        expected.into_iter().rev().collect::<Vec<_>>()
                    );
                }
            }
        }
        assert_cached_bytes(&point);
        assert_cached_bytes(&grouped);
        assert_node_capacity_charges(grouped.root.as_deref().unwrap());
    }
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

#[cfg(test)]
#[test]
fn packed_layout_preserves_node_header_budget() {
    println!(
        "Node={} Body={} Leaf={} Entry={} Child={} Metadata={} Charge={} Scan={} Iter={}",
        size_of::<Node>(),
        size_of::<Body>(),
        size_of::<Leaf>(),
        size_of::<Entry>(),
        size_of::<Child>(),
        size_of::<MetadataReservation>(),
        size_of::<Charge<CHARGE_NODE>>(),
        size_of::<ScanIter>(),
        size_of::<Iter>()
    );
    assert!(
        size_of::<Node>() <= 56,
        "generic Node grew beyond retained11"
    );
}
#[cfg(test)]
#[test]
fn direct_packed_merge_late_generic_admission_refunds_emitted_pages() {
    let empty = Index::new(256 << 10).unwrap();
    let seed: Vec<_> = (0u128..96)
        .map(|n| Change {
            database: 1,
            key: Bytes::new(&n.to_be_bytes()).unwrap(),
            value: Some(Bytes::new(&[1; 8]).unwrap()),
        })
        .collect();
    let base = empty.prepare_grouped(&seed).unwrap();
    let key = 0u128.to_be_bytes();
    let held = base.get(1, &key).unwrap();
    let address = held.as_ptr();
    let before = base.stats();
    let mut changes: Vec<_> = (0u128..256)
        .map(|n| Change {
            database: 1,
            key: Bytes::new(&n.to_be_bytes()).unwrap(),
            value: Some(Bytes::new(&[9; 8]).unwrap()),
        })
        .collect();
    changes.push(Change {
        database: 2,
        key: Bytes::new(&[255; 16]).unwrap(),
        value: Some(Bytes::new(&vec![7; 400_000]).unwrap()),
    });
    assert!(matches!(
        base.prepare_grouped(&changes),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(base.stats().charged_bytes, before.charged_bytes);
    assert_eq!(base.stats().nodes, before.nodes);
    assert_eq!(base.stats().large_allocations, before.large_allocations);
    assert_eq!(held, &[1; 8]);
    assert_eq!(held.as_ptr(), address);
    assert_eq!(base.len(), 96);
    assert!(base.iter().all(|(db, _, value)| db == 1 && value == [1; 8]));
}
#[cfg(test)]
#[test]
fn typed_copy_runs_split_final_partitions_and_keep_old_snapshot_bytes() {
    let seed: Vec<_> = (0u128..96)
        .map(|n| Change {
            database: 1,
            key: Bytes::new(&(n * 4).to_be_bytes()).unwrap(),
            value: Some(Bytes::new(&(n as u64).to_le_bytes()).unwrap()),
        })
        .collect();
    let base = Index::new(4 << 20).unwrap().prepare_grouped(&seed).unwrap();
    let held_key = (31u128 * 4).to_be_bytes();
    let held = base.get(1, &held_key).unwrap();
    let address = held.as_ptr();
    let mut changes: Vec<_> = (0u128..160)
        .map(|n| Change {
            database: 1,
            key: Bytes::new(&(n * 4 + 1).to_be_bytes()).unwrap(),
            value: Some(Bytes::new(&(10_000 + n as u64).to_le_bytes()).unwrap()),
        })
        .collect();
    for n in [5u128, 31, 32, 63, 64] {
        changes.push(Change {
            database: 1,
            key: Bytes::new(&(n * 4).to_be_bytes()).unwrap(),
            value: Some(Bytes::new(&(11_000 + n as u64).to_le_bytes()).unwrap()),
        });
    }
    for n in [0u128, 20, 47, 60, 70, 95] {
        changes.push(Change {
            database: 1,
            key: Bytes::new(&(n * 4).to_be_bytes()).unwrap(),
            value: None,
        });
    }
    changes.reverse();
    changes.push(Change {
        database: 1,
        key: Bytes::new(&3u128.to_be_bytes()).unwrap(),
        value: None,
    });
    changes.push(Change {
        database: 1,
        key: Bytes::new(&(5u128 * 4).to_be_bytes()).unwrap(),
        value: None,
    });
    changes.push(Change {
        database: 1,
        key: Bytes::new(&(5u128 * 4).to_be_bytes()).unwrap(),
        value: Some(Bytes::new(&99u64.to_le_bytes()).unwrap()),
    });
    let mut expected: std::collections::BTreeMap<_, _> = base
        .iter()
        .map(|(db, key, value)| ((db, key.to_vec()), value.to_vec()))
        .collect();
    for change in &changes {
        let key = (change.database, change.key.as_slice().to_vec());
        match &change.value {
            Some(value) => {
                expected.insert(key, value.as_slice().to_vec());
            }
            None => {
                expected.remove(&key);
            }
        }
    }
    let next = base.prepare_grouped(&changes).unwrap();
    assert_eq!(
        next.iter()
            .map(|(db, key, value)| ((db, key.to_vec()), value.to_vec()))
            .collect::<std::collections::BTreeMap<_, _>>(),
        expected
    );
    assert_eq!(
        next.encoded_rows_bytes(),
        expected
            .iter()
            .map(|((_, key), value)| 16 + key.len() + value.len())
            .sum::<usize>()
    );
    assert_eq!(held, &31u64.to_le_bytes());
    assert_eq!(held.as_ptr(), address);
    let noops: Vec<_> = next
        .iter()
        .map(|(db, key, value)| Change {
            database: db,
            key: Bytes::new(key).unwrap(),
            value: Some(Bytes::new(value).unwrap()),
        })
        .collect();
    assert!(next.prepare_grouped(&noops).unwrap().same_state(&next));
}
