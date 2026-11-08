//! Streaming construction from canonical prefixes in unsigned network/length order.

use crate::node::{Node, branch, check_child, common, same_family, width};
use ulmdb_core::{
    Prefix,
    storage::{Error, Reference, Result, WriteStore},
};

/// A completed compressed radix tree and its construction counts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BulkBuild {
    pub root: Reference,
    pub records: u64,
    pub nodes: u64,
    /// Maximum open ancestor count, bounded by 33 for IPv4 and 129 for IPv6.
    pub max_frontier: usize,
}

/// Build a compressed binary radix tree without rewriting an existing object.
///
/// Input must contain one address family, sorted by unsigned network address and
/// then prefix length, with no duplicate prefix. Values may be empty and are
/// limited to 64 MiB. Nodes are written in postorder; at most address-width + 1
/// open ancestors are retained.
/// A late iterator/store error may leave private allocations: the caller must
/// discard or roll back its transaction/store before reusing that space.
pub fn build_sorted<S, I, V>(store: &mut S, entries: I) -> Result<BulkBuild>
where
    S: WriteStore,
    I: IntoIterator<Item = Result<(Prefix, V)>>,
    V: AsRef<[u8]>,
{
    let mut report = BulkBuild {
        root: Reference::NULL,
        records: 0,
        nodes: 0,
        max_frontier: 0,
    };
    let mut stack: Vec<Node> = Vec::new();
    let mut previous: Option<Prefix> = None;
    for entry in entries {
        let (prefix, value) = entry?;
        let value = value.as_ref();
        if value.len() > store.max_value_size() {
            return Err(Error::LimitExceeded);
        }
        let records = report.records.checked_add(1).ok_or(Error::LimitExceeded)?;
        if let Some(prior) = previous {
            if !same_family(prior.network(), prefix.network()) {
                return Err(Error::InvalidInput("mixed radix address families"));
            }
            match (prior.network(), prior.length()).cmp(&(prefix.network(), prefix.length())) {
                std::cmp::Ordering::Equal => {
                    return Err(Error::InvalidInput("duplicate sorted radix prefix"));
                }
                std::cmp::Ordering::Greater => {
                    return Err(Error::InvalidInput("unsorted radix prefixes"));
                }
                std::cmp::Ordering::Less => {}
            }
            let shared = common(prior, prefix);
            let mut closed = None;
            while stack
                .last()
                .is_some_and(|node| node.prefix.length() > shared)
            {
                let mut node = stack.pop().unwrap();
                if let Some(child) = closed.take() {
                    attach(&mut node, child)?;
                }
                closed = Some(seal(store, node, &mut report.nodes)?);
            }
            if let Some(child) = closed {
                if stack
                    .last()
                    .is_some_and(|node| node.prefix.length() == shared)
                {
                    attach(stack.last_mut().unwrap(), child)?;
                } else {
                    let parent = Prefix::normalize(prefix.network(), shared)
                        .map_err(|_| Error::Corrupt("radix builder common prefix"))?;
                    let mut node = Node::leaf(parent, Reference::NULL);
                    attach(&mut node, child)?;
                    stack.push(node);
                }
            }
        } else {
            stack.reserve_exact(usize::from(width(prefix.network())) + 1);
        }
        let payload = store.put_payload(value)?;
        valid_reference(payload)?;
        stack.push(Node::leaf(prefix, payload));
        if stack.len() > usize::from(width(prefix.network())) + 1 {
            return Err(Error::Corrupt("radix builder frontier depth"));
        }
        report.max_frontier = report.max_frontier.max(stack.len());
        report.records = records;
        previous = Some(prefix);
    }
    let mut closed = None;
    while let Some(mut node) = stack.pop() {
        if let Some(child) = closed.take() {
            attach(&mut node, child)?;
        }
        closed = Some(seal(store, node, &mut report.nodes)?);
    }
    if let Some((_, reference)) = closed {
        report.root = reference;
    }
    Ok(report)
}

fn valid_reference(reference: Reference) -> Result<()> {
    if reference.is_null() || reference.segment == 0 || !reference.offset.is_multiple_of(16) {
        return Err(Error::Corrupt("radix builder allocation reference"));
    }
    Ok(())
}

fn attach(parent: &mut Node, child: (Prefix, Reference)) -> Result<()> {
    let (prefix, reference) = child;
    if !same_family(parent.prefix.network(), prefix.network())
        || parent.prefix.length() >= width(prefix.network())
    {
        return Err(Error::Corrupt("radix builder child family or depth"));
    }
    let side = branch(prefix.network(), parent.prefix.length());
    check_child(parent.prefix, prefix, side)?;
    if !parent.children[side].is_null() {
        return Err(Error::Corrupt("radix builder occupied child"));
    }
    parent.children[side] = reference;
    Ok(())
}

fn seal<S: WriteStore>(store: &mut S, node: Node, nodes: &mut u64) -> Result<(Prefix, Reference)> {
    if node.value.is_null() && node.children.iter().any(|child| child.is_null()) {
        return Err(Error::Corrupt("uncompressed radix builder branch"));
    }
    let count = nodes.checked_add(1).ok_or(Error::LimitExceeded)?;
    let reference = store.allocate(&node.encode(), 16)?;
    valid_reference(reference)?;
    *nodes = count;
    Ok((node.prefix, reference))
}
