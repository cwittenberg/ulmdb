//! Reserved values always use a real payload reference. The byte-value path and
//! its inline/no-op optimizations remain unchanged.
use super::*;
use crate::ordering::{Bytewise, Comparator, stored};
use ulmdb_core::storage::ReservedPayload;
fn cell<S: WriteStore>(
    store: &mut S,
    key: &[u8],
    value: &ReservedPayload,
    old: Option<&[u8]>,
) -> Result<CellBuffer> {
    let mut cell = leaf_cell(store, key, &[], old)?;
    let flags = page::u16_at(cell.as_bytes(), 2)?;
    let key_size = if flags & 1 != 0 {
        16
    } else {
        usize::from(page::u16_at(cell.as_bytes(), 0)?)
    };
    let at = 8 + key_size;
    cell.bytes[at..at + 16].copy_from_slice(&value.reference().encode());
    cell.bytes[2..4].copy_from_slice(&(flags | 2).to_le_bytes());
    cell.bytes[4..8].copy_from_slice(&(value.len() as u32).to_le_bytes());
    cell.length = at + 16;
    cell.length += cell.length % 2;
    Ok(cell)
}
fn change_reserved<S: WriteStore, C: Comparator + ?Sized>(
    store: &mut S,
    root: Reference,
    key: &[u8],
    value: &ReservedPayload,
    expected: Option<u16>,
    right_edge: bool,
    order: &C,
) -> Result<Change> {
    let changed = change_reserved_inner(store, root, key, value, expected, right_edge, order)?;
    page_change_receipt(store, root, changed)
}

#[allow(clippy::too_many_arguments)]
fn change_reserved_inner<S: WriteStore, C: Comparator + ?Sized>(
    store: &mut S,
    root: Reference,
    key: &[u8],
    value: &ReservedPayload,
    expected: Option<u16>,
    right_edge: bool,
    order: &C,
) -> Result<Change> {
    let (level, index, count, child, migrate) = {
        let p = Page::load(store, root)?;
        if expected.is_some_and(|n| n != p.level) {
            return Err(Error::Corrupt("tree level mismatch"));
        }
        let index = p.search_with(store, key, p.level != 0, order)?;
        (
            p.level,
            index,
            p.count,
            if p.level != 0 {
                Some(p.child(store, index)?)
            } else {
                None
            },
            p.compact().is_some() && !cfg!(feature = "compact-pages"),
        )
    };
    if level == 0 {
        let mut bytes = crate::page_buffer::PageBuffer::new(store.page_size());
        bytes.copy_from_slice(Page::load(store, root)?.bytes);
        let p = Page::decode(&bytes)?;
        let exists = if index < p.count {
            let actual = p.key_at(store, index)?;
            stored(order, actual)?;
            order.compare(actual, key).is_eq()
        } else {
            false
        };
        let canonical = p
            .compact()
            .map(|_| crate::compact::Canonical::new(&p))
            .transpose()?;
        let mut cells = if let Some(c) = &canonical {
            c.slices()
        } else {
            (0..p.count)
                .map(|i| p.raw_cell(i))
                .collect::<Result<Vec<_>>>()?
        };
        let representative = if exists {
            Some(p.key_at(store, index)?.to_vec())
        } else {
            None
        };
        let replacement = cell(
            store,
            representative.as_deref().unwrap_or(key),
            value,
            if exists { Some(cells[index]) } else { None },
        )?;
        if exists {
            cells.remove(index);
        }
        cells.insert(index, replacement.as_bytes());
        return persist(
            store,
            root,
            0,
            Reference::NULL,
            &cells,
            right_edge && !exists && index == count,
        );
    }
    let child = child.unwrap();
    let result = change_reserved(
        store,
        child,
        key,
        value,
        Some(level - 1),
        right_edge && index == count,
        order,
    )?;
    if result.split.is_empty() && !migrate {
        let offset = Page::load(store, root)?.child_offset(index)?;
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
/// Insert an initialized store-issued payload without copying its value bytes.
pub fn put_reserved<S: WriteStore>(
    store: &mut S,
    root: Reference,
    key: &[u8],
    value: &ReservedPayload,
) -> Result<Reference> {
    put_reserved_with(store, root, key, value, &Bytewise)
}
/// Reserved insertion under the immutable comparator bound to this tree.
pub fn put_reserved_with<S: WriteStore, C: Comparator + ?Sized>(
    store: &mut S,
    root: Reference,
    key: &[u8],
    value: &ReservedPayload,
    order: &C,
) -> Result<Reference> {
    order.check_ready()?;
    order.validate(key)?;
    value.check(store)?;
    if value.len() > u32::MAX as usize {
        return Err(Error::LimitExceeded);
    }
    if root.is_null() {
        let cell = cell(store, key, value, None)?;
        return Ok(persist(store, root, 0, Reference::NULL, &[cell.as_bytes()], false)?.root);
    }
    let level = Page::load(store, root)?.level;
    let result = change_reserved(store, root, key, value, None, true, order)?;
    finish_change(store, result, level)
}
