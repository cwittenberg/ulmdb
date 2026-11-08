#[cfg(any(test, feature = "compact-pages"))]
use crate::page::PAGE_SIZE;
use crate::page::{self, HEADER, INLINE_KEY, INLINE_VALUE, MAX_DEPTH, Page};
use ulmdb_core::storage::{Error, Reference, Result, WriteStore};
pub mod ordered;

// Fixed buffers cover every cell the builder creates; overflow data stays in
// payload records. Normal changed cells and separators need no heap buffers.
const MAX_CELL: usize = 8 + INLINE_KEY + INLINE_VALUE;
pub(crate) struct CellBuffer {
    bytes: [u8; MAX_CELL],
    length: usize,
}
impl CellBuffer {
    fn empty() -> Self {
        Self {
            bytes: [0; MAX_CELL],
            length: 0,
        }
    }
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}

pub(crate) struct Separator {
    length: u16,
    flags: u16,
    representation: [u8; INLINE_KEY],
    size: usize,
    // A conforming external builder can use a larger inline threshold. Only
    // that uncommon separator needs ownership beyond our bounded fast buffer.
    large_inline: Option<Vec<u8>>,
}
impl Separator {
    pub(crate) fn from_cell(raw: &[u8], level: u16) -> Result<Self> {
        let length = page::u16_at(raw, 0)?;
        let flags = page::u16_at(raw, 2)? & 1;
        let size = if flags != 0 { 16 } else { usize::from(length) };
        let start = if level == 0 { 8 } else { 20 };
        let raw = raw
            .get(start..start + size)
            .ok_or(Error::Corrupt("short separator"))?;
        let mut representation = [0; INLINE_KEY];
        let large_inline = if size > INLINE_KEY {
            Some(raw.to_vec())
        } else {
            representation[..size].copy_from_slice(raw);
            None
        };
        Ok(Self {
            length,
            flags,
            representation,
            size,
            large_inline,
        })
    }
}

mod inline_insert;
pub use inline_insert::classify_dense_v1_leaf;
mod many;
mod prune;
mod reserved;
use many::persist_many;
pub use reserved::{put_reserved, put_reserved_with};
// Keep the common one-split result inline and the legacy Change size unchanged.
#[allow(clippy::large_enum_variant)]
enum Split {
    None,
    // A true no-op is distinct from an in-place private rewrite, whose root
    // reference also stays equal but can still need final root collapse.
    Unchanged,
    One((Separator, Reference)),
    Many(Box<[(Separator, Reference)]>),
}
impl Split {
    fn is_empty(&self) -> bool {
        matches!(self, Self::None | Self::Unchanged)
    }
}
struct Change {
    // NULL with no split explicitly propagates an emptied subtree. Only the
    // delete path produces it; an internal page never stores a null child.
    root: Reference,
    split: Split,
}

pub(crate) fn leaf_cell<S: WriteStore>(
    store: &mut S,
    key: &[u8],
    value: &[u8],
    old: Option<&[u8]>,
) -> Result<CellBuffer> {
    let value_len = u32::try_from(value.len()).map_err(|_| Error::LimitExceeded)?;
    let mut cell = CellBuffer::empty();
    let mut flags = 0u16;
    let mut end = 8;
    // `old` belongs to the stack-owned page, never to the mutable arena.
    // An exact-key replacement can retain its existing encoded key reference.
    let old_flags = old
        .map(|raw| page::u16_at(raw, 2))
        .transpose()?
        .unwrap_or(0);
    let key_len = if old_flags & 1 != 0 {
        let raw = old.ok_or(Error::Corrupt("missing old leaf"))?;
        cell.bytes[end..end + 16]
            .copy_from_slice(raw.get(8..24).ok_or(Error::Corrupt("short old key"))?);
        flags |= 1;
        end += 16;
        u16::MAX
    } else if key.len() <= INLINE_KEY.min(store.page_size() / 8) {
        cell.bytes[end..end + key.len()].copy_from_slice(key);
        end += key.len();
        key.len() as u16
    } else {
        flags |= 1;
        cell.bytes[end..end + 16].copy_from_slice(&store.put_payload(key)?.encode());
        end += 16;
        u16::MAX
    };
    if value.len() <= INLINE_VALUE.min(store.page_size() / 8) {
        cell.bytes[end..end + value.len()].copy_from_slice(value);
        end += value.len();
    } else {
        flags |= 2;
        let reference = if old_flags & 2 != 0 {
            let raw = old.ok_or(Error::Corrupt("missing old leaf"))?;
            let key_size = if old_flags & 1 != 0 {
                16
            } else {
                usize::from(page::u16_at(raw, 0)?)
            };
            let start = 8 + key_size;
            let old = Reference::decode(
                raw.get(start..start + 16)
                    .ok_or(Error::Corrupt("short old value"))?,
            )?;
            store.replace_payload(old, value)?
        } else {
            store.put_payload(value)?
        };
        cell.bytes[end..end + 16].copy_from_slice(&reference.encode());
        end += 16;
    }
    cell.bytes[..2].copy_from_slice(&key_len.to_le_bytes());
    cell.bytes[2..4].copy_from_slice(&flags.to_le_bytes());
    cell.bytes[4..8].copy_from_slice(&value_len.to_le_bytes());
    cell.length = end + end % 2;
    Ok(cell)
}

