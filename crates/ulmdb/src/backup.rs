//! Independent, verified copies of one committed snapshot.
mod client_copy;
mod client_export;
mod controlled_export;
mod control;
pub(crate) mod controlled_verify;
mod incremental;
mod job;
#[cfg(test)]
mod archive_tests;
use crate::{
    Address, DatabaseKind, Environment, Error, Prefix, ReadTxn, Result, catalog::Database,
};
pub use client_copy::{
    NewEnvironmentCopyReport, PairCopyNormalization, PairCopyReport, compact_file_copy,
    create_empty_file_copy, create_empty_file_copy_externally_synchronized,
    create_empty_file_copy_with_access, create_empty_file_copy_with_page_size,
    create_empty_file_copy_with_transform, export_file_pair,
};
pub use client_export::export_independent_snapshot;
pub use controlled_export::{export_independent_snapshot_with_control, export_snapshot_with_control, export_with_control};
pub use control::{BackupControl, BackupLimits, BackupProgress, BackupStopReason, CancellationToken};
pub use controlled_verify::{
    verify_independent_snapshot_with_control, verify_snapshot_with_control,
    verify_with_comparators_and_control, verify_with_control,
};
pub use incremental::{IncrementalLimits, IncrementalReport};
pub use job::{
    BackupJob, JobStep, WorkBudget, export_independent_snapshot_job,
    export_snapshot_job, verify_independent_snapshot_job, verify_snapshot_job,
};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, Write},
    ops::Bound,
    path::{Path, PathBuf},
};
use ulmdb_core::storage::{ReadStore, Reference};
use ulmdb_storage::{Manifest, SegmentEntry, SegmentTable};

/// Identity and inventory of the exported commit, unchanged by export/restore.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Summary {
    pub uuid: [u8; 16],
    pub generation: u128,
    pub sequence: u64,
    pub commit: u128,
    pub segments: usize,
    pub segment_bytes: u64,
    /// Deduplicated descriptor/data/metadata physical files; excludes pointer and lock files.
    pub physical_bytes: u64,
}
impl Summary {
    fn from_manifest(directory: &Path, manifest: &Manifest) -> Result<Self> {
        Ok(Self {
            uuid: manifest.uuid,
            generation: manifest.generation,
            sequence: manifest.sequence,
            commit: manifest.commit,
            segments: manifest.segments.len(),
            physical_bytes: if ulmdb_storage::range::is_selected(directory)? {
                fs::metadata(directory.join("RANGE.pack"))?.len()
            } else {
                physical_files(directory, manifest)?.iter().try_fold(
                    0u64,
                    |sum, (path, file)| {
                        sum.checked_add(physical_length(path, file)?)
                            .ok_or(Error::LimitExceeded)
                    },
                )?
            },
            segment_bytes: manifest.segments.iter().try_fold(0u64, |sum, segment| {
                sum.checked_add(segment.length).ok_or(Error::LimitExceeded)
            })?,
        })
    }
}

/// Validate the reachable logical indexes of one pinned committed snapshot.
/// Segment file hashes are checked when its mappings are loaded. This does not
/// authorize external writes to files behind live immutable mappings.
pub fn verify(environment: &Environment) -> Result<Summary> {
    let snapshot = environment.current.load_full();
    validate(&snapshot.segments, &snapshot.manifest)?;
    Summary::from_manifest(&environment.directory, &snapshot.manifest)
}

/// Fully verify the exact committed file snapshot owned by an independent pin.
/// Concurrent publishers may advance CURRENT. Fresh shared Capture exclusion
/// keeps this snapshot's physical assets available throughout validation and
/// summary construction; no source writer capability is acquired or changed.
/// Custom ordering retains the same explicit-registration requirement as verify.
pub fn verify_independent_snapshot(source: impl AsRef<Path>, read: &ReadTxn) -> Result<Summary> {
    let snapshot = &read.snapshot;
    snapshot.check_external()?;
    if snapshot.range_backed || snapshot.external.is_some() {
        return Err(Error::InvalidInput(
            "independent verification requires coordinated files",
        ));
    }
    let pin = snapshot.physical_pin.as_ref().ok_or(Error::InvalidInput(
        "independent verification requires a physical pin",
    ))?;
    // The coordinator checks creator PID before any path lookup or gate wait.
    let capture = pin.coordination().try_capture()?;
    let source = source.as_ref().canonicalize()?;
    if !pin.matches_environment(&source, &snapshot.manifest)? {
        return Err(Error::InvalidInput(
            "physical snapshot pin belongs to another source",
        ));
    }
    validate(&snapshot.segments, &snapshot.manifest)?;
    let summary = Summary::from_manifest(&source, &snapshot.manifest)?;
    if !pin.matches_environment(&source, &snapshot.manifest)? {
        return Err(Error::InvalidInput(
            "physical snapshot pin belongs to another source",
        ));
    }
    drop(capture);
    Ok(summary)
}

