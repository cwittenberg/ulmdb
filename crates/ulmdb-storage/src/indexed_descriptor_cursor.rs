//! Bounded plaintext Indexed descriptor information. No namespace, graph, pin,
//! currentness or unlink authority is created by this module.
use crate::transform::TransformContext;
use crate::{
    DescriptorAuthentication,
    descriptor_source::{DescriptorSource, ReadProgress},
};
use crate::{
    DescriptorCursorBudget, DescriptorFootprintHeader, Error, Manifest, Reference, Result,
    SegmentEntry, manifest::number,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::File,
    mem::ManuallyDrop,
};

const HEADER: usize = 256;
const PAGE: usize = 64;
const HANDLES: usize = 16;
const MAX_ENTRIES: usize = 1_048_575;

/// Every descriptor-byte read and hash, including external origin headers, is
/// charged to `read_bytes`. Work charges include header/local framing, node
/// entry, node decoding, and branch completion, not just emitted leaf rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IndexedDescriptorProgress {
    pub read_bytes: usize,
    pub work_units: usize,
    pub ready: bool,
    /// Supply this exact origin's immutable descriptor extent before resuming.
    /// No opaque resolver callback or unbudgeted descriptor decode is invoked.
    pub needed_origin: Option<u128>,
    pub required_read_bytes: usize,
}

pub struct IndexedDescriptorFootprint {
    authentication: DescriptorAuthentication,
    checkpoint: [u8; 64],
    header: DescriptorFootprintHeader,
    pages: BTreeMap<usize, Vec<SegmentEntry>>,
    origins: BTreeSet<u128>,
    // Move validation indexes into the result as well: finish must not free an
    // inventory-sized tree of temporary allocations outside the step budget.
    _slots: BTreeMap<u64, u32>,
    _seen: BTreeMap<(u128, u64), u32>,
}
impl IndexedDescriptorFootprint {
    pub fn authentication(&self) -> DescriptorAuthentication {
        self.authentication
    }
    pub fn checkpoint(&self) -> &[u8; 64] {
        &self.checkpoint
    }
    pub fn header(&self) -> &DescriptorFootprintHeader {
        &self.header
    }
    pub fn entries(&self) -> impl Iterator<Item = &SegmentEntry> {
        self.pages.values().flat_map(|p| p.iter())
    }
    pub fn entry(&self, index: usize) -> Option<&SegmentEntry> {
        self.pages.get(&(index / PAGE))?.get(index % PAGE)
    }
    pub fn metadata_origins(&self) -> impl Iterator<Item = u128> + '_ {
        self.origins.iter().copied()
    }
    pub fn metadata_origin_after(&self, after: Option<u128>) -> Option<u128> {
        use std::ops::Bound::{Excluded, Unbounded};
        self.origins.range((after.map_or(Unbounded, Excluded), Unbounded)).next().copied()
    }
}

struct Source {
    reader: DescriptorSource,
    length: usize,
    header: [u8; HEADER],
    header_used: usize,
    header_checked: bool,
}
impl Source {
    fn new(file: File, start: u64, length: usize) -> Result<Self> {
        Self::with_transform(file, start, length, None)
    }
    fn with_transform(
        file: File,
        start: u64,
        length: usize,
        context: Option<(u128, Arc<TransformContext>)>,
    ) -> Result<Self> {
        if !(HEADER..=crate::MAX_DESCRIPTOR_BYTES as usize).contains(&length) {
            return Err(Error::Corrupt("indexed descriptor extent"));
        }
        Ok(Self {
            reader: DescriptorSource::new(file, start, length, context)?,
            length,
            header: [0; HEADER],
            header_used: 0,
            header_checked: false,
        })
    }
    fn check(&self) -> Result<()> {
        self.reader.check()
    }
    fn read(
        &mut self,
        at: usize,
        into: &mut [u8],
        budget: DescriptorCursorBudget,
        p: &mut IndexedDescriptorProgress,
    ) -> Result<ReadProgress> {
        let r = self.reader.read(
            at,
            into,
            budget.bytes - p.read_bytes,
            budget.entries - p.work_units,
        )?;
        p.read_bytes += r.read_bytes;
        p.work_units += r.work_units;
        p.required_read_bytes = r.required_read_bytes;
        Ok(r)
    }
}
fn progressed(r: &ReadProgress) -> bool {
    r.copied != 0 || r.read_bytes != 0 || r.work_units != 0
}

