//! Durable named snapshot pins. Creation, release and opening share the writer
//! lifecycle fence with reclamation; serving reads never acquire this lock.
use crate::{Environment, Error, ReadTxn, Result, environment};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
};
use ulmdb_storage::Manifest;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedSnapshot {
    pub name: String,
    pub generation: u128,
    pub sequence: u64,
    pub commit: u128,
    pub segments: usize,
    pub segment_bytes: u64,
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn path(env: &Environment, name: &str) -> Result<PathBuf> {
    if !valid_name(name) {
        return Err(Error::InvalidInput(
            "retention name must be 1..64 ASCII letters/digits/_/-",
        ));
    }
    Ok(env.directory.join("retained").join(format!("{name}.pin")))
}
pub(crate) fn read_pin(env: &Environment, path: &Path) -> Result<Manifest> {
    if !fs::symlink_metadata(env.directory.join("retained"))?
        .file_type()
        .is_dir()
    {
        return Err(Error::Corrupt("invalid retention directory"));
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.len() != 64 {
        return Err(Error::Corrupt("invalid retention pointer file"));
    }
    let mut bytes = [0; 64];
    File::open(path)?.read_exact(&mut bytes)?;
    let manifest = ulmdb_storage::read_checkpoint_with_transform(
        &env.directory,
        &bytes,
        env.current.load().segments.transform().cloned(),
    )?;
    if manifest.uuid != env.current.load().manifest.uuid {
        return Err(Error::Corrupt("foreign retained snapshot"));
    }
    Ok(manifest)
}
pub(crate) fn pins(env: &Environment) -> Result<Vec<(String, Manifest)>> {
    pins_bounded(env, 4096, usize::MAX, usize::MAX)
}
pub(crate) fn pins_bounded(
    env: &Environment,
    roots: usize,
    inventory: usize,
    names: usize,
) -> Result<Vec<(String, Manifest)>> {
    match fs::symlink_metadata(env.directory.join("retained")) {
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(Error::Corrupt("invalid retention directory"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    }
    let entries = match fs::read_dir(env.directory.join("retained")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut pins = Vec::new();
    let mut inventory_entries = 0usize;
    for (scanned, entry) in entries.enumerate() {
        if scanned >= names {
            return Err(Error::LimitExceeded);
        }
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(name) = file_name
            .to_str()
            .and_then(|name| name.strip_suffix(".pin"))
        else {
            continue;
        };
        if !valid_name(name) {
            return Err(Error::Corrupt("invalid retained snapshot name"));
        }
        if pins.len() >= roots {
            return Err(Error::LimitExceeded);
        }
        let manifest = read_pin(env, &entry.path())?;
        inventory_entries = inventory_entries
            .checked_add(manifest.segments.len())
            .ok_or(Error::LimitExceeded)?;
        if inventory_entries > inventory {
            return Err(Error::LimitExceeded);
        }
        pins.push((name.to_owned(), manifest));
    }
    pins.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(pins)
}
fn info(name: String, manifest: &Manifest) -> Result<RetainedSnapshot> {
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
fn ensure_directory(env: &Environment) -> Result<()> {
    let directory = env.directory.join("retained");
    match fs::create_dir(&directory) {
        Ok(()) => {
            ulmdb_platform::sync_path(&env.directory)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if !fs::symlink_metadata(&directory)?.file_type().is_dir() {
                return Err(Error::Corrupt("invalid retention directory"));
            }
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
impl Environment {
    pub fn retain_snapshot(&self, name: &str, read: &ReadTxn) -> Result<RetainedSnapshot> {
        #[cfg(feature = "range-lifecycle")]
        if self.is_range_backed() {
            return crate::range_retention::retain(self, name, read);
        }
        self.require_coordinated()?;
        self.require_file_backend()?;
        let destination = path(self, name)?;
        let _writer = self.writer.lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        let snapshot = &read.snapshot;
        if snapshot.owner != self.current.load().owner {
            return Err(Error::StaleHandle);
        }
        crate::backup::validate_file_for_admission(&snapshot.segments, &snapshot.manifest)?;
        if pins(self)?.len() >= 4096 {
            return Err(Error::LimitExceeded);
        }
        ensure_directory(self)?;
        match fs::symlink_metadata(&destination) {
            Ok(_) => return Err(Error::InvalidInput("retained snapshot name already exists")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let directory = self.directory.join("retained");
        let temporary = directory.join(format!(".prepare-{}.tmp", ulmdb_storage::new_identity()?));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&ulmdb_storage::checkpoint_bytes(&snapshot.manifest)?)?;
        ulmdb_platform::sync_file(&file)?;
        ulmdb_platform::rename_replace(&temporary, &destination)
            .map_err(ulmdb_platform::rename_original_error)?;
        ulmdb_platform::sync_path(&directory)?;
        self.maintenance_retention_changed();
        info(name.to_owned(), &snapshot.manifest)
    }
    pub fn retained_snapshots(&self) -> Result<Vec<RetainedSnapshot>> {
        #[cfg(feature = "range-lifecycle")]
        if self.is_range_backed() {
            return crate::range_retention::list(self);
        }
        self.require_coordinated()?;
        self.require_file_backend()?;
        let _writer = self.writer.lock().map_err(|_| Error::Busy)?;
        pins(self)?
            .into_iter()
            .map(|(name, m)| info(name, &m))
            .collect()
    }
    pub fn open_retained(&self, name: &str) -> Result<ReadTxn> {
        #[cfg(feature = "range-lifecycle")]
        if self.is_range_backed() {
            return crate::range_retention::open(self, name);
        }
        self.require_coordinated()?;
        self.require_file_backend()?;
        let pointer = path(self, name)?;
        let _writer = self.writer.lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        let manifest = read_pin(self, &pointer)?;
        let base = self.current.load();
        let mut snapshot =
            environment::load_snapshot_like(&self.directory, manifest, base.owner, &base.segments)?;
        drop(base);
        crate::backup::validate_file_for_admission(&snapshot.segments, &snapshot.manifest)?;
        self.admit_snapshot(&mut snapshot)?;
        self.pin_existing_snapshot(&mut snapshot)?;
        let snapshot = Arc::new(snapshot);
        self.retired
            .lock()
            .map_err(|_| Error::Busy)?
            .push(Arc::downgrade(&snapshot));
        Ok(ReadTxn { snapshot })
    }
    pub fn release_snapshot(&self, name: &str) -> Result<()> {
        #[cfg(feature = "range-lifecycle")]
        if self.is_range_backed() {
            return crate::range_retention::release(self, name);
        }
        self.require_coordinated()?;
        self.require_file_backend()?;
        let pointer = path(self, name)?;
        let _writer = self.writer.lock().map_err(|_| Error::Busy)?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        read_pin(self, &pointer)?;
        fs::remove_file(pointer)?;
        // Reclamation also flushes this directory before trusting absent pins.
        // A failed release sync cannot authorize deleting a durable old pin's data.
        ulmdb_platform::sync_path(self.directory.join("retained"))?;
        self.maintenance_retention_changed();
        Ok(())
    }
}
