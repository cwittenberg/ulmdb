//! Copy captures retain genuine physical authority before facade projection.
use super::*;
use ulmdb::{ExternalFileCopyPairCapture, FileCopyPair, FileCopyView, OwnedFileCopyPairCapture};

pub(crate) enum CopyCapture {
    ExternalPair(ExternalFileCopyPairCapture),
    ExternalView(ExternalFileCopyPairCapture, bool),
    Pair(OwnedFileCopyPairCapture),
    Current(OwnedFileCapture),
    Selected(OwnedFileCopyPairCapture),
    Previous(OwnedFileCopyPairCapture),
}
pub(crate) enum AdmittedCopy {
    Pair(FileCopyPair),
    View(FileCopyView),
}
impl CopyCapture {
    /// The caller has already released Visibility; only pinned graph/residency
    /// admission remains. No virtual-MAIN reader can construct these tokens.
    pub(crate) fn admit(
        self,
        advice: ulmdb::ReadAhead,
        transform: Transform,
        cache: chunks::Options,
    ) -> FfiResult<AdmittedCopy> {
        let policy = ResidencyPolicy::Demand;
        match self {
            Self::ExternalPair(pair) => pair
                .admit_with_cache_options(policy, advice, transform, cache)
                .map(AdmittedCopy::Pair),
            Self::ExternalView(pair, previous) => pair
                .admit_view_with_cache_options(previous, policy, advice, transform, cache)
                .map(AdmittedCopy::View),
            Self::Pair(pair) => pair
                .admit_with_cache_options(policy, advice, transform, cache)
                .map(AdmittedCopy::Pair),
            Self::Current(current) => current
                .admit_copy_view_with_cache_options(policy, advice, transform, cache)
                .map(AdmittedCopy::View),
            Self::Selected(pair) => pair
                .admit_selected_view_with_cache_options(policy, advice, transform, cache)
                .map(AdmittedCopy::View),
            Self::Previous(pair) => pair
                .admit_predecessor_view_with_cache_options(policy, advice, transform, cache)
                .map(AdmittedCopy::View),
        }
        .map_err(code)
    }
}
fn view(pair: &OwnedFileCopyPairCapture) -> FfiResult<Meta> {
    if let (Some(manifest), Some(checkpoint)) =
        (pair.predecessor_manifest(), pair.predecessor_checkpoint())
    {
        return Meta::new(manifest, *checkpoint);
    }
    let m = pair.selected_manifest();
    if m.sequence == 0
        && m.previous == 0
        && m.catalog.is_null()
        && m.parent == 0
        && m.next_database == 1
        && m.next_segment == 1
        && m.segments.is_empty()
    {
        Meta::new(m, *pair.selected_checkpoint())
    } else {
        Err(libc::ENOTSUP)
    }
}
fn previous(pair: OwnedFileCopyPairCapture, compact: bool) -> FfiResult<CopyCapture> {
    let chosen = view(&pair)?;
    if compact {
        Ok(CopyCapture::Previous(pair))
    } else if pair.selected_checkpoint() == &chosen.checkpoint {
        Ok(CopyCapture::Pair(pair))
    } else {
        // Upstream writes both canonical metas but an older reader's payload
        // extent. Those documented reference-bug paths produce corrupt output;
        // reject explicitly before creating any owned/user destination.
        Err(libc::ENOTSUP)
    }
}
pub(crate) fn capture(shared: &Shared, flags: u32, compact: bool) -> FfiResult<CopyCapture> {
    shared.check_process()?;
    if shared.poisoned.load(Ordering::Acquire) {
        return Err(-30795);
    }
    if let Some(external) = &shared.external {
        let pair = external
            .capture_copy_pair(shared.transform.clone())
            .map_err(code)?;
        if shared.previous.active() {
            let chosen = external_view(&pair)?;
            if compact {
                return Ok(CopyCapture::ExternalView(pair, true));
            }
            if pair.selected_checkpoint() != &chosen.checkpoint {
                // Same documented older-view raw-copy limitation as coordinated
                // PREV. Decline before any target/staging creation.
                return Err(libc::ENOTSUP);
            }
        }
        return Ok(if compact {
            CopyCapture::ExternalView(pair, false)
        } else {
            CopyCapture::ExternalPair(pair)
        });
    }
    for _ in 0..RETRIES {
        let directory = begin_shared(shared)?;
        let lease = shared
            .visibility
            .as_ref()
            .map(|v| v.lease(false))
            .transpose()?;
        let record = shared
            .visibility
            .as_ref()
            .zip(lease.as_ref())
            .map(|(visibility, lease)| visibility.read(lease))
            .transpose()?;
        let current =
            OwnedFileCapture::current_for_copy(&shared.directory, directory).map_err(code)?;
        let durable = selected(&current)?;
        let offset = match record.as_ref() {
            Some(record) => record.matches(durable)? && record.kind == Kind::Offset,
            None => shared.previous.active(),
        };
        if offset {
            let pair = match current.into_copy_pair() {
                Ok(pair) => pair,
                Err(Error::Busy) => continue,
                Err(error) => return Err(code(error)),
            };
            let chosen = view(&pair)?;
            if let Some(record) = record.as_ref()
                && chosen.checkpoint != record.view
            {
                return Err(CORRUPT);
            }
            let selected_fallback = record.is_some()
                && pair
                    .predecessor_manifest()
                    .unwrap_or(pair.selected_manifest())
                    .sequence
                    == 0
                && flags & RDONLY != 0
                && !shared.previous.active();
            let result = if selected_fallback {
                if compact {
                    CopyCapture::Selected(pair)
                } else {
                    CopyCapture::Pair(pair)
                }
            } else {
                previous(pair, compact)?
            };
            drop(lease);
            return Ok(result);
        }
        let result = if compact {
            CopyCapture::Current(current)
        } else {
            match current.into_copy_pair() {
                Ok(pair) => CopyCapture::Pair(pair),
                Err(Error::Busy) => continue,
                Err(error) => return Err(code(error)),
            }
        };
        drop(lease);
        return Ok(result);
    }
    Err(libc::EBUSY)
}

// External roots remain genuine D/P metadata authority; this function only
// chooses the same local NOLOCK view role and verified genesis fallback.
fn external_view(pair: &ExternalFileCopyPairCapture) -> FfiResult<Meta> {
    if let (Some(manifest), Some(checkpoint)) = (
        pair.predecessor_manifest(),
        pair.predecessor_checkpoint().map_err(code)?,
    ) {
        return Meta::new(manifest, checkpoint);
    }
    let m = pair.selected_manifest();
    if m.sequence == 0
        && m.previous == 0
        && m.catalog.is_null()
        && m.parent == 0
        && m.next_database == 1
        && m.next_segment == 1
        && m.segments.is_empty()
    {
        Meta::new(m, *pair.selected_checkpoint())
    } else {
        Err(libc::ENOTSUP)
    }
}