#[derive(Clone, Copy, Debug)]
struct NodeRef {
    origin: u128,
    offset: u64,
    length: u32,
    hash: [u8; 32],
}
impl NodeRef {
    fn decode(b: &[u8]) -> Result<Self> {
        let r = Self {
            origin: u128::from_le_bytes(number(b, 0)?),
            offset: u64::from_le_bytes(number(b, 16)?),
            length: u32::from_le_bytes(number(b, 24)?),
            hash: number(b, 32)?,
        };
        if b.len() != 64
            || r.origin == 0
            || r.offset < HEADER as u64
            || !r.offset.is_multiple_of(32)
            || ![96, 160].contains(&r.length)
            || r.offset
                .checked_add(u64::from(r.length))
                .is_none_or(|end| end > crate::MAX_DESCRIPTOR_BYTES)
            || b[28..32] != [0; 4]
        {
            return Err(Error::Corrupt("invalid metadata node reference"));
        }
        Ok(r)
    }
}
#[derive(Clone, Copy)]
struct Summary {
    prefix: u64,
    depth: u8,
    count: usize,
}
enum Stage {
    Enter,
    Left {
        prefix: u64,
        depth: u8,
        right: NodeRef,
    },
    Right {
        prefix: u64,
        depth: u8,
        left: Summary,
    },
    Complete {
        prefix: u64,
        depth: u8,
        left: Summary,
        right: Summary,
    },
}
struct Frame {
    reference: NodeRef,
    parent_depth: Option<u8>,
    stage: Stage,
}
struct Reading {
    reference: NodeRef,
    bytes: [u8; 160],
    used: usize,
    hash: Sha256,
}
struct State {
    selected: Source,
    checkpoint: [u8; 64],
    generation: u128,
    expected_hash: Option<[u8; 32]>,
    hash: Sha256,
    header: Option<DescriptorFootprintHeader>,
    local_count: usize,
    local_at: usize,
    local_buffer: [u8; 160],
    local_used: usize,
    local_length: usize,
    slots: BTreeMap<u64, u32>,
    authenticated: bool,
    frames: Vec<Frame>,
    reading: Option<Reading>,
    sources: BTreeMap<u128, Source>,
    order: VecDeque<u128>,
    seen: BTreeMap<(u128, u64), u32>,
    visits: usize,
    max_entries: usize,
    count: usize,
    prior: u64,
    first_chunk: Option<u64>,
    postorder: usize,
    visited_local: usize,
    pages: BTreeMap<usize, Vec<SegmentEntry>>,
    origins: BTreeSet<u128>,
    ready: bool,
    failed: bool,
}
/// The caller retains the existing immutable-source/liveness contract across
/// steps. Held FDs preserve only their own bytes, not missing external origins.
/// This cursor never opens a path, registers a pin, or authorizes retirement.
/// Optional transforms decode one authenticated frame at a time; no whole
/// descriptor mapping or plaintext temporary file is created.
pub struct IndexedDescriptorCursor {
    creator: u32,
    state: ManuallyDrop<Option<State>>,
}
impl Drop for IndexedDescriptorCursor {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            drop(ManuallyDrop::into_inner(std::mem::replace(
                &mut self.state,
                ManuallyDrop::new(None),
            )));
        }
    }
}
impl IndexedDescriptorCursor {
    /// Metadata-only construction; descriptor bytes are read only by advance.
    pub fn new(
        file: File,
        start: u64,
        length: usize,
        checkpoint: [u8; 64],
        max_entries: usize,
    ) -> Result<Self> {
        Self::build(file, start, length, checkpoint, max_entries, None)
    }
    pub fn new_with_transform(
        file: File,
        start: u64,
        length: usize,
        checkpoint: [u8; 64],
        max_entries: usize,
        context: Arc<TransformContext>,
    ) -> Result<Self> {
        Self::build(file, start, length, checkpoint, max_entries, Some(context))
    }
    /// Observe a generation's content hash, not an externally authenticated
    /// checkpoint. The footprint retains this explicit authentication status.
    pub fn observe_identity(
        file: File,
        start: u64,
        length: usize,
        generation: u128,
        max_entries: usize,
    ) -> Result<Self> {
        Self::observe_identity_with_transform(file, start, length, generation, max_entries, None)
    }
    pub fn observe_identity_with_transform(
        file: File,
        start: u64,
        length: usize,
        generation: u128,
        max_entries: usize,
        context: Option<Arc<TransformContext>>,
    ) -> Result<Self> {
        if generation == 0 {
            return Err(Error::InvalidInput("indexed observed identity"));
        }
        let cp = crate::descriptor_extent::observed_checkpoint(generation, 4, [0; 32]);
        let mut c = Self::build(file, start, length, cp, max_entries, context)?;
        c.state.as_mut().unwrap().expected_hash = None;
        Ok(c)
    }
    fn build(
        file: File,
        start: u64,
        length: usize,
        checkpoint: [u8; 64],
        max_entries: usize,
        context: Option<Arc<TransformContext>>,
    ) -> Result<Self> {
        let (generation, expected_hash) = crate::checkpoint_identity(&checkpoint)?;
        if &checkpoint[..8] != b"ULMDBT04" {
            return Err(Error::InvalidInput("indexed checkpoint required"));
        }
        if max_entries > MAX_ENTRIES {
            return Err(Error::InvalidInput("indexed descriptor limits"));
        }
        let selected =
            Source::with_transform(file, start, length, context.map(|c| (generation, c)))?;
        let mut frames = Vec::new();
        frames
            .try_reserve_exact(65)
            .map_err(|_| Error::LimitExceeded)?;
        Ok(Self {
            creator: std::process::id(),
            state: ManuallyDrop::new(Some(State {
                selected,
                checkpoint,
                generation,
                expected_hash: Some(expected_hash),
                hash: Sha256::new(),
                header: None,
                local_count: 0,
                local_at: HEADER,
                local_buffer: [0; 160],
                local_used: 0,
                local_length: 8,
                slots: BTreeMap::new(),
                authenticated: false,
                frames,
                reading: None,
                sources: BTreeMap::new(),
                order: VecDeque::new(),
                seen: BTreeMap::new(),
                visits: 0,
                max_entries,
                count: 0,
                prior: 0,
                first_chunk: None,
                postorder: HEADER,
                visited_local: 0,
                pages: BTreeMap::new(),
                origins: BTreeSet::new(),
                ready: false,
                failed: false,
            })),
        })
    }
    fn check_creator(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited indexed descriptor cursor"));
        }
        Ok(())
    }
    /// Attach only the exact currently requested origin. Locating its extent,
    /// filename and lifetime is the caller's responsibility; no hidden resolver
    /// or descriptor IO occurs here. At most16 external handles stay owned.
    pub fn supply_origin(
        &mut self,
        origin: u128,
        file: File,
        start: u64,
        length: usize,
    ) -> Result<()> {
        self.supply_origin_with_transform(origin, file, start, length, None)
    }
    pub fn supply_origin_with_transform(
        &mut self,
        origin: u128,
        file: File,
        start: u64,
        length: usize,
        context: Option<Arc<TransformContext>>,
    ) -> Result<()> {
        self.check_creator()?;
        let s = self
            .state
            .as_mut()
            .ok_or(Error::InvalidInput("consumed indexed cursor"))?;
        if s.failed {
            return Err(Error::InvalidInput("failed indexed descriptor cursor"));
        }
        let result = (|| {
            s.selected.check()?;
            if s.needed_origin() != Some(origin) {
                return Err(Error::InvalidInput("unexpected indexed origin"));
            }
            let source = Source::with_transform(file, start, length, context.map(|c| (origin, c)))?;
            if s.sources.len() == HANDLES {
                let old = s
                    .order
                    .pop_front()
                    .ok_or(Error::Corrupt("indexed origin cache"))?;
                s.sources.remove(&old);
            }
            s.sources.insert(origin, source);
            s.order.push_back(origin);
            Ok(())
        })();
        if result.is_err() {
            s.failed = true;
        }
        result
    }
    pub fn advance(&mut self, budget: DescriptorCursorBudget) -> Result<IndexedDescriptorProgress> {
        self.check_creator()?;
        let s = self
            .state
            .as_mut()
            .ok_or(Error::InvalidInput("consumed indexed cursor"))?;
        if s.failed {
            return Err(Error::InvalidInput("failed indexed descriptor cursor"));
        }
        let result = (|| {
            s.selected.check()?;
            let p = s.advance(budget)?;
            s.selected.check()?;
            Ok(p)
        })();
        if result.is_err() {
            s.failed = true;
        }
        result
    }
    pub fn finish(mut self) -> Result<IndexedDescriptorFootprint> {
        self.check_creator()?;
        let s = self
            .state
            .as_ref()
            .ok_or(Error::InvalidInput("consumed indexed cursor"))?;
        if s.failed {
            return Err(Error::InvalidInput("failed indexed descriptor cursor"));
        }
        s.selected.check()?;
        if !s.ready {
            return Err(Error::InvalidInput("indexed descriptor cursor incomplete"));
        }
        let s = self
            .state
            .take()
            .ok_or(Error::InvalidInput("consumed indexed cursor"))?;
        Ok(IndexedDescriptorFootprint {
            authentication: if s.expected_hash.is_some() {
                DescriptorAuthentication::ExpectedCheckpoint
            } else {
                DescriptorAuthentication::ObservedContent
            },
            checkpoint: s.checkpoint,
            header: s
                .header
                .ok_or(Error::Corrupt("indexed descriptor header"))?,
            pages: s.pages,
            origins: s.origins,
            _slots: s.slots,
            _seen: s.seen,
        })
    }
}

