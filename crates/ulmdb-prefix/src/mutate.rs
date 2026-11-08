use crate::node::{Node, branch, common, same_family};
use ulmdb_core::{
    Prefix,
    storage::{Error, Reference, Result, WriteStore},
};

/// Insert or replace an exact prefix. The caller publishes the returned root.
pub fn insert<S: WriteStore + ?Sized>(
    store: &mut S,
    root: Reference,
    prefix: Prefix,
    value: &[u8],
) -> Result<Reference> {
    if root.is_null() {
        return new_leaf(store, prefix, value);
    }
    let node = Node::read(store, root)?;
    if !same_family(node.prefix.network(), prefix.network()) {
        return Err(Error::InvalidInput("prefix root address family mismatch"));
    }
    insert_node(store, root, node, prefix, value)
}

fn new_leaf<S: WriteStore + ?Sized>(
    store: &mut S,
    prefix: Prefix,
    value: &[u8],
) -> Result<Reference> {
    let payload = store.put_payload(value)?;
    store.allocate(&Node::leaf(prefix, payload).encode(), 16)
}

fn insert_node<S: WriteStore + ?Sized>(
    store: &mut S,
    root: Reference,
    mut node: Node,
    prefix: Prefix,
    value: &[u8],
) -> Result<Reference> {
    let shared = common(node.prefix, prefix);
    if shared < node.prefix.length() {
        let parent_prefix = Prefix::normalize(prefix.network(), shared)
            .map_err(|_| Error::Corrupt("invalid radix split depth"))?;
        let mut parent = Node::leaf(parent_prefix, Reference::NULL);
        parent.children[branch(node.prefix.network(), shared)] = root;
        if shared == prefix.length() {
            parent.value = store.put_payload(value)?;
        } else {
            parent.children[branch(prefix.network(), shared)] = new_leaf(store, prefix, value)?;
        }
        return store.allocate(&parent.encode(), 16);
    }
    if node.prefix == prefix {
        let payload = store.replace_payload(node.value, value)?;
        if payload == node.value {
            // The writer changed only its private payload bytes. The encoded
            // node and all ancestor references remain exactly the same.
            return Ok(root);
        }
        node.value = payload;
    } else {
        let side = branch(prefix.network(), node.prefix.length());
        let next = match node.child(store, side)? {
            Some(child) => insert_node(store, node.children[side], child, prefix, value)?,
            None => new_leaf(store, prefix, value)?,
        };
        if next == node.children[side] {
            return Ok(root);
        }
        node.children[side] = next;
    }
    store.rewrite(root, &node.encode(), 16)
}

/// Delete only the exact value, preserving descendants and inherited fallback.
pub fn delete<S: WriteStore + ?Sized>(
    store: &mut S,
    root: Reference,
    prefix: Prefix,
) -> Result<Reference> {
    if root.is_null() {
        return Ok(root);
    }
    let mut node = Node::read(store, root)?;
    if !node.prefix.contains_prefix(prefix) {
        return Ok(root);
    }
    if node.prefix == prefix {
        if node.value.is_null() {
            return Ok(root);
        }
        node.value = Reference::NULL;
    } else {
        let side = branch(prefix.network(), node.prefix.length());
        node.child(store, side)?;
        let next = delete(store, node.children[side], prefix)?;
        if next == node.children[side] {
            // A private rewrite can return the same reference. The parent bytes
            // already refer to it; compression below still examines its topology.
            if !node.value.is_null() || node.children.iter().filter(|r| !r.is_null()).count() != 1 {
                return Ok(root);
            }
        }
        node.children[side] = next;
    }
    if node.value.is_null() {
        match (node.children[0].is_null(), node.children[1].is_null()) {
            (true, true) => return Ok(Reference::NULL),
            (false, true) => return Ok(node.children[0]),
            (true, false) => return Ok(node.children[1]),
            (false, false) => {}
        }
    }
    store.rewrite(root, &node.encode(), 16)
}