pub(crate) fn branch_cell<S: WriteStore>(
    store: &mut S,
    separator: &Separator,
    child: Reference,
) -> Result<CellBuffer> {
    let mut cell = CellBuffer::empty();
    let (length, flags, size) = if let Some(key) = &separator.large_inline {
        cell.bytes[20..36].copy_from_slice(&store.put_payload(key)?.encode());
        (u16::MAX, 1u16, 16)
    } else {
        cell.bytes[20..20 + separator.size]
            .copy_from_slice(&separator.representation[..separator.size]);
        (separator.length, separator.flags, separator.size)
    };
    cell.bytes[..2].copy_from_slice(&length.to_le_bytes());
    cell.bytes[2..4].copy_from_slice(&flags.to_le_bytes());
    cell.bytes[4..20].copy_from_slice(&child.encode());
    let end = 20 + size;
    cell.length = end + end % 2;
    Ok(cell)
}

#[cfg(test)]
pub(crate) fn balanced_split(level: u16, cells: &[&[u8]], total: usize) -> Result<usize> {
    balanced_split_for(PAGE_SIZE, level, cells, total)
}
pub(crate) fn balanced_split_for(
    page_size: usize,
    level: u16,
    cells: &[&[u8]],
    total: usize,
) -> Result<usize> {
    // Externally built valid pages may contain inline cells much larger
    // than this builder creates. Crossing the halfway point can overshoot
    // one page even though another boundary fits both pages.
    let mut left_used = HEADER;
    let mut best = None;
    let mut imbalance = usize::MAX;
    for middle in 1..cells.len() {
        left_used += 2 + cells[middle - 1].len();
        let mut right_used = HEADER + total - left_used;
        if level != 0 {
            // An internal split promotes this cell into the parent.
            right_used -= 2 + cells[middle].len();
        }
        if left_used <= page_size && right_used <= page_size {
            let distance = left_used.abs_diff(right_used);
            if distance < imbalance {
                best = Some(middle);
                imbalance = distance;
            }
        }
    }
    best.ok_or(Error::LimitExceeded)
}

