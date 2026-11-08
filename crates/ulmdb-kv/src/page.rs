use ulmdb_core::storage::{Error, ReadStore, Reference, Result};

pub const PAGE_SIZE: usize = 4096;
pub(crate) const HEADER: usize = 32;
// Raw V1 lower is <=4096. Validated V2 widths need nine/ten bits; the
// discriminator and both fields fit in 20 bits on every supported target.
const COMPACT_LAYOUT: usize = 1 << 19;
// Shared proof is specific to this persisted geometry; changes must be explicit.
const _: () = assert!(PAGE_SIZE == 4096 && HEADER == 32);
pub(crate) const MAX_DEPTH: u16 = 64;
// Bound every cell so even a page with huge keys/values can always split.
pub(crate) const INLINE_KEY: usize = 256;
pub(crate) const INLINE_VALUE: usize = 512;

pub(crate) fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes
            .get(at..at + 2)
            .ok_or(Error::Corrupt("short page field"))?
            .try_into()
            .map_err(|_| Error::Corrupt("page integer"))?,
    ))
}

/// Maintenance/write validation only: equal-width big-endian words preserve
/// byte ordering. Mixed lengths and other widths keep the slice comparison.
#[inline]
pub(crate) fn cmp_validation_keys(left: &[u8], right: &[u8]) -> std::cmp::Ordering {
    #[cfg(feature = "word-search")]
    if let (Ok(left), Ok(right)) = (<&[u8; 8]>::try_from(left), <&[u8; 8]>::try_from(right)) {
        return u64::from_be_bytes(*left).cmp(&u64::from_be_bytes(*right));
    }
    left.cmp(right)
}

#[derive(Clone, Copy)]
pub(crate) struct Page<'a> {
    pub bytes: &'a [u8],
    pub level: u16,
    pub count: usize,
    pub first: Reference,
    upper: usize,
    // V1: checked lower slot end. V2: checked fixed widths plus layout flag.
    layout_word: usize,
}

pub(crate) struct Cell<'a> {
    pub key: &'a [u8],
    pub value: &'a [u8],
    pub raw: &'a [u8],
    pub start: usize,
}

impl<'a> Page<'a> {
    pub fn load<S: ReadStore>(store: &'a S, at: Reference) -> Result<Self> {
        if at.is_null() {
            return Err(Error::Corrupt("null tree page"));
        }
        let size = ulmdb_core::PageSize::new(store.page_size())
            .map_err(|_| Error::Corrupt("invalid store page size"))?
            .get();
        let bytes = store.read(at, size)?;
        if bytes.len() != size {
            return Err(Error::Corrupt("short tree page"));
        }
        Self::decode(bytes)
    }

    pub fn decode(bytes: &'a [u8]) -> Result<Self> {
        let page_size = ulmdb_core::PageSize::new(bytes.len())
            .map_err(|_| Error::Corrupt("short tree page"))?
            .get();
        let tag = u16_at(bytes, 0)?;
        let level = u16_at(bytes, 4)?;
        let raw_count = u16_at(bytes, 6)?;
        let count = usize::from(raw_count);
        let raw_lower = u16_at(bytes, 8)?;
        let lower = if page_size == 65536 && raw_lower == 0 {
            65536
        } else {
            usize::from(raw_lower)
        };
        let raw_upper = u16_at(bytes, 10)?;
        let upper = if page_size == 65536 && raw_upper == 0 {
            65536
        } else {
            usize::from(raw_upper)
        };
        let first = Reference::decode(&bytes[16..32])?;
        let version = u16_at(bytes, 2)?;
        let geometry = if version == 3 {
            page_size != PAGE_SIZE
                && u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize == page_size
                && lower == HEADER + 2 * count
                && lower <= upper
                && upper <= page_size
                && upper.is_multiple_of(2)
        } else if version == 1 && page_size == PAGE_SIZE {
            ulmdb_core::page_geometry::valid_geometry(raw_count, raw_lower, raw_upper)
                && bytes[12..16] == [0; 4]
        } else if version == 2 && page_size == PAGE_SIZE {
            crate::compact::valid(
                level == 0,
                raw_count,
                raw_lower,
                raw_upper,
                u16_at(bytes, 12)?,
                u16_at(bytes, 14)?,
            )
        } else {
            false
        };
        if !geometry
            || level >= MAX_DEPTH
            || (level == 0 && (tag != 2 || !first.is_null()))
            || (level != 0 && (tag != 3 || first.is_null()))
        {
            return Err(Error::Corrupt("invalid tree page header"));
        }
        Ok(Self {
            bytes,
            level,
            count,
            first,
            upper,
            layout_word: if version == 2 {
                let layout = crate::compact::Layout::read(bytes);
                COMPACT_LAYOUT | layout.key | (layout.value << 9)
            } else {
                lower
            },
        })
    }