/// Full structural and declared-order audit using caller-supplied custom ordering.
/// No callback is persisted; callers must register the same ordering on every use.
pub fn verify_with_comparators(
    environment: &Environment,
    callbacks: &BTreeMap<Vec<u8>, crate::ordering::DatabaseComparators>,
) -> Result<Summary> {
    let snapshot = environment.current.load_full();
    if snapshot.segments.is_chunked() {
        crate::chunked::admission::admit_with_policy(
            &snapshot.segments,
            &snapshot.manifest,
            chunk_budget(&snapshot.segments),
            false,
            callbacks,
        )?;
    } else {
        validate_with_policy(&snapshot.segments, &snapshot.manifest, false, callbacks)?;
    }
    Summary::from_manifest(&environment.directory, &snapshot.manifest)
}

/// Pin the current committed snapshot once, then export while writers continue.
pub fn export(environment: &Environment, target: impl AsRef<Path>) -> Result<Summary> {
    export_snapshot(environment, &environment.begin_read(), target)
}

/// Export a specific older read transaction from this environment.
/// The transaction pins the complete inventory throughout validation and copying.
pub fn export_snapshot(
    environment: &Environment,
    read: &ReadTxn,
    target: impl AsRef<Path>,
) -> Result<Summary> {
    #[cfg(feature = "range-lifecycle")]
    if read.snapshot.range_backed {
        if read.snapshot.owner != environment.current.load().owner {
            return Err(Error::InvalidInput(
                "snapshot belongs to another environment",
            ));
        }
        return export_range_snapshot(read, target.as_ref());
    }
    environment.require_coordinated()?;
    environment.require_file_backend()?;
    let snapshot = read.snapshot.clone();
    if snapshot.owner != environment.current.load().owner {
        return Err(Error::InvalidInput(
            "snapshot belongs to another environment",
        ));
    }
    validate_file_for_admission(&snapshot.segments, &snapshot.manifest)?;
    let source_summary = Summary::from_manifest(&environment.directory, &snapshot.manifest)?;
    let target = target.as_ref();
    // Exclusive creation also rejects empty directories, files and symlinks.
    ulmdb_storage::access::create_layout(
        target,
        ulmdb_storage::access::FileAccessContext::open(&environment.directory)?.policy(),
    )?;
    let lock = ulmdb_storage::access::options_for_entry(&target.join("LOCK"))?
        .create(false)
        .read(true)
        .write(true)
        .open(target.join("LOCK"))?;
    let _lock = ulmdb_storage::WriterFence::try_lock(lock).map_err(|_| Error::Busy)?;
    let mut changed_directories = BTreeSet::new();
    for (source, file) in physical_files(&environment.directory, &snapshot.manifest)? {
        // Legacy install serializes its canonical descriptor. Cohort install
        // validates/reuses the exact immutable file copied here.
        if matches!(file.kind, PhysicalKind::LegacyDescriptor) {
            continue;
        }
        let destination = copy_physical_to_root(&environment.directory, target, &source, &file)?;
        changed_directories.insert(
            destination
                .parent()
                .ok_or(Error::Corrupt("backup destination parent"))?
                .to_path_buf(),
        );
    }
    for directory in changed_directories {
        ulmdb_platform::sync_path(directory)?;
    }
    // Validate copied bytes and every reachable index before CURRENT can exist.
    // Re-decode the selected root at its destination before publication. For
    // Indexed roots this authenticates every copied metadata node, including
    // cohorts that contain no live data segment prefix.
    let decoded = if snapshot.manifest.format_minor >= 3 {
        Some(ulmdb_storage::read_checkpoint_with_transform(
            target,
            &ulmdb_storage::checkpoint_bytes(&snapshot.manifest)?,
            snapshot.segments.transform().cloned(),
        )?)
    } else {
        None
    };
    let copied_manifest = decoded.as_ref().unwrap_or(&snapshot.manifest);
    let copied = if snapshot.segments.is_chunked() {
        crate::environment::load_snapshot_like(
            target,
            copied_manifest.clone(),
            snapshot.owner,
            &snapshot.segments,
        )?
        .segments
    } else {
        SegmentTable::load_with_profile(
            target,
            &copied_manifest.segments,
            snapshot.segments.read_ahead(),
            snapshot.segments.transform().cloned(),
            copied_manifest.value_limit,
        )?
    };
    if ulmdb_storage::transform::check_configuration(target, snapshot.segments.transform())?
        .is_some_and(|uuid| uuid != copied_manifest.uuid)
    {
        return Err(Error::CryptoMismatch);
    }
    validate_file_for_admission(&copied, copied_manifest)?;
    // The selected logical identity is unchanged even if a metadata-only
    // origin moved to an archive during the copy. Account the actual copy.
    let summary = if copied_manifest.format_minor >= 3 {
        Summary::from_manifest(target, copied_manifest)?
    } else {
        // Legacy's canonical descriptor is installed only during publication.
        source_summary
    };
    if let Some(role) = environment.replica_role {
        crate::replication::write_role(target, role)?;
    }
    // Once pointer installation starts, a sync error may leave a complete
    // visible CURRENT whose durability is uncertain. Match commit semantics.
    if snapshot.manifest.format_minor >= 3 {
        // Exact archived metadata dependencies live in commits/, so this is a
        // verified immutable-file import rather than an empty-commits install.
        // All copied data, metadata and their directories were fenced above;
        // prepare_existing rechecks the destination descriptor before selection.
        let checkpoint = ulmdb_storage::checkpoint_bytes(&snapshot.manifest)?;
        ulmdb_storage::prepare_existing_with_transform(
            target, &checkpoint, snapshot.segments.transform().cloned(),
        ).and_then(|prepared| prepared.publish_bootstrap())
            .map_err(|_| Error::OutcomeUnknown)?;
    } else {
        ulmdb_storage::install_snapshot(target, &snapshot.manifest)
            .map_err(|_| Error::OutcomeUnknown)?;
    }
    Ok(summary)
}