fn persist<S: WriteStore>(
    store: &mut S,
    old: Reference,
    level: u16,
    first: Reference,
    cells: &[&[u8]],
    right_append: bool,
) -> Result<Change> {
    let total = HEADER + cells.len() * 2 + cells.iter().map(|cell| cell.len()).sum::<usize>();
    #[cfg(feature = "compact-pages")]
    let compact = if store.page_size() == PAGE_SIZE {
        crate::compact::uniform(level, cells)
    } else {
        None
    };
    #[cfg(not(feature = "compact-pages"))]
    let compact: Option<crate::compact::Layout> = None;
    if compact.is_some_and(|layout| cells.len() <= layout.capacity(level == 0))
        || total <= store.page_size()
    {
        let bytes = page::encode_for(store.page_size(), level, first, cells)?;
        let root = if old.is_null() {
            store.allocate(&bytes, store.page_size())?
        } else {
            store.rewrite(old, &bytes, store.page_size())?
        };
        return Ok(Change {
            root,
            split: Split::None,
        });
    }
    // A sorted import repeatedly fills the global rightmost leaf. Keep its
    // existing records together and place the new tail in a fresh leaf; a
    // half split would strand half of every completed page for future updates.
    // The explicit byte bound preserves imported/noncanonical cell layouts.
    let middle = if let Some(layout) = compact {
        if right_append && level == 0 && cells.len() > 1 && cells.len() - 1 <= layout.capacity(true)
        {
            cells.len() - 1
        } else {
            cells.len() / 2
        }
    } else {
        let append_middle = cells.len().checked_sub(1).filter(|middle| {
            right_append
                && level == 0
                && *middle != 0
                && total - 2 - cells[*middle].len() <= store.page_size()
        });
        if let Some(middle) = append_middle {
            middle
        } else {
            match balanced_split_for(store.page_size(), level, cells, total) {
                Ok(middle) => middle,
                Err(Error::LimitExceeded) => return persist_many(store, old, level, first, cells),
                Err(error) => return Err(error),
            }
        }
    };
    if middle == 0 || middle >= cells.len() {
        return Err(Error::LimitExceeded);
    }
    let separator = Separator::from_cell(cells[middle], level)?;
    let (right_first, right_cells) = if level == 0 {
        (Reference::NULL, &cells[middle..])
    } else {
        (
            Reference::decode(&cells[middle][4..20])?,
            &cells[middle + 1..],
        )
    };
    let left_bytes = page::encode_for(store.page_size(), level, first, &cells[..middle])?;
    let right_bytes = page::encode_for(store.page_size(), level, right_first, right_cells)?;
    let right = store.allocate(&right_bytes, store.page_size())?;
    let root = if old.is_null() {
        store.allocate(&left_bytes, store.page_size())?
    } else {
        store.rewrite(old, &left_bytes, store.page_size())?
    };
    Ok(Change {
        root,
        split: Split::One((separator, right)),
    })
}

enum Plan {
    Noop,
    PatchValue {
        offset: usize,
    },
    Leaf {
        index: usize,
        exists: bool,
    },
    Branch {
        level: u16,
        index: usize,
        child: Reference,
        offset: usize,
        last_child: bool,
        migrate: bool,
    },
}

fn plan<S: WriteStore>(
    store: &S,
    root: Reference,
    key: &[u8],
    value: Option<&[u8]>,
    expected: Option<u16>,
) -> Result<Plan> {
    let page = Page::load(store, root)?;
    if expected.is_some_and(|level| level != page.level) {
        return Err(Error::Corrupt("tree level mismatch"));
    }
    let index = page.search(store, key, page.level != 0)?;
    if page.level == 0 {
        if let Some(layout) = page.compact() {
            // V2 key_at and cell check the same complete inline row bounds.
            // Decode once; no payload dereference or extra check on a miss.
            if index < page.count {
                let cell = page.cell(store, index)?;
                if cell.key == key {
                    if let Some(value) = value {
                        if cell.value == value {
                            return Ok(Plan::Noop);
                        }
                        if cfg!(feature = "compact-pages") && layout.value == value.len() {
                            return Ok(Plan::PatchValue {
                                offset: cell.start + layout.key,
                            });
                        }
                    }
                    return Ok(Plan::Leaf {
                        index,
                        exists: true,
                    });
                }
            }
            return Ok(if value.is_none() {
                Plan::Noop
            } else {
                Plan::Leaf {
                    index,
                    exists: false,
                }
            });
        }
        let exists = index < page.count && page.key_at(store, index)? == key;
        if value.is_none() && !exists {
            return Ok(Plan::Noop);
        }
        if let Some(value) = value.filter(|_| exists) {
            let raw = page.raw_cell(index)?;
            let flags = page::u16_at(raw, 2)?;
            // The encoded inline span must already have this exact size.
            // An overflow payload's semantic length is never an inline span.
            if flags & 2 == 0
                && u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize == value.len()
            {
                let cell = page.cell(store, index)?;
                if cell.value == value {
                    return Ok(Plan::Noop);
                }
                let key_size = if flags & 1 != 0 {
                    16
                } else {
                    usize::from(page::u16_at(raw, 0)?)
                };
                let offset = cell
                    .start
                    .checked_add(8 + key_size)
                    .ok_or(Error::LimitExceeded)?;
                return Ok(Plan::PatchValue { offset });
            }
        }
        return Ok(Plan::Leaf { index, exists });
    }
    let child = page.child(store, index)?;
    let offset = page.child_offset(index)?;
    Ok(Plan::Branch {
        level: page.level,
        index,
        child,
        offset,
        last_child: index == page.count,
        migrate: page.compact().is_some() && !cfg!(feature = "compact-pages"),
    })
}

