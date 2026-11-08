//! Explicit client-copy identity semantics; generic export/compact are unchanged.
use super::*;
use crate::{FileCopyPair, FileCopyView, PublicationMode};
use ulmdb_storage::{BootstrapPairTarget, PreparedPublication};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PairCopyNormalization {
    /// The captured source was a valid standalone snapshot whose declared
    /// physical predecessor was genuinely unavailable. No predecessor is minted.
    UnavailablePredecessor,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairCopyReport {
    pub source_selected: Summary,
    pub source_selected_checkpoint: [u8; 64],
    /// The actual persisted target root, potentially physically normalized.
    pub selected: Summary,
    pub predecessor: Option<Summary>,
    pub selected_checkpoint: [u8; 64],
    pub predecessor_checkpoint: Option<[u8; 64]>,
    pub normalization: Option<PairCopyNormalization>,
    pub physical_files: usize,
    pub physical_bytes: u64,
}
fn normalized_standalone(source: &Manifest) -> Result<Manifest> {
    let mut claimed = source.metadata_origins();
    claimed.extend(source.segments.iter().map(|e| e.cohort).filter(|g| *g != 0));
    claimed.insert(source.generation);
    claimed.insert(source.previous);
    let generation = (0..16)
        .map(|_| ulmdb_storage::new_identity())
        .find_map(|r| match r {
            Ok(g) if claimed.contains(&g) => None,
            other => Some(other),
        })
        .ok_or(Error::LimitExceeded)??;
    let mut manifest = source.clone();
    manifest.generation = generation;
    manifest.previous = 0;
    if manifest.format_minor == 4 {
        // Remove source-only metadata origins with a genuine complete index
        // rebuild. User data roots, IDs and allocator floors are unchanged.
        manifest.prepare_indexed(None)?;
    }
    Ok(manifest)
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewEnvironmentCopyReport {
    pub source_checkpoint: Option<[u8; 64]>,
    pub selected: Summary,
    pub selected_checkpoint: [u8; 64],
    pub genesis_checkpoint: [u8; 64],
    pub database_id_floor: u64,
}
fn point(name: &str) -> Result<()> {
    #[cfg(feature = "fault-injection")]
    ulmdb_storage::injected_fault(name)?;
    #[cfg(not(feature = "fault-injection"))]
    let _ = name;
    Ok(())
}
struct Asset {
    kind: PhysicalKind,
    length: u64,
    prefixes: Vec<SegmentEntry>,
}
fn inventory(directory: &Path, roots: &[&Manifest]) -> Result<BTreeMap<PathBuf, Asset>> {
    // File formats2/3/4 seal immutable files. A data header binds its exact
    // segment ID/total length/hash; two different valid append prefixes of one
    // inode cannot occur. A cohort can also be an origin/descriptor-only asset;
    // dedupe those roles while retaining its complete file and prefix proof.
    // This does not extend the contract to append/Range mutable file families.
    let mut assets = BTreeMap::<PathBuf, Asset>::new();
    for manifest in roots {
        for (path, file) in physical_files(directory, manifest)? {
            let (path, file, length) = if let PhysicalKind::MetadataOrigin(origin) = file.kind {
                // Resolve metadata-only roles before unioning D/P inventories.
                // A present original remains a Cohort and can share the same
                // asset with a data prefix from the other root. An archive can
                // never satisfy a data role, which retains its original path.
                let (path, input, kind) = metadata_origin_source(&path, origin)?;
                let file = PhysicalFile { kind, prefix: None };
                let length = input.metadata()?.len();
                validate_physical_length(None, length, &file)?;
                (path, file, length)
            } else {
                let length = physical_length(&path, &file)?;
                (path, file, length)
            };
            let asset = assets.entry(path).or_insert_with(|| Asset {
                kind: file.kind,
                length,
                prefixes: Vec::new(),
            });
            if asset.length != length
                || std::mem::discriminant(&asset.kind) != std::mem::discriminant(&file.kind)
            {
                return Err(Error::Corrupt("copy physical union conflict"));
            }
            if let Some(entry) = file.prefix {
                if asset.prefixes.iter().any(|old| {
                    old.id == entry.id && (old.length != entry.length || old.hash != entry.hash)
                }) {
                    return Err(Error::Corrupt("copy segment union conflict"));
                }
                if !asset
                    .prefixes
                    .iter()
                    .any(|old| old.length == entry.length && old.hash == entry.hash)
                {
                    asset.prefixes.push(entry.clone());
                }
            }
        }
    }
    Ok(assets)
}
fn copy_asset(source: &Path, target: &Path, asset: &Asset) -> Result<()> {
    point("copy.asset.before_open")?;
    let before = fs::symlink_metadata(source)?;
    if !before.is_file() || before.len() != asset.length {
        return Err(Error::Corrupt("copy source asset type/length"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut input = options.open(source)?;
    let actual = input.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (actual.dev(), actual.ino()) != (before.dev(), before.ino()) {
            return Err(Error::Corrupt("copy source asset replaced"));
        }
    }
    if !actual.is_file() || actual.len() != asset.length {
        return Err(Error::Corrupt("copy source asset changed"));
    }
    let mut output = ulmdb_storage::access::options_for_entry(target)?
        .create_new(true)
        .write(true)
        .open(target)?;
    let mut magic = [0; 8];
    use ulmdb_platform::FileExt;
    input.read_exact_at(&mut magic, 0)?;
    let transformed = &magic == b"ULMDBTX1";
    let mut hashes: Vec<_> = asset
        .prefixes
        .iter()
        .map(|e| (e.length, Sha256::new(), e.hash))
        .collect();
    let mut remaining = asset.length;
    let mut buffer = [0; 131_072];
    while remaining != 0 {
        let length = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| Error::LimitExceeded)?;
        let n = input.read(&mut buffer[..length])?;
        if n == 0 {
            return Err(Error::Corrupt("truncated copy asset"));
        }
        output.write_all(&buffer[..n])?;
        for (left, hash, _) in &mut hashes {
            let prefix =
                usize::try_from((*left).min(n as u64)).map_err(|_| Error::LimitExceeded)?;
            hash.update(&buffer[..prefix]);
            *left -= prefix as u64;
        }
        remaining -= n as u64;
    }
    if input.read(&mut buffer[..1])? != 0
        || !transformed
            && hashes.into_iter().any(|(left, hash, expected)| {
                left != 0 || <[u8; 32]>::from(hash.finalize()) != expected
            })
    {
        return Err(Error::Corrupt("copy prefix authentication"));
    }
    let after = fs::symlink_metadata(source)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (after.dev(), after.ino()) != (actual.dev(), actual.ino()) {
            return Err(Error::Corrupt("copy source asset replaced after read"));
        }
    }
    if !after.is_file() || after.len() != asset.length {
        return Err(Error::Corrupt("copy source asset changed after read"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = output.metadata()?.permissions();
        permissions.set_readonly(true);
        output.set_permissions(permissions)?;
    }
    point("copy.asset.before_sync")?;
    ulmdb_platform::sync_file(&output)?;
    point("copy.asset.after_sync")?;
    Ok(())
}
fn audit_target(directory: &Path, checkpoint: &[u8; 64]) -> Result<crate::environment::Snapshot> {
    audit_target_with_transform(directory, checkpoint, None)
}
fn audit_target_with_transform(
    directory: &Path,
    checkpoint: &[u8; 64],
    transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
) -> Result<crate::environment::Snapshot> {
    audit_target_with_cache_options(directory, checkpoint, transform, None)
}
fn audit_target_with_cache_options(
    directory: &Path,
    checkpoint: &[u8; 64],
    transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    cache: Option<crate::chunked::CacheOptions>,
) -> Result<crate::environment::Snapshot> {
    let manifest =
        ulmdb_storage::read_checkpoint_with_transform(directory, checkpoint, transform.clone())?;
    if ulmdb_storage::transform::check_configuration(directory, transform.as_ref())?
        .is_some_and(|uuid| uuid != manifest.uuid)
    {
        return Err(Error::CryptoMismatch);
    }
    let owner = crate::client_snapshot::snapshot_owner()?;
    let snapshot = if let Some(cache) = cache {
        crate::environment::load_snapshot_with_cache(
            directory,
            manifest,
            owner,
            crate::ReadAhead::Normal,
            transform,
            &cache,
        )?
    } else {
        crate::environment::load_snapshot_with_transform(
            directory,
            manifest,
            owner,
            crate::ReadAhead::Normal,
            transform,
        )?
    };
    validate_file_for_admission(&snapshot.segments, &snapshot.manifest)?;
    Ok(snapshot)
}
/// Export the exact authenticated D/P pair while independent source writers run.
/// Target must be absent. This never copies facade visibility/lease metadata.
pub fn export_file_pair(source: &FileCopyPair, target: impl AsRef<Path>) -> Result<PairCopyReport> {
    source.check()?;
    let selected = source.selected();
    let previous = source.predecessor();
    let mut roots = vec![&selected.snapshot.manifest];
    if let Some(previous) = previous {
        roots.push(&previous.snapshot.manifest);
    }
    for read in std::iter::once(selected).chain(previous) {
        validate_file_for_admission(&read.snapshot.segments, &read.snapshot.manifest)?;
    }
    let mut assets = inventory(source.directory(), &roots)?;
    let source_selected = Summary::from_manifest(source.directory(), roots[0])?;
    let source_selected_checkpoint = selected.file_checkpoint()?;
    let predecessor_checkpoint = previous.map(ReadTxn::file_checkpoint).transpose()?;
    let normalized = if previous.is_none() && roots[0].previous != 0 {
        Some(normalized_standalone(roots[0])?)
    } else {
        None
    };
    if let Some(manifest) = &normalized {
        // Source's standalone descriptor is not target selection authority.
        // Keep only files reached as data or metadata origins by the new root.
        let mut needed: BTreeSet<_> = manifest
            .segments
            .iter()
            .map(|entry| ulmdb_storage::segment_path(source.directory(), entry))
            .collect();
        for origin in manifest
            .metadata_origins()
            .into_iter()
            .filter(|g| *g != manifest.generation)
        {
            let nominal = source
                .directory()
                .join("cohorts")
                .join(format!("{origin:032x}.cohort"));
            needed.insert(metadata_origin_source(&nominal, origin)?.0);
        }
        assets.retain(|path, _| {
            needed.contains(path) || path.file_name().is_some_and(|n| n == "TRANSFORM")
        });
    }
    let access = ulmdb_storage::access::FileAccessContext::open(source.directory())?.policy();
    let target = if source.selected().snapshot.external.is_some() {
        BootstrapPairTarget::create_externally_synchronized(target, access)?
    } else {
        BootstrapPairTarget::create_with_access(target, access)?
    };
    let directory = target.directory()?.to_path_buf();
    let mut directories = BTreeSet::new();
    for (path, asset) in assets {
        target.directory()?;
        let relative = path
            .strip_prefix(source.directory())
            .map_err(|_| Error::Corrupt("copy source path escape"))?;
        let output = directory.join(relative);
        copy_asset(&path, &output, &asset)?;
        directories.insert(
            output
                .parent()
                .ok_or(Error::Corrupt("copy asset parent"))?
                .to_path_buf(),
        );
    }
    for path in directories {
        point("copy.assets.before_directory_sync")?;
        ulmdb_platform::sync_path(path)?;
        point("copy.assets.after_directory_sync")?;
    }
    target.directory()?;
    let current = if let Some(manifest) = &normalized {
        point("copy.normalize.before_prepare")?;
        let publication = prepared(&directory, manifest)?;
        point("copy.normalize.after_prepare")?;
        publication
    } else {
        ulmdb_storage::prepare_existing_with_transform(
            &directory,
            &source_selected_checkpoint,
            selected.snapshot.segments.transform().cloned(),
        )?
    };
    let selected_checkpoint = *current.checkpoint();
    point("copy.before_target_audit")?;
    let actual = audit_target_with_cache_options(
        &directory,
        &selected_checkpoint,
        selected.snapshot.segments.transform().cloned(),
        crate::chunked::CacheOptions::from_table(&selected.snapshot.segments),
    )?;
    let actual_previous = predecessor_checkpoint
        .as_ref()
        .map(|checkpoint| {
            audit_target_with_cache_options(
                &directory,
                checkpoint,
                selected.snapshot.segments.transform().cloned(),
                crate::chunked::CacheOptions::from_table(&selected.snapshot.segments),
            )
        })
        .transpose()?;
    point("copy.after_target_audit")?;
    let mut output_roots = vec![&actual.manifest];
    if let Some(previous) = &actual_previous {
        output_roots.push(&previous.manifest);
    }
    let output_assets = inventory(&directory, &output_roots)?;
    let report = PairCopyReport {
        source_selected,
        source_selected_checkpoint,
        selected: Summary::from_manifest(&directory, &actual.manifest)?,
        predecessor: actual_previous
            .as_ref()
            .map(|p| Summary::from_manifest(&directory, &p.manifest))
            .transpose()?,
        selected_checkpoint,
        predecessor_checkpoint,
        normalization: normalized
            .as_ref()
            .map(|_| PairCopyNormalization::UnavailablePredecessor),
        physical_files: output_assets.len(),
        physical_bytes: output_assets.values().try_fold(0u64, |n, a| {
            n.checked_add(a.length).ok_or(Error::LimitExceeded)
        })?,
    };
    let previous = predecessor_checkpoint
        .as_ref()
        .map(|p| {
            ulmdb_storage::prepare_existing_with_transform(
                &directory,
                p,
                selected.snapshot.segments.transform().cloned(),
            )
        })
        .transpose()?;
    target.prepare(current, previous)?.publish()?;
    Ok(report)
}
fn genesis(mode: PublicationMode) -> Result<Manifest> {
    if !cfg!(target_os = "linux") {
        return Err(Error::InvalidInput("file copy is Linux-only"));
    }
    let mut manifest = Manifest::empty()?;
    manifest.format_minor = match mode {
        PublicationMode::Legacy => 2,
        PublicationMode::Cohort => 3,
        PublicationMode::Indexed => 4,
        _ => return Err(Error::InvalidInput("Range client copy unsupported")),
    };
    if manifest.format_minor == 4 {
        manifest.prepare_indexed(None)?;
    }
    Ok(manifest)
}
fn prepared(directory: &Path, manifest: &Manifest) -> Result<PreparedPublication> {
    ulmdb_storage::prepare_publication(directory, manifest)
}
/// Explicit fresh genesis0, with no source-content or source allocator claim.
pub fn create_empty_file_copy(
    target: impl AsRef<Path>,
    mode: PublicationMode,
) -> Result<NewEnvironmentCopyReport> {
    create_empty_file_copy_with_page_size(target, mode, crate::PageSize::DEFAULT)
}
pub fn create_empty_file_copy_with_page_size(
    target: impl AsRef<Path>,
    mode: PublicationMode,
    page_size: crate::PageSize,
) -> Result<NewEnvironmentCopyReport> {
    create_empty_file_copy_with_transform(target, mode, page_size, None)
}
pub fn create_empty_file_copy_with_transform(
    target: impl AsRef<Path>,
    mode: PublicationMode,
    page_size: crate::PageSize,
    transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
) -> Result<NewEnvironmentCopyReport> {
    create_empty_file_copy_with_access(
        target,
        mode,
        page_size,
        transform,
        ulmdb_storage::access::FileAccess::PRIVATE,
    )
}
pub fn create_empty_file_copy_with_access(
    target: impl AsRef<Path>,
    mode: PublicationMode,
    page_size: crate::PageSize,
    transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    access: ulmdb_storage::access::FileAccess,
) -> Result<NewEnvironmentCopyReport> {
    create_empty_inner(target, mode, page_size, transform, access, false)
}
pub fn create_empty_file_copy_externally_synchronized(
    target: impl AsRef<Path>,
    mode: PublicationMode,
    page_size: crate::PageSize,
    transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    access: ulmdb_storage::access::FileAccess,
) -> Result<NewEnvironmentCopyReport> {
    create_empty_inner(target, mode, page_size, transform, access, true)
}
fn create_empty_inner(
    target: impl AsRef<Path>,
    mode: PublicationMode,
    page_size: crate::PageSize,
    transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    access: ulmdb_storage::access::FileAccess,
    external: bool,
) -> Result<NewEnvironmentCopyReport> {
    let mut manifest = genesis(mode)?.with_transform_context(transform.clone());
    manifest.page_size = page_size;
    let target = if external {
        BootstrapPairTarget::create_externally_synchronized(target, access)?
    } else {
        BootstrapPairTarget::create_with_access(target, access)?
    };
    let directory = target.directory()?.to_path_buf();
    ulmdb_storage::transform::create_configuration(&directory, manifest.uuid, transform.as_ref())?;
    let publication = prepared(&directory, &manifest)?;
    let checkpoint = *publication.checkpoint();
    audit_target_with_transform(&directory, &checkpoint, transform)?;
    let report = NewEnvironmentCopyReport {
        source_checkpoint: None,
        selected: Summary::from_manifest(&directory, &manifest)?,
        selected_checkpoint: checkpoint,
        genesis_checkpoint: checkpoint,
        database_id_floor: 1,
    };
    target.prepare(publication, None)?.publish()?;
    Ok(report)
}
/// Rebuild all actual chosen-view data into genuine new namespace business1/G0.
/// No facade emptiness inference and no generic compact/export identity change.
pub fn compact_file_copy(
    source: &FileCopyView,
    target: impl AsRef<Path>,
) -> Result<NewEnvironmentCopyReport> {
    source.check()?;
    let base = &source.read().snapshot;
    validate_file_for_admission(&base.segments, &base.manifest)?;
    let mode = match base.manifest.format_minor {
        2 => PublicationMode::Legacy,
        3 => PublicationMode::Cohort,
        4 => PublicationMode::Indexed,
        _ => return Err(Error::InvalidInput("unsupported compact-copy format")),
    };
    let mut genesis = genesis(mode)?;
    genesis.page_size = base.manifest.page_size;
    genesis.value_limit = base.manifest.value_limit;
    genesis = genesis.with_transform_context(base.segments.transform().cloned());
    let access = ulmdb_storage::access::FileAccessContext::open(source.directory())?.policy();
    let target = if source.read().snapshot.external.is_some() {
        BootstrapPairTarget::create_externally_synchronized(target, access)?
    } else {
        BootstrapPairTarget::create_with_access(target, access)?
    };
    let directory = target.directory()?.to_path_buf();
    let predecessor = prepared(&directory, &genesis)?;
    ulmdb_storage::transform::create_configuration(
        &directory,
        genesis.uuid,
        base.segments.transform(),
    )?;
    let mut arena = ulmdb_storage::SegmentBuilder::new_with_geometry_and_transform(
        &directory,
        1,
        base.segments.read_ahead(),
        base.manifest.page_size,
        base.segments.transform().cloned(),
    )?
    .with_value_limit(base.manifest.value_limit);
    let mut catalog = BTreeMap::new();
    let mut catalog_builder = ulmdb_kv::SortedBuilder::new();
    if let Some(bytes) =
        (crate::catalog_profile::read_file(&base.segments, base.manifest.catalog)?).marker()
    {
        catalog_builder.push(&mut arena, b"", &bytes)?;
    }
    for (name, old) in &base.catalog {
        let root = match old.kind {
            DatabaseKind::Kv => {
                if base.segments.is_chunked() {
                    ulmdb_kv::rebuild_inorder(
                        &mut arena,
                        crate::chunked::stream::Pairs::new(&base.segments, old.root),
                    )?
                    .root
                } else {
                    ulmdb_kv::rebuild_inorder(&mut arena, cursor(&base.segments, old.root)?)?.root
                }
            }
            DatabaseKind::Duplicates { width } => {
                if base.segments.is_chunked() {
                    crate::duplicates::copy_buckets_scoped(
                        &base.segments,
                        old.root,
                        &mut arena,
                        width,
                    )?
                    .0
                } else {
                    crate::duplicates::copy_buckets(&base.segments, old.root, &mut arena, width)?.0
                }
            }
            DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => {
                let whole = Prefix::new(
                    if old.kind == DatabaseKind::Ipv4 {
                        Address::V4(0)
                    } else {
                        Address::V6(0)
                    },
                    0,
                )
                .map_err(|_| Error::Corrupt("compact copy prefix family"))?;
                if base.segments.is_chunked() {
                    ulmdb_prefix::build_sorted(
                        &mut arena,
                        crate::chunked::stream::Subnets::new(&base.segments, old.root, whole),
                    )?
                    .root
                } else {
                    ulmdb_prefix::build_sorted(
                        &mut arena,
                        ulmdb_prefix::subnets(&base.segments, old.root, whole)?,
                    )?
                    .root
                }
            }
        };
        let db = Database {
            ordering: old.ordering,
            id: old.id,
            kind: old.kind,
            root,
        };
        catalog_builder.push(
            &mut arena,
            name,
            &db.encode_for_profile(base.manifest.page_size, base.manifest.value_limit),
        )?;
        catalog.insert(name.clone(), db);
    }
    let root = catalog_builder.finish(&mut arena)?.root;
    let (segments, next_segment) = arena.finish()?;
    let mut manifest = genesis.clone_metadata();
    manifest.generation = ulmdb_storage::new_identity()?;
    manifest.commit = ulmdb_storage::new_identity()?;
    manifest.parent = genesis.commit;
    manifest.previous = genesis.generation;
    manifest.sequence = 1;
    manifest.catalog = root;
    manifest.next_database = base.manifest.next_database;
    manifest.next_segment = next_segment;
    manifest.segments = segments.entries().into();
    if manifest.format_minor == 4 {
        manifest.prepare_indexed(None)?;
    }
    let publication = prepared(&directory, &manifest)?;
    let checkpoint = *publication.checkpoint();
    let after = audit_target_with_cache_options(
        &directory,
        &checkpoint,
        base.segments.transform().cloned(),
        crate::chunked::CacheOptions::from_table(&base.segments),
    )?;
    crate::compact::equivalent(base, &after.segments, after.manifest.catalog, &catalog)?;
    let genesis_checkpoint = *predecessor.checkpoint();
    audit_target_with_cache_options(
        &directory,
        &genesis_checkpoint,
        base.segments.transform().cloned(),
        crate::chunked::CacheOptions::from_table(&base.segments),
    )?;
    let report = NewEnvironmentCopyReport {
        source_checkpoint: Some(*source.view_checkpoint()),
        selected: Summary::from_manifest(&directory, &manifest)?,
        selected_checkpoint: checkpoint,
        genesis_checkpoint,
        database_id_floor: manifest.next_database,
    };
    target.prepare(publication, Some(predecessor))?.publish()?;
    Ok(report)
}