/// Verify an exported directory, then restore an independent, writable engine.
/// The source is exclusively opened for the operation; target must not exist.
pub fn restore(source: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<Summary> {
    if ulmdb_storage::range::is_selected(source.as_ref())? {
        return Err(Error::InvalidInput("range restore pending PACK-03"));
    }
    let environment = Environment::open(source)?;
    export(&environment, target)
}

#[derive(Clone, Copy)]
enum PhysicalKind {
    LegacySegment,
    LegacyDescriptor,
    Cohort,
    MetadataOrigin(u128),
    MetadataArchive,
    TransformConfiguration,
}
struct PhysicalFile<'a> {
    kind: PhysicalKind,
    prefix: Option<&'a SegmentEntry>,
}

// Segment origins are physical locations, not roots to recursively activate.
// The selected root's cohort is included even when it has no segment prefix.
fn physical_files<'a>(
    directory: &Path,
    manifest: &'a Manifest,
) -> Result<BTreeMap<PathBuf, PhysicalFile<'a>>> {
    let mut files = BTreeMap::new();
    if directory.join("TRANSFORM").try_exists()? {
        files.insert(
            directory.join("TRANSFORM"),
            PhysicalFile {
                kind: PhysicalKind::TransformConfiguration,
                prefix: None,
            },
        );
    }
    files.insert(
        ulmdb_storage::descriptor_path(directory, manifest),
        PhysicalFile {
            kind: if manifest.format_minor >= 3 {
                PhysicalKind::Cohort
            } else {
                PhysicalKind::LegacyDescriptor
            },
            prefix: None,
        },
    );
    for origin in manifest.metadata_origins() {
        let path = directory.join("cohorts").join(format!("{origin:032x}.cohort"));
        files
            .entry(path)
            .or_insert(PhysicalFile {
                kind: PhysicalKind::MetadataOrigin(origin),
                prefix: None,
            });
    }
    for entry in &manifest.segments {
        let path = ulmdb_storage::segment_path(directory, entry);
        let file = files.entry(path).or_insert(PhysicalFile {
            kind: if entry.cohort == 0 {
                PhysicalKind::LegacySegment
            } else {
                PhysicalKind::Cohort
            },
            prefix: None,
        });
        // A real data prefix can never be replaced by a metadata-only archive.
        file.kind = if entry.cohort == 0 { PhysicalKind::LegacySegment } else { PhysicalKind::Cohort };
        if file.prefix.is_some_and(|prior| {
            prior.id != entry.id || prior.length != entry.length || prior.hash != entry.hash
        }) {
            return Err(Error::Corrupt(
                "conflicting segments share one physical backup file",
            ));
        }
        file.prefix = Some(entry);
    }
    Ok(files)
}

