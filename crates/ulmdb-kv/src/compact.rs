//! Explicit version2 fixed-width inline pages; no slots or per-record headers.
use crate::page::{Cell, HEADER, INLINE_KEY, INLINE_VALUE, PAGE_SIZE, Page};
use ulmdb_core::storage::{Error, Reference, Result};
#[derive(Clone, Copy)]
pub(crate) struct Layout {
    pub key: usize,
    pub value: usize,
}
impl Layout {
    pub fn read(bytes: &[u8]) -> Self {
        Self {
            key: u16::from_le_bytes([bytes[12], bytes[13]]) as usize,
            value: u16::from_le_bytes([bytes[14], bytes[15]]) as usize,
        }
    }
    pub fn stride(self, leaf: bool) -> usize {
        self.key + if leaf { self.value } else { 16 }
    }
    pub fn capacity(self, leaf: bool) -> usize {
        let stride = self.stride(leaf);
        if stride == 0 {
            1
        } else {
            (PAGE_SIZE - HEADER) / stride
        }
    }
    pub fn start(self, leaf: bool, index: usize) -> usize {
        HEADER + index * self.stride(leaf)
    }
    pub fn record(
        self,
        bytes: &[u8],
        leaf: bool,
        count: usize,
        index: usize,
    ) -> Result<(&[u8], usize)> {
        if index >= count {
            return Err(Error::Corrupt("tree slot outside page"));
        }
        let start = self.start(leaf, index);
        let end = start + self.stride(leaf);
        Ok((
            bytes
                .get(start..end)
                .ok_or(Error::Corrupt("compact record extent"))?,
            start,
        ))
    }
    pub fn cell<'a>(
        self,
        bytes: &'a [u8],
        leaf: bool,
        count: usize,
        index: usize,
    ) -> Result<Cell<'a>> {
        let (raw, start) = self.record(bytes, leaf, count, index)?;
        let key = raw
            .get(..self.key)
            .ok_or(Error::Corrupt("compact key extent"))?;
        let value = if leaf {
            &raw[self.key..]
        } else {
            if Reference::decode(&raw[self.key..])?.is_null() {
                return Err(Error::Corrupt("null branch child"));
            }
            &[]
        };
        Ok(Cell {
            key,
            value,
            raw,
            start,
        })
    }
    pub fn search(self, page: &Page<'_>, needle: &[u8], upper: bool) -> Result<usize> {
        #[cfg(feature = "word-search")]
        {
            if self.key == 8
                && let Ok(n) = <&[u8; 8]>::try_from(needle)
            {
                return search_word_records(self, page, u64::from_be_bytes(*n), upper);
            }
            search_byte_records(self, page, needle, upper)
        }
        #[cfg(not(feature = "word-search"))]
        {
            let (mut lo, mut hi) = (0, page.count);
            let leaf = page.level == 0;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                let (row, _) = self.record(page.bytes, leaf, page.count, mid)?;
                let key = &row[..self.key];
                if key < needle || upper && key == needle {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            Ok(lo)
        }
    }
}
// Keep numeric traversal separate from the fallback's slice-comparison calls.
// Each path retains the original midpoint order and checked record extents.
#[cfg(feature = "word-search")]
#[inline(never)]
fn search_word_records(layout: Layout, page: &Page<'_>, needle: u64, upper: bool) -> Result<usize> {
    let leaf = page.level == 0;
    if leaf && layout.value == 8 {
        return search_word_rows::<16>(page, needle, upper);
    }
    if !leaf {
        return search_word_rows::<24>(page, needle, upper);
    }
    let (mut lo, mut hi) = (0, page.count);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let (row, _) = layout.record(page.bytes, leaf, page.count, mid)?;
        let key = u64::from_be_bytes(row[..8].try_into().unwrap());
        if key < needle || upper && key == needle {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    Ok(lo)
}

#[cfg(feature = "word-search")]
#[inline(never)]
fn search_byte_records(
    layout: Layout,
    page: &Page<'_>,
    needle: &[u8],
    upper: bool,
) -> Result<usize> {
    let (mut lo, mut hi) = (0, page.count);
    let leaf = page.level == 0;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let (row, _) = layout.record(page.bytes, leaf, page.count, mid)?;
        let key = &row[..layout.key];
        if key < needle || upper && key == needle {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    Ok(lo)
}

// Checked byte-array rows express the common compact layouts without integer
// pointer casts or alignment assumptions. Other widths use the record path.
#[cfg(feature = "word-search")]
#[inline]
fn search_word_rows<const STRIDE: usize>(
    page: &Page<'_>,
    needle: u64,
    upper: bool,
) -> Result<usize> {
    let body = page
        .bytes
        .get(HEADER..)
        .ok_or(Error::Corrupt("compact record extent"))?;
    let (rows, _) = body.as_chunks::<STRIDE>();
    let rows = rows
        .get(..page.count)
        .ok_or(Error::Corrupt("compact record extent"))?;
    let (mut lo, mut hi) = (0, rows.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let key = u64::from_be_bytes(rows[mid][..8].try_into().unwrap());
        if key < needle || upper && key == needle {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    Ok(lo)
}

/// Exact point lookup for a decoded leaf with eight-byte inline keys/values.
/// Keep the original lower-bound midpoint and checked row extents, then borrow
/// the selected stored key/value directly instead of decoding the cell again.
#[cfg(feature = "word-search")]
#[inline(never)]
pub(crate) fn word_leaf_pair<'a>(
    page: &Page<'a>,
    needle: u64,
) -> Result<Option<(&'a [u8], &'a [u8])>> {
    let body = page
        .bytes
        .get(HEADER..)
        .ok_or(Error::Corrupt("compact record extent"))?;
    let (rows, _) = body.as_chunks::<16>();
    let rows = rows
        .get(..page.count)
        .ok_or(Error::Corrupt("compact record extent"))?;
    let (mut lo, mut hi) = (0, rows.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let key = u64::from_be_bytes(rows[mid][..8].try_into().unwrap());
        if key < needle {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo == rows.len() {
        return Ok(None);
    }
    let row = &rows[lo];
    let key = u64::from_be_bytes(row[..8].try_into().unwrap());
    Ok((key == needle).then_some((&row[..8], &row[8..])))
}

pub(crate) fn valid(leaf: bool, count: u16, lower: u16, upper: u16, key: u16, value: u16) -> bool {
    const _: () =
        assert!(HEADER == 32 && PAGE_SIZE == 4096 && INLINE_KEY == 256 && INLINE_VALUE == 512);
    ulmdb_core::compact_page_geometry::valid_geometry(leaf, count, lower, upper, key, value)
}
/// Canonical V1 input cells; no payload dereferences or benchmark-specific widths.
#[cfg(feature = "compact-pages")]
pub(crate) fn uniform(level: u16, cells: &[&[u8]]) -> Option<Layout> {
    let leaf = level == 0;
    let mut layout = None;
    for raw in cells {
        let key = u16::from_le_bytes(raw.get(..2)?.try_into().ok()?) as usize;
        let flags = u16::from_le_bytes(raw.get(2..4)?.try_into().ok()?);
        let value = if leaf {
            u32::from_le_bytes(raw.get(4..8)?.try_into().ok()?) as usize
        } else {
            0
        };
        if flags != 0 || key > INLINE_KEY || value > INLINE_VALUE {
            return None;
        }
        let end = if leaf { 8 + key + value } else { 20 + key };
        if raw.len() != end + end % 2 || end % 2 != 0 && raw[end] != 0 {
            return None;
        }
        let next = Layout { key, value };
        if layout.is_some_and(|old: Layout| old.key != key || old.value != value) {
            return None;
        }
        layout = Some(next);
    }
    Some(layout.unwrap_or(Layout { key: 0, value: 0 }))
}
#[cfg(any(feature = "compact-pages", test))]
pub(crate) fn encode(
    level: u16,
    first: Reference,
    cells: &[&[u8]],
    layout: Layout,
) -> Result<[u8; PAGE_SIZE]> {
    if cells.len() > layout.capacity(level == 0) {
        return Err(Error::LimitExceeded);
    }
    let mut out = [0; PAGE_SIZE];
    out[..2].copy_from_slice(&(if level == 0 { 2u16 } else { 3 }).to_le_bytes());
    out[2..4].copy_from_slice(&2u16.to_le_bytes());
    out[4..6].copy_from_slice(&level.to_le_bytes());
    out[6..8].copy_from_slice(&(cells.len() as u16).to_le_bytes());
    out[8..10].copy_from_slice(&(HEADER as u16).to_le_bytes());
    out[10..12].copy_from_slice(
        &((HEADER + cells.len() * layout.stride(level == 0)) as u16).to_le_bytes(),
    );
    out[12..14].copy_from_slice(&(layout.key as u16).to_le_bytes());
    out[14..16].copy_from_slice(&(layout.value as u16).to_le_bytes());
    out[16..32].copy_from_slice(&first.encode());
    for (i, raw) in cells.iter().enumerate() {
        let start = layout.start(level == 0, i);
        let h = if level == 0 { 8 } else { 20 };
        out[start..start + layout.key].copy_from_slice(&raw[h..h + layout.key]);
        if level == 0 {
            out[start + layout.key..start + layout.key + layout.value]
                .copy_from_slice(&raw[h + layout.key..h + layout.key + layout.value]);
        } else {
            out[start + layout.key..start + layout.key + 16].copy_from_slice(&raw[4..20]);
        }
    }
    Ok(out)
}
/// One flat transient arena for structural normalization; never a serving cache.
pub(crate) struct Canonical {
    bytes: Vec<u8>,
    ends: Vec<usize>,
}
impl Canonical {
    pub fn new(page: &Page<'_>) -> Result<Self> {
        let layout = page
            .compact()
            .ok_or(Error::Corrupt("expected compact normalization"))?;
        let leaf = page.level == 0;
        let size = (if leaf { 8 } else { 20 }) + layout.key + if leaf { layout.value } else { 0 };
        let size = size + size % 2;
        let mut bytes = Vec::new();
        let mut ends = Vec::new();
        bytes
            .try_reserve_exact(page.count * size)
            .map_err(|_| Error::LimitExceeded)?;
        ends.try_reserve_exact(page.count)
            .map_err(|_| Error::LimitExceeded)?;
        for index in 0..page.count {
            let cell = layout.cell(page.bytes, leaf, page.count, index)?;
            let start = bytes.len();
            bytes.resize(start + size, 0);
            bytes[start..start + 2].copy_from_slice(&(layout.key as u16).to_le_bytes());
            if leaf {
                bytes[start + 4..start + 8].copy_from_slice(&(layout.value as u32).to_le_bytes());
                bytes[start + 8..start + 8 + layout.key].copy_from_slice(cell.key);
                bytes[start + 8 + layout.key..start + 8 + layout.key + layout.value]
                    .copy_from_slice(cell.value);
            } else {
                bytes[start + 4..start + 20].copy_from_slice(&cell.raw[layout.key..]);
                bytes[start + 20..start + 20 + layout.key].copy_from_slice(cell.key);
            }
            ends.push(bytes.len());
        }
        Ok(Self { bytes, ends })
    }
    pub fn slices(&self) -> Vec<&[u8]> {
        let mut start = 0;
        self.ends
            .iter()
            .map(|&end| {
                let row = &self.bytes[start..end];
                start = end;
                row
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