fn prefix(id: u64, depth: u8) -> u64 {
    if depth == 0 {
        0
    } else {
        id & (u64::MAX << (64 - depth))
    }
}
fn bit(id: u64, depth: u8) -> bool {
    id & (1u64 << (63 - depth)) != 0
}
impl State {
    fn needed_origin(&self) -> Option<u128> {
        self.reading
            .as_ref()
            .map(|r| r.reference.origin)
            .filter(|o| *o != self.generation && !self.sources.contains_key(o))
    }
    fn decode_header(&mut self) -> Result<()> {
        let b = &self.selected.header;
        let (minor, page_size, value_limit, count) =
            Manifest::decode_prefix_with_length(b, self.selected.length)?;
        if minor != 4 {
            return Err(Error::Corrupt("indexed descriptor version"));
        }
        if count > self.max_entries {
            return Err(Error::LimitExceeded);
        }
        let local = u32::from_le_bytes(number(b, 116)?) as usize;
        if local > count.saturating_mul(2).saturating_sub(1) {
            return Err(Error::Corrupt("metadata local count exceeds tree"));
        }
        let h = DescriptorFootprintHeader {
            format_minor: minor,
            page_size,
            value_limit,
            uuid: number(b, 16)?,
            generation: u128::from_le_bytes(number(b, 32)?),
            epoch: u64::from_le_bytes(number(b, 48)?),
            sequence: u64::from_le_bytes(number(b, 56)?),
            commit: u128::from_le_bytes(number(b, 64)?),
            parent: u128::from_le_bytes(number(b, 80)?),
            previous: u128::from_le_bytes(number(b, 128)?),
            catalog: Reference::decode(&b[96..112])?,
            next_database: u64::from_le_bytes(number(b, 176)?),
            next_segment: u64::from_le_bytes(number(b, 184)?),
            segment_count: count,
        };
        if h.uuid == [0; 16]
            || h.generation != self.generation
            || h.generation == h.previous
            || h.epoch == 0
            || h.commit == 0
            || h.next_database == 0
            || h.next_segment == 0
        {
            return Err(Error::Corrupt("invalid root identities/counters"));
        }
        if count == 0 {
            if b[192..256] != [0; 64] || local != 0 {
                return Err(Error::Corrupt("nonempty metadata root for empty inventory"));
            }
        } else {
            let root = NodeRef::decode(&b[192..256])?;
            if root.origin != self.generation {
                return Err(Error::Corrupt("selected metadata root must be local"));
            }
            self.frames.push(Frame {
                reference: root,
                parent_depth: None,
                stage: Stage::Enter,
            });
        }
        self.local_count = local;
        self.header = Some(h);
        Ok(())
    }
    fn scan(
        &mut self,
        budget: DescriptorCursorBudget,
        p: &mut IndexedDescriptorProgress,
    ) -> Result<bool> {
        if self.selected.header_used < HEADER {
            let take = HEADER - self.selected.header_used;
            let mut bytes = [0; HEADER];
            let at = self.selected.header_used;
            let r = self.selected.read(at, &mut bytes[..take], budget, p)?;
            self.hash.update(&bytes[..r.copied]);
            self.selected.header[at..at + r.copied].copy_from_slice(&bytes[..r.copied]);
            self.selected.header_used += r.copied;
            return Ok(progressed(&r));
        }
        if self.header.is_none() {
            if p.work_units == budget.entries {
                return Ok(false);
            }
            self.decode_header()?;
            p.work_units += 1;
            return Ok(true);
        }
        if self.local_at == self.selected.length {
            if self.slots.len() != self.local_count {
                return Err(Error::Corrupt("metadata local blob count"));
            }
            let hash: [u8; 32] = self.hash.clone().finalize().into();
            if self.expected_hash.is_some_and(|expected| expected != hash) {
                return Err(Error::Corrupt("root descriptor hash mismatch"));
            }
            if self.expected_hash.is_none() {
                self.checkpoint =
                    crate::descriptor_extent::observed_checkpoint(self.generation, 4, hash);
            }
            self.authenticated = true;
            return Ok(true);
        }
        if self.local_used == self.local_length {
            if self.local_length == 8 {
                // Framing is a distinct fixed work unit, even for partial reads.
                if p.work_units == budget.entries {
                    return Ok(false);
                }
                let len = u32::from_le_bytes(number(&self.local_buffer, 4)?) as usize;
                if ![96, 160].contains(&len)
                    || self
                        .local_at
                        .checked_add(len)
                        .is_none_or(|end| end > self.selected.length)
                {
                    return Err(Error::Corrupt("metadata local blob extent"));
                }
                self.local_length = len;
                p.work_units += 1;
                return Ok(true);
            }
            if p.work_units == budget.entries {
                return Ok(false);
            }
            self.slots
                .insert(self.local_at as u64, self.local_length as u32);
            if self.slots.len() > self.local_count {
                return Err(Error::Corrupt("metadata local blob count"));
            }
            self.local_at += self.local_length;
            self.local_used = 0;
            self.local_length = 8;
            p.work_units += 1;
            return Ok(true);
        }
        let take = self.local_length - self.local_used;
        let mut bytes = [0; 160];
        let r = self.selected.read(
            self.local_at + self.local_used,
            &mut bytes[..take],
            budget,
            p,
        )?;
        self.hash.update(&bytes[..r.copied]);
        self.local_buffer[self.local_used..self.local_used + r.copied]
            .copy_from_slice(&bytes[..r.copied]);
        self.local_used += r.copied;
        Ok(progressed(&r))
    }