fn metadata_origin_source(cohort: &Path, origin: u128) -> Result<(PathBuf, File, PhysicalKind)> {
    let directory = cohort.parent().and_then(Path::parent)
        .ok_or(Error::Corrupt("backup metadata origin parent"))?;
    let once = || {
        // Storage admits the exact header/extent and never falls through from
        // a present damaged original. Only one archival transition can occur.
        let source = ulmdb_storage::open_descriptor_generation(directory, origin, 4)?;
        if source.format_minor != 4 {
            return Err(Error::Corrupt("backup metadata origin format"));
        }
        let held = ulmdb_platform::file_identity(&source.file)?;
        let (path, kind) = match ulmdb_platform::path_identity(cohort) {
            Ok(named) => {
                if !named.is_file() || !held.is_file() || !named.same_object(&held)
                    || named.length != held.length {
                    return Err(Error::Corrupt("backup metadata origin replaced"));
                }
                (cohort.to_path_buf(), PhysicalKind::Cohort)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let archive = directory.join("commits").join(format!("{origin:032x}.history"));
                let named = ulmdb_platform::path_identity(&archive)?;
                if !named.is_file() || !held.is_file() || !named.same_object(&held)
                    || named.length != held.length {
                    // The cohort may have moved after its descriptor was
                    // opened. Retry through storage to select its archive.
                    return Err(Error::Io(std::io::Error::from(std::io::ErrorKind::NotFound)));
                }
                (archive, PhysicalKind::MetadataArchive)
            }
            Err(error) => return Err(error.into()),
        };
        Ok((path, source.file, kind))
    };
    match once() {
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => once(),
        result => result,
    }
}

