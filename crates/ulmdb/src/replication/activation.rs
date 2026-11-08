//! Validate staged deltas, import immutable files, then select one complete root.
use super::{
    AssetDescription, AssetId, Cursor, ReplicationError as Error, Result, VerifiedTransfer, assets,
};
use crate::{
    Environment,
    environment::{Snapshot, snapshot_from_parts},
};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::{Arc, atomic::Ordering},
};
use ulmdb_storage::{Manifest, ResidencyController, ResidencyPolicy, SegmentTable};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    Activated,
    AlreadyCurrent,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyReport {
    pub outcome: ApplyOutcome,
    pub cursor: Cursor,
    pub imported_files: usize,
    pub imported_bytes: u64,
}
fn point(name: &str) -> Result<()> {
    #[cfg(feature = "fault-injection")]
    ulmdb_storage::injected_fault(name)?;
    #[cfg(not(feature = "fault-injection"))]
    let _ = name;
    Ok(())
}
fn cursor(manifest: &Manifest) -> Result<Cursor> {
    Ok(Cursor {
        uuid: manifest.uuid,
        epoch: manifest.epoch,
        sequence: manifest.sequence,
        commit: manifest.commit,
        checkpoint: ulmdb_storage::checkpoint_bytes(manifest)?,
    })
}
fn generation(cursor: &Cursor) -> Result<u128> {
    Ok(ulmdb_storage::checkpoint_identity(&cursor.checkpoint)?.0)
}
pub(super) fn lineage(base: &Manifest, target: &Manifest) -> Result<()> {
    if target.uuid != base.uuid {
        return Err(Error::Conflict);
    }
    if target.epoch != base.epoch {
        return Err(Error::EpochConflict);
    }
    if target.previous != base.generation || target.generation == base.generation {
        return Err(Error::Gap);
    }
    let logical = base.sequence.checked_add(1) == Some(target.sequence)
        && target.parent == base.commit
        && target.commit != base.commit;
    let physical = target.sequence == base.sequence
        && target.commit == base.commit
        && target.parent == base.parent;
    if !logical && !physical {
        return Err(Error::Gap);
    }
    if target.next_database < base.next_database || target.next_segment < base.next_segment {
        return Err(Error::Corrupt("replication identity counter regressed"));
    }
    Ok(())
}
fn candidate(
    frame: &VerifiedTransfer,
    base: Option<&Snapshot>,
    fallback: Option<&Path>,
) -> Result<Snapshot> {
    point("replica.before_validate")?;
    let manifest = match fallback {
        Some(directory) => ulmdb_storage::read_checkpoint_with_fallback(
            frame.staging_directory(),
            directory,
            &frame.header.target.checkpoint,
        )?,
        None => ulmdb_storage::read_checkpoint(
            frame.staging_directory(),
            &frame.header.target.checkpoint,
        )?,
    };
    if cursor(&manifest)? != frame.header.target {
        return Err(Error::Corrupt("transfer/root identity mismatch"));
    }
    let target_assets = assets::ids(&manifest);
    let expected = if let Some(base) = base {
        lineage(&base.manifest, &manifest)?;
        target_assets
            .difference(&assets::ids(&base.manifest))
            .copied()
            .collect::<BTreeSet<_>>()
    } else {
        target_assets
    };
    let actual = frame
        .assets
        .iter()
        .map(|asset| asset.id)
        .collect::<BTreeSet<_>>();
    if expected != actual || actual.len() != frame.assets.len() {
        return Err(Error::Corrupt(
            "transfer asset set differs from snapshot delta",
        ));
    }
    let mut segments = SegmentTable::empty_with_read_ahead(
        base.map_or(crate::ReadAhead::Normal, |b| b.segments.read_ahead()),
    )?;
    for entry in &manifest.segments {
        if let Some(base) = base {
            if segments.reuse_from(&base.segments, entry)? {
                continue;
            }
            if entry.id < base.manifest.next_segment {
                return Err(Error::Corrupt("replication reused a burned segment ID"));
            }
        }
        segments.add(frame.staging_directory(), entry.clone())?;
    }
    crate::backup::validate_for_admission(&segments, &manifest)?;
    let snapshot = snapshot_from_parts(segments, manifest, base.map_or(0, |base| base.owner))?;
    point("replica.after_validate")?;
    Ok(snapshot)
}
fn equal_files(a: &Path, b: &Path, length: u64) -> Result<()> {
    let metadata = fs::symlink_metadata(b)?;
    if !metadata.file_type().is_file() || metadata.len() != length {
        return Err(Error::Corrupt("immutable replica file collision"));
    }
    let mut a = File::open(a)?;
    let mut b = File::open(b)?;
    let mut left = [0; 131072];
    let mut right = [0; 131072];
    let mut remaining = length;
    while remaining != 0 {
        let n = remaining.min(left.len() as u64) as usize;
        a.read_exact(&mut left[..n])?;
        b.read_exact(&mut right[..n])?;
        if left[..n] != right[..n] {
            return Err(Error::Corrupt("immutable replica file content collision"));
        }
        remaining -= n as u64;
    }
    Ok(())
}
struct ImportTemporary(std::path::PathBuf);
impl Drop for ImportTemporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn copy_new(source: &Path, destination: &Path, length: u64) -> Result<ImportTemporary> {
    let mut input = File::open(source)?;
    let mut output = ulmdb_storage::access::options_for_entry(destination)?
        .write(true)
        .create_new(true)
        .open(destination)?;
    let owned = ImportTemporary(destination.to_path_buf());
    let mut remaining = length;
    let mut buffer = [0; 131072];
    while remaining != 0 {
        let n = remaining.min(buffer.len() as u64) as usize;
        input.read_exact(&mut buffer[..n])?;
        output.write_all(&buffer[..n])?;
        remaining -= n as u64;
    }
    let mut permissions = output.metadata()?.permissions();
    permissions.set_readonly(true);
    output.set_permissions(permissions)?;
    ulmdb_platform::sync_file(&output)?;
    Ok(owned)
}
/// Return whether the staged inode was linked (so its verified mapping may be
/// retained without another data hash). Never overwrite an existing file.
fn import_one(
    directory: &Path,
    frame: &VerifiedTransfer,
    asset: &AssetDescription,
) -> Result<(bool, bool)> {
    import_asset(directory, frame.staging_directory(), asset)
}
/// Import a separately validated immutable asset without granting replica or
/// authentication authority. Both callers validate their complete frame first.
pub(crate) fn import_asset(
    directory: &Path,
    staging: &Path,
    asset: &AssetDescription,
) -> Result<(bool, bool)> {
    let source = asset.id.path(staging);
    let target = asset.id.path(directory);
    match asset.id {
        AssetId::Descriptor(id) => {
            if AssetId::Cohort(id).path(directory).try_exists()? {
                return Err(Error::Corrupt(
                    "replica generation claimed by other storage type",
                ));
            }
        }
        AssetId::Cohort(id) => {
            if AssetId::Descriptor(id).path(directory).try_exists()? {
                return Err(Error::Corrupt(
                    "replica generation claimed by other storage type",
                ));
            }
        }
        AssetId::Segment(_) => {}
    }
    let source_meta = fs::symlink_metadata(&source)?;
    if !source_meta.file_type().is_file() || source_meta.len() != asset.length {
        return Err(Error::Corrupt("staged file changed after verification"));
    }
    point("replica.before_file_import")?;
    let (created, linked) = match fs::hard_link(&source, &target) {
        Ok(()) => (true, true),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            equal_files(&source, &target, asset.length)?;
            (false, false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            let temporary = target
                .parent()
                .ok_or(Error::Corrupt("replica asset parent"))?
                .join(format!(
                    ".prepare-replica-{:032x}.tmp",
                    ulmdb_storage::new_identity()?
                ));
            let _temporary_guard = copy_new(&source, &temporary, asset.length)?;
            let result: Result<(bool, bool)> = match fs::hard_link(&temporary, &target) {
                Ok(()) => Ok((true, false)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    equal_files(&source, &target, asset.length)?;
                    Ok((false, false))
                }
                Err(error) => Err(error.into()),
            };
            result?
        }
        Err(error) => return Err(error.into()),
    };
    let imported = File::open(&target)?;
    if created {
        ulmdb_storage::access::FileAccessContext::open(directory)?
            .configure_imported_file(&imported)?;
    }
    ulmdb_platform::sync_file(&imported)?;
    point("replica.after_file_import")?;
    Ok((created, linked))
}
fn import(
    directory: &Path,
    frame: &VerifiedTransfer,
    staged: &Snapshot,
    base: Option<&Snapshot>,
) -> Result<(SegmentTable, usize, u64)> {
    point("replica.before_import")?;
    let mut linked = BTreeSet::new();
    let mut parents = BTreeSet::new();
    let mut count = 0;
    let mut bytes = 0u64;
    for asset in &frame.assets {
        let (created, same_inode) = import_one(directory, frame, asset)?;
        if same_inode {
            linked.insert(asset.id);
        }
        if created {
            count += 1;
            bytes = bytes
                .checked_add(asset.length)
                .ok_or(Error::LimitExceeded)?;
        }
        parents.insert(
            asset
                .id
                .path(directory)
                .parent()
                .ok_or(Error::Corrupt("replica asset parent"))?
                .to_path_buf(),
        );
    }
    point("replica.before_directory_sync")?;
    for parent in parents {
        ulmdb_platform::sync_path(parent)?;
    }
    point("replica.after_directory_sync")?;
    let mut live = SegmentTable::empty_with_read_ahead(
        base.map_or(crate::ReadAhead::Normal, |b| b.segments.read_ahead()),
    )?;
    for entry in &staged.manifest.segments {
        if let Some(base) = base
            && live.reuse_from(&base.segments, entry)?
        {
            continue;
        }
        let id = if entry.cohort != 0 {
            AssetId::Cohort(entry.cohort)
        } else {
            AssetId::Segment(entry.hash)
        };
        if linked.contains(&id) {
            if !live.reuse_from(&staged.segments, entry)? {
                return Err(Error::Corrupt("verified stage lost mapping"));
            }
        } else {
            live.add(directory, entry.clone())?;
        }
    }
    point("replica.after_import")?;
    Ok((live, count, bytes))
}
/// Install an authenticated, validated full snapshot into an absent target.
pub fn bootstrap(frame: &VerifiedTransfer, target: impl AsRef<Path>) -> Result<Environment> {
    bootstrap_with_residency(frame, target, ResidencyPolicy::Demand)
}
/// Admit only the imported destination mappings before any pointer publication.
/// Stage validation does not reserve or lock a second copy of those mappings.
pub fn bootstrap_with_residency(
    frame: &VerifiedTransfer,
    target: impl AsRef<Path>,
    policy: ResidencyPolicy,
) -> Result<Environment> {
    if frame.header.base.is_some() {
        return Err(Error::InvalidInput("bootstrap requires a full snapshot"));
    }
    let controller = ResidencyController::new(policy)?;
    let staged = candidate(frame, None, None)?;
    let directory = target.as_ref();
    ulmdb_storage::create_layout(directory)?;
    let lock = Environment::lock(directory)?;
    let (segments, _, _) = import(directory, frame, &staged, None)?;
    let mut snapshot = snapshot_from_parts(segments, staged.manifest, 0)?;
    snapshot.admission = Some(controller.admit(&snapshot.segments)?);
    super::write_role(
        directory,
        super::ReplicaIdentity {
            key_id: frame.header.key_id,
            uuid: snapshot.manifest.uuid,
            epoch: snapshot.manifest.epoch,
        },
    )?;
    // Construct the unexposed environment now: admission, role loading and
    // owner initialization must not introduce a late failure after CURRENT.
    let env = Environment::from_verified_with_controller(directory, snapshot, lock, controller)?;
    point("replica.before_prepare")?;
    let prepared = ulmdb_storage::prepare_existing(directory, &frame.header.target.checkpoint)?;
    point("replica.after_prepare")?;
    point("replica.before_publish")?;
    prepared
        .publish_bootstrap()
        .map_err(|_| Error::Engine(crate::Error::OutcomeUnknown))?;
    let result = (|| {
        point("replica.after_publish")?;
        point("replica.before_activate")?;
        point("replica.after_activate")?;
        Ok(env)
    })();
    result.map_err(|_: Error| Error::Engine(crate::Error::OutcomeUnknown))
}
impl Environment {
    /// Apply only the authenticated direct successor of this read-only replica.
    pub fn apply_replication(&self, frame: &VerifiedTransfer) -> Result<ApplyReport> {
        self.require_file_backend()?;
        let _writer = self
            .writer
            .lock()
            .map_err(|_| Error::Engine(crate::Error::Busy))?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::Engine(crate::Error::OutcomeUnknown));
        }
        let role = self
            .replica_role
            .ok_or(Error::InvalidInput("target is not a replica"))?;
        if role.key_id != frame.header.key_id || role.uuid != frame.header.target.uuid {
            return Err(Error::UnauthorizedPublisher);
        }
        if role.epoch != frame.header.target.epoch {
            return Err(Error::EpochConflict);
        }
        let base = self.current.load_full();
        let current = cursor(&base.manifest)?;
        if current == frame.header.target {
            return Ok(ApplyReport {
                outcome: ApplyOutcome::AlreadyCurrent,
                cursor: current,
                imported_files: 0,
                imported_bytes: 0,
            });
        }
        if frame.header.base != Some(current) {
            return Err(Error::Gap);
        }
        if generation(&current)? != base.manifest.generation {
            return Err(Error::Corrupt("replica cursor generation"));
        }
        let staged = candidate(frame, Some(&base), Some(&self.directory))?;
        let (segments, count, bytes) = import(&self.directory, frame, &staged, Some(&base))?;
        let mut snapshot = snapshot_from_parts(segments, staged.manifest, base.owner)?;
        self.admit_snapshot(&mut snapshot)?;
        point("replica.before_prepare")?;
        let prepared =
            ulmdb_storage::prepare_existing(&self.directory, &frame.header.target.checkpoint)?;
        if let Some(coordination) = &self.coordination {
            snapshot.pin_with_prepared_capture(&coordination.try_capture()?, &prepared)?;
        }
        point("replica.after_prepare")?;
        point("replica.before_publish")?;
        let result: Result<()> = (|| {
            prepared.publish()?;
            point("replica.after_publish")?;
            point("replica.before_activate")?;
            self.activate(Arc::new(snapshot))?;
            point("replica.after_activate")?;
            Ok(())
        })();
        if result.is_err() {
            self.poisoned.store(true, Ordering::Release);
            return Err(Error::Engine(crate::Error::OutcomeUnknown));
        }
        Ok(ApplyReport {
            outcome: ApplyOutcome::Activated,
            cursor: frame.header.target,
            imported_files: count,
            imported_bytes: bytes,
        })
    }
}

#[cfg(test)]
mod import_tests {
    use super::*;
    #[test]
    fn interrupted_copy_removes_only_its_owned_temporary() {
        let parent = std::env::temp_dir().join(format!(
            "ulmdb-import-copy-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&parent).unwrap();
        let source = parent.join("source");
        let target = parent.join("temporary");
        fs::write(&source, b"abc").unwrap();
        assert!(copy_new(&source, &target, 5).is_err());
        assert!(!target.exists());
        fs::write(&target, b"existing").unwrap();
        assert!(copy_new(&source, &target, 3).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"existing");
        assert_eq!(fs::read(&source).unwrap(), b"abc");
        fs::remove_dir_all(parent).unwrap();
    }
}
