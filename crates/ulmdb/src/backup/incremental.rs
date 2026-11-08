//! Incremental immutable-asset backups with an exact base and atomic selection.
use super::*;
use crate::replication::{AssetDescription, AssetId};
use std::sync::Arc;
use std::sync::atomic::Ordering;

const HEADER: usize = 384;
const ASSET: usize = 64;
const MAGIC: &[u8; 8] = b"ULMDBD01";
const MAX_FILES: u32 = 1_048_576;
const MAX_BYTES: u64 = 1 << 49;

#[derive(Clone, Copy, Debug)]
pub struct IncrementalLimits {
    pub max_files: u32,
    pub max_bytes: u64,
    pub max_history: usize,
}
impl Default for IncrementalLimits {
    fn default() -> Self {
        Self {
            max_files: 1_048_576,
            max_bytes: 1 << 49,
            max_history: 1_048_576,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncrementalReport {
    pub base_sequence: u64,
    pub sequence: u64,
    pub files: u32,
    pub payload_bytes: u64,
}
fn engine(error: crate::replication::ReplicationError) -> Error {
    match error {
        crate::replication::ReplicationError::Engine(e) => e,
        crate::replication::ReplicationError::Io(e) => Error::Io(e),
        crate::replication::ReplicationError::LimitExceeded => Error::LimitExceeded,
        _ => Error::Corrupt("invalid incremental asset"),
    }
}
fn point(name: &str) -> Result<()> {
    #[cfg(feature = "fault-injection")]
    ulmdb_storage::injected_fault(name)?;
    #[cfg(not(feature = "fault-injection"))]
    let _ = name;
    Ok(())
}
fn asset_encode(asset: AssetDescription) -> [u8; ASSET] {
    let mut b = [0; ASSET];
    match asset.id {
        AssetId::Segment(hash) => {
            b[0] = 1;
            b[8..40].copy_from_slice(&hash);
        }
        AssetId::Descriptor(id) => {
            b[0] = 2;
            b[8..24].copy_from_slice(&id.to_le_bytes());
        }
        AssetId::Cohort(id) => {
            b[0] = 3;
            b[8..24].copy_from_slice(&id.to_le_bytes());
        }
    }
    b[40..48].copy_from_slice(&asset.length.to_le_bytes());
    b
}
fn asset_decode(b: &[u8; ASSET]) -> Result<AssetDescription> {
    if b[1..8].iter().chain(&b[48..]).any(|x| *x != 0) {
        return Err(Error::Corrupt("incremental asset reserved bytes"));
    }
    let id = match b[0] {
        1 => AssetId::Segment(b[8..40].try_into().unwrap()),
        kind @ (2 | 3) if b[24..40] == [0; 16] => {
            let id = u128::from_le_bytes(b[8..24].try_into().unwrap());
            if id == 0 {
                return Err(Error::Corrupt("zero incremental generation"));
            }
            if kind == 2 {
                AssetId::Descriptor(id)
            } else {
                AssetId::Cohort(id)
            }
        }
        _ => return Err(Error::Corrupt("incremental asset type")),
    };
    let length = u64::from_le_bytes(b[40..48].try_into().unwrap());
    id.check_length(length).map_err(engine)?;
    Ok(AssetDescription { id, length })
}
struct Stage(PathBuf);
impl Stage {
    fn new(directory: &Path) -> Result<Self> {
        let path = directory.join("transactions").join(format!(
            ".incremental-{:032x}",
            ulmdb_storage::new_identity()?
        ));
        fs::create_dir(&path)?;
        let stage = Self(path);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&stage.0, fs::Permissions::from_mode(0o700))?;
        }
        for name in ["segments", "commits", "cohorts"] {
            fs::create_dir(stage.0.join(name))?;
        }
        Ok(stage)
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn history(
    directory: &Path,
    target: &Manifest,
    since: u64,
    limit: usize,
    transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
) -> Result<Manifest> {
    let mut current = target.clone();
    for _ in 0..limit.min(MAX_FILES as usize) {
        if current.sequence == since {
            return Ok(current);
        }
        if current.sequence < since || current.previous == 0 {
            break;
        }
        let older = ulmdb_storage::read_generation_with_transform(
            directory,
            current.previous,
            current.format_minor,
            transform.clone(),
        )?;
        if older.uuid != target.uuid
            || older.epoch != target.epoch
            || older.sequence > current.sequence
            || older.generation == current.generation
            || (older.sequence < current.sequence && current.parent != older.commit)
            || (older.sequence == current.sequence
                && (current.commit != older.commit || current.parent != older.parent))
        {
            return Err(Error::Corrupt("incremental history lineage"));
        }
        current = older;
    }
    Err(Error::InvalidInput("incremental base history unavailable"))
}
impl Environment {
    /// Stream files added since an authenticated retained business transaction.
    /// The base backup must preserve the exact native checkpoint. Compact copies
    /// and another branch are not interchangeable bases. Bytes are native deltas.
    pub fn dump_incremental(
        &self,
        since: u64,
        output: &mut impl Write,
    ) -> Result<IncrementalReport> {
        self.dump_incremental_with_limits(since, output, IncrementalLimits::default())
    }
    pub fn dump_incremental_with_limits(
        &self,
        since: u64,
        output: &mut impl Write,
        limits: IncrementalLimits,
    ) -> Result<IncrementalReport> {
        self.current.load().check_external()?;
        self.require_file_backend()?;
        self.begin_read()
            .dump_incremental_snapshot(&self.directory, since, output, limits)
    }
}
impl ReadTxn {
    /// Export this exact physically pinned snapshot while independent writers
    /// continue. The directory must be the pin's original native namespace.
    pub fn dump_incremental_snapshot(
        &self,
        directory: &Path,
        since: u64,
        output: &mut impl Write,
        limits: IncrementalLimits,
    ) -> Result<IncrementalReport> {
        if self.snapshot.range_backed {
            return Err(Error::InvalidInput(
                "Range incremental snapshot unsupported",
            ));
        }
        if since == 0 {
            return Err(Error::InvalidInput("incremental base must be nonzero"));
        }
        // Capture excludes reclamation, while ordinary writers remain free to
        // publish. The immutable selected reader fixes this whole export.
        let coordination = if self.snapshot.external.is_none() {
            Some(
                self.snapshot
                    .physical_pin
                    .as_ref()
                    .ok_or(Error::InvalidInput(
                        "incremental source requires a physical pin",
                    ))?
                    .coordination(),
            )
        } else {
            None
        };
        let _capture = coordination.as_ref().map(|c| c.try_capture()).transpose()?;
        let read = self;
        let target = &read.snapshot.manifest;
        if let Some(external) = &read.snapshot.external {
            // This is caller-owned exclusion/liveness authority, never a pin.
            // It must bind the supplied namespace, not just matching root bytes.
            external.check(directory, target)?;
        } else {
            read.snapshot.check_physical_pin(directory)?;
        }
        let availability=ulmdb_storage::read_history_availability(directory,target.uuid,target.epoch,
            read.snapshot.segments.transform().cloned())?;
        if since < availability.floor_sequence {
            return Err(Error::InvalidInput("incremental base expired by explicit history retention policy"));
        }
        if since >= target.sequence {
            return Ok(IncrementalReport {
                base_sequence: since,
                sequence: target.sequence,
                files: 0,
                payload_bytes: 0,
            });
        }
        let base = history(
            directory,
            target,
            since,
            limits.max_history,
            read.snapshot.segments.transform().cloned(),
        )?;
        let previous = if target.previous == 0 {
            None
        } else {
            Some(ulmdb_storage::read_generation_with_transform(
                directory,
                target.previous,
                target.format_minor,
                read.snapshot.segments.transform().cloned(),
            )?)
        };
        if previous
            .as_ref()
            .is_some_and(|p| p.uuid != target.uuid || p.epoch != target.epoch)
        {
            return Err(Error::Corrupt("incremental predecessor realm"));
        }
        validate_file_for_admission(&read.snapshot.segments, target)?;
        if let Some(previous) = &previous {
            let mut table = read
                .snapshot
                .segments
                .empty_like()?
                .with_page_size(previous.page_size)?
                .with_value_limit(previous.value_limit)?;
            for entry in &previous.segments {
                if !table.reuse_from(&read.snapshot.segments, entry)? {
                    table.add(directory, entry.clone())?;
                }
            }
            validate_file_for_admission(&table, previous)?;
        }
        let mut ids = crate::replication::assets::ids(target);
        if let Some(previous) = &previous {
            ids.extend(crate::replication::assets::ids(previous));
        }
        let base_ids = crate::replication::assets::ids(&base);
        let files =
            crate::replication::assets::describe_ids(directory, ids.difference(&base_ids).copied())
                .map_err(engine)?;
        let count = u32::try_from(files.len()).map_err(|_| Error::LimitExceeded)?;
        let total = files.iter().try_fold(0u64, |n, f| {
            n.checked_add(f.length).ok_or(Error::LimitExceeded)
        })?;
        if count > limits.max_files.min(MAX_FILES) || total > limits.max_bytes.min(MAX_BYTES) {
            return Err(Error::LimitExceeded);
        }
        let mut header = [0; HEADER];
        header[..8].copy_from_slice(MAGIC);
        header[8..10].copy_from_slice(&1u16.to_le_bytes());
        header[10..12].copy_from_slice(&(HEADER as u16).to_le_bytes());
        header[12..16].copy_from_slice(&u32::from(previous.is_some()).to_le_bytes());
        header[16..32].copy_from_slice(&target.uuid);
        header[32..40].copy_from_slice(&target.epoch.to_le_bytes());
        header[40..48].copy_from_slice(&base.sequence.to_le_bytes());
        header[48..64].copy_from_slice(&base.commit.to_le_bytes());
        header[64..72].copy_from_slice(&target.sequence.to_le_bytes());
        header[72..88].copy_from_slice(&target.commit.to_le_bytes());
        header[88..152].copy_from_slice(&ulmdb_storage::checkpoint_bytes(&base)?);
        header[152..216].copy_from_slice(&ulmdb_storage::checkpoint_bytes(target)?);
        if let Some(previous) = &previous {
            header[216..280].copy_from_slice(&ulmdb_storage::checkpoint_bytes(previous)?);
        }
        header[280..284].copy_from_slice(&count.to_le_bytes());
        header[288..296].copy_from_slice(&total.to_le_bytes());
        let mut hash = Sha256::new();
        hash.update(header);
        output.write_all(&header)?;
        let mut buffer = [0; 131_072];
        for asset in files {
            let metadata = asset_encode(asset);
            hash.update(metadata);
            output.write_all(&metadata)?;
            let mut input = File::open(asset.id.path(directory))?;
            let mut remaining = asset.length;
            while remaining != 0 {
                let n = remaining.min(buffer.len() as u64) as usize;
                input.read_exact(&mut buffer[..n])?;
                hash.update(&buffer[..n]);
                output.write_all(&buffer[..n])?;
                remaining -= n as u64;
            }
            if input.read(&mut buffer[..1])? != 0 {
                return Err(Error::Corrupt("incremental source grew"));
            }
        }
        if let Some(external) = &read.snapshot.external {
            external.check(directory, target)?;
        }
        output.write_all(&hash.finalize())?;
        Ok(IncrementalReport {
            base_sequence: base.sequence,
            sequence: target.sequence,
            files: count,
            payload_bytes: total,
        })
    }
}
impl Environment {
    /// Validate the complete delta before importing any immutable assets and
    /// selecting its exact target. Callers own exclusive environment access.
    pub fn load_incremental(&self, input: &mut impl Read) -> Result<IncrementalReport> {
        self.load_incremental_checked(input, IncrementalLimits::default(), |_| Ok(()))
    }
    /// A prepublication admission check, e.g. a facade's logical capacity limit.
    /// The reader is a genuinely pinned prepared snapshot, not a selected head.
    pub fn load_incremental_checked(
        &self,
        input: &mut impl Read,
        limits: IncrementalLimits,
        check: impl FnOnce(&ReadTxn) -> Result<()>,
    ) -> Result<IncrementalReport> {
        self.current.load().check_external()?;
        self.require_file_backend()?;
        if self.is_replica() {
            return Err(Error::InvalidInput("incremental load on replica"));
        }
        self.current.load().check_physical_pin(&self.directory)?;
        let _writer = if self.is_externally_synchronized() {
            self.writer.try_lock().map_err(|_| Error::Busy)?
        } else {
            self.writer.lock().map_err(|_| Error::Busy)?
        };
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::OutcomeUnknown);
        }
        let base = self.current.load_full();
        self.check_external_head()?;
        let mut header = [0; HEADER];
        loop {
            match input.read(&mut header[..1]) {
                Ok(0) => {
                    return Ok(IncrementalReport {
                        base_sequence: base.manifest.sequence,
                        sequence: base.manifest.sequence,
                        files: 0,
                        payload_bytes: 0,
                    });
                }
                Ok(_) => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
        input.read_exact(&mut header[1..])?;
        if &header[..8] != MAGIC
            || header[8..10] != 1u16.to_le_bytes()
            || header[10..12] != (HEADER as u16).to_le_bytes()
            || header[284..288] != [0; 4]
            || header[296..].iter().any(|x| *x != 0)
        {
            return Err(Error::Corrupt("incremental header"));
        }
        let has_previous = match u32::from_le_bytes(header[12..16].try_into().unwrap()) {
            0 if header[216..280] == [0; 64] => false,
            1 => true,
            _ => return Err(Error::Corrupt("incremental previous presence")),
        };
        let base_seq = u64::from_le_bytes(header[40..48].try_into().unwrap());
        let target_seq = u64::from_le_bytes(header[64..72].try_into().unwrap());
        let count = u32::from_le_bytes(header[280..284].try_into().unwrap());
        let total = u64::from_le_bytes(header[288..296].try_into().unwrap());
        if count > limits.max_files.min(MAX_FILES) || total > limits.max_bytes.min(MAX_BYTES) {
            return Err(Error::LimitExceeded);
        }
        if base_seq == 0
            || target_seq <= base_seq
            || base_seq != base.manifest.sequence
            || header[16..32] != base.manifest.uuid
            || header[32..40] != base.manifest.epoch.to_le_bytes()
            || header[48..64] != base.manifest.commit.to_le_bytes()
            || header[88..152] != ulmdb_storage::checkpoint_bytes(&base.manifest)?
        {
            return Err(Error::InvalidInput("incremental base mismatch or replay"));
        }
        let stage = Stage::new(&self.directory)?;
        let mut hash = Sha256::new();
        hash.update(header);
        let mut assets = Vec::new();
        let mut last = None;
        let mut consumed = 0u64;
        let mut buffer = [0; 131_072];
        for _ in 0..count {
            let mut metadata = [0; ASSET];
            input.read_exact(&mut metadata)?;
            let asset = asset_decode(&metadata)?;
            if last.is_some_and(|id| id >= asset.id) {
                return Err(Error::Corrupt("incremental asset order"));
            }
            last = Some(asset.id);
            consumed = consumed
                .checked_add(asset.length)
                .ok_or(Error::LimitExceeded)?;
            if consumed > total {
                return Err(Error::Corrupt("incremental payload total"));
            }
            hash.update(metadata);
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(asset.id.path(&stage.0))?;
            let mut remaining = asset.length;
            while remaining != 0 {
                let n = remaining.min(buffer.len() as u64) as usize;
                input.read_exact(&mut buffer[..n])?;
                hash.update(&buffer[..n]);
                file.write_all(&buffer[..n])?;
                remaining -= n as u64;
            }
            let mut permissions = file.metadata()?.permissions();
            permissions.set_readonly(true);
            file.set_permissions(permissions)?;
            assets.push(asset);
        }
        let mut tag = [0; 32];
        input.read_exact(&mut tag)?;
        if consumed != total || hash.finalize().as_slice() != tag {
            return Err(Error::Corrupt("incremental checksum"));
        }
        if input.read(&mut buffer[..1])? != 0 {
            return Err(Error::Corrupt("incremental trailing bytes"));
        }
        point("incremental.after_receive")?;
        let target_cp: [u8; 64] = header[152..216].try_into().unwrap();
        let target = ulmdb_storage::read_checkpoint_with_fallback_and_transform(
            &stage.0,
            &self.directory,
            &target_cp,
            base.segments.transform().cloned(),
        )?;
        if target.uuid != base.manifest.uuid
            || target.epoch != base.manifest.epoch
            || target.page_size != base.manifest.page_size
            || target.sequence != target_seq
            || target.commit.to_le_bytes() != header[72..88]
            || target.next_database < base.manifest.next_database
            || target.next_segment < base.manifest.next_segment
        {
            return Err(Error::Corrupt("incremental target identity"));
        }
        let previous = if has_previous {
            let checkpoint: [u8; 64] = header[216..280].try_into().unwrap();
            let manifest = match ulmdb_storage::read_checkpoint_with_fallback_and_transform(
                &stage.0,
                &self.directory,
                &checkpoint,
                base.segments.transform().cloned(),
            ) {
                Ok(m) => m,
                Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    ulmdb_storage::read_checkpoint_with_transform(
                        &self.directory,
                        &checkpoint,
                        base.segments.transform().cloned(),
                    )?
                }
                Err(e) => return Err(e),
            };
            if target.previous != manifest.generation
                || manifest.uuid != target.uuid
                || manifest.epoch != target.epoch
                || !(manifest.sequence == target.sequence
                    || manifest.sequence.checked_add(1) == Some(target.sequence))
                || (manifest.sequence < target.sequence && target.parent != manifest.commit)
                || (manifest.sequence == target.sequence
                    && (target.commit != manifest.commit || target.parent != manifest.parent))
            {
                return Err(Error::Corrupt("incremental predecessor lineage"));
            }
            Some(manifest)
        } else {
            if target.previous != 0 {
                return Err(Error::Corrupt("missing incremental predecessor"));
            }
            None
        };
        let mut expected = crate::replication::assets::ids(&target);
        if let Some(p) = &previous {
            expected.extend(crate::replication::assets::ids(p));
        }
        let base_ids = crate::replication::assets::ids(&base.manifest);
        let expected: BTreeSet<_> = expected.difference(&base_ids).copied().collect();
        if expected != assets.iter().map(|a| a.id).collect() {
            return Err(Error::Corrupt("incremental asset inventory"));
        }
        // Map and audit every chosen graph before any destination import.
        let load = |manifest: &Manifest| -> Result<SegmentTable> {
            let mut table = base
                .segments
                .empty_like()?
                .with_page_size(manifest.page_size)?
                .with_value_limit(manifest.value_limit)?;
            for entry in &manifest.segments {
                if !table.reuse_from(&base.segments, entry)? {
                    table.add(&stage.0, entry.clone())?;
                }
            }
            validate_file_for_admission(&table, manifest)?;
            Ok(table)
        };
        let staged = load(&target)?;
        if let Some(p) = &previous {
            let _ = load(p)?;
        }
        point("incremental.after_validate")?;
        let mut linked = BTreeSet::new();
        for asset in &assets {
            let (_, same_inode) =
                crate::replication::activation::import_asset(&self.directory, &stage.0, asset)
                    .map_err(engine)?;
            if same_inode {
                linked.insert(asset.id);
            }
        }
        for dir in ["segments", "commits", "cohorts"] {
            ulmdb_platform::sync_path(self.directory.join(dir))?;
        }
        // A cached Source binds inode timestamps as well as bytes. Linking an
        // imported staging inode changes ctime; the later staging unlink changes
        // it again. Finish those owned namespace changes before authenticating
        // its final live identity. Never weaken the cache's replacement guard.
        let staged = if base.segments.is_chunked() {
            for asset in &assets {
                fs::remove_file(asset.id.path(&stage.0))?;
            }
            drop(staged);
            None
        } else {
            Some(staged)
        };
        let mut live = base
            .segments
            .empty_like()?
            .with_page_size(target.page_size)?
            .with_value_limit(target.value_limit)?;
        for entry in &target.segments {
            if live.reuse_from(&base.segments, entry)? {
                continue;
            }
            let id = if entry.cohort == 0 {
                AssetId::Segment(entry.hash)
            } else {
                AssetId::Cohort(entry.cohort)
            };
            if linked.contains(&id) {
                if let Some(staged) = &staged {
                    if live.reuse_from(staged, entry)? {
                        continue;
                    }
                }
            }
            live.add(&self.directory, entry.clone())?;
        }
        let mut snapshot = crate::environment::snapshot_from_parts(live, target, base.owner)?;
        snapshot.external = base.external.clone();
        self.admit_snapshot(&mut snapshot)?;
        let predecessor_cp = previous
            .as_ref()
            .map(ulmdb_storage::checkpoint_bytes)
            .transpose()?;
        let publication = if let Some(external) = &base.external {
            IncrementalPublication::External(external.storage.prepare_imported_snapshot(
                &target_cp,
                &ulmdb_storage::checkpoint_bytes(&base.manifest)?,
                predecessor_cp.as_ref(),
                base.segments.transform().cloned(),
            )?)
        } else {
            IncrementalPublication::Coordinated(
                ulmdb_storage::prepare_imported_snapshot_with_transform(
                    &self.directory,
                    &target_cp,
                    &ulmdb_storage::checkpoint_bytes(&base.manifest)?,
                    predecessor_cp.as_ref(),
                    base.segments.transform().cloned(),
                )?,
            )
        };
        if base.external.is_none() {
            snapshot.pin_prepared(&self.directory, publication.prepared())?;
        }
        let ready = snapshot.prepare_activation(self, publication.prepared())?;
        check(&ready.read())?;
        point("incremental.before_publish")?;
        let result = (|| {
            publication.publish()?;
            point("incremental.after_publish")?;
            self.activate_prepared(ready)
        })();
        if result.is_err() {
            self.poisoned.store(true, Ordering::Release);
            return Err(Error::OutcomeUnknown);
        }
        Ok(IncrementalReport {
            base_sequence: base_seq,
            sequence: target_seq,
            files: count,
            payload_bytes: total,
        })
    }
}

// Both variants validate the same actual D/P/base pair. The external token is
// issued only by the creator/namespace-bound caller synchronization authority.
enum IncrementalPublication {
    Coordinated(ulmdb_storage::PreparedSnapshotImport),
    External(ulmdb_storage::ExternalSnapshotImport),
}
impl IncrementalPublication {
    fn prepared(&self) -> &ulmdb_storage::PreparedPublication {
        match self {
            Self::Coordinated(p) => p.prepared(),
            Self::External(p) => p.prepared(),
        }
    }
    fn publish(self) -> Result<()> {
        match self {
            Self::Coordinated(p) => p.publish(),
            Self::External(p) => p.publish(),
        }
    }
}