fn physical_length(path: &Path, file: &PhysicalFile<'_>) -> Result<u64> {
    if let PhysicalKind::MetadataOrigin(origin) = file.kind {
        let (_, input, kind) = metadata_origin_source(path, origin)?;
        let length = input.metadata()?.len();
        validate_physical_length(None, length, &PhysicalFile { kind, prefix: None })?;
        return Ok(length);
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(Error::Corrupt("nonregular backup source"));
    }
    let length = metadata.len();
    validate_physical_length(Some(path), length, file)?;
    Ok(length)
}
fn validate_physical_length(source_path: Option<&Path>, length: u64, file: &PhysicalFile<'_>) -> Result<()> {
    let valid = match file.kind {
        PhysicalKind::TransformConfiguration => (128..=8192).contains(&length),
        PhysicalKind::LegacyDescriptor => (192..=67_109_056).contains(&length),
        PhysicalKind::LegacySegment => file.prefix.is_some_and(|entry| {
            (4096..=ulmdb_core::ValueLimit::MAXIMUM.segment_limit()).contains(&entry.length)
                && (length == entry.length
                    || source_path.and_then(|path| File::open(path).ok())
                        .and_then(|f| ulmdb_storage::transform::encoded_geometry(&f).ok())
                        .is_some_and(|(logical, encoded)| {
                            logical == entry.length && encoded == length
                        }))
        }),
        PhysicalKind::Cohort => {
            (224..=ulmdb_storage::MAX_COHORT_BYTES).contains(&length)
                && file.prefix.is_none_or(|entry| {
                    (4096..=ulmdb_core::ValueLimit::MAXIMUM.segment_limit()).contains(&entry.length)
                        && entry
                            .length
                            .checked_add(224)
                            .is_some_and(|minimum| length >= minimum)
                })
        }
        PhysicalKind::MetadataArchive => {
            let max = ulmdb_storage::MAX_DESCRIPTOR_BYTES;
            (320..=max + max / 4 + 8192 + 128).contains(&length)
                && file.prefix.is_none()
        }
        PhysicalKind::MetadataOrigin(_) => return Err(Error::Corrupt("unresolved metadata origin")),
    };
    if !valid {
        return Err(Error::Corrupt("invalid backup physical file bounds"));
    }
    Ok(())
}
// New copy admission rejects static nonregular assets before ordinary mapping
// opens. This is descriptor/inventory preflight, not a logical graph audit.
pub(crate) fn preflight_copy_assets(directory: &Path, manifest: &Manifest) -> Result<()> {
    for (path, file) in physical_files(directory, manifest)? {
        physical_length(&path, &file)?;
    }
    Ok(())
}
fn copy_physical(source: &Path, target: &Path, file: &PhysicalFile<'_>) -> Result<()> {
    let remaining = physical_length(source, file)?;
    copy_physical_input(File::open(source)?, target, file, remaining)
}
fn copy_physical_to_root(source_root: &Path, target_root: &Path, nominal: &Path, file: &PhysicalFile<'_>) -> Result<PathBuf> {
    // Retain one checked origin descriptor while copying it. Concurrent
    // archival can retire its pathname without blocking native maintenance.
    let resolved = if let PhysicalKind::MetadataOrigin(origin) = file.kind {
        Some(metadata_origin_source(nominal, origin)?)
    } else { None };
    let actual_source = resolved.as_ref().map_or(nominal, |(path, _, _)| path.as_path());
    let relative = actual_source.strip_prefix(source_root)
        .map_err(|_| Error::Corrupt("backup source path escapes environment"))?;
    let destination = target_root.join(relative);
    if let Some((_, input, kind)) = resolved {
        let actual = PhysicalFile { kind, prefix: None };
        let length = input.metadata()?.len();
        validate_physical_length(None, length, &actual)?;
        copy_physical_input(input, &destination, &actual, length)?;
    } else {
        copy_physical(nominal, &destination, file)?;
    }
    Ok(destination)
}
fn copy_physical_input(mut input: File, target: &Path, file: &PhysicalFile<'_>, mut remaining: u64) -> Result<()> {
    input.rewind()?;
    let mut prefix_remaining = file.prefix.map_or(0, |entry| entry.length);
    let mut magic = [0; 8];
    use ulmdb_platform::FileExt;
    input.read_exact_at(&mut magic, 0)?;
    let transformed = &magic == b"ULMDBTX1";
    if transformed {
        prefix_remaining = 0;
    }
    let mut output = ulmdb_storage::access::options_for_entry(target)?
        .create_new(true)
        .write(true)
        .open(target)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 131_072];
    while remaining != 0 {
        let length = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| Error::LimitExceeded)?;
        let read = input.read(&mut buffer[..length])?;
        if read == 0 {
            return Err(Error::Corrupt("truncated backup physical file"));
        }
        output.write_all(&buffer[..read])?;
        let prefix =
            usize::try_from(prefix_remaining.min(read as u64)).map_err(|_| Error::LimitExceeded)?;
        hash.update(&buffer[..prefix]);
        prefix_remaining -= prefix as u64;
        remaining -= read as u64;
    }
    if input.read(&mut buffer[..1])? != 0
        || prefix_remaining != 0
        || !transformed
            && file
                .prefix
                .is_some_and(|entry| <[u8; 32]>::from(hash.finalize()) != entry.hash)
    {
        return Err(Error::Corrupt(
            "backup segment prefix hash/physical length mismatch",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = output.metadata()?.permissions();
        permissions.set_readonly(true);
        output.set_permissions(permissions)?;
    }
    ulmdb_platform::sync_file(&output)?;
    Ok(())
}

fn cursor<S: ReadStore>(store: &S, root: Reference) -> Result<ulmdb_kv::Cursor<'_, S>> {
    ulmdb_kv::Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, false)
}

