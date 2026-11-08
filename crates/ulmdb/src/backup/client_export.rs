//! Export a validated, independently pinned file snapshot without a writer fence.
use super::*;

/// Copy an independently opened committed snapshot while source writers run.
///
/// The source must have the pinned environment UUID and retain its immutable
/// files throughout copying. Concurrent removal or corruption fails validation
/// before the destination's `CURRENT` is installed. The target must be absent.
/// This API does not acquire, release or mutate the source writer capability.
pub fn export_independent_snapshot(
    source: impl AsRef<Path>,
    read: &ReadTxn,
    target: impl AsRef<Path>,
) -> Result<Summary> {
    if read.snapshot.external.is_some() {
        read.snapshot.check_external()?;
        return Err(Error::InvalidInput("external snapshot export unsupported"));
    }
    let source = source.as_ref();
    let snapshot = read.snapshot.clone();
    if let Some(pin) = &snapshot.physical_pin
        && !pin.matches_environment(source, &snapshot.manifest)?
    {
        return Err(Error::InvalidInput(
            "physical snapshot pin belongs to another source",
        ));
    }
    #[cfg(feature = "range-lifecycle")]
    if snapshot.range_backed {
        let checkpoint = snapshot
            .range_checkpoint
            .as_ref()
            .ok_or(Error::InvalidInput(
                "range export requires format6 checkpoint",
            ))?;
        if !checkpoint.matches_directory(source)? {
            return Err(Error::InvalidInput("snapshot belongs to another source"));
        }
        return super::export_range_snapshot(read, target.as_ref());
    }
    #[cfg(feature = "range-lifecycle")]
    if ulmdb_storage::range::is_selected(source)? {
        return Err(Error::InvalidInput(
            "file snapshot cannot bind range source",
        ));
    }
    #[cfg(not(feature = "range-lifecycle"))]
    if snapshot.range_backed || ulmdb_storage::range::is_selected(source)? {
        return Err(Error::InvalidInput(
            "independent range export is unsupported",
        ));
    }
    if ulmdb_storage::read_current(source)?.uuid != snapshot.manifest.uuid {
        return Err(Error::InvalidInput("snapshot belongs to another source"));
    }
    validate_file_for_admission(&snapshot.segments, &snapshot.manifest)?;
    // All source preflight happens before any destination is created.
    let summary = Summary::from_manifest(source, &snapshot.manifest)?;
    let role = crate::replication::load_role(source, &snapshot.manifest)?;
    let target = target.as_ref();
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
    for (path, file) in physical_files(source, &snapshot.manifest)? {
        if matches!(file.kind, PhysicalKind::LegacyDescriptor) {
            continue;
        }
        let destination = copy_physical_to_root(source, target, &path, &file)?;
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
    // Legacy's canonical descriptor is written only by install_snapshot below.
    // Cohort/Indexed copies already contain their exact descriptor; account the
    // copied physical namespace, including any resolved metadata-only archive.
    let summary = if snapshot.manifest.format_minor >= 3 {
        Summary::from_manifest(target, copied_manifest)?
    } else {
        summary
    };
    if let Some(role) = role {
        crate::replication::write_role(target, role)?;
    }
    if snapshot.manifest.format_minor >= 3 {
        let checkpoint = ulmdb_storage::checkpoint_bytes(&snapshot.manifest)?;
        ulmdb_storage::prepare_existing_with_transform(
            target,
            &checkpoint,
            snapshot.segments.transform().cloned(),
        )
        .and_then(|prepared| prepared.publish_bootstrap())
        .map_err(|_| Error::OutcomeUnknown)?;
    } else {
        ulmdb_storage::install_snapshot(target, &snapshot.manifest)
            .map_err(|_| Error::OutcomeUnknown)?;
    }
    Ok(summary)
}