    /// Complete local V2 validation over this exact immutable page image.
    /// Inline V2 records require no payload reads or V1 occupancy bitmap.
    pub fn validate_compact(&self) -> Result<()> {
        let layout = self
            .compact()
            .ok_or(Error::Corrupt("expected compact validation"))?;
        let mut previous: Option<&[u8]> = None;
        for index in 0..self.count {
            let cell = layout.cell(self.bytes, self.level == 0, self.count, index)?;
            if previous.is_some_and(|key| cmp_validation_keys(key, cell.key).is_ge()) {
                return Err(Error::Corrupt("unordered tree keys"));
            }
            previous = Some(cell.key);
        }
        // A reduction lets LLVM check long zero tails in parallel while still
        // rejecting every nonzero byte, including any final vector remainder.
        if self.bytes[self.upper..].iter().fold(0u8, |acc, b| acc | b) != 0 {
            return Err(Error::Corrupt("nonzero compact page tail"));
        }
        Ok(())
    }

    pub fn compact(&self) -> Option<crate::compact::Layout> {
        if self.layout_word & COMPACT_LAYOUT != 0 {
            Some(crate::compact::Layout {
                key: self.layout_word & 0x1ff,
                value: (self.layout_word >> 9) & 0x3ff,
            })
        } else {
            None
        }
    }
    /// Layout-aware physical metadata for cold audits; Cell's hot return shape stays unchanged.
    pub fn encoding(&self, cell: &Cell<'_>) -> Result<(u16, usize, usize)> {
        if let Some(layout) = self.compact() {
            return Ok((0, 0, layout.key));
        }
        let flags = u16_at(cell.raw, 2)?;
        Ok((
            flags,
            if self.level == 0 { 8 } else { 20 },
            if flags & 1 != 0 {
                16
            } else {
                usize::from(u16_at(cell.raw, 0)?)
            },
        ))
    }
    pub fn child_offset(&self, index: usize) -> Result<usize> {
        if self.level == 0 || index > self.count {
            return Err(Error::Corrupt("invalid branch child index"));
        }
        if index == 0 {
            return Ok(16);
        }
        if let Some(layout) = self.compact() {
            return Ok(layout.start(false, index - 1) + layout.key);
        }
        Ok(usize::from(u16_at(self.bytes, HEADER + (index - 1) * 2)?) + 4)
    }
    fn cell_bytes(&self, index: usize) -> Result<(&'a [u8], usize)> {
        if index >= self.count {
            return Err(Error::Corrupt("tree slot outside page"));
        }
        let start = usize::from(u16_at(self.bytes, HEADER + index * 2)?);
        if start < self.upper || start < self.layout_word || start % 2 != 0 {
            return Err(Error::Corrupt("invalid tree cell offset"));
        }
        Ok((
            self.bytes
                .get(start..)
                .ok_or(Error::Corrupt("tree cell offset"))?,
            start,
        ))
    }