// A corrupt DAG can revisit empty subtrees exponentially even when page levels
// strictly decrease. Charge every checked read, including point-lookup routing
// verification, against a finite inventory-derived work allowance.
struct ValidationStore<'a, S: ReadStore> {
    inner: &'a S,
    remaining: Cell<u64>,
    page_size: usize,
    value_limit: usize,
}
impl<S: ReadStore> ReadStore for ValidationStore<'_, S> {
    fn max_value_size(&self) -> usize {
        self.value_limit
    }
    fn page_size(&self) -> usize {
        self.page_size
    }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        let remaining = self
            .remaining
            .get()
            .checked_sub(1)
            .ok_or(Error::Corrupt("backup validation read budget exhausted"))?;
        self.remaining.set(remaining);
        self.inner.read(at, length)
    }
}
#[derive(Debug)]
pub(crate) struct ValidationWork {
    reads: u64,
    max_tree_pages: u64,
}
#[derive(Clone, Copy)]
struct ValidationBudget {
    reads: u64,
    tree_pages: u64,
}
impl ValidationBudget {
    fn for_manifest(manifest: &Manifest) -> Result<Self> {
        let bytes = manifest.segments.iter().try_fold(0u64, |total, segment| {
            total
                .checked_add(segment.length)
                .ok_or(Error::LimitExceeded)
        })?;
        Self::for_geometry(bytes, manifest.page_size.get())
    }
    fn for_bytes(bytes: u64) -> Result<Self> {
        Self::for_geometry(bytes, 4096)
    }
    fn for_geometry(bytes: u64, page_size: usize) -> Result<Self> {
        Ok(Self {
            reads: bytes
                .checked_mul(64)
                .and_then(|n| n.checked_add(4096))
                .ok_or(Error::LimitExceeded)?,
            tree_pages: bytes
                .checked_div(page_size as u64)
                .and_then(|n| n.checked_add(1))
                .and_then(|n| n.checked_mul(64))
                .ok_or(Error::LimitExceeded)?,
        })
    }
}
impl ValidationWork {
    // This answers only a work-bound question. Same immutable mappings and root
    // context are supplied by prune's private same-call projection constructor.
    pub(crate) fn fits(&self, manifest: &Manifest) -> Result<bool> {
        let budget = ValidationBudget::for_manifest(manifest)?;
        Ok(self.reads <= budget.reads && self.max_tree_pages <= budget.tree_pages)
    }
}
pub(crate) fn validate(store: &SegmentTable, manifest: &Manifest) -> Result<()> {
    if store.is_chunked() {
        return crate::chunked::admission::admit_with_policy(
            store,
            manifest,
            chunk_budget(store),
            false,
            &BTreeMap::new(),
        )
        .map(|_| ());
    }
    validate_store(store, manifest)
}
fn chunk_budget(store: &SegmentTable) -> crate::chunked::AdmissionLimits {
    store
        .chunk_budget()
        .map(
            |(reads, nodes, metadata_bytes)| crate::chunked::AdmissionLimits {
                reads,
                nodes,
                metadata_bytes,
            },
        )
        .unwrap_or_default()
}

