//! Incremental evacuation through ordinary, complete business snapshots.
//! A private cutoff is minted from the current monotonic segment allocator.
//! Every returned root remains a complete graph; no draft roots or fake IDs
//! survive a step. Only a successful ordinary activation installs its progress.
use super::{MaintenancePolicy, MaintenanceStatus};
use crate::{
    DatabaseKind, Environment, Error, Result, WriteTxn, catalog::Database, environment::Snapshot,
};
use std::{collections::BTreeMap, ops::Bound};
use ulmdb_core::storage::{ReadStore, Reference, WriteStore};
use ulmdb_storage::{Arena, Manifest};

const MAX_VISITS: usize = 1024;
const MAX_COMPLETED: usize = 8; // EXPERIMENT TEST CAP ONLY
const MAX_DEPTH: usize = 256;
#[path = "v7_retained/frontier.rs"]
mod frontier;
use frontier::Frontier;

#[derive(Clone, Default)]
pub(crate) struct Progress {
    cutoff: u64,
    after: Option<Vec<u8>>,
    catalog: bool,
    complete: Frontier,
    kept_segments: Frontier,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Kv,
    Duplicates,
    Payload,
    Bucket,
    Radix4,
    Radix6,
}
#[derive(Clone, Copy)]
struct Link {
    offset: usize,
    reference: Reference,
    kind: Kind,
}
struct Budget {
    allowance: u64,
    bytes: u64,
    visits: usize,
    copied: u64,
    inspected: usize,
}
impl Budget {
    fn claim(&mut self, bytes: usize) -> bool {
        let Ok(bytes) = u64::try_from(bytes) else {
            return false;
        };
        if bytes > self.bytes {
            return false;
        }
        self.bytes -= bytes;
        true
    }
}

