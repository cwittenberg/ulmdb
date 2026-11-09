//! Sorted updates preserve untouched radix subtrees and share ancestor work.
use crate::node::{Node, branch, common, same_family};
use ulmdb_core::{
    Prefix,
    storage::{Error, Reference, Result, WriteStore},
};

const CHUNK_RECORDS: usize = 256;
const CHUNK_BYTES: usize = 1024 * 1024;

/// Resulting root and accepted input count, including unchanged values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchUpsert {
    pub root: Reference,
    pub records: u64,
}

/// Insert or replace sorted exact prefixes in an existing compressed tree.
/// Input must use one family and increase strictly by network address/length.
/// Each chunk shares ancestor rewrites and reuses untouched subtrees without
/// scanning them. Buffers hold at most 256 input items and one MiB plus one
/// maximum-size value. A late error requires store rollback or discard.
pub fn upsert_sorted<S, I, V>(store: &mut S, mut root: Reference, entries: I) -> Result<BatchUpsert>
where
    S: WriteStore,
    I: IntoIterator<Item = Result<(Prefix, V)>>,
    V: AsRef<[u8]>,
{
    let mut entries = entries.into_iter();
    let mut previous: Option<Prefix> = None;
    let mut records = 0u64;
    loop {
        let mut chunk = Vec::with_capacity(CHUNK_RECORDS);
        let mut bytes = 0usize;
        while chunk.len() < CHUNK_RECORDS && bytes < CHUNK_BYTES {
            let Some(entry) = entries.next() else { break };
            let (prefix, value) = entry?;
            if value.as_ref().len() > store.max_value_size() {
                return Err(Error::InvalidInput(
                    "sorted prefix upsert value limit exceeded",
                ));
            }
            if let Some(prior) = previous {
                if !same_family(prior.network(), prefix.network()) {
                    return Err(Error::InvalidInput("mixed sorted prefix upsert families"));
                }
                if (prior.network(), prior.length()) >= (prefix.network(), prefix.length()) {
                    return Err(Error::InvalidInput(
                        "sorted upsert prefixes must increase strictly",
                    ));
                }
            }
            previous = Some(prefix);
            bytes = bytes
                .checked_add(value.as_ref().len())
                .and_then(|n| n.checked_add(32))
                .ok_or(Error::LimitExceeded)?;
            chunk.push((prefix, value));
        }
        if chunk.is_empty() {
            break;
        }
        records = records
            .checked_add(chunk.len() as u64)
            .ok_or(Error::LimitExceeded)?;
        root = change_chunk(store, root, &chunk)?;
    }
    Ok(BatchUpsert { root, records })
}

fn change_chunk<S: WriteStore, V: AsRef<[u8]>>(
    store: &mut S,
    root: Reference,
    entries: &[(Prefix, V)],
) -> Result<Reference> {
    if entries.is_empty() {
        return Ok(root);
    }
    if entries.len() == 1 {
        return crate::insert(store, root, entries[0].0, entries[0].1.as_ref());
    }
    if root.is_null() {
        return Ok(crate::build_sorted(
            store,
            entries
                .iter()
                .map(|(prefix, value)| Ok((*prefix, value.as_ref()))),
        )?
        .root);
    }
    let old = Node::read(store, root)?;
    let first = entries.first().unwrap().0;
    let last = entries.last().unwrap().0;
    if !same_family(old.prefix.network(), first.network()) {
        return Err(Error::InvalidInput("prefix root address family mismatch"));
    }
    let shared = common(old.prefix, first).min(common(first, last));
    let existing = shared == old.prefix.length();
    let mut node = if existing {
        old
    } else {
        let prefix = Prefix::normalize(first.network(), shared)
            .map_err(|_| Error::Corrupt("batch radix split depth"))?;
        let mut node = Node::leaf(prefix, Reference::NULL);
        node.children[branch(old.prefix.network(), shared)] = root;
        node
    };
    let mut start = 0;
    if first == node.prefix {
        node.value = store.replace_payload(node.value, entries[0].1.as_ref())?;
        start = 1;
    }
    // At most address width + 1 recursive frames. Strict prefix ordering puts
    // the optional ancestor first and all left descendants before the right.
    if start < entries.len() {
        let middle = start
            + entries[start..].partition_point(|(prefix, _)| branch(prefix.network(), shared) == 0);
        for (side, range) in [(0, start..middle), (1, middle..entries.len())] {
            if range.is_empty() {
                continue;
            }
            node.child(store, side)?;
            node.children[side] = change_chunk(store, node.children[side], &entries[range])?;
        }
    }
    if existing && node.children == old.children && node.value == old.value {
        Ok(root)
    } else if existing {
        store.rewrite(root, &node.encode(), 16)
    } else {
        store.allocate(&node.encode(), 16)
    }
}
