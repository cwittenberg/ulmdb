//! Checked dense slotted-leaf insertion without rebuilding unchanged cells.
use super::*;

const SMALL_SLOT_BYTES: usize = 1024;
enum Slots {
    Small {
        bytes: [u8; SMALL_SLOT_BYTES],
        len: usize,
    },
    Large(Vec<u8>),
}
impl Slots {
    fn new(len: usize) -> Result<Self> {
        if len <= SMALL_SLOT_BYTES {
            Ok(Self::Small {
                bytes: [0; SMALL_SLOT_BYTES],
                len,
            })
        } else {
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(len)
                .map_err(|_| Error::LimitExceeded)?;
            bytes.resize(len, 0);
            Ok(Self::Large(bytes))
        }
    }
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Small { bytes, len } => &bytes[..*len],
            Self::Large(bytes) => bytes,
        }
    }
    fn bytes_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Small { bytes, len } => &mut bytes[..*len],
            Self::Large(bytes) => bytes,
        }
    }
}

// One bit per two-byte cell unit. The default geometry needs just256bytes;
// even the largest persisted page needs only4096bytes of temporary metadata.
enum Occupied {
    Small([u64; 32]),
    Large(Vec<u64>),
}
impl Occupied {
    fn new(size: usize) -> Result<Self> {
        if size <= 4096 {
            return Ok(Self::Small([0; 32]));
        }
        let len = size.div_ceil(128);
        let mut words = Vec::new();
        words
            .try_reserve_exact(len)
            .map_err(|_| Error::LimitExceeded)?;
        words.resize(len, 0);
        Ok(Self::Large(words))
    }
    fn mark(&mut self, start: usize, length: usize) -> Result<()> {
        let words = match self {
            Self::Small(words) => &mut words[..],
            Self::Large(words) => words,
        };
        mark_occupied(words,start,length)
    }
}
fn mark_occupied(words:&mut[u64],start:usize,length:usize)->Result<()> {
    let first = start / 2;
    let end = (start + length) / 2;
    for index in first / 64..=(end - 1) / 64 {
            let lo = if index == first / 64 { first % 64 } else { 0 };
            let hi = if index == (end - 1) / 64 {
                (end - 1) % 64 + 1
            } else {
                64
            };
            let mask = (u64::MAX << lo) & (u64::MAX >> (64 - hi));
            if words[index] & mask != 0 {
                return Err(Error::Corrupt("overlapping inline leaf cells"));
            }
            words[index] |= mask;
    }
    Ok(())
}

/// Classify only4KiB V1 leaves. None means unsupported format/geometry (including
/// valid branches); malformed supported leaves return an error. Some(false)
/// means valid, nonoverlapping cells with physical fragmentation. Some(true)
/// includes physically permuted dense slots. No overflow payload is read and
/// no ordering/graph/admission authority follows from this local classification.
/// The occupancy bitmap is exactly256 stack bytes; there is no heap scratch.
pub fn classify_dense_v1_leaf(bytes:&[u8])->Result<Option<bool>> {
    if bytes.len()!=crate::page::PAGE_SIZE || page::u16_at(bytes,2)?!=1 {return Ok(None);}
    let page=Page::decode(bytes)?;
    if page.level!=0 {return Ok(None);}
    let upper=usize::from(page::u16_at(bytes,10)?);
    let mut occupied=[0u64;32];let mut total=0usize;
    for index in 0..page.count {
        let raw=page.raw_cell(index)?;
        let start=usize::from(page::u16_at(bytes,HEADER+index*2)?);
        mark_occupied(&mut occupied,start,raw.len())?;
        total=total.checked_add(raw.len()).ok_or(Error::LimitExceeded)?;
    }
    Ok(Some(total==bytes.len()-upper))
}

pub(super) struct Insert {
    pub header: [u8; 6],
    pub slot: usize,
    pub cell: usize,
    slots: Slots,
}
impl Insert {
    pub fn slots(&self) -> &[u8] {
        self.slots.bytes()
    }
}

/// Every unchanged cell keeps the ordinary raw-cell validation. A dense body
/// may be physically permuted relative to key slots, including after earlier
/// middle insertions. Overlap is rejected; genuine fragmentation falls back to
/// the existing encoder. This owns only the changed slot suffix, never a page.
pub(super) fn prepare(
    page: &Page<'_>,
    key: &[u8],
    value: &[u8],
    index: usize,
) -> Result<Option<Insert>> {
    prepare_inner(page,key,value,index,false)
}
pub(super) fn prepare_admitted(page:&Page<'_>,key:&[u8],value:&[u8],index:usize)->Result<Option<Insert>> {
    let admitted=page.bytes.len()==crate::page::PAGE_SIZE && page::u16_at(page.bytes,2)?==1;
    prepare_inner(page,key,value,index,admitted)
}
fn prepare_inner(page:&Page<'_>,key:&[u8],value:&[u8],index:usize,admitted:bool)->Result<Option<Insert>> {
    let size = page.bytes.len();
    if page.compact().is_some()
        || page.level != 0
        || key.len() > INLINE_KEY.min(size / 8)
        || value.len() > INLINE_VALUE.min(size / 8)
    {
        return Ok(None);
    }
    if index > page.count {
        return Err(Error::Corrupt("inline insert slot outside page"));
    }
    let raw_upper = usize::from(page::u16_at(page.bytes, 10)?);
    let upper = if raw_upper == 0 && size == 65536 {
        size
    } else {
        raw_upper
    };
    let length = 8 + key.len() + value.len();
    let length = length + length % 2;
    let lower = HEADER + (page.count + 1) * 2;
    let Some(cell) = upper.checked_sub(length).filter(|cell| *cell >= lower) else {
        return Ok(None);
    };
    if !admitted {
        let mut occupied = Occupied::new(size)?;
        let mut total = 0usize;
        for at in 0..page.count {
            let raw = page.raw_cell(at)?;
            let start = usize::from(page::u16_at(page.bytes, HEADER + at * 2)?);
            occupied.mark(start, raw.len())?;
            total = total.checked_add(raw.len()).ok_or(Error::LimitExceeded)?;
        }
        if total != size - upper {
            return Ok(None);
        }
    }
    let slot = HEADER + index * 2;
    let old_end = HEADER + page.count * 2;
    let mut slots = Slots::new(old_end - slot + 2)?;
    slots.bytes_mut()[..2].copy_from_slice(&(cell as u16).to_le_bytes());
    slots.bytes_mut()[2..].copy_from_slice(&page.bytes[slot..old_end]);
    let mut header = [0; 6];
    header[..2].copy_from_slice(&((page.count + 1) as u16).to_le_bytes());
    header[2..4].copy_from_slice(&(lower as u16).to_le_bytes());
    header[4..6].copy_from_slice(&(cell as u16).to_le_bytes());
    Ok(Some(Insert {
        header,
        slot,
        cell,
        slots,
    }))
}
