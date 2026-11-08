use crate::{
    page::{self, HEADER, Page},
    write::{Separator, balanced_split_for, branch_cell},
};
use ulmdb_core::storage::{Error, Reference, Result, WriteStore};

pub(super) struct Node {
    pub reference: Reference,
    pub minimum: Separator,
    pub level: u16,
}
pub(super) struct Buffer {
    bytes: crate::page_buffer::PageBuffer,
    pub minimum: Separator,
    pub level: u16,
    pub first: Reference,
    pub count: usize,
    upper: usize,
}
impl Buffer {
    pub fn new_for(size: usize, level: u16, first: Reference, minimum: Separator) -> Result<Self> {
        if (level == 0) != first.is_null() {
            return Err(Error::Corrupt("bulk page first-child kind"));
        }
        Ok(Self {
            bytes: page::encode_v1_for(size, level, first, &[])?,
            minimum,
            level,
            first,
            count: 0,
            upper: size,
        })
    }
    pub fn push(&mut self, cell: &[u8]) -> Result<bool> {
        if cell.is_empty() || !cell.len().is_multiple_of(2) {
            return Err(Error::Corrupt("bulk unpadded cell"));
        }
        let Some(upper) = self.upper.checked_sub(cell.len()) else {
            return Ok(false);
        };
        let count = self.count.checked_add(1).ok_or(Error::LimitExceeded)?;
        let lower = HEADER
            .checked_add(count.checked_mul(2).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::LimitExceeded)?;
        if lower > upper {
            return Ok(false);
        }
        self.bytes[upper..self.upper].copy_from_slice(cell);
        self.bytes[HEADER + self.count * 2..HEADER + count * 2]
            .copy_from_slice(&(upper as u16).to_le_bytes());
        self.count = count;
        self.upper = upper;
        Ok(true)
    }
    fn prepare(&mut self) -> Result<()> {
        let count = u16::try_from(self.count).map_err(|_| Error::LimitExceeded)?;
        let lower = u16::try_from(HEADER + self.count * 2).map_err(|_| Error::LimitExceeded)?;
        self.bytes[6..8].copy_from_slice(&count.to_le_bytes());
        self.bytes[8..10].copy_from_slice(&lower.to_le_bytes());
        self.bytes[10..12].copy_from_slice(&(self.upper as u16).to_le_bytes());
        Page::decode(&self.bytes)?;
        Ok(())
    }
    pub fn seal<S: WriteStore>(mut self, store: &mut S) -> Result<Node> {
        self.prepare()?;
        let reference = store.allocate(&self.bytes, store.page_size())?;
        if reference.segment == 0 || !reference.offset.is_multiple_of(store.page_size() as u64) {
            return Err(Error::Corrupt("bulk store returned invalid page reference"));
        }
        Ok(Node {
            reference,
            minimum: self.minimum,
            level: self.level,
        })
    }
}

/// Both suffix pages stay private until EOF. Redistribute their canonical cells
/// by byte occupancy, promoting the right branch's minimum exactly once.
pub(super) fn rebalance<S: WriteStore>(
    store: &mut S,
    mut left: Buffer,
    mut right: Buffer,
) -> Result<(Buffer, Buffer)> {
    if left.level != right.level || left.level == 0 && (left.count == 0 || right.count == 0) {
        return Err(Error::Corrupt("bulk suffix page mismatch"));
    }
    left.prepare()?;
    right.prepare()?;
    let level = left.level;
    let a = Page::decode(&left.bytes)?;
    let b = Page::decode(&right.bytes)?;
    let bridge = if level == 0 {
        None
    } else {
        Some(branch_cell(store, &right.minimum, right.first)?)
    };
    let mut cells = Vec::with_capacity(a.count + b.count + usize::from(bridge.is_some()));
    for i in 0..a.count {
        cells.push(a.raw_cell(i)?);
    }
    if let Some(cell) = &bridge {
        cells.push(cell.as_bytes());
    }
    for i in 0..b.count {
        cells.push(b.raw_cell(i)?);
    }
    let total = HEADER + cells.len() * 2 + cells.iter().map(|cell| cell.len()).sum::<usize>();
    let middle = balanced_split_for(store.page_size(), level, &cells, total)?;
    let minimum = Separator::from_cell(cells[middle], level)?;
    let (first, right_cells) = if level == 0 {
        (Reference::NULL, &cells[middle..])
    } else {
        (
            Reference::decode(&cells[middle][4..20])?,
            &cells[middle + 1..],
        )
    };
    let mut output_left = Buffer::new_for(store.page_size(), level, left.first, left.minimum)?;
    let mut output_right = Buffer::new_for(store.page_size(), level, first, minimum)?;
    for cell in &cells[..middle] {
        if !output_left.push(cell)? {
            return Err(Error::Corrupt("bulk left suffix overflow"));
        }
    }
    for cell in right_cells {
        if !output_right.push(cell)? {
            return Err(Error::Corrupt("bulk right suffix overflow"));
        }
    }
    Ok((output_left, output_right))
}
