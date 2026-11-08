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
    search_from(store, node.as_ref(), address, limit, exact)
}

pub(crate) fn search_from<'a, S: ReadStore + ?Sized>(
    store: &'a S,
    root: Option<&Node>,
    address: Address,
    limit: u8,
    exact: bool,
) -> Result<Option<(Prefix, &'a [u8])>> {
    let Some(root) = root else {
        return Ok(None);
    };
    // The cached root remains borrowed for a miss. Copy traversal state only
    // after its compressed prefix matches; selected descendants keep all their
    // ordinary decoder and parent/child checks.
    if root.prefix.length() > limit || !root.prefix.contains_address(address) {
        return Ok(None);
    }
    let mut node = *root;
    let mut best = None;
    loop {
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
        // Check all skipped bits after decoding/validating the selected child,
        // preserving both the store-read sequence and corruption precedence.
        if child.prefix.length() > limit || !child.prefix.contains_address(address) {
            break;
        }
        node = child;
    }
    best.map(|(prefix, value)| store.payload(value).map(|bytes| (prefix, bytes)))
        .transpose()
}
