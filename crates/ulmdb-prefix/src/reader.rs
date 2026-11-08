//! Decode the immutable root once per prepared snapshot view. Descendants keep
//! their ordinary checked decoder and parent/child correspondence validation.
use crate::{lookup::search_from, node::Node};
use ulmdb_core::{
    Address, Prefix,
    storage::{ReadStore, Reference, Result},
};

pub struct Reader<'a, S: ReadStore + ?Sized> {
    store: &'a S,
    root: Option<Node>,
}
impl<'a, S: ReadStore + ?Sized> Reader<'a, S> {
    pub fn new(store: &'a S, root: Reference) -> Result<Self> {
        let root = if root.is_null() {
            None
        } else {
            Some(Node::read(store, root)?)
        };
        Ok(Self { store, root })
    }
    pub fn root_prefix(&self) -> Option<Prefix> {
        self.root.map(|node| node.prefix)
    }
    pub fn longest_match(&self, address: Address) -> Result<Option<(Prefix, &'a [u8])>> {
        let limit = match address {
            Address::V4(_) => 32,
            Address::V6(_) => 128,
        };
        search_from(self.store, self.root.as_ref(), address, limit, false)
    }
    pub fn exact(&self, prefix: Prefix) -> Result<Option<(Prefix, &'a [u8])>> {
        search_from(
            self.store,
            self.root.as_ref(),
            prefix.network(),
            prefix.length(),
            true,
        )
    }
    pub fn covering(&self, prefix: Prefix) -> Result<Option<(Prefix, &'a [u8])>> {
        search_from(
            self.store,
            self.root.as_ref(),
            prefix.network(),
            prefix.length(),
            false,
        )
    }
}

#[cfg(test)]
#[path = "root_borrow/tests.rs"]
mod root_borrow_tests;
