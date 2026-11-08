//! Complete checked page/cell traversal for maintenance, outside hot reads.
use crate::ordering::{Bytewise, Comparator, stored};
use crate::page::{MAX_DEPTH, Page};
use ulmdb_core::storage::{Error, ReadStore, Reference, Result};

/// Counts are reference visits, not unique physical objects. Aliased empty
/// subgraphs may be visited repeatedly; every visit consumes `max_pages`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreeAudit {
    pub pages: u64,
    pub branch_pages: u64,
    pub leaf_pages: u64,
    pub leaf_records: u64,
    pub separators: u64,
    pub overflow_keys: u64,
    pub overflow_values: u64,
    pub height: u16,
}
#[derive(Clone, Copy)]
struct Frame<'a> {
    page: Page<'a>,
    next_child: usize,
    lower: Option<&'a [u8]>,
    upper: Option<&'a [u8]>,
}
fn add(counter: &mut u64, value: usize) -> Result<()> {
    *counter = counter
        .checked_add(u64::try_from(value).map_err(|_| Error::LimitExceeded)?)
        .ok_or(Error::LimitExceeded)?;
    Ok(())
}
fn payload_reference(bytes: &[u8]) -> Result<()> {
    let at = Reference::decode(bytes)?;
    if at.segment == 0 || !at.offset.is_multiple_of(16) {
        return Err(Error::Corrupt("invalid audited overflow reference"));
    }
    Ok(())
}
// Keep the original explicit traversal state and add only its leaf observer.
#[allow(clippy::too_many_arguments)]
fn inspect<'a, S: ReadStore, V: FnMut(&[u8], &[u8]), C: Comparator + ?Sized>(
    store: &'a S,
    at: Reference,
    expected: Option<u16>,
    lower: Option<&'a [u8]>,
    upper: Option<&'a [u8]>,
    max_pages: u64,
    audit: &mut TreeAudit,
    visitor: &mut V,
    order: Option<&C>,
) -> Result<Frame<'a>> {
    if audit.pages == max_pages {
        return Err(Error::LimitExceeded);
    }
    if at.segment == 0 || !at.offset.is_multiple_of(store.page_size() as u64) {
        return Err(Error::Corrupt("invalid audited page reference"));
    }
    let page = Page::load(store, at)?;
    if expected.is_some_and(|level| page.level != level) {
        return Err(Error::Corrupt("audited child level mismatch"));
    }
    // Existing local checks dereference EVERY key/value, including branch
    // overflow separators unused by any nonempty descendant or point lookup.
    if let Some(order) = order {
        crate::validate_page_with(store, at, order)?;
    } else {
        crate::validate_page_structure(store, at)?;
    }
    add(&mut audit.pages, 1)?;
    if expected.is_none() {
        audit.height = page.level + 1;
    }
    if page.level == 0 {
        add(&mut audit.leaf_pages, 1)?;
        add(&mut audit.leaf_records, page.count)?;
    } else {
        add(&mut audit.branch_pages, 1)?;
        add(&mut audit.separators, page.count)?;
    }
    for index in 0..page.count {
        let cell = page.cell(store, index)?;
        if let Some(order) = order {
            stored(order, cell.key)?;
            if lower.is_some_and(|bound| order.compare(cell.key, bound).is_lt())
                || upper.is_some_and(|bound| order.compare(cell.key, bound).is_ge())
            {
                return Err(Error::Corrupt(
                    "audited key outside ancestor separator bounds",
                ));
            }
        }
        let (flags, header, inline_key_bytes) = page.encoding(&cell)?;
        let key_bytes = if flags & 1 != 0 {
            payload_reference(&cell.raw[header..header + 16])?;
            add(&mut audit.overflow_keys, 1)?;
            16
        } else {
            inline_key_bytes
        };
        if page.level == 0 && flags & 2 != 0 {
            let start = header + key_bytes;
            payload_reference(&cell.raw[start..start + 16])?;
            add(&mut audit.overflow_values, 1)?;
        }
        if page.level == 0 {
            visitor(cell.key, cell.value);
        }
    }
    Ok(Frame {
        page,
        next_child: 0,
        lower,
        upper,
    })
}

