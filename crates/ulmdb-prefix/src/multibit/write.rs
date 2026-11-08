use super::{
    Budget, Visits,
    wire::{self, Node},
};
use ulmdb_core::{
    Prefix,
    storage::{Error, Reference, Result, WriteStore},
};

fn leaf<S: WriteStore + ?Sized>(store: &mut S, prefix: Prefix, value: &[u8]) -> Result<Reference> {
    let mut node = Node::empty(wire::aligned_prefix(prefix));
    node.values[wire::slot(prefix)] = store.put_payload(value)?;
    node.persist(store, None)
}
/// Incremental path copy; only equal-size objects use the store's private rewrite.
pub fn insert<S: WriteStore + ?Sized>(
    store: &mut S,
    root: Reference,
    prefix: Prefix,
    value: &[u8],
) -> Result<Reference> {
    insert_with_budget(store, root, prefix, value, Budget::default())
}
pub fn insert_with_budget<S: WriteStore + ?Sized>(
    store: &mut S,
    root: Reference,
    prefix: Prefix,
    value: &[u8],
    budget: Budget,
) -> Result<Reference> {
    if root.is_null() {
        return leaf(store, prefix, value);
    }
    let mut visits = Visits::new(budget);
    let node = Node::read(store, root, &mut visits)?;
    if !wire::family(node.base.network(), prefix.network()) {
        return Err(Error::InvalidInput("multibit root family mismatch"));
    }
    put(store, root, node, prefix, value, &mut visits)
}
fn put<S: WriteStore + ?Sized>(
    store: &mut S,
    at: Reference,
    mut node: Node,
    prefix: Prefix,
    value: &[u8],
    visits: &mut Visits,
) -> Result<Reference> {
    let target = wire::aligned_prefix(prefix);
    let old_len = node.len();
    if node.base == target {
        let i = wire::slot(prefix);
        let old = node.values[i];
        if !old.is_null() && store.payload(old)? == value {
            return Ok(at);
        }
        node.values[i] = if old.is_null() {
            store.put_payload(value)?
        } else {
            store.replace_payload(old, value)?
        };
        return node.persist(store, Some((at, old_len)));
    }
    if node.base.contains_prefix(target) {
        let side = wire::nibble(prefix.network(), node.base.length());
        let old = node.children[side];
        let new = if let Some((child, decoded)) = node.child(store, side, visits)? {
            put(store, child, decoded, prefix, value, visits)?
        } else {
            leaf(store, prefix, value)?
        };
        if old == new {
            return Ok(at);
        }
        node.children[side] = new;
        return node.persist(store, Some((at, old_len)));
    }
    let depth = wire::common(node.base, target) / 4 * 4;
    let mut parent = Node::empty(Prefix::normalize(prefix.network(), depth).unwrap());
    parent.children[wire::nibble(node.base.network(), depth)] = at;
    if parent.base == target {
        parent.values[wire::slot(prefix)] = store.put_payload(value)?;
    } else {
        parent.children[wire::nibble(prefix.network(), depth)] = leaf(store, prefix, value)?;
    }
    parent.persist(store, None)
}
pub fn delete<S: WriteStore + ?Sized>(
    store: &mut S,
    root: Reference,
    prefix: Prefix,
) -> Result<Reference> {
    delete_with_budget(store, root, prefix, Budget::default())
}
pub fn delete_with_budget<S: WriteStore + ?Sized>(
    store: &mut S,
    root: Reference,
    prefix: Prefix,
    budget: Budget,
) -> Result<Reference> {
    if root.is_null() {
        return Ok(root);
    }
    let mut visits = Visits::new(budget);
    let node = Node::read(store, root, &mut visits)?;
    if !wire::family(node.base.network(), prefix.network()) {
        return Err(Error::InvalidInput("multibit root family mismatch"));
    }
    remove(store, root, node, prefix, &mut visits)
}
fn remove<S: WriteStore + ?Sized>(
    store: &mut S,
    at: Reference,
    mut node: Node,
    prefix: Prefix,
    visits: &mut Visits,
) -> Result<Reference> {
    let target = wire::aligned_prefix(prefix);
    let old_len = node.len();
    if node.base == target {
        let index = wire::slot(prefix);
        if node.values[index].is_null() {
            return Ok(at);
        }
        node.values[index] = Reference::NULL;
    } else if node.base.contains_prefix(target) {
        let side = wire::nibble(prefix.network(), node.base.length());
        let Some((child, decoded)) = node.child(store, side, visits)? else {
            return Ok(at);
        };
        let next = remove(store, child, decoded, prefix, visits)?;
        if next == child {
            return Ok(at);
        }
        node.children[side] = next;
    } else {
        return Ok(at);
    }
    let (v, c) = node.masks();
    if v == 0 {
        if c == 0 {
            return Ok(Reference::NULL);
        }
        if c.count_ones() == 1 {
            // Promotion follows this edge even when the removed value was in
            // the parent. Validate the surviving node's family/depth/route.
            return node
                .child(store, c.trailing_zeros() as usize, visits)?
                .map(|(at, _)| at)
                .ok_or(Error::Corrupt("missing multibit promoted child"));
        }
    }
    node.persist(store, Some((at, old_len)))
}
