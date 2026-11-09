//! Bounded sorted updates sharing the changed pages and their ancestor paths.
use super::*;
use crate::ordering::{Bytewise, Comparator, stored};

const CHUNK_RECORDS: usize = 256;
const CHUNK_BYTES: usize = 1024 * 1024;

/// The resulting tree and number of accepted input records, including no-ops.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchUpsert {
    pub root: Reference,
    pub records: u64,
}

/// Insert or replace strictly increasing keys in an existing tree.
///
/// Updates share each touched leaf and ancestor per bounded chunk; untouched
/// subtrees are reused without scanning them. At most 256 input items and one
/// MiB plus one maximum-size record are buffered. Existing objects obey the
/// store's CoW policy. A late iterator/store error can leave private changes:
/// callers must discard or roll back the store before reusing it.
pub fn upsert_sorted<S, I, K, V>(store: &mut S, root: Reference, entries: I) -> Result<BatchUpsert>
where
    S: WriteStore,
    I: IntoIterator<Item = Result<(K, V)>>,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
{
    upsert_sorted_with(store, root, entries, &Bytewise)
}

/// Sorted upsert under the supplied comparison authority. Equivalent input
/// keys are rejected; replacing an existing equality class keeps its stored
/// key bytes, as ordinary comparator-aware `put_with` does.
pub fn upsert_sorted_with<S, I, K, V, C>(
    store: &mut S,
    mut root: Reference,
    entries: I,
    order: &C,
) -> Result<BatchUpsert>
where
    S: WriteStore,
    I: IntoIterator<Item = Result<(K, V)>>,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
    C: Comparator + ?Sized,
{
    order.check_ready()?;
    let mut entries = entries.into_iter();
    let mut previous: Option<Vec<u8>> = None;
    let mut records = 0u64;
    loop {
        let mut chunk = Vec::with_capacity(CHUNK_RECORDS);
        let mut bytes = 0usize;
        while chunk.len() < CHUNK_RECORDS && bytes < CHUNK_BYTES {
            let Some(entry) = entries.next() else { break };
            let (key, value) = entry?;
            let key_bytes = key.as_ref();
            let value_bytes = value.as_ref();
            if key_bytes.len() > store.max_key_size() || value_bytes.len() > store.max_value_size()
            {
                return Err(Error::InvalidInput("sorted upsert size limit exceeded"));
            }
            order.validate(key_bytes)?;
            let prior = chunk
                .last()
                .map(|(key, _): &(K, V)| key.as_ref())
                .or(previous.as_deref());
            if prior.is_some_and(|prior| !order.compare(prior, key_bytes).is_lt()) {
                return Err(Error::InvalidInput(
                    "sorted upsert keys must increase strictly",
                ));
            }
            bytes = bytes
                .checked_add(key_bytes.len())
                .and_then(|n| n.checked_add(value_bytes.len()))
                .ok_or(Error::LimitExceeded)?;
            chunk.push((key, value));
        }
        if chunk.is_empty() {
            break;
        }
        records = records
            .checked_add(chunk.len() as u64)
            .ok_or(Error::LimitExceeded)?;
        previous = Some(chunk.last().unwrap().0.as_ref().to_vec());
        let level = if root.is_null() {
            0
        } else {
            Page::load(store, root)?.level
        };
        let changed = change_chunk(store, root, &chunk, Some(level), true, order)?;
        root = finish_change(store, changed, level)?;
    }
    Ok(BatchUpsert { root, records })
}

fn change_chunk<S, K, V, C>(
    store: &mut S,
    root: Reference,
    entries: &[(K, V)],
    expected: Option<u16>,
    right_edge: bool,
    order: &C,
) -> Result<Change>
where
    S: WriteStore,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
    C: Comparator + ?Sized,
{
    // Sparse subtrees retain the ordinary patch/append/no-op implementation.
    if entries.len() == 1 && !root.is_null() {
        let (key, value) = &entries[0];
        return if order.is_bytewise() {
            change(
                store,
                root,
                key.as_ref(),
                Some(value.as_ref()),
                expected,
                right_edge,
            )
        } else {
            ordered::change_with(
                store,
                root,
                key.as_ref(),
                Some(value.as_ref()),
                expected,
                right_edge,
                order,
            )
        };
    }
    if root.is_null() {
        let cells = entries
            .iter()
            .map(|(key, value)| leaf_cell(store, key.as_ref(), value.as_ref(), None))
            .collect::<Result<Vec<_>>>()?;
        let cells = cells.iter().map(CellBuffer::as_bytes).collect::<Vec<_>>();
        return persist_batch(store, root, 0, Reference::NULL, &cells);
    }
    let bytes = Page::load(store, root)?.bytes.to_vec();
    let page = Page::decode(&bytes)?;
    if expected.is_some_and(|level| level != page.level) {
        return Err(Error::Corrupt("batch tree level mismatch"));
    }
    let changed = if page.level == 0 {
        change_leaf_batch(store, root, &page, entries, order)?
    } else {
        change_branch_batch(store, root, &page, entries, right_edge, order)?
    };
    page_change_receipt(store, root, changed)
}

