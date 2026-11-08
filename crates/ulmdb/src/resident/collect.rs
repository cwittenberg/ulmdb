//! Explicit charged full-live-graph retirement, never a history scan on each put.
use super::{Environment, Snapshot, audit, environment::Writer, journal::Journal};
use crate::{Error, Result};
use std::sync::Arc;
use ulmdb_storage::resident::wal_codec::{Kind, Record};
#[derive(Clone, Copy, Debug)]
pub struct CollectReport {
    pub retired_versions: usize,
    /// Full-traversal reads. Cache membership/empty-source checks are reported
    /// separately by collect_cache_stats when that feature is enabled.
    pub graph_reads: u64,
    pub kv_pages: u64,
    pub prefix_records: u64,
    pub free_pages_before: usize,
    pub free_pages_after: Option<usize>,
    pub sequence: u64,
    pub physical_lsn: u64,
}
impl Environment {
    pub fn collect(&self) -> Result<CollectReport> {
        self.check_process()?;
        let mut w = self.writer.lock().map_err(|_| Error::Busy)?;
        self.collect_locked(&mut w)
    }
    pub(super) fn collect_locked(&self, w: &mut Writer) -> Result<CollectReport> {
        if w.poisoned || w.journal.is_poisoned() || self.pool.stats()?.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        let before = self.pool.stats()?;
        let base = self.current.load_full();
        #[cfg(feature = "resident-prefix-audit-cache")]
        let mut next_cache = None;
        #[cfg(feature = "resident-prefix-audit-cache")]
        let result = if let Some(cache) = &w.prefix_audit_cache {
            cache
                .audit(
                    &self.pool,
                    &base.image,
                    base.root,
                    base.next_database,
                    self.options.graph_read_budget,
                )
                .map(|(checked, next)| {
                    next_cache = Some(next);
                    checked
                })
        } else {
            audit::audit(
                &base.image,
                base.root,
                base.next_database,
                self.options.graph_read_budget,
            )
        };
        #[cfg(not(feature = "resident-prefix-audit-cache"))]
        let result = audit::audit(
            &base.image,
            base.root,
            base.next_database,
            self.options.graph_read_budget,
        );
        let checked = match result {
            Ok(c) => c,
            Err(e) => {
                if matches!(e, Error::Corrupt(_)) {
                    w.poisoned = true;
                    self.pool.quarantine();
                    #[cfg(feature = "resident-prefix-audit-cache")]
                    {
                        w.prefix_audit_cache = None;
                    }
                }
                return Err(e);
            }
        };
        let retired = base
            .image
            .members()
            .filter(|k| !checked.live.contains(k))
            .collect::<Vec<_>>();
        let count = retired.len();
        if retired.is_empty() {
            #[cfg(feature = "resident-prefix-audit-cache")]
            if let Some(cache) = next_cache {
                w.prefix_audit_cache = Some(cache);
            }
            return Ok(CollectReport {
                retired_versions: 0,
                graph_reads: checked.reads,
                kv_pages: checked.kv_pages,
                prefix_records: checked.prefix_records,
                free_pages_before: before.free_pages,
                free_pages_after: Some(before.free_pages),
                sequence: base.receipt.sequence,
                physical_lsn: base.receipt.lsn,
            });
        }
        let mut pending = w
            .incremental
            .as_ref()
            .map(|certificate| certificate.prepare_collection(&base, &retired, &checked.live))
            .transpose()?;
        let image = match &pending {
            Some(pending) => pending.image().clone(),
            None => base.image.without_versions(&retired)?,
        };
        let metrics = pending.as_ref().map(|p| p.metrics());
        let boot = w.journal.boot();
        let lsn = base
            .receipt
            .lsn
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        let collections = w.collections.checked_add(1).ok_or(Error::LimitExceeded)?;
        let mut ready = None;
        let mut ready_certificate = None;
        let ready_callback =
            |receipt, metadata: Option<&ulmdb_storage::resident::wal_codec_v2::Record>| {
                if let Some(pending) = pending.as_ref() {
                    pending.validate_record(
                        metadata.ok_or(Error::Corrupt("incremental collection requires RL2"))?,
                    )?;
                }
                ready_certificate = pending
                    .take()
                    .map(|p| p.bind(receipt, base.commit))
                    .transpose()?;
                ready = Some(Arc::new(Snapshot {
                    image,
                    catalog: super::catalog::Catalog::from_map_with_capacity(
                        checked.catalog,
                        checked.catalog_capacity,
                    )?,
                    root: base.root,
                    next_database: base.next_database,
                    receipt,
                    commit: base.commit,
                }));
                Ok(())
            };
        let result = match &mut w.journal {
            Journal::Literal(j) => {
                let record = Record {
                    uuid: boot.uuid,
                    epoch: boot.epoch,
                    lsn,
                    sequence: base.receipt.sequence,
                    commit: base.commit,
                    parent_commit: base.commit,
                    parent: base.receipt,
                    root: base.root,
                    next_database: base.next_database,
                    kind: Kind::Collect,
                    allocations: Vec::new(),
                    retired,
                };
                j.commit_checked(record, |_, receipt| ready_callback(receipt, None))
            }
            Journal::Compact(j) => {
                let record = ulmdb_storage::resident::wal_codec_v2::Record {
                    uuid: boot.uuid,
                    epoch: boot.epoch,
                    lsn,
                    sequence: base.receipt.sequence,
                    commit: base.commit,
                    parent_commit: base.commit,
                    parent: base.receipt,
                    root: base.root,
                    next_database: base.next_database,
                    kind: Kind::Collect,
                    allocations: Vec::new(),
                    retired,
                };
                j.commit_checked(record, |metadata, receipt| {
                    ready_callback(receipt, Some(metadata))
                })
            }
        };
        if let Err(e) = result {
            if w.journal.is_poisoned() {
                w.poisoned = true;
                self.pool.quarantine();
                #[cfg(feature = "resident-prefix-audit-cache")]
                {
                    w.prefix_audit_cache = None;
                }
            }
            return Err(e);
        }
        #[cfg(feature = "resident-prefix-audit-cache")]
        if let Some(cache) = next_cache {
            w.prefix_audit_cache = Some(cache);
        }
        w.collections = collections;
        if let Some(certificate) = ready_certificate {
            w.incremental = Some(certificate.activate());
            w.incremental_metrics = metrics.expect("incremental collection has metrics");
        }
        self.current
            .store(ready.expect("resident collection prepared before IO"));
        drop(base);
        Ok(CollectReport {
            retired_versions: count,
            graph_reads: checked.reads,
            kv_pages: checked.kv_pages,
            prefix_records: checked.prefix_records,
            free_pages_before: before.free_pages,
            free_pages_after: self.pool.stats().ok().map(|s| s.free_pages),
            sequence: self.sequence(),
            physical_lsn: lsn,
        })
    }
}
