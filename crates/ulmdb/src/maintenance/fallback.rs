//! Prepare optional maintenance beside an already ready business candidate.
//! No selection is attempted here; errors leave that candidate authoritative.
use super::{MaintenancePolicy, RewriteProgress, rewrite::RewriteContext};
use crate::{
    Environment, Error, Result, environment::Snapshot, physical::PreparedSnapshot,
    prepared::FilePublication,
};
use std::sync::atomic::Ordering;
use ulmdb_storage::{Arena, PreparedPublication};

pub(crate) struct Alternative {
    pub candidate: Option<(PreparedSnapshot, FilePublication)>,
    pub progress: Option<RewriteProgress>,
    pub retired: bool,
}

impl Environment {
    pub(crate) fn maintenance_fallback_state(&self) -> Result<(bool, Option<RewriteProgress>)> {
        if !self.maintenance_enabled.load(Ordering::Acquire) {
            return Ok((false, None));
        }
        let state = self.maintenance.lock().map_err(|_| Error::Busy)?;
        Ok((state.fallback, state.rewrite.clone()))
    }
    pub(crate) fn note_fallback_error(&self, error: &Error) {
        if let Ok(mut state) = self.maintenance.lock() {
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            state.request_orphan_discovery();
            state.status.fallback_rewrite_deferred =
                state.status.fallback_rewrite_deferred.saturating_add(1);
            state.status.last_fallback_rewrite_error = Some(error.to_string());
        }
    }
    pub(crate) fn prepare_maintenance_alternative(
        &self,
        ordinary: &PreparedSnapshot,
        parent: &Snapshot,
        data_policy: crate::Durability,
    ) -> Result<Option<Alternative>> {
        let (threshold, byte_threshold) = match self.maintenance_policy()? {
            MaintenancePolicy::LegacyResumable {
                segment_threshold,
                rewrite_budget_bytes,
                ..
            }
            | MaintenancePolicy::CrossFormatResumable {
                segment_threshold,
                rewrite_budget_bytes,
                ..
            } => (segment_threshold, rewrite_budget_bytes.saturating_mul(4)),
            _ => return Ok(None),
        };
        let read = ordinary.read();
        let source = &read.snapshot;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        let fixed_pressure = self.reusable_fixed_map_pressure()?;
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let fixed_pressure = false;
        if self
            .maintenance
            .lock()
            .map_err(|_| Error::Busy)?
            .rewrite
            .is_none()
            && !fixed_pressure
            && source.manifest.segments.len() < threshold
            && source
                .manifest
                .segments
                .iter()
                .fold(0u64, |n, e| n.saturating_add(e.length))
                < byte_threshold
        {
            return Ok(None);
        }
        // V1 remains append-only: a speculative alternative would consume
        // permanent slots even on fallback. V2 can reclaim failed alternatives
        // under the scheduler's fresh writer/Capture root set. Ordinary Ready B
        // remains genuinely admitted/pinned throughout this whole function.
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            let reusable = if let Some(fixed) = source.segments.fixed_map() {
                Some(fixed.supports_reuse()?)
            } else if let Some(declaration) = source.segments.fixed_declaration() {
                Some(declaration.supports_reuse()?)
            } else {
                None
            };
            if reusable == Some(false) {
                self.note_fallback_error(&Error::LimitExceeded);
                return Ok(None);
            }
        }
        if let Ok(mut state) = self.maintenance.lock() {
            #[cfg(any(target_os = "linux", target_os = "macos", windows))]
            state.request_orphan_discovery();
            state.status.fallback_rewrite_attempts =
                state.status.fallback_rewrite_attempts.saturating_add(1);
        }
        point("maintenance.optional.before_rewrite")?;
        let mut arena = Arena::new(source.segments.clone(), source.manifest.next_segment)?;
        let mut catalog = source.catalog.clone();
        let mut manifest = source.manifest.clone_metadata();
        manifest.generation = ulmdb_storage::new_identity()?;
        // Preserve the actual business parent, sequence and commit. The source
        // candidate was never selected and is not a synthetic previous commit.
        let (progress, retired) = RewriteContext {
            env: self,
            base: parent,
            arena: &mut arena,
            catalog: &mut catalog,
            force_start: true,
        }
        .run(&mut manifest)?;
        if arena.dirty_bytes() == 0 && !retired && manifest.catalog == source.manifest.catalog {
            return Ok(Some(Alternative {
                candidate: None,
                progress,
                retired,
            }));
        }
        point("maintenance.optional.before_seal")?;
        let (segments, publication, sealed) = if !retired && manifest.format_minor == 4 {
            let (segments, prepared) = arena.seal_indexed_with_durability(
                &self.directory,
                &mut manifest,
                &source.manifest,
                data_policy,
            )?;
            (segments, Some(prepared), None)
        } else if !retired && manifest.format_minor == 3 {
            let (segments, prepared) =
                arena.seal_cohort_with_durability(&self.directory, &mut manifest, data_policy)?;
            (segments, Some(prepared), None)
        } else {
            let (segments, entry, sealed) =
                arena.seal_with_durability(&self.directory, data_policy)?;
            if entry.is_some() {
                manifest.next_segment = manifest
                    .next_segment
                    .checked_add(1)
                    .ok_or(Error::LimitExceeded)?;
            }
            manifest.segments = segments.entries().into();
            if manifest.format_minor >= 3 {
                if manifest.format_minor == 4 {
                    manifest.prepare_indexed(None)?;
                }
                let publication = ulmdb_storage::prepare_publication_with_durability(
                    &self.directory,
                    &manifest,
                    data_policy,
                )?;
                (segments, Some(publication), None)
            } else {
                (segments, None, Some(sealed))
            }
        };
        let segments = if source.segments.is_chunked() {
            drop(segments);
            crate::environment::cached_candidate_segments(
                &self.directory,
                &manifest,
                &source.segments,
                data_policy,
            )?
        } else {
            segments.with_value_limit(manifest.value_limit)?
        };
        let mut snapshot = Snapshot {
            range_backed: false,
            external: None,
            physical_pin: None,
            #[cfg(feature = "range-lifecycle")]
            range_checkpoint: None,
            owner: source.owner,
            manifest,
            segments,
            catalog,
            admission: None,
        };
        point("maintenance.optional.before_admission")?;
        self.admit_snapshot_from(&mut snapshot, source)?;
        point("maintenance.optional.before_descriptor")?;
        // All failures here precede selection. Avoid the legacy conservative
        // descriptor poison boundary, which belongs to the ordinary candidate.
        let prepared: PreparedPublication =
            self.prepare_business_file_snapshot(&mut snapshot, publication, sealed, true)?;
        let durability = self.cached_file_durability()?;
        let publication = FilePublication::Ordinary(prepared.prepare_ready(durability.as_ref())?);
        let snapshot = snapshot.prepare_activation(self, publication.prepared())?;
        if let Ok(mut state) = self.maintenance.lock() {
            state.status.last_fallback_rewrite_error = None;
        }
        Ok(Some(Alternative {
            candidate: Some((snapshot, publication)),
            progress,
            retired,
        }))
    }
}

fn point(name: &str) -> Result<()> {
    #[cfg(feature = "fault-injection")]
    ulmdb_storage::injected_fault(name)?;
    #[cfg(not(feature = "fault-injection"))]
    let _ = name;
    Ok(())
}
