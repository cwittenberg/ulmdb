//! Optional exact-key front end over stable version-pool heads.
//!
//! Buckets contain safe Arc-owned membership histories. Values remain owned by
//! the caller's View, and replacing a value never edits this index. A complete
//! chain proves absence; a chain exceeding the probe limit falls back to the
//! authoritative ordered index. Published nodes are only removed after the
//! pool's conservative oldest-View floor passes their retirement.
//!
//! Requested bucket, node, key, retirement and preparation capacities share the
//! ordered Index's quota. Allocator usable size/RSS and caller-owned input are
//! separate. A refused optional prepare must permanently detach this Lookup
//! from subsequent snapshots: an incompletely maintained cache is not complete.
use super::index::{Change, Index, MetadataReservation};
use super::versioned::{decode_handle, MembershipProof};
use crate::{Error, Result};
use arc_swap::ArcSwapOption;
use std::{
    collections::{hash_map::RandomState, VecDeque},
    hash::{BuildHasher, Hash, Hasher},
    mem::size_of,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard, OnceLock,
    },
};
use ulmdb_storage::snapshot_versions::{Head, View};

const INLINE: usize = 16;
const MAX_PROBES: usize = 16;
const MAX_BUILD_PROBES: usize = 256;
const BULK_BASE_MINIMUM: usize = 4096;
const ARC_HEADER: usize = 2 * size_of::<usize>();
const FOREVER: u64 = u64::MAX;

enum Key {
    Inline { length: u8, bytes: [u8; INLINE] },
    Large(Box<[u8]>),
}
impl Key {
    fn new(bytes: &[u8]) -> Result<Self> {
        if bytes.len() <= INLINE {
            let mut copy = [0; INLINE];
            copy[..bytes.len()].copy_from_slice(bytes);
            Ok(Self::Inline {
                length: bytes.len() as u8,
                bytes: copy,
            })
        } else {
            let mut copy = Vec::new();
            copy.try_reserve_exact(bytes.len())
                .map_err(|_| Error::LimitExceeded)?;
            copy.extend_from_slice(bytes);
            Ok(Self::Large(copy.into_boxed_slice()))
        }
    }
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Inline { length, bytes } => &bytes[..usize::from(*length)],
            Self::Large(bytes) => bytes,
        }
    }
}
struct Counters {
    nodes: AtomicUsize,
    bytes: AtomicUsize,
}
struct Node {
    hash: u64,
    database: u64,
    key: Key,
    head: Head,
    birth: u64,
    end: AtomicU64,
    next: ArcSwapOption<Node>,
    ticket: MetadataReservation,
    counters: Arc<Counters>,
}
impl Drop for Node {
    fn drop(&mut self) {
        self.counters.nodes.fetch_sub(1, Ordering::Relaxed);
        self.counters
            .bytes
            .fetch_sub(self.ticket.bytes(), Ordering::Relaxed);
    }
}
struct Retired {
    epoch: u64,
    bucket: usize,
    node: Arc<Node>,
}
struct GcCursor {
    previous: Option<Arc<Node>>,
    current: Option<Arc<Node>>,
}
struct Writer {
    epoch: u64,
    pending: Vec<Option<Arc<Node>>>,
    pending_ticket: MetadataReservation,
    touched: Vec<usize>,
    deletes: Vec<(usize, Arc<Node>)>,
    retired: VecDeque<Retired>,
    gc: Option<GcCursor>,
    workspace: MetadataReservation,
    built_nodes: usize,
    published_inserts: u64,
    published_deletes: u64,
    gc_steps: u64,
    initialized_buckets: usize,
}
struct Core {
    buckets: Box<[OnceLock<ArcSwapOption<Node>>]>,
    hash: RandomState,
    max_nodes: usize,
    enabled: AtomicBool,
    retired_count: AtomicUsize,
    counters: Arc<Counters>,
    writer: Mutex<Writer>,
    header: MetadataReservation,
}
#[derive(Debug, Eq, PartialEq)]
pub(super) enum Outcome<'a> {
    Hit(&'a [u8]),
    Absent,
    Fallback,
}
#[derive(Debug, Eq, PartialEq)]
pub(super) enum Resolved<'a> {
    Present { head: Head, value: &'a [u8] },
    Absent,
    Fallback,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Stats {
    pub enabled: bool,
    pub buckets: usize,
    pub max_nodes: usize,
    pub live_nodes: usize,
    pub node_bytes: usize,
    pub header_bytes: usize,
    pub workspace_bytes: usize,
    pub retired_nodes: usize,
    pub built_nodes: usize,
    pub published_inserts: u64,
    pub published_deletes: u64,
    pub gc_steps: u64,
    pub base_entries: usize,
    pub base_bytes: usize,
    pub base_capacity: usize,
    pub delta_nodes: usize,
    pub base_build_work: u64,
    pub initialized_buckets: usize,
}

struct Slot {
    database: u64,
    key: Key,
    head: u64,
}
struct FlatTable {
    slots: Box<[Slot]>,
    control: Box<[u8]>,
    entries: usize,
    birth: u64,
    build_work: u64,
    ticket: MetadataReservation,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Binding {
    Head(Head),
    Absent,
    Fallback,
}
fn fingerprint(hash: u64) -> u8 {
    ((hash >> 57) as u8) | 0x80
}
impl FlatTable {
    fn build(index: &Index, core: &Core, birth: u64, view: Option<&View>) -> Result<Arc<Self>> {
        let capacity = index
            .len()
            .checked_mul(5)
            .and_then(|n| n.checked_add(3))
            .map(|n| n / 4)
            .and_then(usize::checked_next_power_of_two)
            .ok_or(Error::LimitExceeded)?
            .max(2);
        let mut bytes = capacity
            .checked_mul(size_of::<Slot>() + size_of::<u8>())
            .and_then(|n| n.checked_add(size_of::<Self>() + ARC_HEADER))
            .ok_or(Error::LimitExceeded)?;
        for (_, key, _) in index.iter() {
            if key.len() > INLINE {
                bytes = bytes.checked_add(key.len()).ok_or(Error::LimitExceeded)?;
            }
        }
        let ticket = index.reserve_metadata(bytes)?;
        let mut slots = filled(capacity, || Slot {
            database: 0,
            key: Key::Inline {
                length: 0,
                bytes: [0; INLINE],
            },
            head: 0,
        })?;
        let mut control = filled(capacity, || 0u8)?;
        let mut build_work = 0u64;
        for (database, key, stored) in index.iter() {
            let head = decode_handle(stored)?;
            if view.is_some_and(|view| view.get(head).is_none()) {
                return Err(Error::Corrupt("snapshot-log flat lookup seed head"));
            }
            let hash = core.hash(database, key);
            let start = hash as usize & (capacity - 1);
            let mut inserted = false;
            for distance in 0..capacity.min(MAX_BUILD_PROBES) {
                build_work = build_work.saturating_add(1);
                let at = start.wrapping_add(distance) & (capacity - 1);
                if control[at] == 0 {
                    slots[at] = Slot {
                        database,
                        key: Key::new(key)?,
                        head: head.to_u64(),
                    };
                    control[at] = fingerprint(hash);
                    inserted = true;
                    break;
                }
            }
            if !inserted {
                return Err(Error::LimitExceeded);
            }
        }
        Ok(Arc::new(Self {
            slots,
            control,
            entries: index.len(),
            birth,
            build_work,
            ticket,
        }))
    }
    #[inline]
    fn binding(&self, hash: u64, database: u64, key: &[u8]) -> Binding {
        let mask = self.control.len() - 1;
        let start = hash as usize & mask;
        let tag = fingerprint(hash);
        for distance in 0..MAX_PROBES.min(self.control.len()) {
            let at = start.wrapping_add(distance) & mask;
            let control = self.control[at];
            if control == 0 {
                return Binding::Absent;
            }
            if control == tag {
                let slot = &self.slots[at];
                if slot.database == database && slot.key.bytes() == key {
                    return Binding::Head(Head::from_u64(slot.head).expect("validated flat head"));
                }
            }
        }
        Binding::Fallback
    }
    fn resolve<'a>(&self, view: &'a View, hash: u64, database: u64, key: &[u8]) -> Resolved<'a> {
        if view.epoch() < self.birth {
            return Resolved::Absent;
        }
        match self.binding(hash, database, key) {
            Binding::Head(head) => view
                .get(head)
                .map(|value| Resolved::Present { head, value })
                .unwrap_or(Resolved::Fallback),
            Binding::Absent => Resolved::Absent,
            Binding::Fallback => Resolved::Fallback,
        }
    }
}

