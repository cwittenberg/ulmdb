//! Exact reserved metadata, carried by the actual catalog root through copying.
use crate::{CatalogCapacity, Result};
use ulmdb_core::storage::{ReadStore, Reference};
pub(crate) fn read<S: ReadStore>(store: &S, root: Reference) -> Result<CatalogCapacity> {
    ulmdb_kv::get(store, root, b"")?
        .map(CatalogCapacity::decode_marker)
        .transpose()
        .map(|p| p.unwrap_or_default())
}
/// Scalar discovery on cached file tables owns a fresh short read scope. It
/// returns no borrow/pin authority and cannot populate the serving scope.
pub(crate) fn read_file(
    store: &ulmdb_storage::SegmentTable,
    root: Reference,
) -> Result<CatalogCapacity> {
    if store.is_chunked() {
        let scope = store.fresh_read_scope()?;
        read(&scope, root)
    } else {
        read(store, root)
    }
}
impl crate::ReadTxn {
    pub fn catalog_capacity(&self) -> Result<CatalogCapacity> {
        let manifest = self.file_manifest()?;
        read_file(&self.snapshot.segments, manifest.catalog)
    }
}
impl crate::WriteTxn<'_> {
    pub fn catalog_capacity(&self) -> CatalogCapacity {
        self.catalog_capacity
    }
    pub fn set_catalog_capacity(&mut self, capacity: CatalogCapacity) -> Result<()> {
        self.live()?;
        self.env.require_file_backend()?;
        if self.catalog.len() > capacity.get() {
            return Err(crate::Error::LimitExceeded);
        }
        if self.catalog_capacity != capacity {
            self.catalog_capacity = capacity;
            self.mark_changed(b"");
        }
        Ok(())
    }
}

/// Charge discovery against the same cold/incremental graph read allowance.
pub(crate) fn read_bounded<S: ReadStore>(
    store: &S,
    root: Reference,
    budget: u64,
) -> Result<(CatalogCapacity, u64)> {
    struct Checked<'a, S> {
        inner: &'a S,
        left: std::cell::Cell<u64>,
    }
    impl<S: ReadStore> ReadStore for Checked<'_, S> {
        fn page_size(&self) -> usize {
            self.inner.page_size()
        }
        fn max_key_size(&self) -> usize {
            self.inner.max_key_size()
        }
        fn max_value_size(&self) -> usize {
            self.inner.max_value_size()
        }
        fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
            self.left.set(
                self.left
                    .get()
                    .checked_sub(1)
                    .ok_or(crate::Error::LimitExceeded)?,
            );
            self.inner.read(at, len)
        }
    }
    let checked = Checked {
        inner: store,
        left: std::cell::Cell::new(budget),
    };
    let capacity = read(&checked, root)?;
    Ok((capacity, budget - checked.left.get()))
}