pub(crate) fn validate_store<S: ReadStore>(store: &S, manifest: &Manifest) -> Result<()> {
    validate_store_measured(store, manifest).map(|_| ())
}
pub(crate) fn validate_store_measured<S: ReadStore>(
    store: &S,
    manifest: &Manifest,
) -> Result<ValidationWork> {
    validate_with_policy(store, manifest, false, &BTreeMap::new())
}
pub(crate) fn validate_file_for_admission(store: &SegmentTable, manifest: &Manifest) -> Result<()> {
    if store.is_chunked() {
        let mut budget = crate::chunked::AdmissionLimits::default();
        if let Some((reads, nodes, metadata_bytes)) = store.chunk_budget() {
            budget = crate::chunked::AdmissionLimits {
                reads,
                nodes,
                metadata_bytes,
            };
        }
        crate::chunked::admission::admit(store, manifest, budget).map(|_| ())
    } else {
        validate_for_admission(store, manifest)
    }
}
pub(crate) fn validate_for_admission<S: ReadStore>(store: &S, manifest: &Manifest) -> Result<()> {
    validate_for_admission_measured(store, manifest).map(|_| ())
}
pub(crate) fn validate_for_admission_measured<S: ReadStore>(
    store: &S,
    manifest: &Manifest,
) -> Result<ValidationWork> {
    validate_with_policy(store, manifest, true, &BTreeMap::new())
}
fn validate_with_policy<S: ReadStore>(
    store: &S,
    manifest: &Manifest,
    structural_custom: bool,
    callbacks: &BTreeMap<Vec<u8>, crate::ordering::DatabaseComparators>,
) -> Result<ValidationWork> {
    if let Some(observer) = ulmdb_core::work::current_observer() {
        return controlled_verify::validate_store_with_observer(
            store, manifest, structural_custom, callbacks, observer.as_ref(),
        );
    }
    let budget = ValidationBudget::for_manifest(manifest)?;
    let checked = ValidationStore {
        inner: store,
        remaining: Cell::new(budget.reads),
        page_size: manifest.page_size.get(),
        value_limit: manifest.value_limit.get(),
    };
    let max_tree_pages = validate_indexes(
        &checked,
        manifest.catalog,
        manifest.next_database,
        budget.tree_pages,
        structural_custom,
        callbacks,
    )?;
    Ok(ValidationWork {
        reads: budget.reads - checked.remaining.get(),
        max_tree_pages,
    })
}
pub(crate) fn validate_materialized_graph<S: ReadStore>(
    store: &S,
    catalog: Reference,
    next_database: u64,
    image_bytes: u64,
) -> Result<()> {
    let budget = ValidationBudget::for_bytes(image_bytes)?;
    let checked = ValidationStore {
        inner: store,
        remaining: Cell::new(budget.reads),
        page_size: store.page_size(),
        value_limit: store.max_value_size(),
    };
    validate_indexes(
        &checked,
        catalog,
        next_database,
        budget.tree_pages,
        true,
        &BTreeMap::new(),
    )
    .map(|_| ())
}
struct ValueBoundStore<'a, S: ReadStore> {
    inner: &'a S,
    limit: usize,
}
impl<S: ReadStore> ReadStore for ValueBoundStore<'_, S> {
    fn page_size(&self) -> usize {
        self.inner.page_size()
    }
    fn max_value_size(&self) -> usize {
        self.limit
    }
    fn read(&self, r: Reference, n: usize) -> Result<&[u8]> {
        self.inner.read(r, n)
    }
}
fn validate_indexes<S: ReadStore>(
    store: &S,
    catalog: Reference,
    next_database: u64,
    page_budget: u64,
    structural_custom: bool,
    callbacks: &BTreeMap<Vec<u8>, crate::ordering::DatabaseComparators>,
) -> Result<u64> {
    let mut max_tree_pages = ulmdb_kv::audit_tree(store, catalog, page_budget)?.pages;
    let mut identities = BTreeSet::new();
    let capacity = crate::catalog_profile::read(store, catalog)?;
    let mut databases = 0usize;
    let mut prior = None;
    for item in cursor(store, catalog)? {
        let (name, bytes) = item?;
        if name.is_empty() {
            crate::CatalogCapacity::decode_marker(bytes)?;
            continue;
        }
        if name.len() > 255
            || prior.is_some_and(|p: &[u8]| p >= name)
            || databases >= capacity.get()
        {
            return Err(Error::Corrupt("invalid backup catalog ordering/name"));
        }
        prior = Some(name);
        databases += 1;
        let db = Database::decode_for_profile(
            bytes,
            crate::PageSize::new(store.page_size())?,
            crate::ValueLimit::new(store.max_value_size())?,
        )?;
        if db.id >= next_database
            || !identities.insert(db.id)
            || ulmdb_kv::get(store, catalog, name)? != Some(bytes)
        {
            return Err(Error::Corrupt("invalid backup catalog identity/routing"));
        }
        let limited = ValueBoundStore {
            inner: store,
            limit: Database::declared_value_limit(bytes)?.get(),
        };
        let store = &limited;
        match db.kind {
            DatabaseKind::Kv | DatabaseKind::Duplicates { .. } => {
                let pages = if db.ordering == Default::default() {
                    match db.kind {
                        DatabaseKind::Kv => validate_kv(store, db.root, page_budget)?,
                        DatabaseKind::Duplicates { width } => {
                            validate_duplicates(store, db.root, width, page_budget)?
                        }
                        _ => unreachable!(),
                    }
                } else {
                    let default = crate::ordering::DatabaseComparators::default();
                    crate::ordering_audit::database(
                        store,
                        &db,
                        page_budget,
                        callbacks.get(name).unwrap_or(&default),
                        structural_custom,
                    )?
                };
                max_tree_pages = max_tree_pages.max(pages);
            }
            DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => validate_prefix(store, db.root, db.kind)?,
        }
    }
    Ok(max_tree_pages)
}
fn validate_kv<S: ReadStore>(store: &S, root: Reference, page_budget: u64) -> Result<u64> {
    let tree_pages = ulmdb_kv::audit_tree(store, root, page_budget)?.pages;
    let mut prior = None;
    for item in cursor(store, root)? {
        let (key, value) = item?;
        if key.len() > store.max_key_size()
            || value.len() > store.max_value_size()
            || prior.is_some_and(|p: &[u8]| p >= key)
            || ulmdb_kv::get(store, root, key)? != Some(value)
        {
            return Err(Error::Corrupt("invalid backup KV ordering/size/routing"));
        }
        prior = Some(key);
    }
    Ok(tree_pages)
}
fn validate_duplicates<S: ReadStore>(
    store: &S,
    root: Reference,
    width: Option<u32>,
    page_budget: u64,
) -> Result<u64> {
    crate::duplicates::audit_buckets(store, root, width, page_budget)
}
fn validate_prefix<S: ReadStore>(store: &S, root: Reference, kind: DatabaseKind) -> Result<()> {
    let address = if kind == DatabaseKind::Ipv4 {
        Address::V4(0)
    } else {
        Address::V6(0)
    };
    let all = Prefix::new(address, 0).map_err(|_| Error::Corrupt("backup prefix family"))?;
    let mut count = 0u64;
    for item in ulmdb_prefix::subnets(store, root, all)? {
        let (prefix, value) = item?;
        if value.len() > store.max_value_size()
            || ulmdb_prefix::exact(store, root, prefix)? != Some((prefix, value))
        {
            return Err(Error::Corrupt("invalid backup prefix value/routing"));
        }
        count = count.checked_add(1).ok_or(Error::LimitExceeded)?;
    }
    if !root.is_null() && count == 0 {
        return Err(Error::Corrupt("empty/wrong-family backup prefix root"));
    }
    Ok(())
}

