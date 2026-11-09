//! Comparator-aware CoW mutation; byte wrappers retain their existing hot path.
use super::*;
use crate::ordering::{Comparator, stored};

pub fn put_with<S: WriteStore, C: Comparator + ?Sized>(
    store: &mut S,
    root: Reference,
    key: &[u8],
    value: &[u8],
    order: &C,
) -> Result<Reference> {
    order.validate(key)?;
    if order.is_bytewise() {
        return super::put(store, root, key, value);
    }
    if value.len() > u32::MAX as usize {
        return Err(Error::LimitExceeded);
    }
    if root.is_null() {
        let cell = leaf_cell(store, key, value, None)?;
        return Ok(persist(store, root, 0, Reference::NULL, &[cell.as_bytes()], false)?.root);
    }
    let level = Page::load(store, root)?.level;
    let result = change_with(store, root, key, Some(value), None, true, order)?;
    finish_change(store, result, level)
}
pub fn delete_with<S: WriteStore, C: Comparator + ?Sized>(
    store: &mut S,
    root: Reference,
    key: &[u8],
    order: &C,
) -> Result<Reference> {
    order.validate(key)?;
    if order.is_bytewise() {
        return super::delete(store, root, key);
    }
    if root.is_null() {
        return Ok(root);
    }
    let level = Page::load(store, root)?.level;
    let result = change_with(store, root, key, None, None, false, order)?;
    let root = finish_change(store, result, level)?;
    let page = Page::load(store, root)?;
    Ok(if page.level == 0 && page.count == 0 {
        store.retire_page_edge(root)?;
        Reference::NULL
    } else {
        root
    })
}

fn plan_with<S: WriteStore, C: Comparator + ?Sized>(
    store: &S,
    root: Reference,
    key: &[u8],
    value: Option<&[u8]>,
    expected: Option<u16>,
    order: &C,
) -> Result<Plan> {
    let page = Page::load(store, root)?;
    if expected.is_some_and(|level| level != page.level) {
        return Err(Error::Corrupt("tree level mismatch"));
    }
    let index = page.search_with(store, key, page.level != 0, order)?;
    if page.level != 0 {
        return Ok(Plan::Branch {
            level: page.level,
            index,
            child: page.child(store, index)?,
            offset: page.child_offset(index)?,
            last_child: index == page.count,
            migrate: page.compact().is_some() && !cfg!(feature = "compact-pages"),
        });
    }
    let exists = if index < page.count {
        let cell = page.cell(store, index)?;
        stored(order, cell.key)?;
        if order.compare(cell.key, key).is_eq() {
            if let Some(value) = value {
                if cell.value == value {
                    return Ok(Plan::Noop);
                }
                if let Some(layout) = page.compact() {
                    if cfg!(feature = "compact-pages") && layout.value == value.len() {
                        return Ok(Plan::PatchValue {
                            offset: cell.start + layout.key,
                        });
                    }
                } else {
                    let flags = page::u16_at(cell.raw, 2)?;
                    if flags & 2 == 0
                        && u32::from_le_bytes(cell.raw[4..8].try_into().unwrap()) as usize
                            == value.len()
                    {
                        let key_size = if flags & 1 != 0 {
                            16
                        } else {
                            usize::from(page::u16_at(cell.raw, 0)?)
                        };
                        return Ok(Plan::PatchValue {
                            offset: cell.start + 8 + key_size,
                        });
                    }
                }
            }
            true
        } else {
            false
        }
    } else {
        false
    };
    Ok(if value.is_none() && !exists {
        Plan::Noop
    } else {
        Plan::Leaf { index, exists }
    })
}

pub(super) fn change_with<S: WriteStore, C: Comparator + ?Sized>(
    store: &mut S,
    root: Reference,
    key: &[u8],
    value: Option<&[u8]>,
    expected: Option<u16>,
    right_edge: bool,
    order: &C,
) -> Result<Change> {
    let changed = change_with_inner(store, root, key, value, expected, right_edge, order)?;
    page_change_receipt(store, root, changed)
}

