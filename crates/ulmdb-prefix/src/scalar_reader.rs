//! Same public prepared-reader API; traversal dispatches to one scalar family.
use crate::scalar_lookup::{Root, read_root, search_from};
use ulmdb_core::{
    Address, Prefix,
    storage::{ReadStore, Reference, Result},
};
pub struct Reader<'a, S: ReadStore + ?Sized> {
    store: &'a S,
    root: Root,
}
impl<'a, S: ReadStore + ?Sized> Reader<'a, S> {
    pub fn new(store: &'a S, root: Reference) -> Result<Self> {
        Ok(Self {
            store,
            root: read_root(store, root)?,
        })
    }
    pub fn root_prefix(&self) -> Option<Prefix> {
        self.root.prefix()
    }
    pub fn longest_match(&self, address: Address) -> Result<Option<(Prefix, &'a [u8])>> {
        let limit = match address {
            Address::V4(_) => 32,
            Address::V6(_) => 128,
        };
        search_from(self.store, &self.root, address, limit, false)
    }
    pub fn exact(&self, prefix: Prefix) -> Result<Option<(Prefix, &'a [u8])>> {
        search_from(
            self.store,
            &self.root,
            prefix.network(),
            prefix.length(),
            true,
        )
    }
    pub fn covering(&self, prefix: Prefix) -> Result<Option<(Prefix, &'a [u8])>> {
        search_from(
            self.store,
            &self.root,
            prefix.network(),
            prefix.length(),
            false,
        )
    }
}