fn change_leaf_batch<S, K, V, C>(
    store: &mut S,
    root: Reference,
    page: &Page<'_>,
    entries: &[(K, V)],
    order: &C,
) -> Result<Change>
where
    S: WriteStore,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
    C: Comparator + ?Sized,
{
    // Fixed-size inline replacements can change one copied page directly.
    // No cell encoding, payload allocation, or per-record ancestor rewrite.
    let mut patches = Vec::with_capacity(entries.len());
    let mut inline_only = true;
    for (key, value) in entries {
        let index = page.search_with(store, key.as_ref(), false, order)?;
        if index == page.count {
            inline_only = false;
            break;
        }
        let cell = page.cell(store, index)?;
        stored(order, cell.key)?;
        let inline = page.compact().is_some() || page::u16_at(cell.raw, 2)? & 2 == 0;
        if !order.compare(cell.key, key.as_ref()).is_eq()
            || !inline
            || cell.value.len() != value.as_ref().len()
        {
            inline_only = false;
            break;
        }
        if cell.value != value.as_ref() {
            let offset = cell.start
                + if let Some(layout) = page.compact() {
                    layout.key
                } else {
                    8 + if page::u16_at(cell.raw, 2)? & 1 != 0 {
                        16
                    } else {
                        usize::from(page::u16_at(cell.raw, 0)?)
                    }
                };
            patches.push((offset, value.as_ref()));
        }
    }
    if inline_only {
        if patches.is_empty() {
            return Ok(Change {
                root,
                split: Split::Unchanged,
            });
        }
        let mut bytes = page.bytes.to_vec();
        for (offset, value) in patches {
            bytes[offset..offset + value.len()].copy_from_slice(value);
        }
        return Ok(Change {
            root: store.rewrite(root, &bytes, store.page_size())?,
            split: Split::None,
        });
    }
    let canonical = page
        .compact()
        .map(|_| crate::compact::Canonical::new(page))
        .transpose()?;
    let old = if let Some(canonical) = &canonical {
        canonical.slices()
    } else {
        (0..page.count)
            .map(|n| page.raw_cell(n))
            .collect::<Result<Vec<_>>>()?
    };
    enum Cell<'a> {
        Old(&'a [u8]),
        New(usize),
    }
    let mut replacements = Vec::with_capacity(entries.len());
    let mut merged = Vec::with_capacity(old.len() + entries.len());
    let (mut prior, mut next) = (0, 0);
    let mut changed = false;
    while next < entries.len() {
        let (key, value) = &entries[next];
        if prior == old.len() {
            merged.push(Cell::New(replacements.len()));
            replacements.push(leaf_cell(store, key.as_ref(), value.as_ref(), None)?);
            changed = true;
            next += 1;
            continue;
        }
        let cell = page.cell(store, prior)?;
        stored(order, cell.key)?;
        match order.compare(cell.key, key.as_ref()) {
            std::cmp::Ordering::Less => {
                merged.push(Cell::Old(old[prior]));
                prior += 1;
            }
            std::cmp::Ordering::Equal => {
                if cell.value == value.as_ref() {
                    merged.push(Cell::Old(old[prior]));
                } else {
                    let representative = cell.key.to_vec();
                    merged.push(Cell::New(replacements.len()));
                    replacements.push(leaf_cell(
                        store,
                        &representative,
                        value.as_ref(),
                        Some(old[prior]),
                    )?);
                    changed = true;
                }
                prior += 1;
                next += 1;
            }
            std::cmp::Ordering::Greater => {
                merged.push(Cell::New(replacements.len()));
                replacements.push(leaf_cell(store, key.as_ref(), value.as_ref(), None)?);
                changed = true;
                next += 1;
            }
        }
    }
    merged.extend(old[prior..].iter().map(|raw| Cell::Old(raw)));
    if !changed {
        return Ok(Change {
            root,
            split: Split::Unchanged,
        });
    }
    let cells = merged
        .iter()
        .map(|cell| match cell {
            Cell::Old(raw) => *raw,
            Cell::New(index) => replacements[*index].as_bytes(),
        })
        .collect::<Vec<_>>();
    persist_batch(store, root, 0, Reference::NULL, &cells)
}