#[allow(clippy::too_many_arguments)]
fn change_with_inner<S: WriteStore, C: Comparator + ?Sized>(
    store: &mut S,
    root: Reference,
    key: &[u8],
    value: Option<&[u8]>,
    expected: Option<u16>,
    right_edge: bool,
    order: &C,
) -> Result<Change> {
    match plan_with(store, root, key, value, expected, order)? {
        Plan::Noop => Ok(Change {
            root,
            split: Split::None,
        }),
        Plan::PatchValue { offset } => {
            let extent = Page::load(store, root)?.bytes.len();
            Ok(Change {
                root: store.patch(
                    root,
                    extent,
                    offset,
                    value.ok_or(Error::Corrupt("missing patch value"))?,
                    extent,
                )?,
                split: Split::None,
            })
        }
        Plan::Leaf { index, exists } => {
            change_leaf_with(store, root, key, value, index, exists, right_edge, order)
        }
        Plan::Branch {
            level,
            index,
            child,
            offset,
            last_child,
            migrate,
        } => {
            let result = change_with(
                store,
                child,
                key,
                value,
                Some(level - 1),
                right_edge && last_child,
                order,
            )?;
            if result.root == child && result.split.is_empty() {
                return Ok(Change {
                    root,
                    split: Split::None,
                });
            }
            if result.split.is_empty() && !migrate {
                if result.root.is_null() {
                    return Err(Error::Corrupt("null updated branch child"));
                }
                let extent = Page::load(store, root)?.bytes.len();
                return Ok(Change {
                    root: store.patch(root, extent, offset, &result.root.encode(), extent)?,
                    split: Split::None,
                });
            }
            split_branch_with(store, root, level, index, result, order)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn change_leaf_with<S: WriteStore, C: Comparator + ?Sized>(
    store: &mut S,
    root: Reference,
    key: &[u8],
    value: Option<&[u8]>,
    index: usize,
    exists: bool,
    right_edge: bool,
    order: &C,
) -> Result<Change> {
    let bytes = Page::load(store, root)?.bytes.to_vec();
    let page = Page::decode(&bytes)?;
    if page.level != 0 {
        return Err(Error::Corrupt("expected changed leaf"));
    }
    let canonical = page
        .compact()
        .map(|_| crate::compact::Canonical::new(&page))
        .transpose()?;
    let mut cells = if let Some(canonical) = &canonical {
        canonical.slices()
    } else {
        (0..page.count)
            .map(|n| page.raw_cell(n))
            .collect::<Result<Vec<_>>>()?
    };
    // Overwrite preserves the stored equality-class representative. Owning the
    // one changed key also ends any overflow borrow before mutable allocation.
    let representative = if exists {
        let cell = page.cell(store, index)?;
        stored(order, cell.key)?;
        Some(cell.key.to_vec())
    } else {
        None
    };
    let old = if exists { Some(cells[index]) } else { None };
    let replacement = value
        .map(|value| leaf_cell(store, representative.as_deref().unwrap_or(key), value, old))
        .transpose()?;
    if exists {
        cells.remove(index);
    }
    if let Some(cell) = &replacement {
        cells.insert(index, cell.as_bytes());
    }
    persist(
        store,
        root,
        0,
        page.first,
        &cells,
        right_edge && !exists && value.is_some() && index == page.count,
    )
}

fn split_branch_with<S: WriteStore, C: Comparator + ?Sized>(
    store: &mut S,
    root: Reference,
    level: u16,
    index: usize,
    result: Change,
    _order: &C,
) -> Result<Change> {
    let mut bytes = Page::load(store, root)?.bytes.to_vec();
    let page = Page::decode(&bytes)?;
    if page.level != level {
        return Err(Error::Corrupt("changed parent level mismatch"));
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
            .map(|n| page.raw_cell(n))
            .collect::<Result<Vec<_>>>()?
    };
    let one = if let Split::One((separator, right)) = &result.split {
        Some(branch_cell(store, separator, *right)?)
    } else {
        None
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