    fn key<S: ReadStore>(&self, store: &'a S, bytes: &'a [u8]) -> Result<(&'a [u8], usize)> {
        let key_len = u16_at(bytes, 0)?;
        let flags = u16_at(bytes, 2)?;
        let header = if self.level == 0 { 8 } else { 20 };
        if flags & !(if self.level == 0 { 3 } else { 1 }) != 0
            || (flags & 1 != 0 && key_len != u16::MAX)
            || (flags & 1 == 0 && key_len == u16::MAX)
        {
            return Err(Error::Corrupt("invalid tree cell flags"));
        }
        let key_bytes = if flags & 1 != 0 {
            16
        } else {
            usize::from(key_len)
        };
        let key_end = header + key_bytes;
        let key_rep = bytes
            .get(header..key_end)
            .ok_or(Error::Corrupt("short tree key"))?;
        let key = if flags & 1 != 0 {
            store.payload(Reference::decode(key_rep)?)?
        } else {
            key_rep
        };
        Ok((key, key_end))
    }

    pub fn key_at<S: ReadStore>(&self, store: &'a S, index: usize) -> Result<&'a [u8]> {
        if let Some(layout) = self.compact() {
            let (row, _) = layout.record(self.bytes, self.level == 0, self.count, index)?;
            return Ok(&row[..layout.key]);
        }
        Ok(self.key(store, self.cell_bytes(index)?.0)?.0)
    }

    /// Check physical cell extent without dereferencing overflow payloads.
    /// Unchanged records retain their encoded references during page edits.
    pub fn raw_cell(&self, index: usize) -> Result<&'a [u8]> {
        if self.compact().is_some() {
            return Err(Error::Corrupt("compact page has no legacy raw cell"));
        }
        let (bytes, _) = self.cell_bytes(index)?;
        let key_len = u16_at(bytes, 0)?;
        let flags = u16_at(bytes, 2)?;
        if flags & !(if self.level == 0 { 3 } else { 1 }) != 0
            || (flags & 1 != 0) != (key_len == u16::MAX)
        {
            return Err(Error::Corrupt("invalid tree cell flags"));
        }
        let header: usize = if self.level == 0 { 8 } else { 20 };
        let key_len = if flags & 1 != 0 {
            16
        } else {
            usize::from(key_len)
        };
        let key_end = header + key_len;
        bytes
            .get(..key_end)
            .ok_or(Error::Corrupt("short tree key"))?;
        if flags & 1 != 0 && Reference::decode(&bytes[header..key_end])?.is_null() {
            return Err(Error::Corrupt("null overflow key"));
        }
        let end = if self.level == 0 {
            let value_len = u32::from_le_bytes(
                bytes[4..8]
                    .try_into()
                    .map_err(|_| Error::Corrupt("leaf length"))?,
            ) as usize;
            let end = key_end
                .checked_add(if flags & 2 != 0 { 16 } else { value_len })
                .ok_or(Error::Corrupt("leaf length overflow"))?;
            bytes.get(..end).ok_or(Error::Corrupt("short leaf value"))?;
            if flags & 2 != 0 && Reference::decode(&bytes[key_end..end])?.is_null() {
                return Err(Error::Corrupt("null overflow value"));
            }
            end
        } else {
            if Reference::decode(&bytes[4..20])?.is_null() {
                return Err(Error::Corrupt("null branch child"));
            }
            key_end
        };
        let padded = end
            .checked_add(end % 2)
            .ok_or(Error::Corrupt("cell length overflow"))?;
        let raw = bytes
            .get(..padded)
            .ok_or(Error::Corrupt("short cell padding"))?;
        if padded != end && raw[end] != 0 {
            return Err(Error::Corrupt("nonzero cell padding"));
        }
        Ok(raw)
    }

