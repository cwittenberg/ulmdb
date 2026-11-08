use crate::page::Page;
use ulmdb_core::storage::{Error, ReadStore, Reference, Result};

/// Allocation-free point lookup; returned bytes borrow the pinned store.
#[inline]
pub fn get<'a, S: ReadStore>(
    store: &'a S,
    root: Reference,
    key: &[u8],
) -> Result<Option<&'a [u8]>> {
    Ok(get_pair(store, root, key)?.map(|(_, value)| value))
}

/// Allocation-free exact lookup returning the stored key and value. Both slices
/// borrow the pinned store, independently of the caller's search-key lifetime.
#[inline]
pub fn get_pair<'a, S: ReadStore>(
    store: &'a S,
    root: Reference,
    key: &[u8],
) -> Result<Option<(&'a [u8], &'a [u8])>> {
    if root.is_null() {
        return Ok(None);
    }
    get_pair_from_page(store, &Page::load(store, root)?, key)
}

/// Traverse from a root already decoded against this same borrowed store.
#[inline]
pub(crate) fn get_pair_from_page<'a, S: ReadStore>(
    store: &'a S,
    root: &Page<'a>,
    key: &[u8],
) -> Result<Option<(&'a [u8], &'a [u8])>> {
    let mut loaded;
    let mut page = root;
    loop {
        if page.level == 0 {
            #[cfg(feature = "word-search")]
            if let Some(layout) = page.compact()
                && layout.key == 8
                && layout.value == 8
                && let Ok(needle) = <&[u8; 8]>::try_from(key)
            {
                return crate::compact::word_leaf_pair(page, u64::from_be_bytes(*needle));
            }
            let index = page.search(store, key, false)?;
            return if index == page.count {
                Ok(None)
            } else {
                let cell = page.cell(store, index)?;
                Ok((cell.key == key).then_some((cell.key, cell.value)))
            };
        }
        let child = page.child(store, page.search(store, key, true)?)?;
        let expected = page.level - 1;
        loaded = Page::load(store, child)?;
        if loaded.level != expected {
            return Err(Error::Corrupt("tree level mismatch"));
        }
        page = &loaded;
    }
}

/// Number of page levels; null roots have height zero.
pub fn height<S: ReadStore>(store: &S, root: Reference) -> Result<u16> {
    if root.is_null() {
        Ok(0)
    } else {
        Ok(Page::load(store, root)?.level + 1)
    }
}

#[cfg(test)]
#[path = "read/fused_leaf_tests.rs"]
mod fused_leaf_tests;
