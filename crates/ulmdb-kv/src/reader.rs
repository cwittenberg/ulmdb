use crate::{page::Page, read::get_pair_from_page};
use ulmdb_core::storage::{ReadStore, Reference, Result};

/// Bind a checked root once for a batch of allocation-free point reads.
///
/// The root header and extent are validated at construction. Cached slices borrow
/// the store for `'a`; the store must retain its immutable bytes for that lifetime.
/// Every descended page, searched key, overflow payload and returned cell still
/// passes the ordinary checked decoder. No mutable page cache or global state is
/// involved. A leaf root needs no store page load during subsequent queries.
pub struct Reader<'a, S: ReadStore> {
    store: &'a S,
    root: Option<Page<'a>>,
}
impl<'a, S: ReadStore> Reader<'a, S> {
    #[inline]
    pub fn new(store: &'a S, root: Reference) -> Result<Self> {
        Ok(Self {
            store,
            root: if root.is_null() {
                None
            } else {
                Some(Page::load(store, root)?)
            },
        })
    }

    /// Borrow the exact value independently of the search key's lifetime.
    #[inline]
    pub fn get(&self, key: &[u8]) -> Result<Option<&'a [u8]>> {
        Ok(self.get_pair(key)?.map(|(_, value)| value))
    }

    /// Borrow the stored exact key and value from the pinned store.
    #[inline]
    pub fn get_pair(&self, key: &[u8]) -> Result<Option<(&'a [u8], &'a [u8])>> {
        match self.root.as_ref() {
            None => Ok(None),
            Some(page) => get_pair_from_page(self.store, page, key),
        }
    }

    /// Cached page height; does not resolve the store again.
    pub fn height(&self) -> u16 {
        self.root.map_or(0, |page| page.level + 1)
    }
}

#[cfg(test)]
mod tests;