#[cfg(test)]
mod measured_validation_tests {
    use super::*;
    struct Counting<'a> {
        inner: &'a SegmentTable,
        calls: Cell<u64>,
    }
    impl ReadStore for Counting<'_> {
        fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
            self.calls.set(self.calls.get() + 1);
            self.inner.read(at, length)
        }
    }
    #[test]
    fn measured_work_counts_every_read_and_reduced_global_budget_is_independent_of_page_budget() {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-measured-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let environment = Environment::create(&path).unwrap();
        let mut write = environment.begin_write().unwrap();
        write.create_database(b"kv", DatabaseKind::Kv).unwrap();
        write
            .create_database(b"duplicates", DatabaseKind::Duplicates { width: None })
            .unwrap();
        for key in 0..3000u64 {
            write
                .put(b"kv", &key.to_be_bytes(), &key.to_le_bytes())
                .unwrap();
        }
        for value in 0..100u64 {
            write
                .insert_duplicate(b"duplicates", b"tree", &value.to_be_bytes())
                .unwrap();
        }
        write.commit().unwrap();
        let base = environment.current.load_full();
        let counting = Counting {
            inner: &base.segments,
            calls: Cell::new(0),
        };
        let work = validate_store_measured(&counting, &base.manifest).unwrap();
        assert_eq!(
            work.reads,
            counting.calls.get(),
            "every delegated read is charged exactly once"
        );
        assert!(work.fits(&base.manifest).unwrap());
        assert!(work.reads > 4096);
        assert!(work.max_tree_pages <= 64);
        // Deliberately test the budget guard in isolation: these are the empty
        // inventory's old limits, not a purported valid projection certificate.
        let reduced = base.manifest.clone_metadata();
        assert!(
            !work.fits(&reduced).unwrap(),
            "global reads alone exceed the reduced allowance"
        );
        drop(base);
        drop(environment);
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[cfg(feature = "range-lifecycle")]
fn export_range_snapshot(read: &ReadTxn, target: &Path) -> Result<Summary> {
    let snapshot = &read.snapshot;
    let checkpoint = snapshot
        .range_checkpoint
        .as_ref()
        .ok_or(Error::InvalidInput(
            "range export requires format6 checkpoint",
        ))?;
    if !checkpoint.matches_manifest(&snapshot.manifest) {
        return Err(Error::Corrupt("range snapshot authority mismatch"));
    }
    let controller =
        ulmdb_storage::ResidencyController::new(ulmdb_storage::ResidencyPolicy::Demand)?;
    let asset = checkpoint.export_checked(target, &controller, |table, manifest| {
        validate(table, manifest)
    })?;
    let segment_bytes = snapshot
        .manifest
        .segments
        .iter()
        .try_fold(0u64, |sum, entry| {
            sum.checked_add(entry.length).ok_or(Error::LimitExceeded)
        })?;
    Ok(Summary {
        uuid: snapshot.manifest.uuid,
        generation: snapshot.manifest.generation,
        sequence: snapshot.manifest.sequence,
        commit: snapshot.manifest.commit,
        segments: snapshot.manifest.segments.len(),
        segment_bytes,
        physical_bytes: asset.committed_bytes,
    })
}
