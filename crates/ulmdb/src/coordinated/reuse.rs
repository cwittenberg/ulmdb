//! Bounded metadata-only graph certificates with fresh source authentication.
//!
//! Active readers can share ordinary mappings through weak storage chunk owners.
//! No Snapshot weak reference or strong data/pin owner is retained by the cache;
//! a returned Snapshot stays uniquely mutable for pre-exposure projection. Once
//! the last reader ends, a later hit maps/hashes its sources again and reuses only
//! authenticated catalog/graph metadata. Caller retains fresh Capture/root-policy
//! authority. Short cache locks never cover hashing or full admission.
use super::*;
use std::sync::{Arc, Mutex};

const MAX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SOURCES: usize = 128;
const MAX_DATABASES: usize = 1024;

pub(super) struct Cache {
    creator: u32,
    // The pointer itself is immutable while other threads may update the
    // pointed-to mutex/container. Foreign destruction must not walk that data.
    entry: Arc<Mutex<Option<Arc<Entry>>>>,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            creator: std::process::id(),
            entry: Arc::new(Mutex::new(None)),
        }
    }
}
impl Drop for Cache {
    fn drop(&mut self) {
        if self.creator != std::process::id() {
            // Safe creator-owned destruction without introducing unsafe code:
            // leave an additional Arc in the child, then ordinary field Drop
            // cannot destroy state inherited during a parent-thread mutation.
            std::mem::forget(self.entry.clone());
        }
    }
}
struct Entry {
    certificate: Certificate,
    // This retains only weak Chunk owners, never a Snapshot/mapping/pin owner.
    live_table: Mutex<ulmdb_storage::WeakPlainSegmentTable>,
}

/// Explicit metadata-only evidence, constructible only after full admission and
/// both-sided source binding. Legacy Manifest has a flat inventory; Database
/// holds roots/IDs/order descriptors, not a borrowed view or data owner.
struct Certificate {
    checkpoint: [u8; 64],
    sources: Vec<Source>,
    manifest: ulmdb_storage::Manifest,
    catalog: std::collections::BTreeMap<Vec<u8>, crate::catalog::Database>,
    profile: SnapshotProfile,
    read_ahead: crate::ReadAhead,
}

#[derive(Clone, Copy)]
enum SnapshotProfile {
    // These fixed flags exclude range/external/fixed/chunked/transformed state.
    PlainLegacyDemand,
}
struct Source {
    path: PathBuf,
    metadata: Fingerprint,
}
#[cfg(unix)]
#[derive(PartialEq, Eq)]
struct Fingerprint {
    device: u64,
    inode: u64,
    length: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    links: u64,
    blocks: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
#[cfg(not(unix))]
#[derive(PartialEq, Eq)]
struct Fingerprint;

fn fingerprint(path: &Path) -> Result<Fingerprint> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(path)?;
        if m.file_type().is_symlink() || !(m.is_file() || m.is_dir()) {
            return Err(Error::Corrupt("invalid reused source type"));
        }
        Ok(Fingerprint {
            device: m.dev(),
            inode: m.ino(),
            // Publishers may add/rename unrelated entries while Capture SH
            // protects this older selected root. Directory contents/timestamps
            // are not immutable source identity or access-policy evidence.
            length: if m.is_dir() { 0 } else { m.len() },
            mode: m.mode(),
            uid: m.uid(),
            gid: m.gid(),
            links: if m.is_dir() { 0 } else { m.nlink() },
            blocks: if m.is_dir() { 0 } else { m.blocks() },
            modified: if m.is_dir() {
                (0, 0)
            } else {
                (m.mtime(), m.mtime_nsec())
            },
            changed: if m.is_dir() {
                (0, 0)
            } else {
                (m.ctime(), m.ctime_nsec())
            },
        })
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(Error::InvalidInput(
            "source reuse unsupported on this platform",
        ))
    }
}

