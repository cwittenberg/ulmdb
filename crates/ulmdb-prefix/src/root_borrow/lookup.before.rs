use crate::node::{Node, branch};
use ulmdb_core::{
    Address, Prefix,
    storage::{ReadStore, Reference, Result},
};

/// Longest stored prefix containing an address. Returned bytes borrow the store.
pub fn get<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    address: Address,
) -> Result<Option<(Prefix, &[u8])>> {
    longest_match(store, root, address)
}

pub fn longest_match<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    address: Address,
) -> Result<Option<(Prefix, &[u8])>> {
    let limit = match address {
        Address::V4(_) => 32,
        Address::V6(_) => 128,
    };
    search(store, root, address, limit, false)
}

/// Greatest-length stored prefix covering the entire query CIDR.
pub fn covering<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    prefix: Prefix,
) -> Result<Option<(Prefix, &[u8])>> {
    search(store, root, prefix.network(), prefix.length(), false)
}

pub fn exact<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    prefix: Prefix,
) -> Result<Option<(Prefix, &[u8])>> {
    search(store, root, prefix.network(), prefix.length(), true)
}

fn search<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    address: Address,
    limit: u8,
    exact: bool,
) -> Result<Option<(Prefix, &[u8])>> {
    let node = if root.is_null() {
        None
    } else {
        Some(Node::read(store, root)?)
    };
    search_from(store, node, address, limit, exact)
}

pub(crate) fn search_from<S: ReadStore + ?Sized>(
    store: &S,
    root: Option<Node>,
    address: Address,
    limit: u8,
    exact: bool,
) -> Result<Option<(Prefix, &[u8])>> {
    let Some(mut node) = root else {
        return Ok(None);
    };
    let mut best = None;
    loop {
        // This comparison checks every bit on compressed/skipped paths.
        if node.prefix.length() > limit || !node.prefix.contains_address(address) {
            break;
        }
        if !node.value.is_null() && (!exact || node.prefix.length() == limit) {
            best = Some((node.prefix, node.value));
        }
        if node.prefix.length() == limit {
            break;
        }
        let side = branch(address, node.prefix.length());
        let Some(child) = node.child(store, side)? else {
            break;
        };
        node = child;
    }
    best.map(|(prefix, value)| store.payload(value).map(|bytes| (prefix, bytes)))
        .transpose()
}
