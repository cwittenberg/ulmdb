//! Delete-only structural edits: one page per changed ancestor, no sibling walk.
use super::*;

pub(super) fn empty_child<S: WriteStore>(
    store: &mut S,
    root: Reference,
    level: u16,
    index: usize,
) -> Result<Change> {
    let mut bytes = crate::page_buffer::PageBuffer::new(store.page_size());
    bytes.copy_from_slice(Page::load(store, root)?.bytes);
    let page = Page::decode(&bytes)?;
    if page.level != level || level == 0 {
        return Err(Error::Corrupt("pruned parent level mismatch"));
    }
    page.child(store, index)?;
    if page.compact().is_some() {
        page.validate_compact()?;
    }
    let canonical = page
        .compact()
        .map(|_| crate::compact::Canonical::new(&page))
        .transpose()?;
    // Check every raw cell before discarding a separator, including the one
    // removed. As in ordinary reconstruction, overflow payloads stay encoded.
    let mut cells = if let Some(canonical) = &canonical {
        canonical.slices()
    } else {
        (0..page.count)
            .map(|i| page.raw_cell(i))
            .collect::<Result<Vec<_>>>()?
    };
    if cells.is_empty() {
        return Ok(Change {
            root: Reference::NULL,
            split: Split::None,
        });
    }
    let first = if index == 0 {
        // The first separator bounds child 1 inclusively. After removing child
        // 0, child 1 inherits the parent's (weaker) lower bound, so that first
        // separator is removed rather than attached to the next child.
        page.child(store, 1)?
    } else {
        page.first
    };
    cells.remove(index.saturating_sub(1));
    // Keep a unary nonroot branch at the same level. Collapsing it here would
    // make it shallower than its siblings and break exact child-level checks.
    persist(store, root, level, first, &cells, false)
}

pub(super) fn collapse_root<S: WriteStore>(store: &mut S, mut root: Reference) -> Result<Reference> {
    let mut expected = None;
    for _ in 0..MAX_DEPTH {
        if root.is_null() {
            return Ok(root);
        }
        let page = Page::load(store, root)?;
        if expected.is_some_and(|level| level != page.level) {
            return Err(Error::Corrupt("collapsed root level mismatch"));
        }
        if page.count == 0 && page.compact().is_some() {
            page.validate_compact()?;
        }
        if page.level == 0 {
            if page.count == 0 {
                store.retire_page_edge(root)?;
                return Ok(Reference::NULL);
            }
            return Ok(root);
        }
        if page.count != 0 {
            return Ok(root);
        }
        expected = Some(page.level - 1);
        let child = page.child(store, 0)?;
        store.retire_page_edge(root)?;
        root = child;
    }
    Err(Error::Corrupt("collapsed root depth exceeded"))
}