fn sources(directory: &Path, manifest: &ulmdb_storage::Manifest) -> Result<Vec<Source>> {
    let mut paths = std::collections::BTreeSet::new();
    paths.insert(directory.to_path_buf());
    paths.insert(directory.join("LOCK"));
    paths.insert(ulmdb_storage::descriptor_path(directory, manifest));
    for entry in manifest.segments.iter() {
        paths.insert(ulmdb_storage::segment_path(directory, entry));
    }
    let originals: Vec<_> = paths.iter().cloned().collect();
    for path in originals {
        let mut parent = path.parent();
        while let Some(p) = parent {
            if !p.starts_with(directory) {
                break;
            }
            paths.insert(p.to_path_buf());
            if p == directory {
                break;
            }
            parent = p.parent();
        }
    }
    paths
        .into_iter()
        .map(|path| {
            Ok(Source {
                metadata: fingerprint(&path)?,
                path,
            })
        })
        .collect()
}
fn unchanged(sources: &[Source]) -> Result<bool> {
    let mut same = true;
    for source in sources {
        same &= fingerprint(&source.path)? == source.metadata;
    }
    Ok(same)
}
fn eligible(manifest: &ulmdb_storage::Manifest) -> bool {
    cfg!(unix)
        // Cohort prefixes can retain a much larger inode; Indexed also owns
        // metadata resolvers. Their original admission remains unchanged until
        // complete physical/certificate accounting is separately qualified.
        && manifest.format_minor < 3
        && manifest.segments.len() <= MAX_SOURCES
        && manifest
            .segments
            .iter()
            .try_fold(0u64, |sum, e| sum.checked_add(e.length))
            .is_some_and(|bytes| bytes <= MAX_BYTES)
}
fn retained_bytes_within_budget(sources: &[Source]) -> bool {
    #[cfg(unix)]
    {
        let mut total = 0u64;
        for source in sources {
            if source.metadata.mode & libc::S_IFMT == libc::S_IFREG {
                let charge = source
                    .metadata
                    .length
                    .max(source.metadata.blocks.saturating_mul(512));
                let Some(next) = total.checked_add(charge) else {
                    return false;
                };
                total = next;
            }
        }
        total <= MAX_BYTES
    }
    #[cfg(not(unix))]
    {
        let _ = sources;
        false
    }
}
fn check_current_file_access(sources: &[Source]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        for source in sources {
            if source.metadata.mode & libc::S_IFMT != libc::S_IFREG {
                continue;
            }
            let file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
                .open(&source.path)?;
            let actual = file.metadata()?;
            if !actual.is_file()
                || actual.dev() != source.metadata.device
                || actual.ino() != source.metadata.inode
                || actual.len() != source.metadata.length
            {
                return Err(Error::Corrupt("reused source opened identity changed"));
            }
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = sources;
        Err(Error::InvalidInput(
            "source reuse unsupported on this platform",
        ))
    }
}

