//! Incremental authentication and metadata decoding for flat descriptors.
//! Results describe authenticated bytes only. They grant no namespace, pin,
//! graph, currentness or unlink authority. Indexed metadata needs its own walker.
use crate::descriptor_source::DescriptorSource;
use crate::transform::TransformContext;
use crate::{Error, Manifest, Reference, Result, SegmentEntry, manifest::number};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::{collections::BTreeMap, fs::File};

const HEADER: usize = 192;
const ROW: usize = 64;
const PAGE_ENTRIES: usize = 64;
const MAX_ENTRIES: usize = 1_048_575;

#[derive(Clone, Copy, Debug)]
pub struct DescriptorCursorBudget {
    /// Maximum descriptor bytes read and hashed by this call.
    pub bytes: usize,
    /// Maximum decoded rows/header plus authenticated transform-frame units.
    pub entries: usize,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DescriptorCursorProgress {
    pub read_bytes: usize,
    pub decoded_entries: usize,
    pub ready: bool,
    /// Minimum next atomic transformed-source read; zero for plaintext.
    pub required_read_bytes: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DescriptorAuthentication {
    ExpectedCheckpoint,
    ObservedContent,
}
#[derive(Clone, Debug)]
pub struct DescriptorFootprintHeader {
    pub format_minor: u16,
    pub page_size: ulmdb_core::PageSize,
    pub value_limit: ulmdb_core::ValueLimit,
    pub uuid: [u8; 16],
    pub generation: u128,
    pub epoch: u64,
    pub sequence: u64,
    pub commit: u128,
    pub parent: u128,
    pub previous: u128,
    pub catalog: Reference,
    pub next_database: u64,
    pub next_segment: u64,
    pub segment_count: usize,
}
/// Metadata authenticated against the exact supplied checkpoint. Fixed-size
/// pages avoid a final whole-inventory flatten/copy outside the step budget.
pub struct DescriptorFootprint {
    authentication: DescriptorAuthentication,
    checkpoint: [u8; 64],
    header: DescriptorFootprintHeader,
    pages: BTreeMap<usize, Vec<SegmentEntry>>,
}
impl DescriptorFootprint {
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
        self.pages.values().flat_map(|page| page.iter())
    }
    pub fn entry(&self, index: usize) -> Option<&SegmentEntry> {
        self.pages
            .get(&(index / PAGE_ENTRIES))?
            .get(index % PAGE_ENTRIES)
    }
}
/// Owned FD plus checked extent; the expected checkpoint authenticates content,
/// not the provenance of the FD. Callers retain their existing immutable-source
/// contract. Source metadata is rechecked around every nonempty advance. No
/// payload-sized input buffer or complete Manifest decode is used.
pub struct FlatDescriptorCursor {
    creator: u32,
    source: DescriptorSource,
    length: usize,
    checkpoint: [u8; 64],
    generation: u128,
    expected_hash: Option<[u8; 32]>,
    expected_minor: Option<u16>,
    hash: Sha256,
    header_bytes: [u8; HEADER],
    header_used: usize,
    header: Option<DescriptorFootprintHeader>,
    row: [u8; ROW],
    row_used: usize,
    at: usize,
    first_chunk: Option<u64>,
    prior: u64,
    pages: BTreeMap<usize, Vec<SegmentEntry>>,
    max_entries: usize,
    read: usize,
    ready: bool,
    failed: bool,
}
impl FlatDescriptorCursor {
    /// Read a flat 0.1/0.2/0.3 descriptor from an explicit file extent. This
    /// constructor reads metadata only, no descriptor bytes. 0.4 is explicitly
    /// declined: its external authenticated nodes cannot be treated as rows.
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
    /// Observe content identity without an externally authenticated hash. Caller
    /// must validate the resulting realm/generation/link under its own authority.
    pub fn observe_identity(
        file: File,
        start: u64,
        length: usize,
        generation: u128,
        minor: u16,
        max_entries: usize,
    ) -> Result<Self> {
        Self::observe_identity_with_transform(
            file,
            start,
            length,
            generation,
            minor,
            max_entries,
            None,
        )
    }
    pub fn observe_identity_with_transform(
        file: File,
        start: u64,
        length: usize,
        generation: u128,
        minor: u16,
        max_entries: usize,
        context: Option<Arc<TransformContext>>,
    ) -> Result<Self> {
        if generation == 0 || ![1, 2, 3].contains(&minor) {
            return Err(Error::InvalidInput("flat observed identity"));
        }
        let cp = crate::descriptor_extent::observed_checkpoint(generation, minor, [0; 32]);
        let mut cursor = Self::build(file, start, length, cp, max_entries, context)?;
        cursor.expected_hash = None;
        cursor.expected_minor = (minor >= 3).then_some(minor);
        Ok(cursor)
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
        if &checkpoint[..8] == b"ULMDBT04" {
            return Err(Error::InvalidInput("indexed descriptor needs node cursor"));
        }
        if max_entries > MAX_ENTRIES || !(HEADER..=HEADER + ROW * MAX_ENTRIES).contains(&length) {
            return Err(Error::InvalidInput("flat descriptor limits"));
        }
        let source = DescriptorSource::new(file, start, length, context.map(|c| (generation, c)))?;
        Ok(Self {
            creator: std::process::id(),
            source,
            length,
            checkpoint,
            generation,
            expected_hash: Some(expected_hash),
            expected_minor: None,
            hash: Sha256::new(),
            header_bytes: [0; HEADER],
            header_used: 0,
            header: None,
            row: [0; ROW],
            row_used: 0,
            at: 0,
            first_chunk: None,
            prior: 0,
            pages: BTreeMap::new(),
            max_entries,
            read: 0,
            ready: false,
            failed: false,
        })
    }
    fn check(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited descriptor cursor"));
        }
        if self.failed {
            return Err(Error::InvalidInput("failed descriptor cursor"));
        }
        self.source.check()?;
        Ok(())
    }
    /// Advance with exact read/hash and decoded-row budgets. Each call also
    /// performs at most two fixed source-identity queries. Entry allocation is
    /// paged (64 rows/page); no later finalization copies all prior rows.
    pub fn advance(&mut self, budget: DescriptorCursorBudget) -> Result<DescriptorCursorProgress> {
        if let Err(error) = self.check() {
            self.failed = true;
            return Err(error);
        }
        if self.ready {
            return Ok(DescriptorCursorProgress {
                ready: true,
                ..Default::default()
            });
        }
        let result = self.advance_inner(budget);
        match result {
            Ok(progress) => {
                if let Err(error) = self.check() {
                    self.failed = true;
                    return Err(error);
                }
                Ok(progress)
            }
            Err(error) => {
                self.failed = true;
                Err(error)
            }
        }
    }
    fn advance_inner(
        &mut self,
        budget: DescriptorCursorBudget,
    ) -> Result<DescriptorCursorProgress> {
        let mut progress = DescriptorCursorProgress::default();
        loop {
            if self.header.is_none() && self.header_used == HEADER {
                if progress.decoded_entries == budget.entries {
                    break;
                }
                let (minor, page_size, value_limit, count) =
                    Manifest::decode_prefix_with_length(&self.header_bytes, self.length)?;
                if self.expected_minor.is_some_and(|m| m != minor)
                    || minor == 4
                    || (minor >= 3) != (&self.checkpoint[..8] == b"ULMDBC03")
                {
                    return Err(Error::Corrupt("root generation/storage type mismatch"));
                }
                if count > self.max_entries {
                    return Err(Error::LimitExceeded);
                }
                let b = &self.header_bytes;
                let header = DescriptorFootprintHeader {
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
                if header.uuid == [0; 16]
                    || header.generation != self.generation
                    || header.generation == header.previous
                    || header.commit == 0
                    || header.epoch == 0
                    || header.next_database == 0
                    || header.next_segment == 0
                {
                    return Err(Error::Corrupt("invalid root identities/counters"));
                }
                self.header = Some(header);
                progress.decoded_entries += 1;
            }
            if let Some(header) = &self.header {
                if self.at == header.segment_count {
                    if self.read != self.length || header.next_segment <= self.prior {
                        return Err(Error::Corrupt("invalid root inventory extent/counter"));
                    }
                    let hash: [u8; 32] = self.hash.clone().finalize().into();
                    if self.expected_hash.is_some_and(|expected| expected != hash) {
                        return Err(Error::Corrupt("root descriptor hash mismatch"));
                    }
                    if self.expected_hash.is_none() {
                        self.checkpoint = crate::descriptor_extent::observed_checkpoint(
                            self.generation,
                            header.format_minor,
                            hash,
                        );
                    }
                    self.ready = true;
                    progress.ready = true;
                    break;
                }
                if self.row_used == ROW {
                    if progress.decoded_entries == budget.entries {
                        break;
                    }
                    let b = &self.row;
                    let id = u64::from_le_bytes(number(b, 0)?);
                    let first = *self.first_chunk.get_or_insert(id >> 8);
                    if id <= self.prior
                        || id == u64::MAX
                        || (id >> 8) - first >= crate::table::MAX_DIRECTORY_CHUNKS
                        || (header.format_minor < 3
                            && (b[48..56] != 1u64.to_le_bytes() || b[56..64] != [0; 8]))
                    {
                        return Err(Error::Corrupt("invalid root segment inventory"));
                    }
                    let entry = SegmentEntry {
                        id,
                        length: u64::from_le_bytes(number(b, 8)?),
                        hash: number(b, 16)?,
                        cohort: if header.format_minor >= 3 {
                            u128::from_le_bytes(number(b, 48)?)
                        } else {
                            0
                        },
                    };
                    if !(4096..=header.value_limit.segment_bytes() as u64).contains(&entry.length) {
                        return Err(Error::Corrupt("segment exceeds declared value profile"));
                    }
                    let page = self.pages.entry(self.at / PAGE_ENTRIES).or_default();
                    if page.is_empty() {
                        page.try_reserve_exact(PAGE_ENTRIES)
                            .map_err(|_| Error::LimitExceeded)?;
                    }
                    page.push(entry);
                    self.at += 1;
                    self.prior = id;
                    self.row_used = 0;
                    progress.decoded_entries += 1;
                    continue;
                }
            }
            let available = budget.bytes - progress.read_bytes;
            let remaining = if self.header.is_none() {
                HEADER - self.header_used
            } else {
                ROW - self.row_used
            };
            let take = if self.source.is_transformed() {
                remaining
            } else {
                available.min(remaining)
            };
            let destination = if self.header.is_none() {
                &mut self.header_bytes[self.header_used..self.header_used + take]
            } else {
                &mut self.row[self.row_used..self.row_used + take]
            };
            let read = self.source.read(
                self.read,
                destination,
                available,
                budget.entries - progress.decoded_entries,
            )?;
            progress.read_bytes += read.read_bytes;
            progress.decoded_entries += read.work_units;
            progress.required_read_bytes = read.required_read_bytes;
            if read.copied == 0 {
                if read.read_bytes != 0 || read.work_units != 0 {
                    continue;
                }
                break;
            }
            self.hash.update(&destination[..read.copied]);
            self.read += read.copied;
            if self.header.is_none() {
                self.header_used += read.copied;
            } else {
                self.row_used += read.copied;
            }
        }
        Ok(progress)
    }
    pub fn finish(self) -> Result<DescriptorFootprint> {
        self.check()?;
        if !self.ready {
            return Err(Error::InvalidInput("descriptor cursor incomplete"));
        }
        Ok(DescriptorFootprint {
            authentication: if self.expected_hash.is_some() {
                DescriptorAuthentication::ExpectedCheckpoint
            } else {
                DescriptorAuthentication::ObservedContent
            },
            checkpoint: self.checkpoint,
            header: self
                .header
                .ok_or(Error::InvalidInput("descriptor cursor header"))?,
            pages: self.pages,
        })
    }
}

#[cfg(test)]
mod tests;