/// One immutable compact base plus a shared membership-only MVCC delta.
pub(super) struct Lookup {
    base: ArcSwapOption<FlatTable>,
    core: Arc<Core>,
    ticket: MetadataReservation,
}
impl Lookup {
    pub(super) fn new(
        index: &Index,
        view: &View,
        buckets: usize,
        max_nodes: usize,
    ) -> Result<Arc<Self>> {
        Self::build(index, view.epoch(), Some(view), buckets, max_nodes)
    }
    fn build(
        index: &Index,
        epoch: u64,
        view: Option<&View>,
        buckets: usize,
        max_nodes: usize,
    ) -> Result<Arc<Self>> {
        let ticket = index.reserve_metadata(size_of::<Self>() + ARC_HEADER)?;
        let core = Core::empty(index, epoch, buckets, max_nodes)?;
        let base = if index.is_empty() {
            None
        } else {
            Some(FlatTable::build(index, &core, epoch, view)?)
        };
        Ok(Arc::new(Self {
            base: ArcSwapOption::new(base),
            core,
            ticket,
        }))
    }
    #[inline]
    pub(super) fn get<'a>(&self, view: &'a View, database: u64, key: &[u8]) -> Outcome<'a> {
        match self.resolve(view, database, key) {
            Resolved::Present { value, .. } => Outcome::Hit(value),
            Resolved::Absent => Outcome::Absent,
            Resolved::Fallback => Outcome::Fallback,
        }
    }
    #[inline]
    pub(super) fn resolve<'a>(&self, view: &'a View, database: u64, key: &[u8]) -> Resolved<'a> {
        if !self.is_enabled() {
            return Resolved::Fallback;
        }
        let hash = self.core.hash(database, key);
        let delta = self.core.delta(view, database, key, hash);
        let result = match delta {
            Resolved::Absent => match self.base.load().as_ref() {
                Some(base) => base.resolve(view, hash, database, key),
                None => Resolved::Absent,
            },
            result => result,
        };
        if self.is_enabled() {
            result
        } else {
            Resolved::Fallback
        }
    }
    pub(super) fn prepare<'a>(
        &'a self,
        epoch: u64,
        changes: &[Change],
        index: &Index,
    ) -> Result<Ready<'a>> {
        self.core.prepare(
            epoch,
            changes,
            index,
            self.base.load().as_ref().map(Arc::as_ref),
            None,
        )
    }
    pub(super) fn prepare_with_next<'a>(
        &'a self,
        epoch: u64,
        changes: &[Change],
        base_index: &Index,
        next_index: &Index,
    ) -> Result<Ready<'a>> {
        if base_index.is_empty()
            && changes.len() >= BULK_BASE_MINIMUM
            && next_index.len() >= BULK_BASE_MINIMUM
        {
            let mut ready = self.core.prepare(epoch, &[], base_index, None, None)?;
            ready.next_lookup = Some(Self::build(
                next_index,
                epoch,
                None,
                self.core.buckets.len(),
                self.core.max_nodes,
            )?);
            Ok(ready)
        } else {
            self.prepare(epoch, changes, base_index)
        }
    }
    pub(super) fn prepare_proven_with_next<'a>(
        &'a self,
        epoch: u64,
        proof: &MembershipProof<'_, '_>,
        base_index: &Index,
        view: &View,
        next_index: &Index,
    ) -> Result<Ready<'a>> {
        proof.check(base_index, view, epoch)?;
        if base_index.is_empty()
            && proof.changes().len() >= BULK_BASE_MINIMUM
            && next_index.len() >= BULK_BASE_MINIMUM
        {
            let mut ready = self.core.prepare(epoch, &[], base_index, None, None)?;
            ready.next_lookup = Some(Self::build(
                next_index,
                epoch,
                None,
                self.core.buckets.len(),
                self.core.max_nodes,
            )?);
            Ok(ready)
        } else {
            self.core.prepare(
                epoch,
                proof.changes(),
                base_index,
                self.base.load().as_ref().map(Arc::as_ref),
                Some(proof),
            )
        }
    }
    pub(super) fn collect(&self, floor: u64, work: usize) -> Result<usize> {
        self.core.collect(floor, work)
    }
    pub(super) fn is_enabled(&self) -> bool {
        self.core.is_enabled()
    }
    pub(super) fn disable(&self) {
        self.core.disable();
        self.base.store(None);
    }
    pub(super) fn stats(&self) -> Stats {
        let mut stats = self.core.stats();
        stats.header_bytes += self.ticket.bytes();
        if let Some(base) = self.base.load().as_ref() {
            stats.base_entries = base.entries;
            stats.base_bytes = base.ticket.bytes();
            stats.base_capacity = base.control.len();
            stats.base_build_work = base.build_work;
        }
        stats
    }
}
impl Drop for Lookup {
    fn drop(&mut self) {
        self.disable();
    }
}

