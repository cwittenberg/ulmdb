//! Opt-in cooperative snapshot exports. Ordinary synchronous exports keep their
//! existing path; this path meters inventory, copying, hashing and validation.
use super::control::BackupControl;
use super::*;
use crate::environment::Snapshot;

const COPY_BYTES: usize = 65_536;

/// Export the current file snapshot with cooperative cancellation and work
/// limits. A refused operation may leave an unpublished target directory;
/// cancellation never removes an existing path or publishes a partial copy.
pub fn export_with_control(
    environment: &Environment,
    target: impl AsRef<Path>,
    control: &BackupControl,
) -> Result<Summary> {
    export_snapshot_with_control(environment, &environment.begin_read(), target, control)
}

/// Export an exact committed file snapshot with caller-controlled work.
///
/// Plain mapped Legacy, Cohort and Indexed files are supported. Range, chunked,
/// externally synchronized and transformed snapshots reject before target
/// creation. Cancellation or exhausted work limits leave no selected target.
/// Once final pointer publication starts, late cancellation does not change its
/// outcome; publication errors retain the ordinary `OutcomeUnknown` contract.
pub fn export_snapshot_with_control(
    environment: &Environment,
    read: &ReadTxn,
    target: impl AsRef<Path>,
    control: &BackupControl,
) -> Result<Summary> {
    control.scope(|| {
        control.checkpoint(0, 0)?;
        environment.require_coordinated()?;
        environment.require_file_backend()?;
        if read.snapshot.owner != environment.current.load().owner {
            return Err(Error::InvalidInput(
                "snapshot belongs to another environment",
            ));
        }
        check_supported(&read.snapshot)?;
        export_files(
            &environment.directory,
            &read.snapshot,
            target.as_ref(),
            environment.replica_role,
            control,
        )
    })
}

/// Cooperatively export an independently pinned snapshot while writers run.
/// This never acquires or changes the source writer capability. The target
/// must be absent, and an interrupted unpublished target is left for its owner.
pub fn export_independent_snapshot_with_control(
    source: impl AsRef<Path>,
    read: &ReadTxn,
    target: impl AsRef<Path>,
    control: &BackupControl,
) -> Result<Summary> {
    control.scope(|| {
        control.checkpoint(0, 0)?;
        check_supported(&read.snapshot)?;
        let source = source.as_ref();
        let snapshot = &read.snapshot;
        let pin = snapshot.physical_pin.as_ref().ok_or(Error::InvalidInput(
            "independent export requires a physical pin",
        ))?;
        control.checkpoint(1, 0)?;
        if !pin.matches_environment(source, &snapshot.manifest)? {
            return Err(Error::InvalidInput(
                "physical snapshot pin belongs to another source",
            ));
        }
        if ulmdb_storage::range::is_selected(source)?
            || ulmdb_storage::read_current(source)?.uuid != snapshot.manifest.uuid
        {
            return Err(Error::InvalidInput("snapshot belongs to another source"));
        }
        control.checkpoint(1, 0)?;
        let role = crate::replication::load_role(source, &snapshot.manifest)?;
        export_files(source, snapshot, target.as_ref(), role, control)
    })
}

pub(super) fn check_supported(snapshot: &Snapshot) -> Result<()> {
    if snapshot.external.is_some()
        || snapshot.range_backed
        || snapshot.segments.is_chunked()
        || snapshot.segments.transform().is_some()
    {
        return Err(Error::InvalidInput(
            "controlled export requires ordinary untransformed mapped files",
        ));
    }
    Ok(())
}

