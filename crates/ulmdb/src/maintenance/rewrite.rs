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
const MAX_COMPLETED: usize = 1_048_576;
const MAX_RECENT: usize = 65_536;
const MAX_DEPTH: usize = 256;
mod frontier;
use frontier::Frontier;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum ProofMode {
    #[default]
    Reuse,
    RestartRequested,
    Drain,
    Minimal,
}

#[derive(Clone, Default)]
pub(crate) struct Progress {
    cutoff: u64,
    after: Option<Vec<u8>>,
    catalog: bool,
    complete: Frontier,
    recent: Frontier,
    proof_mode: ProofMode,
    #[cfg(test)]
    test_limits: Option<(usize, usize)>,
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
    fn proof_limits(&self) -> (usize, usize) {
        #[cfg(test)]
        if let Some(limits) = self.test_limits {
            return limits;
        }
        (MAX_COMPLETED, MAX_RECENT)
    }
    fn cached(&mut self, key: &(Reference, Kind)) -> bool {
        if self.complete.contains(key) {
            return true;
        }
        if self.proof_mode == ProofMode::Reuse && self.recent.remove(key) {
            // Promotion preserves the combined size. A disposable cache hit
            // must become primary before an unfinished ancestor relies on it.
            self.complete.insert(*key);
            return true;
        }
        false
    }
    fn remember(&mut self, at: Reference, kind: Kind) {
        if matches!(self.proof_mode, ProofMode::RestartRequested | ProofMode::Drain) {
            return;
        }
        let key = (at, kind);
        if self.complete.contains(&key) {
            return;
        }
        self.recent.remove(&key);
        let (total, _) = self.proof_limits();
        if self.complete.len() + self.recent.len() >= total {
            let _ = self.recent.remove_min();
        }
        if self.complete.len() + self.recent.len() < total {
            self.complete.insert(key);
        } else if self.proof_mode == ProofMode::Reuse {
            // Do not discard a large persistent tree during this recursion.
            // The enclosing candidate first links all partial child copies.
            self.proof_mode = ProofMode::RestartRequested;
        }
        // Minimal preserves the old conservative full-cap behavior, without
        // repeatedly requesting a restart that cannot fix its own frontier.
    }
    fn remember_recent(&mut self, key: (Reference, Kind)) {
        if self.proof_mode != ProofMode::Reuse || self.complete.contains(&key)
            || self.recent.contains(&key) {
            return;
        }
        let (total, recent) = self.proof_limits();
        if recent == 0 {
            return;
        }
        if self.recent.len() >= recent || self.complete.len() + self.recent.len() >= total {
            let _ = self.recent.remove_min();
        }
        if self.recent.len() < recent && self.complete.len() + self.recent.len() < total {
            self.recent.insert(key);
        }
    }
    /// Return false for a metadata-only drain call, including the final one.
    /// It never reads a graph, copies data, or grants inventory projection.
    fn begin_step(&mut self, budget: &mut Budget) -> bool {
        if self.proof_mode != ProofMode::Drain {
            return true;
        }
        while budget.visits != 0 {
            let removed = if self.recent.len() != 0 {
                self.recent.remove_min()
            } else {
                self.complete.remove_min()
            };
            if removed.is_none() {
                break;
            }
            budget.visits -= 1;
        }
        if self.complete.len() == 0 && self.recent.len() == 0 {
            self.proof_mode = ProofMode::Minimal;
        }
        false
    }
    /// Must run before any retain_base_matching, including early walk exits.
    fn finish_step(&mut self, done: bool) -> bool {
        if self.proof_mode == ProofMode::RestartRequested {
            self.after = None;
            self.catalog = false;
            self.proof_mode = ProofMode::Drain;
            return false;
        }
        done && self.proof_mode != ProofMode::Drain
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
        if self.cached(&(at, kind)) {
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
        for link in &mut links {
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
            // Subsumption must use the returned child reference, including
            // copies linked into this still-complete parent image.
            link.reference = next;
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
            // Keep the original minimal primary frontier. Subsumed proofs
            // are optional recent hints, never a reason to reduce its capacity.
            for link in &links {
                let key = (link.reference, link.kind);
                if self.complete.remove(&key) {
                    self.remember_recent(key);
                }
            }
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

impl WriteTxn<'_> {
    /// Return progress to be installed only after this exact candidate selects.
    pub(crate) fn incremental_rewrite(
        &mut self,
        manifest: &mut Manifest,
    ) -> Result<(Option<Progress>, bool)> {
        RewriteContext {
            env: self.env,
            base: &self.base,
            arena: &mut self.arena,
            catalog: &mut self.catalog,
            force_start: false,
        }
        .run(manifest)
    }
}
pub(super) struct RewriteContext<'a> {
    pub env: &'a Environment,
    pub base: &'a Snapshot,
    pub arena: &'a mut Arena,
    pub catalog: &'a mut BTreeMap<Vec<u8>, Database>,
    pub force_start: bool,
}
impl RewriteContext<'_> {
    pub(super) fn run(&mut self, manifest: &mut Manifest) -> Result<(Option<Progress>, bool)> {
        if !self
            .env
            .maintenance_enabled
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok((None, false));
        }
        let (threshold, allowance) = match self.env.maintenance_policy()? {
            MaintenancePolicy::LegacyResumable {
                segment_threshold,
                rewrite_budget_bytes,
                ..
            }
            | MaintenancePolicy::CrossFormatResumable {
                segment_threshold,
                rewrite_budget_bytes,
                ..
            } => (segment_threshold, rewrite_budget_bytes),
            _ => return Ok((None, false)),
        };
        let mut progress = self
            .env
            .maintenance
            .lock()
            .map_err(|_| Error::Busy)?
            .rewrite
            .as_ref()
            .cloned();
        if progress.is_none() {
            if !self.force_start
                && self
                    .base
                    .manifest
                    .segments
                    .len()
                    .saturating_add(usize::from(self.arena.dirty_bytes() != 0))
                    < threshold
            {
                return Ok((None, false));
            }
            progress = Some(Progress {
                cutoff: self.base.manifest.next_segment,
                ..Default::default()
            });
        }
        let mut progress = progress.unwrap();
        let committed_progress = progress.clone();
        let checkpoint = self.arena.checkpoint()?;
        let original_catalog = manifest.catalog;
        let mut replaced = Vec::new();
        let mut budget = Budget {
            allowance,
            bytes: allowance,
            visits: MAX_VISITS,
            copied: 0,
            inspected: 0,
        };
        if !progress.begin_step(&mut budget) {
            self.arena.release_checkpoint(&checkpoint)?;
            let mut state = self.env.maintenance.lock().map_err(|_| Error::Busy)?;
            report(&mut state.status, &budget, false);
            return Ok((Some(progress), false));
        }
        let result = (|| {
            replaced
                .try_reserve_exact(self.catalog.len().min(MAX_VISITS))
                .map_err(|_| Error::LimitExceeded)?;
            if !progress.catalog {
                while budget.visits != 0 {
                    let lower = progress
                        .after
                        .as_ref()
                        .map_or(Bound::Unbounded, |name| Bound::Excluded(name.clone()));
                    let next = self
                        .catalog
                        .range((lower, Bound::Unbounded))
                        .next()
                        .map(|(name, db)| (name.clone(), db.clone()));
                    if let Some((name, old)) = next {
                        // Catalog descriptors, including empty databases, consume
                        // work units too. A wide catalog advances many small/empty
                        // databases per step instead of one per business commit.
                        budget.visits -= 1;
                        budget.inspected += 1;
                        let kind = match old.kind {
                            DatabaseKind::Kv => Kind::Kv,
                            DatabaseKind::Duplicates { .. } => Kind::Duplicates,
                            DatabaseKind::Ipv4 => Kind::Radix4,
                            DatabaseKind::Ipv6 => Kind::Radix6,
                        };
                        let (root, done) =
                            progress.walk(&mut self.arena, old.root, kind, &mut budget, 0, 0)?;
                        if root != old.root {
                            let mut next = old.clone();
                            next.root = root;
                            manifest.catalog = ulmdb_kv::put(
                                self.arena,
                                manifest.catalog,
                                &name,
                                &next.encode_for_profile(manifest.page_size, manifest.value_limit),
                            )?;
                            self.catalog.insert(name.clone(), next);
                            replaced.push((name.clone(), old));
                        }
                        if done {
                            progress.after = Some(name);
                        } else {
                            break;
                        }
                    } else {
                        progress.catalog = true;
                        break;
                    }
                }
                if !progress.catalog || budget.visits == 0 {
                    return Ok(false);
                }
            }
            // Every database graph has crossed the cutoff. Ordinary forward
            // mutations only derive from those graphs or introduce fresh data.
            // Catalog values contain those already safe roots; they are copied
            // byte-for-byte while evacuating this final tree.
            let (root, done) = progress.walk(
                &mut self.arena,
                manifest.catalog,
                Kind::Kv,
                &mut budget,
                0,
                0,
            )?;
            manifest.catalog = root;
            let done = progress.finish_step(done);
            if done {
                self.arena.retain_base_matching(|entry| {
                    entry.id >= progress.cutoff
                        || progress.kept_segments.contains(&(
                            Reference {
                                segment: entry.id,
                                offset: 0,
                            },
                            Kind::Payload,
                        ))
                })?;
            }
            Ok(done)
        })();
        match result {
            Ok(done) => {
                // Early database/budget exits must also commit a pending
                // restart instead of installing a RestartRequested state.
                let done = progress.finish_step(done);
                self.arena.release_checkpoint(&checkpoint)?;
                let mut state = self.env.maintenance.lock().map_err(|_| Error::Busy)?;
                report(&mut state.status, &budget, done);
                if done {
                    state.status.last_rewrite_kept_segments = progress.kept_segments.len();
                }
                if done {
                    Ok((None, true))
                } else {
                    Ok((Some(progress), false))
                }
            }
            Err(error) => {
                self.arena.rollback_checkpoint(&checkpoint)?;
                manifest.catalog = original_catalog;
                for (name, old) in replaced {
                    self.catalog.insert(name, old);
                }
                if matches!(error, Error::LimitExceeded) {
                    let mut state = self.env.maintenance.lock().map_err(|_| Error::Busy)?;
                    state.status.deferred_rebuilds =
                        state.status.deferred_rebuilds.saturating_add(1);
                    Ok((Some(committed_progress), false))
                } else {
                    Err(error)
                }
            }
        }
    }
}

fn report(status: &mut MaintenanceStatus, budget: &Budget, done: bool) {
    status.incremental_rewrite_steps = status.incremental_rewrite_steps.saturating_add(1);
    status.incremental_rewrite_objects = status
        .incremental_rewrite_objects
        .saturating_add(budget.inspected as u64);
    status.incremental_rewrite_bytes = status
        .incremental_rewrite_bytes
        .saturating_add(budget.copied);
    if done {
        status.rebuilt_candidates = status.rebuilt_candidates.saturating_add(1);
    }
}

#[cfg(test)]
#[path = "rewrite/proof_reuse_tests.rs"]
mod proof_reuse_tests;

#[cfg(all(test, any(target_os = "linux", target_os = "macos", windows)))]
#[path = "rewrite/churn_progress_tests.rs"]
mod churn_progress_tests;

#[cfg(test)]
#[path = "rewrite/proof_cache_tests.rs"]
mod proof_cache_tests;