fn change<S: WriteStore>(
    store: &mut S,
    root: Reference,
    key: &[u8],
    value: Option<&[u8]>,
    expected: Option<u16>,
    right_edge: bool,
) -> Result<Change> {
    let changed = change_inner(store, root, key, value, expected, right_edge)?;
    page_change_receipt(store, root, changed)
}

fn page_change_receipt<S: WriteStore>(
    store: &mut S,
    old: Reference,
    changed: Change,
) -> Result<Change> {
    if changed.root != old {
        store.retire_page_edge(old)?;
    }
    Ok(changed)
}

fn change_inner<S: WriteStore>(
    store: &mut S,
    root: Reference,
    key: &[u8],
    value: Option<&[u8]>,
    expected: Option<u16>,
    right_edge: bool,
) -> Result<Change> {
    // All planning borrows end before a mutable call can grow or seal the arena.
    // Tiny replacements and unchanged ancestors never copy an entire page.
    match plan(store, root, key, value, expected)? {
        Plan::Noop => Ok(Change {
            root,
            split: Split::Unchanged,
        }),
        Plan::PatchValue { offset } => Ok(Change {
            root: store.patch(
                root,
                store.page_size(),
                offset,
                value.ok_or(Error::Corrupt("missing patch value"))?,
                store.page_size(),
            )?,
            split: Split::None,
        }),
        Plan::Leaf { index, exists } => {
            change_leaf(store, root, key, value, index, exists, right_edge)
        }
        Plan::Branch {
            level,
            index,
            child,
            offset,
            last_child,
            migrate,
        } => {
            let result = change(
                store,
                child,
                key,
                value,
                Some(level - 1),
                right_edge && last_child,
            )?;
            if result.root == child && result.split.is_empty() {
                return Ok(Change {
                    root,
                    split: result.split,
                });
            }
            if result.root.is_null() {
                if value.is_some() || !result.split.is_empty() {
                    return Err(Error::Corrupt("invalid empty child change"));
                }
                return prune::empty_child(store, root, level, index);
            }
            if result.split.is_empty() && !migrate {
                if result.root.is_null() {
                    return Err(Error::Corrupt("null updated branch child"));
                }
                return Ok(Change {
                    root: store.patch(
                        root,
                        store.page_size(),
                        offset,
                        &result.root.encode(),
                        store.page_size(),
                    )?,
                    split: Split::None,
                });
            }
            change_split_branch(store, root, level, index, result)
        }
    }
}

struct InlineAppend {
    header: [u8; 6],
    slot: usize,
    cell: usize,
}

// Admit only the packed V1/V3 image emitted by the ordinary encoder. Validate
// every old raw cell just as the reconstruction path does, without loading its
// overflow payload. External layouts and splits keep that established path.
fn prepare_inline_append(
    page: &Page<'_>,
    key: &[u8],
    value: &[u8],
    index: usize,
) -> Result<Option<InlineAppend>> {
    let size = page.bytes.len();
    if key.len() > INLINE_KEY.min(size / 8) || value.len() > INLINE_VALUE.min(size / 8) {
        return Ok(None);
    }
    if page.compact().is_some() || index != page.count {
        return Ok(None);
    }
    let raw_upper = usize::from(page::u16_at(page.bytes, 10)?);
    let upper = if raw_upper == 0 && size == 65536 {
        65536
    } else {
        raw_upper
    };
    let length = 8 + key.len() + value.len();
    let length = length + length % 2;
    let slot = HEADER + page.count * 2;
    let lower = slot + 2;
    let Some(cell) = upper.checked_sub(length).filter(|cell| *cell >= lower) else {
        return Ok(None);
    };
    let mut end = page.bytes.len();
    for old_index in 0..page.count {
        let raw = page.raw_cell(old_index)?;
        let start = usize::from(page::u16_at(page.bytes, HEADER + old_index * 2)?);
        if end.checked_sub(raw.len()) != Some(start) {
            return Ok(None);
        }
        end = start;
    }
    if end != upper {
        return Ok(None);
    }
    let mut header = [0; 6];
    header[..2].copy_from_slice(&((page.count + 1) as u16).to_le_bytes());
    header[2..4].copy_from_slice(&(lower as u16).to_le_bytes());
    header[4..6].copy_from_slice(&(cell as u16).to_le_bytes());
    Ok(Some(InlineAppend { header, slot, cell }))
}