fn export_files(
    source: &Path,
    snapshot: &Snapshot,
    target: &Path,
    role: Option<crate::replication::ReplicaIdentity>,
    control: &BackupControl,
) -> Result<Summary> {
    super::controlled_verify::validate_with_control(
        &snapshot.segments,
        &snapshot.manifest,
        true,
        control,
    )?;
    let source_summary = summary_with_control(source, &snapshot.manifest, control)?;
    control.checkpoint(1, 0)?;
    let checkpoint = ulmdb_storage::checkpoint_bytes(&snapshot.manifest)?;
    // Preflight above runs before exclusive target creation. No removal is
    // attempted on failure: an existing target can never become ours by error.
    control.checkpoint(1, 0)?;
    ulmdb_storage::access::create_layout(
        target,
        ulmdb_storage::access::FileAccessContext::open(source)?.policy(),
    )?;
    let lock = ulmdb_storage::access::options_for_entry(&target.join("LOCK"))?
        .create(false)
        .read(true)
        .write(true)
        .open(target.join("LOCK"))?;
    let _lock = ulmdb_storage::WriterFence::try_lock(lock).map_err(|_| Error::Busy)?;
    let mut changed_directories = BTreeSet::new();
    for (path, file) in physical_files_with_control(source, &snapshot.manifest, control)? {
        control.checkpoint(1, 0)?;
        let destination = copy_to_root(source, target, &path, &file, control)?;
        changed_directories.insert(
            destination
                .parent()
                .ok_or(Error::Corrupt("backup destination parent"))?
                .to_path_buf(),
        );
    }
    for directory in changed_directories {
        control.checkpoint(1, 0)?;
        ulmdb_platform::sync_path(directory)?;
    }
    // Include Legacy's exact immutable descriptor in the copy as well. This
    // permits one checked prepare_existing path without monolithic install.
    let copied_manifest = ulmdb_storage::read_checkpoint_with_transform(target, &checkpoint, None)?;
    let copied = SegmentTable::load_with_profile(
        target,
        &copied_manifest.segments,
        snapshot.segments.read_ahead(),
        None,
        copied_manifest.value_limit,
    )?;
    if ulmdb_storage::transform::check_configuration(target, None)?.is_some() {
        return Err(Error::CryptoMismatch);
    }
    super::controlled_verify::validate_with_control(&copied, &copied_manifest, true, control)?;
    let summary = summary_with_control(target, &copied_manifest, control)?;
    let mut expected = source_summary;
    // A metadata-only origin may archive between source preflight and copying.
    // Its physical footprint can shrink while the selected logical snapshot is
    // identical; the destination summary accounts its actual resolved bytes.
    expected.physical_bytes = summary.physical_bytes;
    if summary != expected {
        return Err(Error::Corrupt(
            "controlled export snapshot identity or inventory changed",
        ));
    }
    if let Some(role) = role {
        control.checkpoint(1, 64)?;
        crate::replication::write_role(target, role)?;
    }
    let ready = ulmdb_storage::prepare_existing_with_transform(target, &checkpoint, None)?
        .prepare_ready(None)?;
    // Cancellation is meaningful until selection. The final ready operation is
    // small metadata publication; never turn a committed result into Cancelled.
    // Reserve a conservative small metadata envelope, then prevent a late
    // token request from interrupting publication after pointer installation.
    // This scoped suppression does not disable control for an outer IPDon
    // manifest publication that still needs its own cancellation boundary.
    control.checkpoint(1, 4096)?;
    ulmdb_core::work::without_observer(|| ready.publish_bootstrap(crate::Durability::Full))
        .map_err(|_| Error::OutcomeUnknown)?;
    Ok(summary)
}

pub(super) fn summary_with_control(
    directory: &Path,
    manifest: &Manifest,
    control: &BackupControl,
) -> Result<Summary> {
    let mut physical_bytes = 0u64;
    for (path, file) in physical_files_with_control(directory, manifest, control)? {
        control.checkpoint(1, 0)?;
        physical_bytes = physical_bytes
            .checked_add(physical_length(&path, &file)?)
            .ok_or(Error::LimitExceeded)?;
    }
    let mut segment_bytes = 0u64;
    for entry in manifest.segments.iter() {
        control.checkpoint(1, 0)?;
        segment_bytes = segment_bytes
            .checked_add(entry.length)
            .ok_or(Error::LimitExceeded)?;
    }
    Ok(Summary {
        uuid: manifest.uuid,
        generation: manifest.generation,
        sequence: manifest.sequence,
        commit: manifest.commit,
        segments: manifest.segments.len(),
        segment_bytes,
        physical_bytes,
    })
}

