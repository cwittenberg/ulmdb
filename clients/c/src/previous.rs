//! Real native snapshot selection behind the C shared visibility record.
pub(super) mod copy_capture;
pub(super) mod publication_roots;
use super::*;
use std::sync::atomic::{AtomicU8, Ordering};
use ulmdb::{OwnedFileCapture, PreviousError, PreviousSession, ResidencyPolicy};
use ulmdb_storage::coordination::Coordination;
use visibility::{Kind, Meta, Visibility};

pub(super) const NORMAL: u8 = 0;
pub(super) const READONLY: u8 = 1;
pub(super) const PENDING: u8 = 2;
const CORRUPT: c_int = -30796;
const RETRIES: usize = 4;

pub(super) struct PreviousState {
    mode: AtomicU8,
    // Native-only Arc: storing WriterEnvironment here would cycle through Shared.
    pending: Mutex<Option<Arc<PreviousSession>>>,
}
impl PreviousState {
    pub(super) fn new(mode: u8, pending: Option<Arc<PreviousSession>>) -> Self {
        Self {
            mode: AtomicU8::new(mode),
            pending: Mutex::new(pending),
        }
    }
    pub(super) fn active(&self) -> bool {
        self.mode.load(Ordering::Acquire) != NORMAL
    }
    pub(super) fn pending(&self) -> bool {
        self.mode.load(Ordering::Acquire) == PENDING
    }
    pub(super) fn session(&self) -> FfiResult<Option<Arc<PreviousSession>>> {
        Ok(self.pending.lock().map_err(|_| BAD_TXN)?.clone())
    }
    pub(super) fn finish(&self, environment: Option<&process::EnvironmentLease>) -> FfiResult<()> {
        // Preflight the only fallible local ownership access before downgrade.
        // This writer-specific mutex is never taken by reader admission, and
        // no ENV-GATE holder can acquire it on this Shared.
        let mut pending = self.pending.lock().map_err(|_| BAD_TXN)?;
        if pending.is_none() || !self.pending() {
            return Err(BAD_TXN);
        }
        if let Some(environment) = environment {
            environment.downgrade()?;
        }
        pending.take();
        self.mode.store(NORMAL, Ordering::Release);
        Ok(())
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum RoleKind {
    Canonical,
    FirstPrevious,
    FollowOffset,
}
pub(super) struct WriteRole {
    kind: RoleKind,
    // Retain the actual admitted A pin through first compound record update.
    base: Option<ReadTxn>,
}
impl WriteRole {
    pub(super) fn canonical() -> Self {
        Self {
            kind: RoleKind::Canonical,
            base: None,
        }
    }
    pub(super) fn first(&self) -> bool {
        self.kind == RoleKind::FirstPrevious
    }
}

pub(super) fn error(error: PreviousError) -> c_int {
    match error {
        PreviousError::NoPrevious => libc::ENOTSUP,
        PreviousError::Engine(error) => code(error),
    }
}
pub(super) fn meta(read: &ReadTxn) -> FfiResult<Meta> {
    Meta::new(
        read.file_manifest().map_err(code)?,
        read.file_checkpoint().map_err(code)?,
    )
}
fn selected(capture: &OwnedFileCapture) -> FfiResult<Meta> {
    Meta::new(capture.selected_manifest(), *capture.selected_checkpoint())
}
fn viewed(capture: &OwnedFileCapture) -> FfiResult<Meta> {
    Meta::new(capture.view_manifest(), *capture.view_checkpoint())
}
fn begin(directory: &std::path::Path) -> FfiResult<ulmdb_storage::coordination::DirectoryCapture> {
    begin_with_transform(directory, None)
}
fn begin_with_transform(
    directory: &std::path::Path,
    transform: Transform,
) -> FfiResult<ulmdb_storage::coordination::DirectoryCapture> {
    Coordination::capture_directory_with_transform(directory, Default::default(), transform)
        .map_err(code)
}
// Shared callers check their creator before entering this helper. The genuine
// cached wrapper also checks its own creator and all namespace authority.
fn begin_shared(shared: &Shared) -> FfiResult<ulmdb_storage::coordination::DirectoryCapture> {
    match &shared.coordination {
        Some(coordinator) => coordinator.capture_directory().map_err(code),
        None => begin_with_transform(&shared.directory, shared.transform.clone()),
    }
}
fn admit(
    capture: OwnedFileCapture,
    empty_main: bool,
    advice: ulmdb::ReadAhead,
    transform: Transform,
    cache: chunks::Options,
) -> FfiResult<ReadTxn> {
    let read = capture
        .admit_with_cache_options(ResidencyPolicy::Demand, advice, transform, cache)
        .map_err(code)?
        .0;
    if empty_main {
        read.project_empty_kv(MAIN).map_err(code)
    } else {
        Ok(read)
    }
}
fn admit_shared(shared: &Shared, capture: OwnedFileCapture, empty_main: bool) -> FfiResult<ReadTxn> {
    // Fixed-address/window owners retain their separate mapping/retirement
    // protocol; ordinary graph reuse has no authority over those resources.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    let ordinary = shared.fixed_map.is_none() && shared.fixed_declaration.is_none();
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let ordinary = false;
    if ordinary && let Some(coordinator) = &shared.coordination {
        let read = coordinator
            .admit_file_capture_with_read_ahead(capture, shared.read_ahead())
            .map_err(code)?.0;
        return if empty_main { read.project_empty_kv(MAIN).map_err(code) } else { Ok(read) };
    }
    admit(capture, empty_main, shared.read_ahead(), shared.transform.clone(), shared.cache.clone())
}

pub(super) fn open_visibility(
    directory: &std::path::Path,
    lease: Option<&process::EnvironmentLease>,
    transform: Transform,
) -> FfiResult<Option<Visibility>> {
    if !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
        return Ok(None);
    }
    let Some(lease) = lease else {
        return Ok(None);
    };
    let initial = if lease.fresh() {
        let capture =
            OwnedFileCapture::current(directory, begin_with_transform(directory, transform)?)
                .map_err(code)?;
        Some(selected(&capture)?)
    } else {
        None
    };
    Visibility::open(directory, initial, lease).map(Some)
}

/// Initial previous admission, independent of any canonical B user graph.
pub(super) fn initial_previous_external(
    directory: &std::path::Path,
    advice: ulmdb::ReadAhead,
    transform: Transform,
    cache: chunks::Options,
) -> FfiResult<(ReadTxn, Meta, Meta)> {
    let context = ulmdb::ExternalSynchronization::open_namespace(directory, transform.clone())
        .map_err(code)?;
    let frame = context.capture_previous(transform.clone()).map_err(error)?;
    let durable = Meta::new(frame.selected_manifest(), *frame.selected_checkpoint())?;
    let view = Meta::new(frame.view_manifest(), *frame.view_checkpoint())?;
    let read = frame
        .admit_with_cache_options(ResidencyPolicy::Demand, advice, transform, cache)
        .map_err(code)?;
    Ok((read, durable, view))
}
fn external_snapshot(shared: &Shared, previous: bool, empty_main: bool) -> FfiResult<ReadTxn> {
    let external = shared.external.as_ref().ok_or(BAD_TXN)?;
    let read = if previous {
        external
            .capture_previous(shared.transform.clone())
            .map_err(error)?
            .admit_with_cache_options(
                ResidencyPolicy::Demand,
                shared.read_ahead(),
                shared.transform.clone(),
                shared.cache.clone(),
            )
            .map_err(code)?
    } else {
        external
            .begin_read_with_cache_options(
                &shared.directory,
                shared.read_ahead(),
                shared.transform.clone(),
                shared.cache.clone(),
            )
            .map_err(code)?
    };
    if empty_main {
        read.project_empty_kv(MAIN).map_err(code)
    } else {
        Ok(read)
    }
}
pub(super) fn initial_previous(
    directory: &std::path::Path,
    visibility: Option<&Visibility>,
    advice: ulmdb::ReadAhead,
    transform: Transform,
    cache: chunks::Options,
) -> FfiResult<(ReadTxn, Meta, Meta)> {
    for _ in 0..RETRIES {
        let capture = begin_with_transform(directory, transform.clone())?;
        let lease = visibility.map(|v| v.lease(false)).transpose()?;
        if let (Some(v), Some(lease)) = (visibility, lease.as_ref()) {
            v.read(lease)?;
        }
        let capture = match OwnedFileCapture::previous(directory, capture) {
            Ok(capture) => capture,
            Err(PreviousError::Engine(Error::Busy)) => continue,
            Err(other) => return Err(error(other)),
        };
        let durable = selected(&capture)?;
        let view = viewed(&capture)?;
        drop(lease);
        return Ok((
            admit(capture, false, advice, transform, cache.clone())?,
            durable,
            view,
        ));
    }
    Err(libc::EBUSY)
}

/// Install only after A admission; final metadata check does not repeat its graph.
pub(super) fn install(
    directory: &std::path::Path,
    visibility: &Visibility,
    read: ReadTxn,
    durable: Meta,
    view: Meta,
    advice: ulmdb::ReadAhead,
    transform: Transform,
    cache: chunks::Options,
) -> FfiResult<ReadTxn> {
    let mut admitted = Some((read, durable, view));
    for _ in 0..RETRIES {
        let (read, durable, view) = match admitted.take() {
            Some(value) => value,
            None => initial_previous(
                directory,
                Some(visibility),
                advice,
                transform.clone(),
                cache.clone(),
            )?,
        };
        let capture = begin_with_transform(directory, transform.clone())?;
        let lease = visibility.lease(true)?;
        let record = visibility.read(&lease)?;
        let current = OwnedFileCapture::current(directory, capture).map_err(code)?;
        if current.selected_checkpoint() != &durable.checkpoint {
            continue;
        }
        let current = match current.select_previous() {
            Ok(current) => current,
            Err(PreviousError::Engine(Error::Busy)) => continue,
            Err(other) => return Err(error(other)),
        };
        if current.view_checkpoint() != &view.checkpoint {
            return Err(CORRUPT);
        }
        let updated = record.updated(durable, view, Kind::Offset)?;
        visibility.store(&lease, &updated)?;
        return Ok(read);
    }
    Err(libc::EBUSY)
}

pub(super) fn transaction_snapshot(
    shared: &Shared,
    env_flags: u32,
    empty_main: bool,
) -> FfiResult<ReadTxn> {
    shared.check_process()?;
    if shared.external.is_some() {
        return external_snapshot(shared, shared.previous.active(), empty_main);
    }
    let Some(visibility) = &shared.visibility else {
        if shared.previous.active() {
            let capture = OwnedFileCapture::previous(&shared.directory, begin_shared(shared)?)
                .map_err(error)?;
            return admit_shared(shared, capture, empty_main);
        }
        return shared.native_snapshot(empty_main);
    };
    for _ in 0..RETRIES {
        let capture = begin_shared(shared)?; // Capture precedes Visibility.
        let lease = visibility.lease(false)?;
        let record = visibility.read(&lease)?;
        let mut capture = OwnedFileCapture::current(&shared.directory, capture).map_err(code)?;
        let durable = selected(&capture)?;
        if record.matches(durable)? && record.kind == Kind::Offset {
            capture = match capture.select_previous() {
                Ok(capture) => capture,
                Err(PreviousError::Engine(Error::Busy)) => continue,
                Err(PreviousError::NoPrevious) => return Err(CORRUPT),
                Err(other) => return Err(error(other)),
            };
            if capture.view_checkpoint() != &record.view {
                return Err(CORRUPT);
            }
            if capture.view_sequence() == 0 && env_flags & RDONLY != 0 && !shared.previous.active()
            {
                capture = capture.select_selected().map_err(code)?;
            }
        }
        drop(lease);
        return admit_shared(shared, capture, empty_main);
    }
    Err(libc::EBUSY)
}

pub(super) fn metadata_snapshot(
    shared: &Shared,
    previous: bool,
    empty_main: bool,
) -> FfiResult<ReadTxn> {
    shared.check_process()?;
    if shared.external.is_some() {
        return external_snapshot(shared, previous, empty_main);
    }
    if shared.visibility.is_none() && !previous {
        return shared.native_snapshot(empty_main);
    }
    for _ in 0..RETRIES {
        let capture = begin_shared(shared)?;
        let lease = shared
            .visibility
            .as_ref()
            .map(|v| v.lease(false))
            .transpose()?;
        if let (Some(v), Some(lease)) = (&shared.visibility, lease.as_ref()) {
            v.read(lease)?;
        }
        let capture = OwnedFileCapture::current(&shared.directory, capture).map_err(code)?;
        let capture = if previous {
            match capture.select_previous() {
                Ok(capture) => capture,
                Err(PreviousError::Engine(Error::Busy)) => continue,
                Err(other) => return Err(error(other)),
            }
        } else {
            capture
        };
        drop(lease);
        return admit_shared(shared, capture, empty_main);
    }
    Err(libc::EBUSY)
}

/// Called only while common WriterLease is held; returned owner keeps native LOCK.
pub(super) fn writer_owner(
    shared: Arc<Shared>,
) -> FfiResult<(Arc<rollback::WriterEnvironment>, WriteRole)> {
    shared.check_process()?;
    if shared.previous.pending() {
        let session = shared.previous.session()?.ok_or(BAD_TXN)?;
        let owner = Arc::new(rollback::WriterEnvironment::previous(shared, session)?);
        let base = owner.begin_read();
        return Ok((
            owner,
            WriteRole {
                kind: RoleKind::FirstPrevious,
                base: Some(base),
            },
        ));
    }
    let Some(visibility) = &shared.visibility else {
        return Ok((
            Arc::new(rollback::WriterEnvironment::open(shared)?),
            WriteRole::canonical(),
        ));
    };
    for _ in 0..RETRIES {
        // This first record is only a role hint. No native owner is opened while
        // holding Visibility, and the exact durable binding is checked below.
        let hint = {
            let lease = visibility.lease(false)?;
            visibility.read(&lease)?
        };
        let canonical_hint = hint.kind == Kind::Canonical;
        let hint_previous = if canonical_hint {
            // Preserve the held namespace preflight before opening a writer.
            // Release Capture before native LOCK to avoid maintenance cycles.
            let capture = begin_shared(&shared)?;
            OwnedFileCapture::check_directory(&shared.directory, &capture).map_err(code)?;
            false
        } else {
            let capture = OwnedFileCapture::current(&shared.directory, begin_shared(&shared)?)
                .map_err(code)?;
            hint.durable == *capture.selected_checkpoint()
        };
        let mut missing_previous = false;
        let owner = if hint_previous {
            match PreviousSession::open_with_cache_options(
                &shared.directory,
                ResidencyPolicy::Demand,
                shared.read_ahead(),
                shared.transform.clone(),
                shared.cache.clone(),
            ) {
                Ok(session) => Arc::new(rollback::WriterEnvironment::previous(
                    shared.clone(),
                    Arc::new(session),
                )?),
                Err(PreviousError::NoPrevious) => {
                    missing_previous = true;
                    Arc::new(rollback::WriterEnvironment::open(shared.clone())?)
                }
                Err(other) => return Err(error(other)),
            }
        } else {
            Arc::new(rollback::WriterEnvironment::open(shared.clone())?)
        };
        let base = owner.begin_read();
        // Rebind the owner's actual physical pin to Shared's original realm
        // and namespace. Keep Capture through the final Visibility checks;
        // owner admission has already authenticated its selected data.
        let _namespace = if canonical_hint {
            Some(
                base.bind_capture_namespace(&shared.directory, begin_shared(&shared)?)
                    .map_err(code)?,
            )
        } else {
            None
        };
        let lease = visibility.lease(false)?;
        let record = visibility.read(&lease)?;
        let actual = owner.selected_checkpoint().map_err(code)?;
        let wants_previous = record.kind == Kind::Offset && record.durable == actual;
        let base_meta = meta(&base)?;
        if record.uuid != base_meta.uuid || record.epoch != base_meta.epoch {
            return Err(CORRUPT);
        }
        if wants_previous {
            if missing_previous && hint.durable == actual {
                return Err(CORRUPT);
            }
            if base_meta.checkpoint != record.view {
                continue;
            }
            return Ok((
                owner,
                WriteRole {
                    kind: RoleKind::FollowOffset,
                    base: Some(base),
                },
            ));
        }
        if base_meta.checkpoint != actual {
            continue;
        }
        return Ok((owner, WriteRole::canonical()));
    }
    Err(libc::EBUSY)
}

pub(super) fn publish_record(
    shared: &Shared,
    owner: &rollback::WriterEnvironment,
    role: &WriteRole,
    lease: Option<&visibility::Lease<'_>>,
) -> FfiResult<()> {
    let Some(visibility) = &shared.visibility else {
        return Ok(());
    };
    let lease = lease.ok_or(BAD_TXN)?;
    let old = visibility.read(lease)?;
    let durable = meta(&owner.begin_read())?;
    let view = if role.first() {
        meta(role.base.as_ref().ok_or(BAD_TXN)?)?
    } else {
        durable
    };
    visibility.store(
        lease,
        &old.updated(
            durable,
            view,
            if role.first() {
                Kind::Offset
            } else {
                Kind::Canonical
            },
        )?,
    )
}

pub(super) fn finish_first(shared: &Shared, role: &WriteRole) -> FfiResult<()> {
    if role.first() {
        shared.previous.finish(shared._environment_lease.as_ref())?;
        // Clear only this role bit; concurrent NOSYNC/NOMETASYNC changes share
        // this atomic authority and must survive the first PREV publication.
        shared
            .flags
            .fetch_and(!visibility::PREVIOUS, Ordering::AcqRel);
    }
    Ok(())
}
