//! Format6 named checkpoints. File-format pins and reclamation are separate.
use crate::{Environment, Error, ReadTxn, Result, RetainedSnapshot, environment};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
};
const LIMIT: usize = 4096;
const DIRECTORY: &str = "retained-range";
fn owner(env: &Environment) -> Result<()> {
    // Fail promptly for an inherited/held owner before other mutable state is
    // visited. The Range store checks its captured namespace's creator PID.
    env.range
        .as_ref()
        .ok_or(Error::InvalidInput("not a Range owner"))?
        .try_lock()
        .map_err(|_| Error::Busy)?
        .check_lifecycle_owner()
}
fn pointer(env: &Environment, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(Error::InvalidInput(
            "retention name must be 1..64 ASCII letters/digits/_/-",
        ));
    }
    Ok(env.directory.join(DIRECTORY).join(format!("{name}.pin")))
}
fn checkpoint(snapshot: &environment::Snapshot) -> Result<&ulmdb_storage::range::RangeCheckpoint> {
    let checkpoint = snapshot
        .range_checkpoint
        .as_ref()
        .ok_or(Error::InvalidInput("range retention requires format6"))?;
    checkpoint.check_creator()?;
    if !checkpoint.matches_manifest(&snapshot.manifest) {
        return Err(Error::Corrupt("Range retention snapshot binding"));
    }
    Ok(checkpoint)
}
fn check_directory(
    env: &Environment,
    checkpoint: &ulmdb_storage::range::RangeCheckpoint,
) -> Result<()> {
    if !checkpoint.matches_directory(&env.directory)? {
        return Err(Error::Corrupt("Range retention namespace replaced"));
    }
    if env.poisoned.load(Ordering::Acquire) {
        return Err(Error::OutcomeUnknown);
    }
    Ok(())
}
fn entries(env: &Environment) -> Result<Vec<(String, PathBuf)>> {
    let directory = env.directory.join(DIRECTORY);
    match fs::symlink_metadata(&directory) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
        Ok(m) if !m.is_dir() || m.file_type().is_symlink() => {
            return Err(Error::Corrupt("invalid Range retention directory"));
        }
        Ok(_) => {}
    }
    let mut result = Vec::new();
    for (scanned, entry) in fs::read_dir(directory)?.enumerate() {
        if scanned >= LIMIT * 2 {
            return Err(Error::LimitExceeded);
        }
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str().and_then(|n| n.strip_suffix(".pin")) else {
            continue;
        };
        pointer(env, name).map_err(|_| Error::Corrupt("invalid Range retention name"))?;
        if result.len() == LIMIT {
            return Err(Error::LimitExceeded);
        }
        result.push((name.to_owned(), entry.path()));
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}
fn read_record(path: &Path) -> Result<[u8; 256]> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != 256 {
        return Err(Error::Corrupt("invalid Range retention record file"));
    }
    let mut bytes = [0; 256];
    let mut file = File::open(path)?;
    if !ulmdb_platform::file_identity(&file)?.same_object(&ulmdb_platform::path_identity(path)?) {
        return Err(Error::Corrupt("Range retention record replaced"));
    }
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn load(
    env: &Environment,
    anchor: &ulmdb_storage::range::RangeCheckpoint,
    path: &Path,
) -> Result<ulmdb_storage::range::RangeSnapshot> {
    let directory = fs::symlink_metadata(env.directory.join(DIRECTORY))?;
    if !directory.is_dir() || directory.file_type().is_symlink() {
        return Err(Error::Corrupt("invalid Range retention directory"));
    }
    let bytes = read_record(path)?;
    anchor.open_retained_checked(&bytes, &env.residency, |table, manifest| {
        crate::backup::validate_for_admission(table, manifest)
    })
}
fn info(name: String, manifest: &ulmdb_storage::Manifest) -> Result<RetainedSnapshot> {
    Ok(RetainedSnapshot {
        name,
        generation: manifest.generation,
        sequence: manifest.sequence,
        commit: manifest.commit,
        segments: manifest.segments.len(),
        segment_bytes: manifest.segments.iter().try_fold(0u64, |sum, s| {
            sum.checked_add(s.length).ok_or(Error::LimitExceeded)
        })?,
    })
}
pub(crate) fn retain(env: &Environment, name: &str, read: &ReadTxn) -> Result<RetainedSnapshot> {
    owner(env)?;
    let destination = pointer(env, name)?;
    let _writer = env.writer.try_lock().map_err(|_| Error::Busy)?;
    let current = env.current.load_full();
    let anchor = checkpoint(&current)?;
    check_directory(env, anchor)?;
    if read.snapshot.owner != current.owner {
        return Err(Error::StaleHandle);
    }
    let selected = checkpoint(&read.snapshot)?;
    if entries(env)?.len() >= LIMIT {
        return Err(Error::LimitExceeded);
    }
    match fs::symlink_metadata(&destination) {
        Ok(_) => return Err(Error::InvalidInput("retained snapshot name already exists")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    crate::backup::validate_for_admission(&read.snapshot.segments, &read.snapshot.manifest)?;
    let bytes = selected.retained_record(anchor)?;
    let directory = env.directory.join(DIRECTORY);
    match fs::create_dir(&directory) {
        Ok(()) => ulmdb_platform::sync_directory(&env.directory)?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if !fs::symlink_metadata(&directory)?.is_dir() {
                return Err(Error::Corrupt("invalid Range retention directory"));
            }
        }
        Err(e) => return Err(e.into()),
    }
    check_directory(env, anchor)?;
    let temporary = directory.join(format!(
        ".prepare-{:032x}.tmp",
        ulmdb_storage::new_identity()?
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        check_directory(env, anchor)?;
        fs::rename(&temporary, &destination)?;
        (|| {
            check_directory(env, anchor)?;
            ulmdb_platform::sync_directory(&directory)?;
            Ok::<_, Error>(())
        })()
        .map_err(|_| Error::OutcomeUnknown)?;
        info(name.to_owned(), &read.snapshot.manifest)
    })();
    if result.is_err() && anchor.matches_directory(&env.directory).unwrap_or(false) {
        if let (Ok(named), Ok(owned)) = (
            ulmdb_platform::path_identity(&temporary),
            ulmdb_platform::file_identity(&file),
        ) {
            if named.same_object(&owned) {
                let _ = fs::remove_file(&temporary);
            }
        }
    }
    result
}
pub(crate) fn list(env: &Environment) -> Result<Vec<RetainedSnapshot>> {
    owner(env)?;
    let _writer = env.writer.try_lock().map_err(|_| Error::Busy)?;
    let current = env.current.load_full();
    let anchor = checkpoint(&current)?;
    check_directory(env, anchor)?;
    entries(env)?
        .into_iter()
        .map(|(name, path)| load(env, anchor, &path).and_then(|state| info(name, &state.manifest)))
        .collect()
}
pub(crate) fn open(env: &Environment, name: &str) -> Result<ReadTxn> {
    owner(env)?;
    let path = pointer(env, name)?;
    let _writer = env.writer.try_lock().map_err(|_| Error::Busy)?;
    let current = env.current.load_full();
    let anchor = checkpoint(&current)?;
    check_directory(env, anchor)?;
    let state = load(env, anchor, &path)?;
    let mut snapshot =
        environment::snapshot_from_parts(state.segments, state.manifest, current.owner)?;
    snapshot.range_backed = true;
    snapshot.range_checkpoint = state.checkpoint;
    snapshot.admission = Some(state.admission);
    let snapshot = Arc::new(snapshot);
    env.retired
        .lock()
        .map_err(|_| Error::Busy)?
        .push(Arc::downgrade(&snapshot));
    Ok(ReadTxn { snapshot })
}
pub(crate) fn release(env: &Environment, name: &str) -> Result<()> {
    owner(env)?;
    let path = pointer(env, name)?;
    let _writer = env.writer.try_lock().map_err(|_| Error::Busy)?;
    let current = env.current.load_full();
    let anchor = checkpoint(&current)?;
    check_directory(env, anchor)?;
    // Deletion validates the same full authenticated historical graph, but
    // does not admit a new application reader. In particular, overlapping
    // RequiredLocked admission must never make a durable pin undeletable.
    let demand = ulmdb_storage::ResidencyController::new(ulmdb_storage::ResidencyPolicy::Demand)?;
    let directory = fs::symlink_metadata(env.directory.join(DIRECTORY))?;
    if !directory.is_dir() || directory.file_type().is_symlink() {
        return Err(Error::Corrupt("invalid Range retention directory"));
    }
    let bytes = read_record(&path)?;
    anchor.open_retained_checked(&bytes, &demand, |table, manifest| {
        crate::backup::validate_for_admission(table, manifest)
    })?;
    check_directory(env, anchor)?;
    fs::remove_file(path)?;
    (|| {
        check_directory(env, anchor)?;
        ulmdb_platform::sync_directory(&env.directory.join(DIRECTORY))?;
        Ok::<_, Error>(())
    })()
    .map_err(|_| Error::OutcomeUnknown)
}