    pub fn cell<S: ReadStore>(&self, store: &'a S, index: usize) -> Result<Cell<'a>> {
        if let Some(layout) = self.compact() {
            return layout.cell(self.bytes, self.level == 0, self.count, index);
        }
        let (bytes, start) = self.cell_bytes(index)?;
        let (key, key_end) = self.key(store, bytes)?;
        let flags = u16_at(bytes, 2)?;
        let (value, end) = if self.level == 0 {
            let value_len = u32::from_le_bytes(
                bytes
                    .get(4..8)
                    .ok_or(Error::Corrupt("short leaf"))?
                    .try_into()
                    .map_err(|_| Error::Corrupt("leaf length"))?,
            ) as usize;
            let end = key_end
                .checked_add(if flags & 2 != 0 { 16 } else { value_len })
                .ok_or(Error::Corrupt("leaf length overflow"))?;
            let rep = bytes
                .get(key_end..end)
                .ok_or(Error::Corrupt("short leaf value"))?;
            let value = if flags & 2 != 0 {
                store.payload(Reference::decode(rep)?)?
            } else {
                rep
            };
            if value.len() != value_len {
                return Err(Error::Corrupt("overflow value length mismatch"));
            }
            (value, end)
        } else {
            let child = Reference::decode(bytes.get(4..20).ok_or(Error::Corrupt("short branch"))?)?;
            if child.is_null() {
                return Err(Error::Corrupt("null branch child"));
            }
            (&[][..], key_end)
        };
        let physical_end = end
            .checked_add(end % 2)
            .ok_or(Error::Corrupt("cell length overflow"))?;
        let raw = bytes
            .get(..physical_end)
            .ok_or(Error::Corrupt("short cell padding"))?;
        if physical_end != end && raw[end] != 0 {
            return Err(Error::Corrupt("nonzero cell padding"));
        }
        Ok(Cell {
            key,
            value,
            raw,
            start,
        })
    }
    /// Inspect a cell's key, physical representation and checked value length.
    /// Ordinary overflow values need only their authenticated header/extent;
    /// catalog/bucket callers explicitly request the actual value bytes.
    pub(crate) fn cell_metadata<S: ReadStore>(
        &self,
        store: &'a S,
        index: usize,
        materialize_value: bool,
    ) -> Result<(Cell<'a>, usize)> {
        if materialize_value || self.compact().is_some() {
            let cell = self.cell(store, index)?;
            let length = cell.value.len();
            return Ok((cell, length));
        }
        let raw = self.raw_cell(index)?;
        let (_, start) = self.cell_bytes(index)?;
        let (key, key_end) = self.key(store, raw)?;
        let (value, length) = if self.level == 0 {
            let length = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
            if u16_at(raw, 2)? & 2 != 0 {
                let payload = Reference::decode(&raw[key_end..key_end + 16])?;
                if store.payload_length(payload)? != length {
                    return Err(Error::Corrupt("overflow value length mismatch"));
                }
                (&[][..], length)
            } else {
                (&raw[key_end..key_end + length], length)
            }
        } else {
            (&[][..], 0)
        };
        Ok((
            Cell {
                key,
                value,
                raw,
                start,
            },
            length,
        ))
    }

    /// First separator/key >= needle, or > needle for upper-bound traversal.
    #[cfg_attr(feature = "word-search", inline)]
    pub fn search<S: ReadStore>(&self, store: &'a S, needle: &[u8], upper: bool) -> Result<usize> {
        if let Some(layout) = self.compact() {
            return layout.search(self, needle, upper);
        }
        #[cfg(feature = "word-search")]
        if let Ok(bytes) = <&[u8; 8]>::try_from(needle) {
            return self.search_word(store, needle, u64::from_be_bytes(*bytes), upper);
        }
        let (mut lo, mut hi) = (0, self.count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            // Search only keys; large values are touched solely for the hit.
            let key = self.key_at(store, mid)?;
            if key < needle || (upper && key == needle) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Ok(lo)
    }

    pub fn search_with<S: ReadStore, C: crate::ordering::Comparator + ?Sized>(
        &self,
        store: &'a S,
        needle: &[u8],
        upper: bool,
        order: &C,
    ) -> Result<usize> {
        if order.is_bytewise() {
            return self.search(store, needle, upper);
        }
        let (mut low, mut high) = (0, self.count);
        while low < high {
            let mid = low + (high - low) / 2;
            let key = self.key_at(store, mid)?;
            crate::ordering::stored(order, key)?;
            let compared = order.compare(key, needle);
            if compared.is_lt() || upper && compared.is_eq() {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        Ok(low)
    }

    #[cfg(feature = "word-search")]
    #[inline]
    fn search_word<S: ReadStore>(
        &self,
        store: &'a S,
        needle: &[u8],
        word: u64,
        upper: bool,
    ) -> Result<usize> {
        let (mut lo, mut hi) = (0, self.count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let key = self.key_at(store, mid)?;
            if precedes_word(key, needle, word, upper) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Ok(lo)
    }

    pub fn child<S: ReadStore>(&self, _store: &S, index: usize) -> Result<Reference> {
        if self.level == 0 || index > self.count {
            return Err(Error::Corrupt("invalid branch child index"));
        }
        if let Some(layout) = self.compact() {
            if index == 0 {
                return Ok(self.first);
            }
            let (row, _) = layout.record(self.bytes, false, self.count, index - 1)?;
            let child = Reference::decode(&row[layout.key..])?;
            if child.is_null() {
                return Err(Error::Corrupt("null branch child"));
            }
            return Ok(child);
        }
        if index == 0 {
            Ok(self.first)
        } else {
            Reference::decode(&self.raw_cell(index - 1)?[4..20])
        }
    }
}

/// Big-endian words preserve byte ordering for equal eight-byte lengths. Mixed
/// lengths, including overflow keys, retain the ordinary slice comparison.
#[cfg(any(feature = "word-search", test))]
#[inline(always)]
fn precedes_word(key: &[u8], needle: &[u8], word: u64, upper: bool) -> bool {
    if let Ok(key) = <&[u8; 8]>::try_from(key) {
        let key = u64::from_be_bytes(*key);
        key < word || (upper && key == word)
    } else {
        key < needle || (upper && key == needle)
    }
}

#[cfg(test)]
mod word_search_tests;

/// Validate an entire page, including ordering and disjoint physical cells.
/// Child graphs and cross-page separator ranges require a tree-level audit.
pub fn validate_page<S: ReadStore>(store: &S, at: Reference) -> Result<()> {
    let page = Page::load(store, at)?;
    if page.compact().is_some() {
        return page.validate_compact();
    }
    let mut occupied = crate::page_buffer::PageBuffer::new(page.bytes.len());
    let mut previous: Option<&[u8]> = None;
    let mut lowest = page.bytes.len();
    for index in 0..page.count {
        let cell = page.cell(store, index)?;
        if previous.is_some_and(|key| cmp_validation_keys(key, cell.key).is_ge()) {
            return Err(Error::Corrupt("unordered tree keys"));
        }
        for used in &mut occupied[cell.start..cell.start + cell.raw.len()] {
            if *used != 0 {
                return Err(Error::Corrupt("overlapping tree cells"));
            }
            *used = 1;
        }
        lowest = lowest.min(cell.start);
        previous = Some(cell.key);
    }
    if lowest != page.upper {
        return Err(Error::Corrupt("incorrect upper page boundary"));
    }
    Ok(())
}

pub fn validate_page_with<S: ReadStore, C: crate::ordering::Comparator + ?Sized>(
    store: &S,
    at: Reference,
    order: &C,
) -> Result<()> {
    if order.is_bytewise() {
        return validate_page(store, at);
    }
    validate_ordered_or_structure(store, at, Some(order))
}
/// Complete page/cell/payload geometry without asserting an unavailable
/// application ordering. This is not an ordering certificate.
pub fn validate_page_structure<S: ReadStore>(store: &S, at: Reference) -> Result<()> {
    validate_ordered_or_structure::<S, crate::ordering::Bytewise>(store, at, None)
}
fn validate_ordered_or_structure<S: ReadStore, C: crate::ordering::Comparator + ?Sized>(
    store: &S,
    at: Reference,
    order: Option<&C>,
) -> Result<()> {
    let page = Page::load(store, at)?;
    let mut previous = None;
    let mut occupied = vec![false; page.bytes.len()];
    let mut lowest = page.bytes.len();
    for n in 0..page.count {
        let cell = page.cell(store, n)?;
        if let Some(order) = order {
            crate::ordering::stored(order, cell.key)?;
            if previous.is_some_and(|prior| order.compare(prior, cell.key).is_ge()) {
                return Err(Error::Corrupt("unordered tree keys"));
            }
        }
        previous = Some(cell.key);
        if page.compact().is_none() {
            let span = occupied
                .get_mut(cell.start..cell.start + cell.raw.len())
                .ok_or(Error::Corrupt("tree cell extent"))?;
            if span.iter().any(|used| *used) {
                return Err(Error::Corrupt("overlapping tree cells"));
            }
            span.fill(true);
            lowest = lowest.min(cell.start);
        }
    }
    if page.compact().is_some() {
        if page.bytes[page.upper..].iter().any(|byte| *byte != 0) {
            return Err(Error::Corrupt("nonzero compact page tail"));
        }
    } else if lowest != page.upper {
        return Err(Error::Corrupt("incorrect upper page boundary"));
    }
    Ok(())
}

/// Complete local geometry/order validation without retaining ordinary payload
/// bodies. The ReadStore still validates every header and declared extent.
pub(crate) fn validate_page_metadata<S: ReadStore, C: crate::ordering::Comparator + ?Sized>(
    store: &S,
    at: Reference,
    order: Option<&C>,
) -> Result<()> {
    let page = Page::load(store, at)?;
    let mut previous = None;
    let mut occupied = vec![false; page.bytes.len()];
    let mut lowest = page.bytes.len();
    for index in 0..page.count {
        let (cell, _) = page.cell_metadata(store, index, false)?;
        if let Some(order) = order {
            crate::ordering::stored(order, cell.key)?;
            if previous.is_some_and(|prior| order.compare(prior, cell.key).is_ge()) {
                return Err(Error::Corrupt("unordered tree keys"));
            }
        }
        previous = Some(cell.key);
        if page.compact().is_none() {
            let used = occupied
                .get_mut(cell.start..cell.start + cell.raw.len())
                .ok_or(Error::Corrupt("tree cell extent"))?;
            if used.iter().any(|used| *used) {
                return Err(Error::Corrupt("overlapping tree cells"));
            }
            used.fill(true);
            lowest = lowest.min(cell.start);
        }
    }
    if page.compact().is_some() {
        if page.bytes[page.upper..].iter().any(|byte| *byte != 0) {
            return Err(Error::Corrupt("nonzero compact page tail"));
        }
    } else if lowest != page.upper {
        return Err(Error::Corrupt("incorrect upper page boundary"));
    }
    Ok(())
}

pub(crate) fn encode(level: u16, first: Reference, cells: &[&[u8]]) -> Result<[u8; PAGE_SIZE]> {
    #[cfg(feature = "compact-pages")]
    if level < MAX_DEPTH
        && let Some(layout) = crate::compact::uniform(level, cells)
        && cells.len() <= layout.capacity(level == 0)
    {
        return crate::compact::encode(level, first, cells, layout);
    }
    encode_v1(level, first, cells)
}
pub(crate) fn encode_v1(level: u16, first: Reference, cells: &[&[u8]]) -> Result<[u8; PAGE_SIZE]> {
    let used = HEADER + cells.len() * 2 + cells.iter().map(|cell| cell.len()).sum::<usize>();
    if used > PAGE_SIZE || level >= MAX_DEPTH {
        return Err(Error::LimitExceeded);
    }
    let mut bytes = [0; PAGE_SIZE];
    bytes[0..2].copy_from_slice(&(if level == 0 { 2u16 } else { 3 }).to_le_bytes());
    bytes[2..4].copy_from_slice(&1u16.to_le_bytes());
    bytes[4..6].copy_from_slice(&level.to_le_bytes());
    bytes[6..8].copy_from_slice(&(cells.len() as u16).to_le_bytes());
    bytes[8..10].copy_from_slice(&((HEADER + cells.len() * 2) as u16).to_le_bytes());
    bytes[16..32].copy_from_slice(&first.encode());
    let mut offset = PAGE_SIZE;
    for (index, cell) in cells.iter().enumerate() {
        offset -= cell.len();
        bytes[offset..offset + cell.len()].copy_from_slice(cell);
        bytes[HEADER + 2 * index..HEADER + 2 * index + 2]
            .copy_from_slice(&(offset as u16).to_le_bytes());
    }
    bytes[10..12].copy_from_slice(&(offset as u16).to_le_bytes());
    Ok(bytes)
}

#[cfg(test)]
#[path = "page_geometry_tests.rs"]
mod geometry_tests;

/// Explicit variable geometry uses V3; default V1/V2 byte output is unchanged.
pub(crate) fn encode_for(
    size: usize,
    level: u16,
    first: Reference,
    cells: &[&[u8]],
) -> Result<crate::page_buffer::PageBuffer> {
    let size = ulmdb_core::PageSize::new(size)?.get();
    if size == PAGE_SIZE {
        return Ok(crate::page_buffer::PageBuffer::Default(encode(
            level, first, cells,
        )?));
    }
    encode_v1_for(size, level, first, cells)
}
pub(crate) fn encode_v1_for(
    size: usize,
    level: u16,
    first: Reference,
    cells: &[&[u8]],
) -> Result<crate::page_buffer::PageBuffer> {
    let size = ulmdb_core::PageSize::new(size)?.get();
    if size == PAGE_SIZE {
        return Ok(crate::page_buffer::PageBuffer::Default(encode_v1(
            level, first, cells,
        )?));
    }
    let used = HEADER + cells.len() * 2 + cells.iter().map(|c| c.len()).sum::<usize>();
    if used > size || level >= MAX_DEPTH || cells.len() > u16::MAX as usize {
        return Err(Error::LimitExceeded);
    }
    let mut out = crate::page_buffer::PageBuffer::new(size);
    out[..2].copy_from_slice(&(if level == 0 { 2u16 } else { 3u16 }).to_le_bytes());
    out[2..4].copy_from_slice(&3u16.to_le_bytes());
    out[4..6].copy_from_slice(&level.to_le_bytes());
    out[6..8].copy_from_slice(&(cells.len() as u16).to_le_bytes());
    out[8..10].copy_from_slice(&((HEADER + 2 * cells.len()) as u16).to_le_bytes());
    out[12..16].copy_from_slice(&(size as u32).to_le_bytes());
    out[16..32].copy_from_slice(&first.encode());
    let mut offset = size;
    for (i, cell) in cells.iter().enumerate() {
        offset -= cell.len();
        out[offset..offset + cell.len()].copy_from_slice(cell);
        out[HEADER + 2 * i..HEADER + 2 * i + 2].copy_from_slice(&(offset as u16).to_le_bytes());
    }
    out[10..12].copy_from_slice(&(offset as u16).to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod variable_geometry_tests {
    use super::*;
    #[test]
    fn variable_headers_bind_exact_width_and_keep_legacy_bytes() {
        for size in [256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536] {
            let encoded = encode_for(size, 0, Reference::NULL, &[]).unwrap();
            let page = Page::decode(&encoded).unwrap();
            assert_eq!(page.bytes.len(), size);
            assert_eq!(
                page.upper,
                if page.compact().is_some() {
                    HEADER
                } else {
                    size
                }
            );
            if size == 4096 {
                assert_eq!(&*encoded, &encode(0, Reference::NULL, &[]).unwrap());
            } else {
                assert_eq!(u16_at(&encoded, 2).unwrap(), 3);
                let mut damaged = encoded.to_vec();
                damaged[12..16].copy_from_slice(&4096u32.to_le_bytes());
                assert!(Page::decode(&damaged).is_err());
                let mut damaged = encoded.to_vec();
                damaged[2..4].copy_from_slice(&1u16.to_le_bytes());
                assert!(Page::decode(&damaged).is_err());
            }
        }
    }
}