    fn begin(&mut self) -> Result<()> {
        let r = self
            .frames
            .last()
            .ok_or(Error::Corrupt("metadata traversal frame"))?
            .reference;
        let end = r
            .offset
            .checked_add(u64::from(r.length))
            .filter(|end| *end <= crate::MAX_DESCRIPTOR_BYTES)
            .ok_or(Error::Corrupt("metadata node extent overflow/bound"))?;
        let key = (r.origin, r.offset);
        let before = self
            .seen
            .range(..=key)
            .next_back()
            .is_some_and(|((o, at), len)| {
                *o == r.origin
                    && at
                        .checked_add(u64::from(*len))
                        .is_none_or(|end| end > r.offset)
            });
        let after = self
            .seen
            .range(key..)
            .next()
            .is_some_and(|((o, at), _)| *o == r.origin && *at < end);
        let max = self
            .header
            .as_ref()
            .unwrap()
            .segment_count
            .saturating_mul(2)
            .saturating_sub(1);
        if self.visits == max || before || after {
            return Err(Error::Corrupt("metadata alias/overlap/cycle/visit bound"));
        }
        if r.origin == self.generation && self.slots.get(&r.offset) != Some(&r.length) {
            return Err(Error::Corrupt("metadata local node extent"));
        }
        self.seen.insert(key, r.length);
        self.visits += 1;
        self.reading = Some(Reading {
            reference: r,
            bytes: [0; 160],
            used: 0,
            hash: Sha256::new(),
        });
        Ok(())
    }
    fn external_header(
        source: &mut Source,
        origin: u128,
        budget: DescriptorCursorBudget,
        p: &mut IndexedDescriptorProgress,
    ) -> Result<bool> {
        if source.header_used < HEADER {
            let take = HEADER - source.header_used;
            let mut bytes = [0; HEADER];
            let at = source.header_used;
            let r = source.read(at, &mut bytes[..take], budget, p)?;
            source.header[at..at + r.copied].copy_from_slice(&bytes[..r.copied]);
            source.header_used += r.copied;
            return Ok(progressed(&r));
        }
        if !source.header_checked {
            if p.work_units == budget.entries {
                return Ok(false);
            }
            let b = &source.header;
            // Same external-origin header contract as metadata::Resolver. Each
            // referenced node supplies its own authenticated hash and extent.
            if &b[..8] != b"ULMDBROT"
                || b[8..10] != [0; 2]
                || b[10..12] != 4u16.to_le_bytes()
                || b[12..16] != 256u32.to_le_bytes()
                || b[32..48] != origin.to_le_bytes()
                || ![15u64, 31, 47, 63].contains(&u64::from_le_bytes(number(b, 120)?))
            {
                return Err(Error::Corrupt("metadata origin descriptor identity/header"));
            }
            Manifest::page_size_from_header(b)?;
            Manifest::value_limit_from_header(b)?;
            source.header_checked = true;
            p.work_units += 1;
            return Ok(true);
        }
        Ok(false)
    }
    fn read_node(
        &mut self,
        budget: DescriptorCursorBudget,
        p: &mut IndexedDescriptorProgress,
    ) -> Result<bool> {
        let r = self.reading.as_ref().unwrap().reference;
        if r.origin != self.generation {
            let Some(source) = self.sources.get_mut(&r.origin) else {
                p.needed_origin = Some(r.origin);
                return Ok(false);
            };
            if !source.header_checked {
                return Self::external_header(source, r.origin, budget, p);
            }
        }
        let node = self.reading.as_mut().unwrap();
        if node.used < r.length as usize {
            let take = r.length as usize - node.used;
            let source = if r.origin == self.generation {
                &mut self.selected
            } else {
                self.sources.get_mut(&r.origin).unwrap()
            };
            let at = usize::try_from(r.offset)
                .map_err(|_| Error::LimitExceeded)?
                .checked_add(node.used)
                .ok_or(Error::LimitExceeded)?;
            let step = source.read(at, &mut node.bytes[node.used..node.used + take], budget, p)?;
            node.hash
                .update(&node.bytes[node.used..node.used + step.copied]);
            node.used += step.copied;
            return Ok(progressed(&step));
        }

        if p.work_units == budget.entries {
            return Ok(false);
        }
        let node = self.reading.take().unwrap();
        self.decode_node(node)?;
        p.work_units += 1;
        Ok(true)
    }
    fn decode_node(&mut self, node: Reading) -> Result<()> {
        let r = node.reference;
        let b = &node.bytes[..r.length as usize];
        let frame = self.frames.last().unwrap();
        if <[u8; 32]>::from(node.hash.finalize()) != r.hash
            || b[2..4] != 1u16.to_le_bytes()
            || b[4..8] != r.length.to_le_bytes()
            || b[17..32].iter().any(|v| *v != 0)
        {
            return Err(Error::Corrupt("metadata node hash/header"));
        }
        let prefix = u64::from_le_bytes(number(b, 8)?);
        let depth = b[16];
        if depth > 64 || frame.parent_depth.is_some_and(|d| depth <= d) {
            return Err(Error::Corrupt("metadata depth"));
        }
        self.origins.insert(r.origin);
        match u16::from_le_bytes(number(b, 0)?) {
            1 if r.length == 96 && depth == 64 => {
                let entry = SegmentEntry {
                    id: u64::from_le_bytes(number(b, 32)?),
                    length: u64::from_le_bytes(number(b, 40)?),
                    hash: number(b, 48)?,
                    cohort: u128::from_le_bytes(number(b, 80)?),
                };
                if prefix != entry.id {
                    return Err(Error::Corrupt("metadata leaf ID"));
                }
                let first = *self.first_chunk.get_or_insert(entry.id >> 8);
                if entry.id <= self.prior
                    || entry.id == u64::MAX
                    || (entry.id >> 8) - first >= crate::table::MAX_DIRECTORY_CHUNKS
                    || !(4096..=self.header.as_ref().unwrap().value_limit.segment_bytes() as u64)
                        .contains(&entry.length)
                {
                    return Err(Error::Corrupt("invalid indexed segment inventory"));
                }
                let page = self.pages.entry(self.count / PAGE).or_default();
                if page.is_empty() {
                    page.try_reserve_exact(PAGE)
                        .map_err(|_| Error::LimitExceeded)?;
                }
                page.push(entry.clone());
                self.count += 1;
                self.prior = entry.id;
                self.complete(Summary {
                    prefix,
                    depth,
                    count: 1,
                })
            }
            2 if r.length == 160 && depth < 64 && self::prefix(prefix, depth) == prefix => {
                let left = NodeRef::decode(&b[32..96])?;
                let right = NodeRef::decode(&b[96..160])?;
                for child in [left, right] {
                    if (child.origin == r.origin
                        && child
                            .offset
                            .checked_add(u64::from(child.length))
                            .is_none_or(|end| end > r.offset))
                        || (r.origin != self.generation && child.origin == self.generation)
                    {
                        return Err(Error::Corrupt(
                            "metadata immutable/backward child reference",
                        ));
                    }
                }
                self.frames.last_mut().unwrap().stage = Stage::Left {
                    prefix,
                    depth,
                    right,
                };
                self.push(left, depth)
            }
            _ => Err(Error::Corrupt("metadata node tag/length/depth")),
        }
    }
    fn push(&mut self, reference: NodeRef, depth: u8) -> Result<()> {
        if self.frames.len() == 65 {
            return Err(Error::Corrupt("metadata cursor depth bound"));
        }
        self.frames.push(Frame {
            reference,
            parent_depth: Some(depth),
            stage: Stage::Enter,
        });
        Ok(())
    }
    fn complete(&mut self, summary: Summary) -> Result<()> {
        let done = self
            .frames
            .pop()
            .ok_or(Error::Corrupt("metadata traversal frame"))?;
        if done.reference.origin == self.generation {
            if done.reference.offset != self.postorder as u64 {
                return Err(Error::Corrupt("metadata exact postorder mismatch"));
            }
            self.postorder += done.reference.length as usize;
            self.visited_local += 1;
        }
        let Some(parent) = self.frames.last_mut() else {
            let h = self.header.as_ref().unwrap();
            if summary.count != h.segment_count
                || self.count != h.segment_count
                || self.postorder != self.selected.length
                || self.visited_local != self.local_count
                || h.next_segment <= self.prior
            {
                return Err(Error::Corrupt("metadata count/postorder/counters"));
            }
            self.ready = true;
            return Ok(());
        };
        match parent.stage {
            Stage::Left {
                prefix,
                depth,
                right,
            } => {
                parent.stage = Stage::Right {
                    prefix,
                    depth,
                    left: summary,
                };
                self.push(right, depth)
            }
            Stage::Right {
                prefix,
                depth,
                left,
            } => {
                parent.stage = Stage::Complete {
                    prefix,
                    depth,
                    left,
                    right: summary,
                };
                Ok(())
            }
            _ => Err(Error::Corrupt("metadata traversal completion")),
        }
    }
    fn advance(&mut self, budget: DescriptorCursorBudget) -> Result<IndexedDescriptorProgress> {
        let mut p = IndexedDescriptorProgress::default();
        loop {
            if self.ready {
                p.ready = true;
                return Ok(p);
            }
            if !self.authenticated {
                if !self.scan(budget, &mut p)? {
                    return Ok(p);
                }
                continue;
            }
            if self.frames.is_empty() {
                if self.header.as_ref().unwrap().segment_count != 0
                    || self.selected.length != HEADER
                    || self.local_count != 0
                {
                    return Err(Error::Corrupt("empty metadata extent"));
                }
                self.ready = true;
                continue;
            }
            if self.reading.is_some() {
                if !self.read_node(budget, &mut p)? {
                    return Ok(p);
                }
                continue;
            }
            if p.work_units == budget.entries {
                return Ok(p);
            }
            match self.frames.last().unwrap().stage {
                Stage::Enter => self.begin()?,
                Stage::Complete {
                    prefix,
                    depth,
                    left,
                    right,
                } => {
                    if self::prefix(left.prefix, depth) != prefix
                        || self::prefix(right.prefix, depth) != prefix
                        || bit(left.prefix, depth)
                        || !bit(right.prefix, depth)
                        || (left.prefix ^ right.prefix).leading_zeros() != u32::from(depth)
                        || left.depth <= depth
                        || right.depth <= depth
                    {
                        return Err(Error::Corrupt("metadata Patricia geometry"));
                    }
                    self.complete(Summary {
                        prefix,
                        depth,
                        count: left
                            .count
                            .checked_add(right.count)
                            .ok_or(Error::LimitExceeded)?,
                    })?;
                }
                _ => return Err(Error::Corrupt("metadata traversal state")),
            }
            p.work_units += 1;
        }
    }
}

#[cfg(test)]
mod tests;
