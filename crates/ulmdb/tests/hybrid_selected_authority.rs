//! Known-selected asset faults fence writers, never old materialized readers.
use std::{
    fs,
    path::{Path, PathBuf},
};
use ulmdb::{
    DatabaseKind, Error,
    hybrid::{Environment, ImageMode, Options, WalLayout},
};
use ulmdb_storage::hybrid::checkpoint::CheckpointLimits;
struct Dir(PathBuf);
impl Dir {
    fn new(base: &Path) -> Self {
        let path = base.join(format!(
            "ulmdb-selected-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn options(layout: WalLayout, image: ImageMode) -> Options {
    Options {
        wal_layout: layout,
        image,
        chunk: ulmdb_storage::hybrid::ChunkLimits {
            capacity: 1024 * 1024,
            reserve_quantum: 65536,
            max_burned_bytes: 512 * 1024,
        },
        max_image_bytes: 8 * 1024 * 1024,
        ..Options::default()
    }
}
fn set(e: &Environment, n: u64) {
    let mut w = e.begin_write().unwrap();
    if n == 0 {
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    }
    w.put(b"kv", b"state", &n.to_le_bytes()).unwrap();
    w.commit().unwrap();
}
fn overwrite(path: &Path, bytes: &[u8]) {
    if path.exists() {
        ulmdb_platform::set_private_permissions(&std::fs::File::open(path).unwrap(), true).unwrap();
    }
    fs::write(path, bytes).unwrap();
}
#[test]
fn currently_selected_corrupt_missing_or_truncated_assets_fence_and_repair_reopens() {
    for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
        for layout in [
            WalLayout::Dense,
            WalLayout::PreSized {
                capacity: 2 * 1024 * 1024,
            },
        ] {
            for kind in 0..7 {
                let d = Dir::new(base);
                let path = d.0.join("engine");
                let o = options(
                    layout,
                    if kind % 2 == 0 {
                        ImageMode::Anonymous
                    } else {
                        ImageMode::File
                    },
                );
                let e = Environment::create(&path, o).unwrap();
                set(&e, 0);
                let old = e.begin_read();
                let borrowed = old.get(b"kv", b"state").unwrap().unwrap();
                let mut p = e.prepare_checkpoint(CheckpointLimits::default()).unwrap();
                e.activate_checkpoint(&mut p, CheckpointLimits::default())
                    .unwrap();
                set(&e, 1);
                let target = match kind {
                    0 | 6 => p.directory().join("CHECKPOINT"),
                    1 | 3 => p.directory().join("chunk-0000000000000001.image"),
                    2 => p.directory().join("chunk-0000000000000001.origins"),
                    _ => path.join("HYBRID.base"),
                };
                let original = fs::read(&target).unwrap();
                let mut bad = original.clone();
                match kind {
                    0 => bad.truncate(32),
                    6 => bad[264..280].fill(0),
                    1 => bad[4096] ^= 1,
                    2 => bad[0] ^= 1,
                    3 | 4 => {
                        fs::remove_file(&target).unwrap();
                    }
                    _ => bad[0] ^= 1,
                };
                if !matches!(kind, 3 | 4) {
                    overwrite(&target, &bad);
                }
                let wal = fs::read(path.join("HYBRID.wal")).unwrap();
                let error = e
                    .activate_checkpoint(&mut p, CheckpointLimits::default())
                    .err()
                    .unwrap();
                assert!(
                    matches!(
                        error,
                        Error::Corrupt(_) | Error::Io(_) | Error::InvalidInput(_)
                    ),
                    "kind {kind}: {error}"
                );
                assert!(matches!(e.begin_write(), Err(Error::OutcomeUnknown)));
                assert_eq!(borrowed, 0u64.to_le_bytes());
                assert_eq!(wal, fs::read(path.join("HYBRID.wal")).unwrap());
                overwrite(&target, &original);
                drop(e);
                let reopened = Environment::open(&path, o).unwrap();
                assert_eq!(
                    reopened.begin_read().get(b"kv", b"state").unwrap(),
                    Some(1u64.to_le_bytes().as_slice())
                );
                set(&reopened, 2);
            }
        }
    }
}
#[test]
fn unselected_and_obsolete_artifact_failures_do_not_poison_current_authority() {
    let d = Dir::new(Path::new("/tmp"));
    let path = d.0.join("engine");
    let e = Environment::create(&path, options(WalLayout::Dense, ImageMode::Anonymous)).unwrap();
    set(&e, 0);
    let mut unselected = e.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    let asset = unselected.directory().join("CHECKPOINT");
    let original = fs::read(&asset).unwrap();
    overwrite(&asset, b"bad");
    assert!(matches!(
        e.activate_checkpoint(&mut unselected, CheckpointLimits::default()),
        Err(Error::Corrupt(_))
    ));
    set(&e, 1);
    overwrite(&asset, &original);
    e.activate_checkpoint(&mut unselected, CheckpointLimits::default())
        .unwrap();
    set(&e, 2);
    let mut current = e.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    e.activate_checkpoint(&mut current, CheckpointLimits::default())
        .unwrap();
    let selector = fs::read(path.join("HYBRID.base")).unwrap();
    overwrite(&asset, b"bad");
    assert!(matches!(
        e.activate_checkpoint(&mut unselected, CheckpointLimits::default()),
        Err(Error::Corrupt(_))
    ));
    set(&e, 3);
    assert_eq!(selector, fs::read(path.join("HYBRID.base")).unwrap());
}
#[test]
fn all_caller_budgets_and_foreign_issuer_leave_writer_ready() {
    let d = Dir::new(Path::new("/tmp"));
    let path = d.0.join("engine");
    let o = options(WalLayout::Dense, ImageMode::Anonymous);
    let e = Environment::create(&path, o).unwrap();
    set(&e, 0);
    let mut p = e.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    e.activate_checkpoint(&mut p, CheckpointLimits::default())
        .unwrap();
    for kind in 0..4 {
        let mut limits = CheckpointLimits::default();
        match kind {
            0 => limits.max_chunks = 0,
            1 => limits.max_provenance_bytes = 0,
            2 => limits.max_apparent_bytes = 0,
            _ => limits.max_allocated_bytes = 0,
        };
        assert!(
            matches!(
                e.activate_checkpoint(&mut p, limits),
                Err(Error::LimitExceeded)
            ),
            "budget {kind}"
        );
        set(&e, kind + 1);
    }
    let other = Environment::create(d.0.join("other"), o).unwrap();
    set(&other, 0);
    assert!(matches!(
        other.activate_checkpoint(&mut p, CheckpointLimits::default()),
        Err(Error::InvalidInput(_))
    ));
    set(&other, 1);
}

#[test]
fn malicious_u64_descriptor_count_is_corrupt_on_every_host_width_and_fences_selected_writes() {
    let d = Dir::new(Path::new("/tmp"));
    let path = d.0.join("engine");
    let e = Environment::create(&path, options(WalLayout::Dense, ImageMode::Anonymous)).unwrap();
    set(&e, 0);
    let old = e.begin_read();
    let bytes = old.get(b"kv", b"state").unwrap().unwrap();
    let mut prepared = e.prepare_checkpoint(CheckpointLimits::default()).unwrap();
    e.activate_checkpoint(&mut prepared, CheckpointLimits::default())
        .unwrap();
    let descriptor = prepared.directory().join("CHECKPOINT");
    assert_eq!(
        ulmdb_storage::hybrid::checkpoint::read_descriptor(&descriptor, usize::MAX)
            .unwrap()
            .chunks
            .len(),
        1
    );
    let mut bad = fs::read(&descriptor).unwrap();
    bad[168..176].copy_from_slice(&u64::MAX.to_le_bytes());
    overwrite(&descriptor, &bad);
    assert!(matches!(
        ulmdb_storage::hybrid::checkpoint::read_descriptor(&descriptor, usize::MAX),
        Err(Error::Corrupt(_))
    ));
    let wal = fs::read(path.join("HYBRID.wal")).unwrap();
    assert!(matches!(
        e.activate_checkpoint(&mut prepared, CheckpointLimits::default()),
        Err(Error::Corrupt(_))
    ));
    assert!(matches!(e.begin_write(), Err(Error::OutcomeUnknown)));
    assert_eq!(bytes, 0u64.to_le_bytes());
    assert_eq!(wal, fs::read(path.join("HYBRID.wal")).unwrap());
}