// Do not reuse graph evidence for fixed-address/placement declarations. This
// conservative namespace gate intentionally leaves advanced behavior on the
// existing loader; errors are not reclassified by this optional cache probe.
fn ordinary_namespace(directory: &Path) -> bool {
    ["FIXEDMAP", "FIXEDMAP-LEASES"].iter().all(|name| {
        matches!(std::fs::symlink_metadata(directory.join(name)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    })
}

impl Certificate {
    fn new(
        read: &ReadTxn,
        checkpoint: [u8; 64],
        sources: Vec<Source>,
        advice: crate::ReadAhead,
    ) -> Option<Self> {
        let snapshot = &read.snapshot;
        if !eligible(&snapshot.manifest)
            || snapshot.range_backed
            || snapshot.external.is_some()
            || snapshot.manifest.transform_context().is_some()
            || snapshot.segments.is_chunked()
            || snapshot.segments.transform().is_some()
            || snapshot.catalog.len() > MAX_DATABASES
            || snapshot.segments.read_ahead() != advice
            || snapshot
                .physical_pin
                .as_ref()
                .is_none_or(|pin| pin.root().checkpoint() != &checkpoint)
            || snapshot.admission.as_ref().is_none_or(|a| {
                a.report().policy != ResidencyPolicy::Demand || !a.is_for_table(&snapshot.segments)
            })
            || !retained_bytes_within_budget(&sources)
        {
            return None;
        }
        #[cfg(feature = "range-lifecycle")]
        if snapshot.range_checkpoint.is_some() {
            return None;
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if snapshot.segments.fixed_map().is_some()
            || snapshot.segments.fixed_declaration().is_some()
        {
            return None;
        }
        Some(Self {
            checkpoint,
            sources,
            manifest: snapshot.manifest.clone(),
            catalog: snapshot.catalog.clone(),
            profile: SnapshotProfile::PlainLegacyDemand,
            read_ahead: advice,
        })
    }

    fn reader(
        &self,
        segments: ulmdb_storage::SegmentTable,
        pin: ulmdb_storage::coordination::PhysicalPin,
    ) -> Result<ReadTxn> {
        let SnapshotProfile::PlainLegacyDemand = self.profile;
        // ResidencyAdmission is tied to a table revision. Never retain/reuse an
        // old token after remapping, even though Demand has no residency lease.
        let controller = ulmdb_storage::ResidencyController::new(ResidencyPolicy::Demand)?;
        let admission = controller.admit(&segments)?;
        Ok(ReadTxn {
            snapshot: Arc::new(crate::environment::Snapshot {
                range_backed: false,
                external: None,
                physical_pin: Some(Arc::new(pin)),
                #[cfg(feature = "range-lifecycle")]
                range_checkpoint: None,
                owner: crate::client_snapshot::snapshot_owner()?,
                manifest: self.manifest.clone(),
                segments,
                catalog: self.catalog.clone(),
                admission: Some(admission),
            }),
        })
    }
}

impl Entry {
    fn reader(
        &self,
        directory: &Path,
        pin: ulmdb_storage::coordination::PhysicalPin,
    ) -> Result<ReadTxn> {
        // Caller still owns the fresh Capture/root-policy guard through this
        // operation. The cache supplies no independent currentness authority.
        // Permission and source identity checks still precede content reuse.
        ulmdb_storage::access::FileAccessContext::open(directory)?.check()?;
        check_current_file_access(&self.certificate.sources)?;
        // The weak-owner mutex is held only for bounded Arc upgrades, never
        // while opening sources, hashing bytes, or admitting a table.
        let active = self.live_table.lock().map_err(|_| Error::Busy)?.upgrade()?;
        let segments = match active {
            Some(segments) => {
                // A genuine overlapping reader owns these bytes; this reader
                // independently authenticates them before reusing graph proof.
                segments.revalidate_mapped_content()?;
                segments
            }
            None => {
                // No data owner survives solely for this cache. Map/open/hash
                // all source bytes through the ordinary production loader.
                // Reusing graph proof is safe only after this fresh SHA pass.
                ulmdb_storage::SegmentTable::load_with_profile(
                    directory,
                    &self.certificate.manifest.segments,
                    self.certificate.read_ahead,
                    None,
                    self.certificate.manifest.value_limit,
                )?
                .with_page_size(self.certificate.manifest.page_size)?
            }
        };
        if !unchanged(&self.certificate.sources)? {
            return Err(Error::Corrupt("source changed during reused admission"));
        }
        let read = self.certificate.reader(segments, pin)?;
        // No Snapshot downgrade: the returned Arc must remain uniquely mutable
        // by a caller's legitimate pre-exposure empty-MAIN projection.
        let weak = read
            .snapshot
            .segments
            .downgrade_plain()?
            .ok_or(Error::InvalidInput(
                "ordinary graph reuse acquired advanced table",
            ))?;
        *self.live_table.lock().map_err(|_| Error::Busy)? = weak;
        Ok(read)
    }
}

fn same_entry(current: Option<&Arc<Entry>>, observed: Option<&Arc<Entry>>) -> bool {
    match (current, observed) {
        (None, None) => true,
        (Some(current), Some(observed)) => Arc::ptr_eq(current, observed),
        _ => false,
    }
}

impl Cache {
    fn discard_if_current(&self, observed: &Option<Arc<Entry>>) -> Result<()> {
        let removed = {
            let mut current = self.entry.lock().map_err(|_| Error::Busy)?;
            if same_entry(current.as_ref(), observed.as_ref()) {
                current.take()
            } else {
                None
            }
        };
        drop(removed);
        Ok(())
    }

    fn install_if_current(&self, observed: &Option<Arc<Entry>>, entry: Arc<Entry>) -> Result<()> {
        let replaced = {
            let mut current = self.entry.lock().map_err(|_| Error::Busy)?;
            if same_entry(current.as_ref(), observed.as_ref()) {
                current.replace(entry)
            } else {
                None
            }
        };
        drop(replaced);
        Ok(())
    }

    pub(super) fn admit(
        &self,
        directory: &Path,
        policy: ResidencyPolicy,
        pin: ulmdb_storage::coordination::PhysicalPin,
        advice: crate::ReadAhead,
        transform: Option<Arc<ulmdb_storage::transform::TransformContext>>,
        chunks: Option<crate::chunked::CacheOptions>,
    ) -> Result<ReadTxn> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited admission reuse cache"));
        }
        // Only an immutable metadata Arc escapes this short cache lock. Every
        // serving read still gets its own fresh pin, owner and Snapshot Arc.
        let observed = self.entry.lock().map_err(|_| Error::Busy)?.clone();
        if policy != ResidencyPolicy::Demand
            || transform.is_some()
            || chunks.is_some()
            || !eligible(pin.root().manifest())
            || !ordinary_namespace(directory)
        {
            self.discard_if_current(&observed)?;
            return ReadTxn::from_physical_pin_with_cache_options(
                directory, policy, pin, advice, transform, chunks,
            );
        }
        advice.validate()?;
        if !pin.matches_directory(directory)? {
            return Err(Error::Corrupt("reused admission source namespace"));
        }
        let checkpoint = *pin.root().checkpoint();
        if let Some(entry) = observed.as_ref()
            && entry.certificate.checkpoint == checkpoint
            && entry.certificate.read_ahead == advice
        {
            let same = match unchanged(&entry.certificate.sources) {
                Ok(same) => same,
                Err(error) => {
                    // A stale worker must not evict another thread's new head.
                    // Cache cleanup cannot mask the original source error.
                    let _ = self.discard_if_current(&observed);
                    return Err(error);
                }
            };
            if same {
                let result = entry.reader(directory, pin);
                if result.is_err() {
                    let _ = self.discard_if_current(&observed);
                }
                return result;
            }
        }
        let before = sources(directory, pin.root().manifest()).ok();
        // Cold and changed heads still use complete original admission. Neither
        // this loader nor fresh SHA validation executes under the cache mutex.
        let read = match ReadTxn::from_physical_pin_with_cache_options(
            directory, policy, pin, advice, None, None,
        ) {
            Ok(read) => read,
            Err(error) => {
                let _ = self.discard_if_current(&observed);
                return Err(error);
            }
        };
        if let Some(before) = before
            && unchanged(&before).unwrap_or(false)
            && let Some(certificate) = Certificate::new(&read, checkpoint, before, advice)
            && let Some(live_table) = read.snapshot.segments.downgrade_plain().ok().flatten()
        {
            // One installed entry, unchanged source/metadata budgets. Losing an
            // install race is a harmless uncached success, not stale authority.
            let entry = Arc::new(Entry {
                certificate,
                live_table: Mutex::new(live_table),
            });
            let _ = self.install_if_current(&observed, entry);
        } else {
            let _ = self.discard_if_current(&observed);
        }
        Ok(read)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{FileExt, MetadataExt, PermissionsExt},
    };
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "ulmdb-admission-reuse-{}",
                ulmdb_storage::new_identity().unwrap()
            )))
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture(mode: crate::PublicationMode) -> (Directory, Environment) {
        let directory = Directory::new();
        let env = Environment::create_with_publication(&directory.0, mode).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"records", crate::DatabaseKind::Kv)
            .unwrap();
        w.put(b"records", b"key", b"old").unwrap();
        w.commit().unwrap();
        (directory, env)
    }
    #[test]
    fn admission_reuse_fresh_pins_owners_and_changed_head() {
        for mode in [
            crate::PublicationMode::Legacy,
            crate::PublicationMode::Cohort,
            crate::PublicationMode::Indexed,
        ] {
            let (directory, env) = fixture(mode);
            let (coordinator, initial) =
                CoordinatedEnvironment::open_with_initial_read(&directory.0).unwrap();
            let next = coordinator.begin_read().unwrap();
            if mode == crate::PublicationMode::Legacy {
                assert_eq!(
                    initial.get(b"records", b"key").unwrap().unwrap().as_ptr(),
                    next.get(b"records", b"key").unwrap().unwrap().as_ptr()
                );
            } else {
                assert!(coordinator.admission_reuse.entry.lock().unwrap().is_none());
            }
            assert_ne!(initial.snapshot.owner, next.snapshot.owner);
            assert!(!Arc::ptr_eq(
                initial.snapshot.physical_pin.as_ref().unwrap(),
                next.snapshot.physical_pin.as_ref().unwrap()
            ));
            let mut w = env.begin_write().unwrap();
            w.put(b"records", b"key", b"new").unwrap();
            w.commit().unwrap();
            let changed = coordinator.begin_read().unwrap();
            assert_eq!(
                changed.get(b"records", b"key").unwrap(),
                Some(b"new".as_slice())
            );
            assert_eq!(
                initial.get(b"records", b"key").unwrap(),
                Some(b"old".as_slice())
            );
            assert_eq!(
                next.get(b"records", b"key").unwrap(),
                Some(b"old".as_slice())
            );
        }
    }
    #[test]
    fn admission_reuse_does_not_retain_a_reader_pin_after_end() {
        let (directory, _env) = fixture(crate::PublicationMode::Legacy);
        let (coordinator, initial) =
            CoordinatedEnvironment::open_with_initial_read(&directory.0).unwrap();
        let pin = Arc::downgrade(initial.snapshot.physical_pin.as_ref().unwrap());
        drop(initial);
        // Lease names have best-effort cleanup; no live physical capability
        // may be retained merely by keeping the graph/mapping certificate.
        assert!(pin.upgrade().is_none());
        assert!(
            coordinator
                .admission_reuse
                .entry
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .live_table
                .lock()
                .unwrap()
                .upgrade()
                .unwrap()
                .is_none()
        );
        let next = coordinator.begin_read().unwrap();
        assert_eq!(
            next.get(b"records", b"key").unwrap(),
            Some(b"old".as_slice())
        );
    }
    #[test]
    fn admission_reuse_foreign_drop_does_not_destroy_mutable_cache_state() {
        let mut cache = Cache::default();
        let weak = Arc::downgrade(&cache.entry);
        let observer = cache.entry.clone();
        let held = observer.lock().unwrap();
        cache.creator = cache.creator.wrapping_add(1);
        drop(cache);
        drop(held);
        drop(observer);
        // A foreign Drop leaks the inherited mutable graph until exec/exit.
        assert!(weak.upgrade().is_some());
    }
    #[test]
    fn admission_reuse_same_inode_corruption_is_not_hidden_by_metadata() {
        let (directory, _env) = fixture(crate::PublicationMode::Legacy);
        let (coordinator, initial) =
            CoordinatedEnvironment::open_with_initial_read(&directory.0).unwrap();
        let entry = &initial.snapshot.manifest.segments[0];
        let path = ulmdb_storage::segment_path(&directory.0, entry);
        let original = fs::metadata(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        fs::set_permissions(&path, original.permissions()).unwrap();
        let refreshed = coordinator.begin_read().unwrap();
        file.write_all_at(b"X", 0).unwrap();
        file.set_times(fs::FileTimes::new().set_modified(original.modified().unwrap()))
            .unwrap();
        assert_eq!(original.ino(), fs::metadata(&path).unwrap().ino());
        // Force the metadata-match branch as a negative control: even perfect
        // metadata equality cannot substitute for the fresh content digest.
        {
            let mut cached = coordinator.admission_reuse.entry.lock().unwrap();
            Arc::get_mut(cached.as_mut().unwrap())
                .unwrap()
                .certificate
                .sources = sources(&directory.0, &refreshed.snapshot.manifest).unwrap();
        }
        assert!(matches!(
            coordinator.begin_read(),
            Err(Error::Corrupt("reused segment hash validation failed"))
        ));
        assert!(coordinator.admission_reuse.entry.lock().unwrap().is_none());
    }
    #[test]
    fn admission_reuse_replacement_inode_requires_full_admission() {
        let (directory, _env) = fixture(crate::PublicationMode::Legacy);
        let (coordinator, initial) =
            CoordinatedEnvironment::open_with_initial_read(&directory.0).unwrap();
        let entry = &initial.snapshot.manifest.segments[0];
        let path = ulmdb_storage::segment_path(&directory.0, entry);
        let bytes = fs::read(&path).unwrap();
        let permissions = fs::metadata(&path).unwrap().permissions();
        let moved = path.with_extension("held");
        fs::rename(&path, &moved).unwrap();
        fs::write(&path, &bytes).unwrap();
        fs::set_permissions(&path, permissions).unwrap();
        let next = coordinator.begin_read().unwrap();
        assert_ne!(
            initial.get(b"records", b"key").unwrap().unwrap().as_ptr(),
            next.get(b"records", b"key").unwrap().unwrap().as_ptr()
        );
        assert_eq!(
            next.get(b"records", b"key").unwrap(),
            Some(b"old".as_slice())
        );
    }
    #[test]
    fn admission_reuse_parent_symlink_is_rejected() {
        let (directory, _env) = fixture(crate::PublicationMode::Legacy);
        let (coordinator, _initial) =
            CoordinatedEnvironment::open_with_initial_read(&directory.0).unwrap();
        let original = directory.0.join("segments");
        let moved = directory.0.join("held-segments");
        fs::rename(&original, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &original).unwrap();
        assert!(coordinator.begin_read().is_err());
    }
    #[test]
    fn admission_reuse_ignores_unrelated_directory_entry_changes() {
        let (directory, _env) = fixture(crate::PublicationMode::Legacy);
        let (coordinator, initial) =
            CoordinatedEnvironment::open_with_initial_read(&directory.0).unwrap();
        fs::write(directory.0.join("operator-note"), b"keep").unwrap();
        fs::write(directory.0.join("segments").join("operator-note"), b"keep").unwrap();
        let next = coordinator.begin_read().unwrap();
        assert_eq!(
            initial.get(b"records", b"key").unwrap().unwrap().as_ptr(),
            next.get(b"records", b"key").unwrap().unwrap().as_ptr()
        );
    }
    #[test]
    fn admission_reuse_inventory_cap_does_not_retain_an_extra_pin() {
        let (directory, mut env) = fixture(crate::PublicationMode::Legacy);
        env.set_maintenance_policy(crate::MaintenancePolicy::Disabled)
            .unwrap();
        for i in 0..MAX_SOURCES + 1 {
            let mut w = env.begin_write().unwrap();
            w.put(b"records", b"key", &i.to_be_bytes()).unwrap();
            w.commit().unwrap();
        }
        let coordinator = CoordinatedEnvironment::open(&directory.0).unwrap();
        assert!(coordinator.begin_read().is_ok());
        assert!(coordinator.admission_reuse.entry.lock().unwrap().is_none());
    }

    #[test]
    fn admission_reuse_metadata_certificate_retains_no_snapshot_pin_or_data_owner() {
        let (directory, _env) = fixture(crate::PublicationMode::Legacy);
        let (coordinator, initial) =
            CoordinatedEnvironment::open_with_initial_read(&directory.0).unwrap();
        let snapshot = Arc::downgrade(&initial.snapshot);
        let pin = Arc::downgrade(initial.snapshot.physical_pin.as_ref().unwrap());
        drop(initial);
        assert!(snapshot.upgrade().is_none());
        assert!(pin.upgrade().is_none());
        {
            let retained = coordinator.admission_reuse.entry.lock().unwrap();
            let entry = retained.as_ref().unwrap();
            assert!(
                entry
                    .live_table
                    .lock()
                    .unwrap()
                    .upgrade()
                    .unwrap()
                    .is_none()
            );
            assert_eq!(entry.certificate.manifest.sequence, 1);
            assert_eq!(entry.certificate.catalog.len(), 1);
        }
        // This is the expired-owner branch: ordinary loading reopens/maps/SHA
        // authenticates every data file, then reuses the graph/catalog evidence.
        let next = coordinator.begin_read().unwrap();
        assert_eq!(
            next.get(b"records", b"key").unwrap(),
            Some(b"old".as_slice())
        );
        assert_eq!(Arc::strong_count(&next.snapshot), 1);
        assert_eq!(Arc::weak_count(&next.snapshot), 0);
    }

    #[test]
    fn admission_reuse_snapshot_uniqueness_preserves_empty_database_projection() {
        let (directory, _env) = fixture(crate::PublicationMode::Legacy);
        let (coordinator, initial) =
            CoordinatedEnvironment::open_with_initial_read(&directory.0).unwrap();
        assert_eq!(Arc::strong_count(&initial.snapshot), 1);
        assert_eq!(Arc::weak_count(&initial.snapshot), 0);
        let projected = initial.project_empty_kv(b"virtual-main").unwrap();
        assert_eq!(projected.get(b"virtual-main", b"absent").unwrap(), None);
        let next = coordinator.begin_read().unwrap();
        assert_eq!(Arc::weak_count(&next.snapshot), 0);
        assert!(
            next.open_database(b"virtual-main").is_err(),
            "projection must not alter the certificate catalog"
        );
        assert_eq!(
            projected.get(b"records", b"key").unwrap().unwrap().as_ptr(),
            next.get(b"records", b"key").unwrap().unwrap().as_ptr()
        );
        let next = next.project_empty_kv(b"virtual-main").unwrap();
        assert_eq!(next.get(b"virtual-main", b"absent").unwrap(), None);
    }

    #[test]
    fn admission_reuse_metadata_catalog_cap_rejects_before_retaining_a_data_owner() {
        let (directory, _env) = fixture(crate::PublicationMode::Legacy);
        let (coordinator, mut initial) =
            CoordinatedEnvironment::open_with_initial_read(&directory.0).unwrap();
        coordinator.admission_reuse.entry.lock().unwrap().take();
        let checkpoint = initial.file_checkpoint().unwrap();
        // A private negative-control fixture exercises the metadata budget;
        // duplicated IDs are never served or claimed as a valid new graph.
        let snapshot = Arc::get_mut(&mut initial.snapshot).unwrap();
        let database = snapshot.catalog.values().next().unwrap().clone();
        for n in 0..MAX_DATABASES {
            snapshot
                .catalog
                .insert(format!("extra-{n}").into_bytes(), database.clone());
        }
        assert!(
            Certificate::new(
                &initial,
                checkpoint,
                sources(&directory.0, &initial.snapshot.manifest).unwrap(),
                crate::ReadAhead::Normal
            )
            .is_none()
        );
        let snapshot = Arc::downgrade(&initial.snapshot);
        let pin = Arc::downgrade(initial.snapshot.physical_pin.as_ref().unwrap());
        drop(initial);
        assert!(snapshot.upgrade().is_none());
        assert!(pin.upgrade().is_none());
        assert!(coordinator.admission_reuse.entry.lock().unwrap().is_none());
    }

    #[test]
    fn admission_reuse_expired_table_rehashes_before_graph_certificate_reuse() {
        let (directory, _env) = fixture(crate::PublicationMode::Legacy);
        let (coordinator, initial) =
            CoordinatedEnvironment::open_with_initial_read(&directory.0).unwrap();
        let entry = initial.snapshot.manifest.segments[0].clone();
        let manifest = initial.snapshot.manifest.clone();
        let path = ulmdb_storage::segment_path(&directory.0, &entry);
        drop(initial);
        assert!(
            coordinator
                .admission_reuse
                .entry
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .live_table
                .lock()
                .unwrap()
                .upgrade()
                .unwrap()
                .is_none()
        );
        let original = fs::metadata(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        fs::set_permissions(&path, original.permissions()).unwrap();
        file.write_all_at(b"X", 0).unwrap();
        // Force metadata equality: expired mappings must still load/hash data.
        {
            let mut current = coordinator.admission_reuse.entry.lock().unwrap();
            Arc::get_mut(current.as_mut().unwrap())
                .unwrap()
                .certificate
                .sources = sources(&directory.0, &manifest).unwrap();
        }
        assert!(matches!(coordinator.begin_read(), Err(Error::Corrupt(_))));
        assert!(coordinator.admission_reuse.entry.lock().unwrap().is_none());
    }

    #[test]
    fn admission_reuse_conditional_invalidation_cannot_clobber_a_new_entry() {
        let (directory, env) = fixture(crate::PublicationMode::Legacy);
        let (coordinator, initial) =
            CoordinatedEnvironment::open_with_initial_read(&directory.0).unwrap();
        let observed = coordinator.admission_reuse.entry.lock().unwrap().clone();
        let mut write = env.begin_write().unwrap();
        write.put(b"records", b"key", b"new").unwrap();
        write.commit().unwrap();
        let next = coordinator.begin_read().unwrap();
        let current = coordinator.admission_reuse.entry.lock().unwrap().clone();
        assert!(!same_entry(current.as_ref(), observed.as_ref()));
        coordinator
            .admission_reuse
            .discard_if_current(&observed)
            .unwrap();
        assert!(same_entry(
            coordinator.admission_reuse.entry.lock().unwrap().as_ref(),
            current.as_ref()
        ));
        coordinator
            .admission_reuse
            .install_if_current(&observed, observed.as_ref().unwrap().clone())
            .unwrap();
        assert!(same_entry(
            coordinator.admission_reuse.entry.lock().unwrap().as_ref(),
            current.as_ref()
        ));
        assert_eq!(
            initial.get(b"records", b"key").unwrap(),
            Some(b"old".as_slice())
        );
        assert_eq!(
            next.get(b"records", b"key").unwrap(),
            Some(b"new".as_slice())
        );
    }
}