fn physical_files_with_control<'a>(
    directory: &Path,
    manifest: &'a Manifest,
    control: &BackupControl,
) -> Result<BTreeMap<PathBuf, PhysicalFile<'a>>> {
    let mut files = BTreeMap::new();
    control.checkpoint(1, 0)?;
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
    // Do not clone the complete origin set before the first control callback.
    for &origin in manifest.metadata_origins_ref() {
        control.checkpoint(1, 0)?;
        files
            .entry(
                directory
                    .join("cohorts")
                    .join(format!("{origin:032x}.cohort")),
            )
            .or_insert(PhysicalFile {
                kind: PhysicalKind::MetadataOrigin(origin),
                prefix: None,
            });
    }
    for entry in manifest.segments.iter() {
        control.checkpoint(1, 0)?;
        let file = files
            .entry(ulmdb_storage::segment_path(directory, entry))
            .or_insert(PhysicalFile {
                kind: if entry.cohort == 0 {
                    PhysicalKind::LegacySegment
                } else {
                    PhysicalKind::Cohort
                },
                prefix: None,
            });
        file.kind = if entry.cohort == 0 {
            PhysicalKind::LegacySegment
        } else {
            PhysicalKind::Cohort
        };
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

fn copy_to_root(
    source_root: &Path,
    target_root: &Path,
    nominal: &Path,
    file: &PhysicalFile<'_>,
    control: &BackupControl,
) -> Result<PathBuf> {
    control.checkpoint(1, 0)?;
    let resolved = if let PhysicalKind::MetadataOrigin(origin) = file.kind {
        Some(metadata_origin_source(nominal, origin)?)
    } else {
        None
    };
    let actual_source = resolved
        .as_ref()
        .map_or(nominal, |(path, _, _)| path.as_path());
    let relative = actual_source
        .strip_prefix(source_root)
        .map_err(|_| Error::Corrupt("backup source path escapes environment"))?;
    let destination = target_root.join(relative);
    if let Some((_, input, kind)) = resolved {
        let actual = PhysicalFile { kind, prefix: None };
        let length = input.metadata()?.len();
        validate_physical_length(None, length, &actual)?;
        copy_input(input, &destination, &actual, length, control)?;
    } else {
        let length = physical_length(nominal, file)?;
        copy_input(File::open(nominal)?, &destination, file, length, control)?;
    }
    Ok(destination)
}

fn copy_input(
    mut input: File,
    target: &Path,
    file: &PhysicalFile<'_>,
    mut remaining: u64,
    control: &BackupControl,
) -> Result<()> {
    use ulmdb_platform::FileExt;
    control.checkpoint(1, 8)?;
    let mut magic = [0; 8];
    input.read_exact_at(&mut magic, 0)?;
    if &magic == b"ULMDBTX1" {
        return Err(Error::InvalidInput(
            "controlled export does not decode transformed files",
        ));
    }
    input.rewind()?;
    control.checkpoint(1, 0)?;
    let mut output = ulmdb_storage::access::options_for_entry(target)?
        .create_new(true)
        .write(true)
        .open(target)?;
    let mut prefix_remaining = file.prefix.map_or(0, |entry| entry.length);
    let mut hash = Sha256::new();
    let mut buffer = [0u8; COPY_BYTES];
    while remaining != 0 {
        let length =
            usize::try_from(remaining.min(COPY_BYTES as u64)).map_err(|_| Error::LimitExceeded)?;
        control.checkpoint(1, length as u64)?;
        let read = input.read(&mut buffer[..length])?;
        if read == 0 {
            return Err(Error::Corrupt("truncated backup physical file"));
        }
        control.checkpoint(1, read as u64)?;
        output.write_all(&buffer[..read])?;
        let prefix =
            usize::try_from(prefix_remaining.min(read as u64)).map_err(|_| Error::LimitExceeded)?;
        if prefix != 0 {
            control.checkpoint(1, prefix as u64)?;
            hash.update(&buffer[..prefix]);
        }
        prefix_remaining -= prefix as u64;
        remaining -= read as u64;
    }
    control.checkpoint(1, 1)?;
    if input.read(&mut buffer[..1])? != 0
        || prefix_remaining != 0
        || file
            .prefix
            .is_some_and(|entry| <[u8; 32]>::from(hash.finalize()) != entry.hash)
    {
        return Err(Error::Corrupt(
            "backup segment prefix hash/physical length mismatch",
        ));
    }
    #[cfg(unix)]
    {
        control.checkpoint(1, 0)?;
        let mut permissions = output.metadata()?.permissions();
        permissions.set_readonly(true);
        output.set_permissions(permissions)?;
    }
    control.checkpoint(1, 0)?;
    ulmdb_platform::sync_file(&output)?;
    Ok(())
}