fn change_branch_batch<S, K, V, C>(
    store: &mut S,
    root: Reference,
    page: &Page<'_>,
    entries: &[(K, V)],
    right_edge: bool,
    order: &C,
) -> Result<Change>
where
    S: WriteStore,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
    C: Comparator + ?Sized,
{
    // Most existing-index updates preserve the branch's shape. Keep its
    // encoded cells intact and patch child references in one private image;
    // only normalize/reconstruct cells when a child actually splits.
    let mut bytes: Option<Vec<u8>> = None;
    let mut splits: Vec<(usize, Vec<CellBuffer>)> = Vec::new();
    let mut start = 0;
    while start < entries.len() {
        let index = page.search_with(store, entries[start].0.as_ref(), true, order)?;
        let end = if index == page.count {
            entries.len()
        } else {
            let upper = page.cell(store, index)?.key;
            stored(order, upper)?;
            start
                + entries[start..]
                    .partition_point(|(key, _)| order.compare(key.as_ref(), upper).is_lt())
        };
        if end == start {
            return Err(Error::Corrupt("batch child bounds"));
        }
        let child = page.child(store, index)?;
        let result = change_chunk(
            store,
            child,
            &entries[start..end],
            Some(page.level - 1),
            right_edge && index == page.count,
            order,
        )?;
        if result.root != child || !result.split.is_empty() {
            let offset = page.child_offset(index)?;
            let bytes = bytes.get_or_insert_with(|| page.bytes.to_vec());
            bytes[offset..offset + 16].copy_from_slice(&result.root.encode());
            let split = match result.split {
                Split::None | Split::Unchanged => Vec::new(),
                Split::One((separator, right)) => vec![branch_cell(store, &separator, right)?],
                Split::Many(many) => many
                    .iter()
                    .map(|(separator, right)| branch_cell(store, separator, *right))
                    .collect::<Result<Vec<_>>>()?,
            };
            if !split.is_empty() {
                splits.push((index, split));
            }
        }
        start = end;
    }
    let Some(bytes) = bytes else {
        return Ok(Change {
            root,
            split: Split::Unchanged,
        });
    };
    if splits.is_empty() {
        return Ok(Change {
            root: store.rewrite(root, &bytes, store.page_size())?,
            split: Split::None,
        });
    }
    let page = Page::decode(&bytes)?;
    let canonical = page
        .compact()
        .map(|_| crate::compact::Canonical::new(&page))
        .transpose()?;
    let old = if let Some(canonical) = &canonical {
        canonical.slices()
    } else {
        (0..page.count)
            .map(|n| page.raw_cell(n))
            .collect::<Result<Vec<_>>>()?
    };
    let mut cells = old.iter().map(|raw| raw.to_vec()).collect::<Vec<_>>();
    for (index, split) in splits.into_iter().rev() {
        cells.splice(
            index..index,
            split.iter().map(|cell| cell.as_bytes().to_vec()),
        );
    }
    let cells = cells.iter().map(Vec::as_slice).collect::<Vec<_>>();
    persist_batch(store, root, page.level, page.first, &cells)
}

fn persist_batch<S: WriteStore>(
    store: &mut S,
    old: Reference,
    level: u16,
    first: Reference,
    cells: &[&[u8]],
) -> Result<Change> {
    // Ordinary mutation can grow by one cell; a batch can overflow many pages.
    // Its existing V1 normalization handles that case without assuming two
    // compact siblings can contain the complete merged input.
    #[cfg(feature = "compact-pages")]
    if store.page_size() == PAGE_SIZE
        && crate::compact::uniform(level, cells).is_some_and(|layout| {
            cells.len() > 2 * layout.capacity(level == 0) + usize::from(level != 0)
        })
    {
        return persist_many(store, old, level, first, cells);
    }
    persist(store, old, level, first, cells, false)
}