// Layout changes keep the original full-page encoding path unless a checked
// append can preserve every old slot/cell with three bounded atomic patches.
fn change_leaf<S: WriteStore>(
    store: &mut S,
    root: Reference,
    key: &[u8],
    value: Option<&[u8]>,
    index: usize,
    exists: bool,
    right_edge: bool,
) -> Result<Change> {
    let prepared = if store.supports_patch_many(store.page_size(), store.page_size()) {
        let page = Page::load(store, root)?;
        if page.level != 0 {
            return Err(Error::Corrupt("expected changed leaf"));
        }
        if !exists
            && (!cfg!(feature="compact-pages") || store.page_size()!=page::PAGE_SIZE)
            && let Some(value)=value
            && store.admitted_dense_leaf(root,page.bytes)
            && let Some(insert)=inline_insert::prepare_admitted(&page,key,value,index)?
        {
            // The closed source's admission/induction proves old raw cells and
            // density. Fresh search, geometry, cell/slot arithmetic and atomic
            // span validation remain unchanged. No cache or certification read.
            let cell=leaf_cell(store,key,value,None)?;
            let patches=[(6,&insert.header[..]),(insert.slot,insert.slots()),(insert.cell,cell.as_bytes())];
            return Ok(Change{root:store.try_patch_many(root,store.page_size(),&patches,store.page_size())?
                .ok_or(Error::Corrupt("store declined advertised multi-patch"))?,split:Split::None});
        }
        if right_edge
            && !exists
            // Compact emission keeps its existing V1-to-V2 migration behavior.
            && (!cfg!(feature = "compact-pages") || store.page_size() != page::PAGE_SIZE)
            && let Some(value) = value
            && let Some(append) = prepare_inline_append(&page, key, value, index)?
        {
            // The preparation borrow ended before entering the mutable store.
            // Inline bounds guarantee encoding allocates no overflow payload.
            let cell = leaf_cell(store, key, value, None)?;
            let slot = (append.cell as u16).to_le_bytes();
            let patches = [
                (6, &append.header[..]),
                (append.slot, &slot[..]),
                (append.cell, cell.as_bytes()),
            ];
            return Ok(Change {
                root: store
                    .try_patch_many(root, store.page_size(), &patches, store.page_size())?
                    .ok_or(Error::Corrupt("store declined advertised multi-patch"))?,
                split: Split::None,
            });
        }
        if !exists
            && (!cfg!(feature = "compact-pages") || store.page_size() != page::PAGE_SIZE)
            && let Some(value) = value
            && let Some(insert) = inline_insert::prepare(&page, key, value, index)?
        {
            let cell = leaf_cell(store, key, value, None)?;
            let patches = [
                (6, &insert.header[..]),
                (insert.slot, insert.slots()),
                (insert.cell, cell.as_bytes()),
            ];
            return Ok(Change {
                root: store
                    .try_patch_many(root, store.page_size(), &patches, store.page_size())?
                    .ok_or(Error::Corrupt("store declined advertised multi-patch"))?,
                split: Split::None,
            });
        }
        if let Some(layout) = page.compact() {
            page.validate_compact()?;
            #[cfg(feature = "compact-pages")]
            if right_edge
                && !exists
                && index == page.count
                && page.count < layout.capacity(true)
                && key.len() == layout.key
                && let Some(value) = value.filter(|value| value.len() == layout.value)
                && (page.count == 0
                    || page::cmp_validation_keys(
                        layout
                            .cell(page.bytes, true, page.count, page.count - 1)?
                            .key,
                        key,
                    )
                    .is_lt())
            {
                let count = (page.count + 1) as u16;
                let start = layout.start(true, page.count);
                let upper = (start + layout.stride(true)) as u16;
                let count_bytes = count.to_le_bytes();
                let upper_bytes = upper.to_le_bytes();
                // The validated source borrow ends before mutation. These four
                // small spans preserve every old cell and the remaining tail.
                let patches = [
                    (6, &count_bytes[..]),
                    (10, &upper_bytes[..]),
                    (start, key),
                    (start + layout.key, value),
                ];
                return Ok(Change {
                    root: store
                        .try_patch_many(root, store.page_size(), &patches, store.page_size())?
                        .ok_or(Error::Corrupt("store declined advertised multi-patch"))?,
                    split: Split::None,
                });
            }
            #[cfg(not(feature = "compact-pages"))]
            let _ = layout;
        }
        Some(page)
    } else {
        None
    };
    let mut bytes = crate::page_buffer::PageBuffer::new(store.page_size());
    bytes.copy_from_slice(match prepared {
        Some(page) => page.bytes,
        None => Page::load(store, root)?.bytes,
    });
    let page = Page::decode(&bytes)?;
    if page.level != 0 {
        return Err(Error::Corrupt("expected changed leaf"));
    }
    if let Some(layout) = page.compact() {
        if prepared.is_none() {
            page.validate_compact()?;
        }
        #[cfg(feature = "compact-pages")]
        {
            let same_width =
                value.is_none_or(|value| key.len() == layout.key && value.len() == layout.value);
            let count = if value.is_some() && !exists {
                page.count + 1
            } else if value.is_none() && exists {
                page.count - 1
            } else {
                page.count
            };
            if same_width && count <= layout.capacity(true) {
                if count == 0 {
                    return Ok(Change {
                        root: Reference::NULL,
                        split: Split::None,
                    });
                }
                // Page borrows end before mutating this already-owned scratch;
                // the fallback retains its original decode and needs no copy.
                let stride = layout.stride(true);
                let start = layout.start(true, index);
                let end = HEADER + page.count * stride;
                if value.is_some() && !exists {
                    bytes.copy_within(start..end, start + stride);
                } else if value.is_none() && exists {
                    bytes.copy_within(start + stride..end, start);
                    bytes[end - stride..end].fill(0);
                }
                if let Some(value) = value {
                    bytes[start..start + layout.key].copy_from_slice(key);
                    bytes[start + layout.key..start + stride].copy_from_slice(value);
                }
                bytes[6..8].copy_from_slice(&(count as u16).to_le_bytes());
                bytes[10..12].copy_from_slice(&((HEADER + count * stride) as u16).to_le_bytes());
                return Ok(Change {
                    root: store.rewrite(root, &bytes, store.page_size())?,
                    split: Split::None,
                });
            }
        }
        #[cfg(not(feature = "compact-pages"))]
        let _ = layout;
    }
    let canonical = page
        .compact()
        .map(|_| crate::compact::Canonical::new(&page))
        .transpose()?;
    let mut cells = if let Some(canonical) = &canonical {
        canonical.slices()
    } else {
        (0..page.count)
            .map(|i| page.raw_cell(i))
            .collect::<Result<Vec<_>>>()?
    };
    let old = if exists { Some(cells[index]) } else { None };
    let replacement = value
        .map(|value| leaf_cell(store, key, value, old))
        .transpose()?;
    if exists {
        cells.remove(index);
    }
    if let Some(cell) = &replacement {
        cells.insert(index, cell.as_bytes());
    }
    if cells.is_empty() && value.is_none() {
        return Ok(Change {
            root: Reference::NULL,
            split: Split::None,
        });
    }
    let right_append = right_edge && !exists && value.is_some() && index == page.count;
    persist(store, root, 0, page.first, &cells, right_append)
}

