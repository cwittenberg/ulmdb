use super::{Builder, BulkBuild};
use crate::ordering::{Bytewise, Comparator};
use ulmdb_core::storage::{Error, Result, WriteStore};

/// Incremental direct page construction without retaining a mutable store borrow.
///
/// Use the same logical store/address space for every push and finish; rollover
/// stores may change segment IDs while preserving earlier references. Existing
/// objects are never rewritten. Any error poisons the builder permanently and
/// may leave private allocations, so discard or roll back the supplied store.
pub struct SortedBuilder<C: Comparator = Bytewise> {
    order: C,
    pipeline: Option<Builder>,
    previous: [u8; 4096],
    large_previous: Option<Vec<u8>>,
    length: usize,
    present: bool,
    duplicate_values: bool,
}
impl Default for SortedBuilder {
    fn default() -> Self {
        Self::new()
    }
}
impl SortedBuilder {
    /// Ordinary KV limits: keys up to 4096 bytes and values up to 64 MiB.
    pub fn new() -> Self {
        Self::mode(false)
    }
    /// Duplicate-child mode: keys up to 65536 bytes and empty values only.
    /// The larger prior-key buffer is allocated only when first needed.
    pub fn duplicate_values() -> Self {
        Self::mode(true)
    }
    fn mode(duplicate_values: bool) -> Self {
        Self::mode_with_order(duplicate_values, Bytewise)
    }
}
impl<C: Comparator> SortedBuilder<C> {
    pub fn with_order(order: C) -> Self {
        Self::mode_with_order(false, order)
    }
    pub fn duplicate_values_with_order(order: C) -> Self {
        Self::mode_with_order(true, order)
    }
    fn mode_with_order(duplicate_values: bool, order: C) -> Self {
        Self {
            order,
            pipeline: Some(Builder::new()),
            previous: [0; 4096],
            large_previous: None,
            length: 0,
            present: false,
            duplicate_values,
        }
    }
    fn prior(&self) -> &[u8] {
        &self.large_previous.as_deref().unwrap_or(&self.previous)[..self.length]
    }
    /// Append one strictly increasing raw key. Any error forbids returning a root
    /// even if a caller ignores that error and later attempts push or finish.
    pub fn push<S: WriteStore>(&mut self, store: &mut S, key: &[u8], value: &[u8]) -> Result<()> {
        let result = self.push_inner(store, key, value);
        if result.is_err() {
            self.pipeline = None;
        }
        result
    }
    fn push_inner<S: WriteStore>(&mut self, store: &mut S, key: &[u8], value: &[u8]) -> Result<()> {
        if self.pipeline.is_none() {
            return Err(Error::InvalidInput("sorted builder is poisoned"));
        }
        let max_key = if self.duplicate_values {
            65_536
        } else {
            store.max_key_size()
        };
        if key.len() > max_key
            || value.len() > store.max_value_size()
            || (self.duplicate_values && !value.is_empty())
        {
            return Err(Error::InvalidInput("bulk key/value exceeds engine limits"));
        }
        self.order.validate(key)?;
        if self.present {
            match self.order.compare(self.prior(), key) {
                std::cmp::Ordering::Equal => return Err(Error::InvalidInput("duplicate bulk key")),
                std::cmp::Ordering::Greater => {
                    return Err(Error::InvalidInput("bulk keys are not sorted"));
                }
                std::cmp::Ordering::Less => {}
            }
        }
        self.pipeline.as_mut().unwrap().record(store, key, value)?;
        if key.len() > self.previous.len() && self.large_previous.is_none() {
            self.large_previous = Some(vec![0; 65_536]);
        }
        let previous = self
            .large_previous
            .as_deref_mut()
            .unwrap_or(&mut self.previous);
        previous[..key.len()].copy_from_slice(key);
        self.length = key.len();
        self.present = true;
        Ok(())
    }
    /// Complete the tree, consuming this builder. A poisoned builder cannot emit
    /// any root, including a root for records accepted before its first error.
    pub fn finish<S: WriteStore>(mut self, store: &mut S) -> Result<BulkBuild> {
        self.pipeline
            .take()
            .ok_or(Error::InvalidInput("sorted builder is poisoned"))?
            .finish(store)
    }
}