/// Audit the full graph with constant bounded path storage and an explicit work
/// allowance. `max_pages` limits page *visits*, including repeated DAG aliases;
/// exhausted budgets return `LimitExceeded`. A null root needs zero visits.
///
/// Checks every page/cell/payload extent, local ordering and nonoverlap, strict
/// child levels, and ancestor separator bounds. Empty children remain legal.
/// No allocation or record collection is performed. The store must keep borrowed
/// bytes immutable for the call. Budgeting unique objects or authorizing file
/// deletion requires a separate caller-owned tracing/retention protocol.
pub fn audit_tree<S: ReadStore>(store: &S, root: Reference, max_pages: u64) -> Result<TreeAudit> {
    audit_tree_with_leaf_visitor(store, root, max_pages, |_, _| {})
}

/// The same complete graph audit, with a nonfallible observer for every checked
/// leaf record visit. Repeated DAG visits also repeat the observer. An observer
/// may accumulate a constraint flag but must wait for successful graph completion
/// before reporting it: the callback can run before a later graph/budget error.
/// The store must retain immutable borrowed bytes for the entire call.
pub fn audit_tree_with_leaf_visitor<S: ReadStore, V: FnMut(&[u8], &[u8])>(
    store: &S,
    root: Reference,
    max_pages: u64,
    visitor: V,
) -> Result<TreeAudit> {
    audit_impl(store, root, max_pages, visitor, Some(&Bytewise))
}

pub fn audit_tree_with<S: ReadStore, C: Comparator + ?Sized>(
    store: &S,
    root: Reference,
    max_pages: u64,
    order: &C,
) -> Result<TreeAudit> {
    audit_impl(store, root, max_pages, |_, _| {}, Some(order))
}
/// A complete graph/extent/level audit with no claim about key order or routing.
/// Explicitly separate from the existing full ordering audit contract.
pub fn audit_tree_structure<S: ReadStore>(
    store: &S,
    root: Reference,
    max_pages: u64,
) -> Result<TreeAudit> {
    audit_impl::<S, _, Bytewise>(store, root, max_pages, |_, _| {}, None)
}
fn audit_impl<S: ReadStore, V: FnMut(&[u8], &[u8]), C: Comparator + ?Sized>(
    store: &S,
    root: Reference,
    max_pages: u64,
    mut visitor: V,
    order: Option<&C>,
) -> Result<TreeAudit> {
    let mut audit = TreeAudit::default();
    if root.is_null() {
        return Ok(audit);
    }
    let mut stack: [Option<Frame<'_>>; MAX_DEPTH as usize] = [None; MAX_DEPTH as usize];
    stack[0] = Some(inspect(
        store,
        root,
        None,
        None,
        None,
        max_pages,
        &mut audit,
        &mut visitor,
        order,
    )?);
    let mut depth = 1usize;
    while depth != 0 {
        let frame = stack[depth - 1]
            .as_mut()
            .ok_or(Error::Corrupt("invalid audited path stack"))?;
        if frame.page.level == 0 || frame.next_child > frame.page.count {
            stack[depth - 1] = None;
            depth -= 1;
            continue;
        }
        let index = frame.next_child;
        frame.next_child += 1;
        let child = frame.page.child(store, index)?;
        let lower = if index == 0 {
            frame.lower
        } else {
            Some(frame.page.key_at(store, index - 1)?)
        };
        let upper = if index == frame.page.count {
            frame.upper
        } else {
            Some(frame.page.key_at(store, index)?)
        };
        let expected = frame.page.level - 1;
        if depth == stack.len() {
            return Err(Error::Corrupt("audited tree exceeds path bound"));
        }
        stack[depth] = Some(inspect(
            store,
            child,
            Some(expected),
            lower,
            upper,
            max_pages,
            &mut audit,
            &mut visitor,
            order,
        )?);
        depth += 1;
    }
    Ok(audit)
}

#[cfg(test)]
mod tests;