impl Progress {
    fn remember(&mut self, at: Reference, kind: Kind) {
        if self.complete.len() < MAX_COMPLETED {
            self.complete.insert((at, kind));
        }
    }
    fn walk(
        &mut self,
        arena: &mut Arena,
        at: Reference,
        kind: Kind,
        budget: &mut Budget,
        depth: usize,
        path_reserve: u64,
    ) -> Result<(Reference, bool)> {
        if at.is_null() {
            return Ok((at, true));
        }
        if self.complete.contains(&(at, kind)) {
            return Ok((at, true));
        }
        if budget.visits == 0 {
            return Ok((at, false));
        }
        budget.visits -= 1;
        budget.inspected += 1;
        if depth >= MAX_DEPTH {
            return Err(Error::Corrupt("maintenance graph depth"));
        }
        let source = arena.fresh_read_scope()?;
        let length = match kind {
            Kind::Kv | Kind::Duplicates => source.page_size(),
            Kind::Radix4 | Kind::Radix6 => 96,
            Kind::Payload | Kind::Bucket => source
                .payload_length(at)?
                .checked_add(16)
                .ok_or(Error::LimitExceeded)?,
        };
        // New opaque values need no body read/copy: ordinary writers cannot
        // introduce references inside them. The existing extent is checked.
        if kind == Kind::Payload && at.segment >= self.cutoff {
            self.remember(at, kind);
            return Ok((at, true));
        }
        // Reserve this frame's exact encoded copy before descending. This
        // guarantees every partial child update can be linked back to the
        // complete root even when the child's allowance runs out.
        let Some(cost) = length.checked_mul(2) else {
            return Ok((at, false));
        };
        if kind == Kind::Payload
            && at.segment < self.cutoff
            && (cost as u64).saturating_add(path_reserve) > budget.allowance
        {
            // One opaque value is indivisible in the existing wire format.
            // Preserve its authenticated containing segment and continue the
            // rest of the graph; a cold large value must not stall reclamation
            // of every unrelated obsolete segment indefinitely.
            let key = (
                Reference {
                    segment: at.segment,
                    offset: 0,
                },
                Kind::Payload,
            );
            if self.kept_segments.len() >= MAX_COMPLETED && !self.kept_segments.contains(&key) {
                return Err(Error::LimitExceeded);
            }
            self.kept_segments.insert(key);
            self.remember(at, kind);
            return Ok((at, true));
        }
        if !budget.claim(cost) {
            return Ok((at, false));
        }
        let mut bytes = source.read(at, length)?.to_vec();
        let mut links = Vec::new();
        match kind {
            Kind::Kv | Kind::Duplicates => {
                use ulmdb_kv::object_graph::{RelocationKind, inspect_relocation_page};
                let page = inspect_relocation_page(&source, at, kind == Kind::Duplicates)?;
                for link in page.links {
                    links.push(Link {
                        offset: link.offset,
                        reference: link.reference,
                        kind: match link.kind {
                            RelocationKind::Child => kind,
                            RelocationKind::Key => Kind::Payload,
                            RelocationKind::Value if kind == Kind::Duplicates => Kind::Bucket,
                            RelocationKind::Value => Kind::Payload,
                        },
                    });
                }
                for (offset, length) in page.inline_values {
                    let value = bytes
                        .get(offset..offset + length)
                        .ok_or(Error::Corrupt("maintenance inline bucket extent"))?;
                    if let Some(root) = bucket_root(value)? {
                        links.push(Link {
                            offset,
                            reference: root,
                            kind: Kind::Kv,
                        });
                    }
                }
            }
            Kind::Bucket => {
                if let Some(root) = bucket_root(&bytes[16..])? {
                    links.push(Link {
                        offset: 16,
                        reference: root,
                        kind: Kind::Kv,
                    });
                }
            }
            Kind::Radix4 | Kind::Radix6 => {
                let local = ulmdb_prefix::object_graph::inspect_node(&source, at)?;
                let family = match local.prefix.network() {
                    crate::Address::V4(_) => Kind::Radix4,
                    crate::Address::V6(_) => Kind::Radix6,
                };
                if family != kind {
                    return Err(Error::Corrupt("maintenance radix family"));
                }
                for offset in [32, 48, 64] {
                    let reference = Reference::decode(&bytes[offset..offset + 16])?;
                    if !reference.is_null() {
                        links.push(Link {
                            offset,
                            reference,
                            kind: if offset == 64 { Kind::Payload } else { kind },
                        });
                    }
                }
            }
            Kind::Payload => (),
        }
        drop(source);
        let mut finished = true;
        let mut changed = false;
        for link in &links {
            let (next, done) = self.walk(
                arena,
                link.reference,
                link.kind,
                budget,
                depth + 1,
                path_reserve.saturating_add(cost as u64),
            )?;
            finished &= done;
            if next != link.reference {
                bytes[link.offset..link.offset + 16].copy_from_slice(&next.encode());
                changed = true;
            }
            // Stop parsing more descendants once the admitted visit allowance
            // is exhausted. Unvisited references remain intact and valid.
            if budget.visits == 0 {
                finished = false;
                break;
            }
        }
        let next = if changed || (finished && at.segment < self.cutoff) {
            let alignment = if matches!(kind, Kind::Kv | Kind::Duplicates) {
                arena.page_size()
            } else {
                16
            };
            // Allocation forces unchanged pre-cutoff objects to move and
            // avoids Arena::rewrite's immutable equality read through its
            // long-lived base cache scope after our local scope has ended.
            let next = arena.allocate(&bytes, alignment)?;
            budget.copied = budget.copied.saturating_add(length as u64);
            next
        } else {
            at
        };
        if finished {
            // Retain completed child proofs as well as their parent. A later
            // business CoW edit replaces that parent reference, while most of
            // its children remain immutable and already evacuated. Discarding
            // their proofs forces the next step to reread entire unchanged
            // subtrees and can consume the bounded visit allowance forever.
            // Proofs remain cutoff/role/reference bound, installed only after
            // selection and capped by MAX_COMPLETED. At the cap, forgetting a
            // new proof is conservative: it adds work, never deletion authority.
            self.remember(next, kind);
        }
        Ok((next, finished))
    }
}

fn bucket_root(bytes: &[u8]) -> Result<Option<Reference>> {
    if bytes.len() < 24 {
        return Err(Error::Corrupt("maintenance duplicate bucket"));
    }
    let root = Reference::decode(&bytes[..16])?;
    if u64::from_le_bytes(bytes[16..24].try_into().unwrap()) == 0 {
        return Err(Error::Corrupt("maintenance empty duplicate bucket"));
    }
    if root.is_null() {
        return Ok(None);
    }
    if bytes.len() != 24 {
        return Err(Error::Corrupt("maintenance duplicate subtree extent"));
    }
    Ok(Some(root))
}

// Test-only public wrapper around the exact historical walker above.
#[derive(Clone)]
pub(super) struct Driver(Progress);
impl Driver {
    pub(super) fn new(cutoff: u64) -> Self {
        Self(Progress { cutoff, ..Default::default() })
    }
    pub(super) fn step(&mut self, arena: &mut Arena, root: Reference, visits: usize)
        -> Result<(Reference, bool, usize)> {
        let mut budget = Budget { allowance: 16 << 20, bytes: 16 << 20,
            visits, copied: 0, inspected: 0 };
        let (next, done) = self.0.walk(arena, root, Kind::Kv, &mut budget, 0, 0)?;
        assert!(budget.inspected <= visits);
        assert!(budget.copied <= 8 << 20);
        Ok((next, done, budget.inspected))
    }
    pub(super) fn count(&self) -> usize { self.0.complete.len() }
    pub(super) fn contains(&self, at: Reference) -> bool {
        self.0.complete.contains(&(at, Kind::Kv))
    }
}