fn filled<T>(count: usize, mut make: impl FnMut() -> T) -> Result<Box<[T]>> {
    let mut out = Vec::new();
    out.try_reserve_exact(count)
        .map_err(|_| Error::LimitExceeded)?;
    for _ in 0..count {
        out.push(make());
    }
    Ok(out.into_boxed_slice())
}
fn reserve_vec<T>(out: &mut Vec<T>, count: usize, ticket: &mut MetadataReservation) -> Result<()> {
    if count <= out.capacity() {
        return Ok(());
    }
    let delta = (count - out.capacity())
        .checked_mul(size_of::<T>())
        .ok_or(Error::LimitExceeded)?;
    let before = ticket.bytes();
    ticket.resize(before.checked_add(delta).ok_or(Error::LimitExceeded)?)?;
    if out.try_reserve_exact(count - out.len()).is_err() {
        ticket.resize(before)?;
        return Err(Error::LimitExceeded);
    }
    Ok(())
}
fn reserve_queue<T>(
    out: &mut VecDeque<T>,
    count: usize,
    ticket: &mut MetadataReservation,
) -> Result<()> {
    if count <= out.capacity() {
        return Ok(());
    }
    let delta = (count - out.capacity())
        .checked_mul(size_of::<T>())
        .ok_or(Error::LimitExceeded)?;
    let before = ticket.bytes();
    ticket.resize(before.checked_add(delta).ok_or(Error::LimitExceeded)?)?;
    if out.try_reserve_exact(count - out.len()).is_err() {
        ticket.resize(before)?;
        return Err(Error::LimitExceeded);
    }
    Ok(())
}
impl Core {
    fn empty(index: &Index, epoch: u64, buckets: usize, max_nodes: usize) -> Result<Arc<Self>> {
        if buckets == 0 || !buckets.is_power_of_two() || max_nodes < index.len() {
            return Err(Error::LimitExceeded);
        }
        let bytes = buckets
            .checked_mul(size_of::<OnceLock<ArcSwapOption<Node>>>())
            .and_then(|n| {
                n.checked_add(size_of::<Self>() + ARC_HEADER + size_of::<Counters>() + ARC_HEADER)
            })
            .ok_or(Error::LimitExceeded)?;
        let header = index.reserve_metadata(bytes)?;
        let pending_ticket = index.reserve_metadata(0)?;
        let workspace = index.reserve_metadata(0)?;
        let lookup = Arc::new(Self {
            buckets: filled(buckets, OnceLock::new)?,
            hash: RandomState::new(),
            max_nodes,
            enabled: AtomicBool::new(true),
            retired_count: AtomicUsize::new(0),
            counters: Arc::new(Counters {
                nodes: AtomicUsize::new(0),
                bytes: AtomicUsize::new(0),
            }),
            writer: Mutex::new(Writer {
                epoch,
                pending: Vec::new(),
                pending_ticket,
                touched: Vec::new(),
                deletes: Vec::new(),
                retired: VecDeque::new(),
                gc: None,
                workspace,
                built_nodes: 0,
                published_inserts: 0,
                published_deletes: 0,
                gc_steps: 0,
                initialized_buckets: 0,
            }),
            header,
        });
        Ok(lookup)
    }
    fn hash(&self, database: u64, key: &[u8]) -> u64 {
        let mut hasher = self.hash.build_hasher();
        database.hash(&mut hasher);
        key.hash(&mut hasher);
        hasher.finish()
    }
    fn bucket(&self, hash: u64) -> usize {
        hash as usize & (self.buckets.len() - 1)
    }
    fn node(
        &self,
        index: &Index,
        hash: u64,
        database: u64,
        key: &[u8],
        head: Head,
        birth: u64,
        next: Option<Arc<Node>>,
    ) -> Result<Arc<Node>> {
        if self.counters.nodes.load(Ordering::Relaxed) >= self.max_nodes {
            return Err(Error::LimitExceeded);
        }
        let bytes = (size_of::<Node>() + ARC_HEADER)
            .checked_add(if key.len() > INLINE { key.len() } else { 0 })
            .ok_or(Error::LimitExceeded)?;
        let ticket = index.reserve_metadata(bytes)?;
        let key = Key::new(key)?;
        let node = Arc::new(Node {
            hash,
            database,
            key,
            head,
            birth,
            end: AtomicU64::new(FOREVER),
            next: ArcSwapOption::new(next),
            ticket,
            counters: self.counters.clone(),
        });
        self.counters.nodes.fetch_add(1, Ordering::Relaxed);
        self.counters.bytes.fetch_add(bytes, Ordering::Relaxed);
        Ok(node)
    }
    #[inline]
    fn delta<'a>(&self, view: &'a View, database: u64, key: &[u8], hash: u64) -> Resolved<'a> {
        if self.counters.nodes.load(Ordering::Relaxed) == 0 {
            return Resolved::Absent;
        }
        let Some(bucket) = self.buckets[self.bucket(hash)].get() else {
            return Resolved::Absent;
        };
        let root = bucket.load();
        match root.as_ref() {
            None => Resolved::Absent,
            Some(node) => Self::walk(node, view, database, key, hash, MAX_PROBES),
        }
    }
    fn walk<'a>(
        node: &Node,
        view: &'a View,
        database: u64,
        key: &[u8],
        hash: u64,
        remaining: usize,
    ) -> Resolved<'a> {
        if node.hash == hash
            && node.database == database
            && node.key.bytes() == key
            && node.birth <= view.epoch()
            && view.epoch() < node.end.load(Ordering::Acquire)
        {
            if let Some(value) = view.get(node.head) {
                return Resolved::Present {
                    head: node.head,
                    value,
                };
            }
            return Resolved::Fallback;
        }
        let next = node.next.load();
        match next.as_ref() {
            None => Resolved::Absent,
            Some(_) if remaining == 1 => Resolved::Fallback,
            Some(next) => Self::walk(next, view, database, key, hash, remaining - 1),
        }
    }
    /// `index` is the base authoritative membership, and changes are unique
    /// final insert/delete effects produced by the versioned transaction Plan.
    pub(super) fn prepare<'a>(
        &'a self,
        epoch: u64,
        changes: &[Change],
        index: &Index,
        base: Option<&FlatTable>,
        proof: Option<&MembershipProof<'_, '_>>,
    ) -> Result<Ready<'a>> {
        let writer = self
            .writer
            .lock()
            .map_err(|_| Error::Corrupt("snapshot-log lookup writer"))?;
        let mut ready = Ready {
            lookup: self,
            writer,
            epoch,
            published: false,
            inserts: 0,
            next_lookup: None,
        };
        if !self.is_enabled() {
            return Err(Error::LimitExceeded);
        }
        if epoch <= ready.writer.epoch || epoch == FOREVER {
            return Err(Error::Corrupt("snapshot-log lookup epoch"));
        }
        let count = changes.len();
        if changes.iter().any(|change| change.value.is_some()) && ready.writer.pending.is_empty() {
            let bytes = self
                .buckets
                .len()
                .checked_mul(size_of::<Option<Arc<Node>>>())
                .ok_or(Error::LimitExceeded)?;
            ready.writer.pending_ticket.resize(bytes)?;
            ready.writer.pending = match filled(self.buckets.len(), || None) {
                Ok(pending) => pending.into_vec(),
                Err(error) => {
                    ready.writer.pending_ticket.resize(0)?;
                    return Err(error);
                }
            };
        }
        let retire_count = changes
            .iter()
            .filter(|change| change.value.is_none())
            .count();
        let retired = ready
            .writer
            .retired
            .len()
            .checked_add(retire_count)
            .ok_or(Error::LimitExceeded)?;
        if retired > self.max_nodes {
            return Err(Error::LimitExceeded);
        }
        let Writer {
            touched,
            deletes,
            retired: queue,
            workspace,
            ..
        } = &mut *ready.writer;
        reserve_vec(touched, count.min(self.buckets.len()), workspace)?;
        reserve_vec(deletes, retire_count, workspace)?;
        reserve_queue(queue, retired, workspace)?;
        let mut proven_members = proof.map(MembershipProof::members);
        for change in changes {
            let proven_old = proven_members.as_mut().map(|members| {
                let member = members.next().expect("checked proof covers each effect");
                debug_assert!(std::ptr::eq(member.change, change));
                member.old_head
            });
            let key = change.key.as_slice();
            let hash = self.hash(change.database, key);
            let bucket = self.bucket(hash);
            if let Some(value) = &change.value {
                if proven_old.is_some_and(|old| old.is_some())
                    || (proven_old.is_none() && index.get(change.database, key).is_some())
                {
                    return Err(Error::Corrupt("snapshot-log lookup non-insert"));
                }
                let head = decode_handle(value.as_slice())?;
                if self.buckets[bucket].get().is_none() {
                    self.buckets[bucket].get_or_init(ArcSwapOption::empty);
                    ready.writer.initialized_buckets += 1;
                }
                let next = ready.writer.pending[bucket].clone().or_else(|| {
                    self.buckets[bucket]
                        .get()
                        .expect("prepared delta bucket")
                        .load_full()
                });
                let node = self.node(index, hash, change.database, key, head, epoch, next)?;
                if ready.writer.pending[bucket].is_none() {
                    ready.writer.touched.push(bucket);
                }
                ready.writer.pending[bucket] = Some(node);
                ready.inserts += 1;
            } else {
                let head = match proven_old {
                    Some(Some(head)) => head,
                    Some(None) => {
                        return Err(Error::Corrupt("snapshot-log lookup proof non-delete"))
                    }
                    None => decode_handle(
                        index
                            .get(change.database, key)
                            .ok_or(Error::Corrupt("snapshot-log lookup non-delete"))?,
                    )?,
                };
                let mut current = self.buckets[bucket]
                    .get()
                    .and_then(|bucket| bucket.load_full());
                let mut found = None;
                for _ in 0..MAX_PROBES {
                    let Some(node) = current else {
                        break;
                    };
                    if node.hash == hash
                        && node.database == change.database
                        && node.key.bytes() == key
                        && node.head == head
                        && node.end.load(Ordering::Acquire) == FOREVER
                    {
                        found = Some(node);
                        break;
                    }
                    current = node.next.load_full();
                }
                if let Some(node) = found {
                    ready.writer.deletes.push((bucket, node));
                } else if !base.is_some_and(|base| {
                    base.binding(hash, change.database, key) == Binding::Head(head)
                }) {
                    return Err(Error::LimitExceeded);
                }
            }
        }
        Ok(ready)
    }
    /// Each bucket-chain link visited consumes one unit; a saved cursor makes
    /// long collision chains resumable instead of restarting each call.
    pub(super) fn collect(&self, floor: u64, work: usize) -> Result<usize> {
        if self.retired_count.load(Ordering::Relaxed) == 0 {
            return Ok(0);
        }
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| Error::Corrupt("snapshot-log lookup writer"))?;
        if !self.is_enabled() {
            return Ok(0);
        }
        let mut done = 0;
        while done < work {
            let Some(retired) = writer.retired.front() else {
                break;
            };
            if retired.epoch > floor {
                break;
            }
            let bucket = retired.bucket;
            let target = retired.node.clone();
            let mut cursor = writer.gc.take().unwrap_or_else(|| GcCursor {
                previous: None,
                current: self.buckets[bucket]
                    .get()
                    .expect("retired delta bucket")
                    .load_full(),
            });
            let Some(current) = cursor.current.take() else {
                return Err(Error::Corrupt("snapshot-log lookup retirement"));
            };
            if Arc::ptr_eq(&current, &target) {
                let next = current.next.load_full();
                if let Some(previous) = cursor.previous {
                    previous.next.store(next);
                } else {
                    self.buckets[bucket]
                        .get()
                        .expect("retired delta bucket")
                        .store(next);
                }
                writer.retired.pop_front();
                self.retired_count
                    .store(writer.retired.len(), Ordering::Relaxed);
            } else {
                cursor.current = current.next.load_full();
                cursor.previous = Some(current);
                writer.gc = Some(cursor);
            }
            done += 1;
        }
        writer.gc_steps = writer.gc_steps.saturating_add(done as u64);
        Ok(done)
    }
    pub(super) fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
    /// Revoke this derived index for every snapshot before relinquishing any
    /// links. Old snapshots then use their own authoritative immutable tree.
    pub(super) fn disable(&self) {
        let mut writer = self
            .writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !self.enabled.swap(false, Ordering::AcqRel) {
            return;
        }
        // Clear successor links iteratively; dropping a long collision chain
        // must not recurse through all its Arc-owned tails on the writer stack.
        for pending in &mut writer.pending {
            let mut current = pending.take();
            while let Some(node) = current {
                current = node.next.swap(None);
            }
        }
        for bucket in &self.buckets {
            let Some(bucket) = bucket.get() else {
                continue;
            };
            let mut current = bucket.swap(None);
            while let Some(node) = current {
                current = node.next.swap(None);
            }
        }
        writer.gc = None;
        writer.retired = VecDeque::new();
        self.retired_count.store(0, Ordering::Relaxed);
        writer.touched = Vec::new();
        writer.deletes = Vec::new();
        writer.pending = Vec::new();
        writer
            .pending_ticket
            .resize(0)
            .expect("quota shrink cannot fail");
        writer
            .workspace
            .resize(0)
            .expect("quota shrink cannot fail");
    }
    pub(super) fn stats(&self) -> Stats {
        let writer = self
            .writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Stats {
            enabled: self.is_enabled(),
            buckets: self.buckets.len(),
            max_nodes: self.max_nodes,
            live_nodes: self.counters.nodes.load(Ordering::Relaxed),
            node_bytes: self.counters.bytes.load(Ordering::Relaxed),
            header_bytes: self.header.bytes() + writer.pending_ticket.bytes(),
            workspace_bytes: writer.workspace.bytes(),
            retired_nodes: writer.retired.len(),
            built_nodes: writer.built_nodes,
            published_inserts: writer.published_inserts,
            published_deletes: writer.published_deletes,
            gc_steps: writer.gc_steps,
            base_entries: 0,
            base_bytes: 0,
            base_capacity: 0,
            delta_nodes: self.counters.nodes.load(Ordering::Relaxed),
            base_build_work: 0,
            initialized_buckets: writer.initialized_buckets,
        }
    }
}
pub(super) struct Ready<'a> {
    lookup: &'a Core,
    writer: MutexGuard<'a, Writer>,
    epoch: u64,
    published: bool,
    inserts: usize,
    next_lookup: Option<Arc<Lookup>>,
}
impl Ready<'_> {
    pub(super) fn next_lookup(&self) -> Option<Arc<Lookup>> {
        self.next_lookup.clone()
    }
    pub(super) fn publish(mut self) {
        for index in 0..self.writer.touched.len() {
            let bucket = self.writer.touched[index];
            let head = self.writer.pending[bucket].take();
            self.lookup.buckets[bucket]
                .get()
                .expect("prepared delta bucket")
                .store(head);
        }
        self.writer.published_inserts = self
            .writer
            .published_inserts
            .saturating_add(self.inserts as u64);
        while let Some((bucket, node)) = self.writer.deletes.pop() {
            node.end.store(self.epoch, Ordering::Release);
            self.writer.retired.push_back(Retired {
                epoch: self.epoch,
                bucket,
                node,
            });
            self.writer.published_deletes = self.writer.published_deletes.saturating_add(1);
        }
        self.writer.touched.clear();
        self.lookup
            .retired_count
            .store(self.writer.retired.len(), Ordering::Relaxed);
        self.writer.epoch = self.epoch;
        self.published = true;
    }
}
impl Drop for Ready<'_> {
    fn drop(&mut self) {
        if !self.published {
            for index in 0..self.writer.touched.len() {
                let bucket = self.writer.touched[index];
                let mut current = self.writer.pending[bucket].take();
                while let Some(node) = current {
                    if node.birth != self.epoch {
                        break;
                    }
                    current = node.next.swap(None);
                }
            }
            self.writer.touched.clear();
            self.writer.deletes.clear();
        }
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        self.disable();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        index::Bytes,
        versioned::{link, unlink},
    };
    use super::*;
    use ulmdb_storage::snapshot_versions::{Limits, Mutation, PoolWriter};

    fn limits() -> Limits {
        Limits {
            max_bytes: 1 << 20,
            max_heads: 128,
            max_versions: 256,
            max_views: 128,
        }
    }
    fn bindings(index: &Index, database: u64, keys: &[&[u8]], heads: &[Head]) -> Index {
        let changes: Vec<_> = keys
            .iter()
            .zip(heads)
            .map(|(key, head)| link(database, Bytes::new(key).unwrap(), *head).unwrap())
            .collect();
        index.prepare(&changes).unwrap()
    }
    fn live_lookup(index: &Index, view: &View, buckets: usize, max_nodes: usize) -> Arc<Lookup> {
        // Exercise deliberately colliding delta chains independently of the
        // compact base used by ordinary new/open.
        let core = Core::empty(index, view.epoch(), buckets, max_nodes).unwrap();
        let lookup = Arc::new(Lookup {
            base: ArcSwapOption::empty(),
            core,
            ticket: index
                .reserve_metadata(size_of::<Lookup>() + ARC_HEADER)
                .unwrap(),
        });
        for (database, key, stored) in index.iter() {
            let hash = lookup.core.hash(database, key);
            let bucket = lookup.core.bucket(hash);
            let published = lookup.core.buckets[bucket].get_or_init(ArcSwapOption::empty);
            let node = lookup
                .core
                .node(
                    index,
                    hash,
                    database,
                    key,
                    decode_handle(stored).unwrap(),
                    view.epoch(),
                    published.load_full(),
                )
                .unwrap();
            published.store(Some(node));
        }
        lookup.core.writer.lock().unwrap().initialized_buckets = lookup
            .core
            .buckets
            .iter()
            .filter(|bucket| bucket.get().is_some())
            .count();
        lookup
    }
    #[test]
    fn held_views_observe_insert_delete_reinsert_and_replacements_without_hash_edits() {
        let (mut pool, old, heads) = PoolWriter::seed(0, &[b"old", b"other-db"], limits()).unwrap();
        let index = Index::new(4 << 20).unwrap();
        let index = bindings(&index, 0, &[b""], &heads[..1]);
        let index = bindings(&index, u64::MAX, &[b""], &heads[1..]);
        let lookup = Lookup::new(&index, &old, 256, 128).unwrap();
        assert_eq!(lookup.stats().base_entries, 2);
        assert_eq!(lookup.stats().delta_nodes, 0);
        let first = pool
            .prepare(
                1,
                &[Mutation {
                    head: Some(heads[0]),
                    value: Some(b"updated"),
                }],
            )
            .unwrap()
            .publish();
        assert_eq!(lookup.get(&old, 0, b""), Outcome::Hit(b"old"));
        assert_eq!(lookup.get(&first, 0, b""), Outcome::Hit(b"updated"));
        assert_eq!(lookup.get(&first, u64::MAX, b""), Outcome::Hit(b"other-db"));
        assert_eq!(lookup.stats().published_inserts, 0);
        let pool_ready = pool
            .prepare(
                2,
                &[Mutation {
                    head: None,
                    value: Some(b"binary"),
                }],
            )
            .unwrap();
        let key = [255; 40];
        let change = link(0, Bytes::new(&key).unwrap(), pool_ready.heads()[0]).unwrap();
        let ready = lookup
            .prepare(2, std::slice::from_ref(&change), &index)
            .unwrap();
        let next_index = index.prepare(std::slice::from_ref(&change)).unwrap();
        assert_eq!(lookup.get(&first, 0, &key), Outcome::Absent);
        let second = pool_ready.publish();
        ready.publish();
        let index = next_index;
        assert_eq!(lookup.get(&second, 0, &key), Outcome::Hit(b"binary"));
        assert_eq!(lookup.get(&old, 0, &key), Outcome::Absent);
        let pool_ready = pool
            .prepare(
                3,
                &[Mutation {
                    head: Some(heads[0]),
                    value: None,
                }],
            )
            .unwrap();
        let change = unlink(0, Bytes::new(b"").unwrap());
        let ready = lookup
            .prepare(3, std::slice::from_ref(&change), &index)
            .unwrap();
        let next_index = index.prepare(std::slice::from_ref(&change)).unwrap();
        let deleted = pool_ready.publish();
        ready.publish();
        assert_eq!(lookup.get(&deleted, 0, b""), Outcome::Fallback);
        assert!(next_index.get(0, b"").is_none());
        assert_eq!(lookup.get(&old, 0, b""), Outcome::Hit(b"old"));
        assert_eq!(lookup.collect(pool.oldest_view_epoch(), 100).unwrap(), 0);
        drop(old);
        drop(first);
        drop(second);
        pool.collect(100);
        assert_eq!(pool.oldest_view_epoch(), 3);
        assert_eq!(lookup.collect(3, 100).unwrap(), 0);
        assert_eq!(lookup.stats().retired_nodes, 0);
        let pool_ready = pool
            .prepare(
                4,
                &[Mutation {
                    head: None,
                    value: Some(b"reborn"),
                }],
            )
            .unwrap();
        assert_ne!(pool_ready.heads()[0], heads[0]);
        let change = link(0, Bytes::new(b"").unwrap(), pool_ready.heads()[0]).unwrap();
        let ready = lookup
            .prepare(4, std::slice::from_ref(&change), &next_index)
            .unwrap();
        let reborn = pool_ready.publish();
        ready.publish();
        assert_eq!(lookup.get(&reborn, 0, b""), Outcome::Hit(b"reborn"));
        assert_eq!(lookup.get(&deleted, 0, b""), Outcome::Fallback);
        assert_eq!(lookup.stats().published_inserts, 2);
    }
    #[test]
    fn aborted_preparation_and_quota_refusal_preserve_complete_base() {
        let (mut pool, old, heads) = PoolWriter::seed(0, &[b"old"], limits()).unwrap();
        let index = bindings(&Index::new(64 << 10).unwrap(), 3, &[b"a"], &heads);
        let lookup = Lookup::new(&index, &old, 8, 128).unwrap();
        let pool_ready = pool
            .prepare(
                1,
                &[Mutation {
                    head: None,
                    value: Some(b"new"),
                }],
            )
            .unwrap();
        let change = link(3, Bytes::new(b"new").unwrap(), pool_ready.heads()[0]).unwrap();
        let nodes = lookup.stats().node_bytes;
        let ready = lookup
            .prepare(1, std::slice::from_ref(&change), &index)
            .unwrap();
        assert_eq!(lookup.get(&old, 3, b"new"), Outcome::Absent);
        drop(ready);
        assert_eq!(lookup.stats().node_bytes, nodes);
        assert_eq!(lookup.get(&old, 3, b"a"), Outcome::Hit(b"old"));
        let available = index.stats().limit_bytes - index.stats().charged_bytes;
        let pressure = index.reserve_metadata(available - 1).unwrap();
        assert!(lookup
            .prepare(1, std::slice::from_ref(&change), &index)
            .is_err());
        assert_eq!(lookup.stats().node_bytes, nodes);
        drop(pressure);
        drop(
            lookup
                .prepare(1, std::slice::from_ref(&change), &index)
                .unwrap(),
        );
        drop(pool_ready);
        let change = unlink(3, Bytes::new(b"a").unwrap());
        drop(
            lookup
                .prepare(1, std::slice::from_ref(&change), &index)
                .unwrap(),
        );
        assert_eq!(lookup.get(&old, 3, b"a"), Outcome::Hit(b"old"));
        assert_eq!(lookup.stats().retired_nodes, 0);
    }
    #[test]
    fn collisions_fall_back_and_gc_resumes_across_new_bucket_prefixes() {
        let keys: Vec<_> = (0u64..32).map(u64::to_le_bytes).collect();
        let values: Vec<_> = keys.iter().map(|key| key.as_slice()).collect();
        let (mut pool, old, heads) = PoolWriter::seed(0, &values, limits()).unwrap();
        let index = bindings(&Index::new(4 << 20).unwrap(), 9, &values, &heads);
        let lookup = live_lookup(&index, &old, 1, 128);
        assert_eq!(lookup.get(&old, 9, &keys[31]), Outcome::Hit(&keys[31]));
        assert_eq!(lookup.get(&old, 9, &keys[0]), Outcome::Fallback);
        assert_eq!(lookup.get(&old, 9, b"missing"), Outcome::Fallback);
        let change = unlink(9, Bytes::new(&keys[20]).unwrap());
        let pool_ready = pool
            .prepare(
                1,
                &[Mutation {
                    head: Some(heads[20]),
                    value: None,
                }],
            )
            .unwrap();
        let ready = lookup
            .prepare(1, std::slice::from_ref(&change), &index)
            .unwrap();
        let next_index = index.prepare(std::slice::from_ref(&change)).unwrap();
        let deleted = pool_ready.publish();
        ready.publish();
        drop(old);
        pool.collect(100);
        assert_eq!(lookup.collect(pool.oldest_view_epoch(), 1).unwrap(), 1);
        assert_eq!(lookup.stats().retired_nodes, 1);
        let pool_ready = pool
            .prepare(
                2,
                &[Mutation {
                    head: None,
                    value: Some(b"new prefix"),
                }],
            )
            .unwrap();
        let key = 32u64.to_le_bytes();
        let change = link(9, Bytes::new(&key).unwrap(), pool_ready.heads()[0]).unwrap();
        let ready = lookup
            .prepare(2, std::slice::from_ref(&change), &next_index)
            .unwrap();
        let latest = pool_ready.publish();
        ready.publish();
        let mut calls = 0;
        while lookup.stats().retired_nodes != 0 {
            assert_eq!(lookup.collect(pool.oldest_view_epoch(), 1).unwrap(), 1);
            calls += 1;
            assert!(calls < 40);
        }
        assert!(calls > 1);
        assert_eq!(lookup.get(&latest, 9, &key), Outcome::Hit(b"new prefix"));
        assert_eq!(lookup.get(&deleted, 9, &key), Outcome::Fallback);
        assert_eq!(lookup.get(&latest, 9, &keys[20]), Outcome::Fallback);
        assert_eq!(lookup.stats().live_nodes, 32);
    }
    #[test]
    fn late_node_limit_refusal_releases_private_prefix_and_all_owned_capacity() {
        let (mut pool, old, heads) = PoolWriter::seed(0, &[b"old"], limits()).unwrap();
        let index = bindings(&Index::new(4 << 20).unwrap(), 1, &[b"a"], &heads);
        let charged = index.stats().charged_bytes;
        let lookup = Lookup::new(&index, &old, 1, 1).unwrap();
        let pool_ready = pool
            .prepare(
                1,
                &[
                    Mutation {
                        head: None,
                        value: Some(b"b"),
                    },
                    Mutation {
                        head: None,
                        value: Some(b"c"),
                    },
                ],
            )
            .unwrap();
        let changes = [
            link(1, Bytes::new(b"b").unwrap(), pool_ready.heads()[0]).unwrap(),
            link(1, Bytes::new(b"c").unwrap(), pool_ready.heads()[1]).unwrap(),
        ];
        let node_bytes = lookup.stats().node_bytes;
        assert!(lookup.prepare(1, &changes, &index).is_err());
        assert_eq!(lookup.stats().live_nodes, 0);
        assert_eq!(lookup.stats().node_bytes, node_bytes);
        assert_eq!(lookup.get(&old, 1, b"a"), Outcome::Hit(b"old"));
        assert_eq!(lookup.get(&old, 1, b"b"), Outcome::Absent);
        assert_eq!(lookup.get(&old, 1, b"c"), Outcome::Absent);
        drop(pool_ready);
        drop(lookup);
        assert_eq!(index.stats().charged_bytes, charged);
    }
    #[test]
    fn disabling_held_lookup_returns_capacity_and_concurrent_readers_fall_back() {
        let keys: Vec<_> = (0u64..8).map(u64::to_le_bytes).collect();
        let values: Vec<_> = keys.iter().map(|key| key.as_slice()).collect();
        let (mut pool, view, heads) = PoolWriter::seed(0, &values, limits()).unwrap();
        let index = bindings(&Index::new(4 << 20).unwrap(), 1, &values, &heads);
        let lookup = Lookup::new(&index, &view, 1, 128).unwrap();
        let held = lookup.clone();
        // Allocate retained preparation/queue capacity, then abort it.
        let change = unlink(1, Bytes::new(&keys[7]).unwrap());
        drop(
            lookup
                .prepare(1, std::slice::from_ref(&change), &index)
                .unwrap(),
        );
        assert!(lookup.stats().workspace_bytes > 0);
        let before = index.stats().charged_bytes;
        let barrier = Arc::new(std::sync::Barrier::new(2));
        std::thread::scope(|scope| {
            let reader_barrier = barrier.clone();
            let lookup = &lookup;
            let view = &view;
            let keys = &keys;
            scope.spawn(move || {
                reader_barrier.wait();
                for _ in 0..1000 {
                    for key in keys {
                        match lookup.get(view, 1, key) {
                            Outcome::Hit(value) => assert_eq!(value, key),
                            Outcome::Fallback => {}
                            Outcome::Absent => panic!("partial eviction proved false absence"),
                        }
                    }
                }
            });
            barrier.wait();
            lookup.disable();
        });
        assert!(!held.is_enabled());
        assert_eq!(held.stats().live_nodes, 0);
        assert_eq!(held.stats().node_bytes, 0);
        assert_eq!(held.stats().workspace_bytes, 0);
        assert!(index.stats().charged_bytes < before);
        assert_eq!(held.get(&view, 1, &keys[0]), Outcome::Fallback);
        assert_eq!(held.get(&view, 1, b"missing"), Outcome::Fallback);
        assert!(held
            .prepare(1, std::slice::from_ref(&change), &index)
            .is_err());
        assert_eq!(held.collect(pool.oldest_view_epoch(), 100).unwrap(), 0);
        // The authoritative snapshot still resolves its original stable heads.
        assert_eq!(view.get(heads[0]), Some(keys[0].as_slice()));
        pool.collect(100);
    }
    #[test]
    fn first_bulk_load_builds_private_compact_base_without_delta_nodes() {
        let limits = Limits {
            max_bytes: 16 << 20,
            max_heads: 8192,
            max_versions: 16384,
            max_views: 128,
        };
        let (mut pool, old, _) = PoolWriter::seed(0, &[], limits).unwrap();
        let index = Index::new(32 << 20).unwrap();
        let lookup = Lookup::new(&index, &old, 256, 8192).unwrap();
        let keys: Vec<_> = (0u64..4096).map(u64::to_be_bytes).collect();
        let mutations: Vec<_> = keys
            .iter()
            .map(|value| Mutation {
                head: None,
                value: Some(value.as_slice()),
            })
            .collect();
        let pool_ready = pool.prepare(1, &mutations).unwrap();
        let changes: Vec<_> = keys
            .iter()
            .zip(pool_ready.heads())
            .map(|(key, head)| link(11, Bytes::new(key).unwrap(), *head).unwrap())
            .collect();
        let next_index = index.prepare_grouped(&changes).unwrap();
        assert_eq!(lookup.stats().base_entries, 0);
        let ready = lookup
            .prepare_with_next(1, &changes, &index, &next_index)
            .unwrap();
        let next = ready
            .next_lookup()
            .expect("first bulk creates compact base");
        assert_eq!(next.stats().base_entries, keys.len());
        assert_eq!(next.stats().base_capacity, 8192);
        assert_eq!(next.stats().delta_nodes, 0);
        assert_eq!(next.stats().workspace_bytes, 0);
        assert_eq!(next.stats().initialized_buckets, 0);
        assert_eq!(lookup.get(&old, 11, &keys[5]), Outcome::Absent);
        assert_eq!(next.get(&old, 11, &keys[5]), Outcome::Absent);
        let view = pool_ready.publish();
        ready.publish();
        for key in &keys {
            match next.get(&view, 11, key) {
                Outcome::Hit(value) => assert_eq!(value, key),
                Outcome::Fallback => assert_eq!(
                    view.get(decode_handle(next_index.get(11, key).unwrap()).unwrap()),
                    Some(key.as_slice())
                ),
                Outcome::Absent => panic!("compact bulk base lost a key"),
            }
        }
        // A later ordinary insert keeps this base and writes only the delta.
        let pool_ready = pool
            .prepare(
                2,
                &[Mutation {
                    head: None,
                    value: Some(b"later"),
                }],
            )
            .unwrap();
        let change = link(11, Bytes::new(b"later").unwrap(), pool_ready.heads()[0]).unwrap();
        let final_index = next_index.prepare(std::slice::from_ref(&change)).unwrap();
        let ready = next
            .prepare_with_next(2, std::slice::from_ref(&change), &next_index, &final_index)
            .unwrap();
        assert!(ready.next_lookup().is_none());
        let latest = pool_ready.publish();
        ready.publish();
        assert_eq!(next.stats().base_entries, keys.len());
        assert_eq!(next.stats().delta_nodes, 1);
        assert_eq!(next.get(&latest, 11, b"later"), Outcome::Hit(b"later"));
        assert_eq!(next.get(&view, 11, b"later"), Outcome::Absent);
    }
    #[test]
    fn compact_base_probe_limit_and_exact_binary_namespace_comparisons() {
        let empty = Index::new(4 << 20).unwrap();
        let core = Core::empty(&empty, 0, 1, 128).unwrap();
        // The secret is local to this test. Force a 32-slot cluster so reads
        // must fall back at16 probes while construction remains complete.
        let keys: Vec<_> = (0u64..)
            .map(u64::to_be_bytes)
            .filter(|key| core.hash(3, key) as usize & 63 == 0)
            .take(32)
            .collect();
        let values: Vec<_> = keys.iter().map(|key| key.as_slice()).collect();
        let (_, view, heads) = PoolWriter::seed(0, &values, limits()).unwrap();
        let index = bindings(&empty, 3, &values, &heads);
        let base = FlatTable::build(&index, &core, 0, Some(&view)).unwrap();
        assert_eq!(base.control.len(), 64);
        let lookup = Lookup {
            base: ArcSwapOption::new(Some(base)),
            core,
            ticket: index
                .reserve_metadata(size_of::<Lookup>() + ARC_HEADER)
                .unwrap(),
        };
        let mut hits = 0;
        let mut fallbacks = 0;
        for key in &keys {
            match lookup.get(&view, 3, key) {
                Outcome::Hit(value) => {
                    hits += 1;
                    assert_eq!(value, key);
                }
                Outcome::Fallback => fallbacks += 1,
                Outcome::Absent => panic!("probe bound manufactured absence"),
            }
        }
        assert_eq!((hits, fallbacks), (16, 16));
        assert_eq!(lookup.stats().base_build_work, 32 * 33 / 2);
        let key = (0u64..)
            .map(u64::to_be_bytes)
            .find(|key| lookup.core.hash(u64::MAX, key) as usize & 63 == 40)
            .unwrap();
        assert_eq!(lookup.get(&view, u64::MAX, &key), Outcome::Absent);
    }
}