fn change_split_branch<S: WriteStore>(
    store: &mut S,
    root: Reference,
    level: u16,
    index: usize,
    result: Change,
) -> Result<Change> {
    let mut bytes = crate::page_buffer::PageBuffer::new(store.page_size());
    bytes.copy_from_slice(Page::load(store, root)?.bytes);
    let page = Page::decode(&bytes)?;
    if page.level != level {
        return Err(Error::Corrupt("changed parent level mismatch"));
    }
    if page.compact().is_some() {
        page.validate_compact()?;
    }
    page.child(store, index)?;
    let mut first = page.first;
    if index == 0 {
        first = result.root;
    } else {
        let offset = page.child_offset(index)?;
        bytes[offset..offset + 16].copy_from_slice(&result.root.encode());
    }
    let page = Page::decode(&bytes)?;
    let canonical = page
        .compact()
        .map(|_| crate::compact::Canonical::new(&page))
        .transpose()?;
    let mut cells = if let Some(canonical) = &canonical {
        canonical.slices()
    } else {
        (0..page.count)
            .map(|i| page.raw_cell(i))
            .collect::<Result<Vec<_>>>()?
    };
    let one = match &result.split {
        Split::One((separator, right)) => Some(branch_cell(store, separator, *right)?),
        _ => None,
    };
    let many = if let Split::Many(items) = &result.split {
        items
            .iter()
            .map(|(separator, right)| branch_cell(store, separator, *right))
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    if let Some(cell) = &one {
        cells.insert(index, cell.as_bytes());
    }
    for (n, cell) in many.iter().enumerate() {
        cells.insert(index + n, cell.as_bytes());
    }
    persist(store, root, level, first, &cells, false)
}

/// Insert or replace one byte-key value. Changed paths use the store's CoW rewrite.
pub fn put<S: WriteStore>(
    store: &mut S,
    root: Reference,
    key: &[u8],
    value: &[u8],
) -> Result<Reference> {
    if value.len() > u32::MAX as usize {
        return Err(Error::LimitExceeded);
    }
    if root.is_null() {
        let cell = leaf_cell(store, key, value, None)?;
        return Ok(persist(store, root, 0, Reference::NULL, &[cell.as_bytes()], false)?.root);
    }
    let level = Page::load(store, root)?.level;
    let result = change(store, root, key, Some(value), None, true)?;
    finish_change(store, result, level)
}

fn finish_change<S: WriteStore>(store: &mut S, result: Change, level: u16) -> Result<Reference> {
    match result.split {
        Split::None | Split::Unchanged => Ok(result.root),
        Split::One((separator, right)) => {
            if level + 1 >= MAX_DEPTH {
                return Err(Error::LimitExceeded);
            }
            let cell = branch_cell(store, &separator, right)?;
            Ok(persist(
                store,
                Reference::NULL,
                level + 1,
                result.root,
                &[cell.as_bytes()],
                false,
            )?
            .root)
        }
        Split::Many(siblings) => many::promote(store, result.root, level, siblings.into_vec()),
    }
}

/// Delete a key if present, pruning emptied children along the changed path.
/// Internal roots with a sole child collapse; nonroot levels and inclusive
/// separator lower bounds remain intact. Sparse nonempty siblings are not merged.
pub fn delete<S: WriteStore>(store: &mut S, root: Reference, key: &[u8]) -> Result<Reference> {
    if root.is_null() {
        return Ok(root);
    }
    let level = Page::load(store, root)?.level;
    let result = change(store, root, key, None, None, false)?;
    // An imported unary root is legal. A missing delete preserves its exact
    // identity; root equality alone cannot prove no-op in a private store.
    if matches!(result.split, Split::Unchanged) && level != 0 {
        return Ok(root);
    }
    let root = finish_change(store, result, level)?;
    prune::collapse_root(store, root)
}

#[cfg(test)]
#[test]
fn common_change_and_decoder_layout_accounting() {
    #[allow(dead_code)]
    struct OldChange {
        root: Reference,
        split: Option<(Separator, Reference)>,
    }
    eprintln!(
        "COMPACT_LAYOUT Change={} oldChange={} Page={} Cell={}",
        std::mem::size_of::<Change>(),
        std::mem::size_of::<OldChange>(),
        std::mem::size_of::<Page<'_>>(),
        std::mem::size_of::<page::Cell<'_>>()
    );
    assert_eq!(
        std::mem::size_of::<Change>(),
        std::mem::size_of::<OldChange>()
    );
}

#[cfg(test)]
#[path = "write/plan_tests.rs"]
mod plan_tests;

#[cfg(test)]
#[path = "write/inline_append_tests.rs"]
mod inline_append_tests;

#[cfg(test)]
#[path = "write/prune_tests.rs"]
mod prune_tests;

#[cfg(test)]
#[path = "write/page_receipt_tests.rs"]
mod page_receipt_tests;

#[cfg(all(test, feature = "compact-pages"))]
#[path = "write/append_many_tests.rs"]
mod append_many_tests;
