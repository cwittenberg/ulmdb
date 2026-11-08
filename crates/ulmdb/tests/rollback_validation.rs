//! Rollback uses ordinary file-open authentication, not a backup/full-tree audit.
//! Fixtures are sealed through storage before any native owner maps them. Raw
//! damage is introduced only while no owner or borrowed mapping remains alive.
use std::{fs, ops::Bound, path::PathBuf};
use ulmdb::{Environment, Error, PublicationMode, RollbackError, backup};
use ulmdb_storage::{Arena, Manifest, Reference, SegmentTable, WriteStore};

const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];

#[derive(Clone, Copy, Debug)]
enum Shape {
    Valid,
    BadUserTree,
    BadCatalogEncoding,
    DuplicateCatalogId,
    BadCatalogAllocator,
    BadInventoryCounter,
    BadInventoryLength,
}

struct Fixture {
    path: PathBuf,
    a: Manifest,
    a_current: Vec<u8>,
    b_current: Vec<u8>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn database(root: Reference) -> [u8; 80] {
    let mut bytes = [0; 80];
    bytes[..8].copy_from_slice(&1u64.to_le_bytes());
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes()); // KV
    bytes[10..12].copy_from_slice(&1u16.to_le_bytes());
    bytes[16..32].copy_from_slice(&root.encode());
    bytes[32..40].copy_from_slice(&4096u64.to_le_bytes());
    bytes[40..48].copy_from_slice(&67_108_864u64.to_le_bytes());
    bytes[48..56].copy_from_slice(&65_536u64.to_le_bytes());
    bytes[56..64].copy_from_slice(&u64::MAX.to_le_bytes());
    bytes
}

fn publish(path: &std::path::Path, base: &Manifest, shape: Shape, value: &[u8]) -> Manifest {
    // B deliberately has its own complete inventory. A-only damage must not be
    // mistaken for a B-open failure, or hidden by B's already-loaded mappings.
    let mut arena = Arena::new(SegmentTable::default(), base.next_segment).unwrap();
    let root = match shape {
        Shape::BadUserTree => arena.allocate(&[0u8; 4096], 4096).unwrap(),
        _ => ulmdb_kv::put(&mut arena, Reference::NULL, b"key", value).unwrap(),
    };
    let mut descriptor = database(root);
    if matches!(shape, Shape::BadCatalogEncoding) {
        descriptor[10..12].copy_from_slice(&9u16.to_le_bytes());
    }
    let mut catalog = ulmdb_kv::put(&mut arena, Reference::NULL, b"kv", &descriptor).unwrap();
    if matches!(shape, Shape::DuplicateCatalogId) {
        catalog = ulmdb_kv::put(&mut arena, catalog, b"other", &descriptor).unwrap();
    }
    let (table, _) = arena.seal(path).unwrap();
    let mut entries = table.entries();
    let mut manifest = base.clone();
    manifest.generation = ulmdb_storage::new_identity().unwrap();
    manifest.sequence = base.sequence + 1;
    manifest.commit = manifest.generation;
    manifest.parent = base.commit;
    manifest.previous = base.generation;
    manifest.catalog = catalog;
    manifest.next_database = if matches!(shape, Shape::BadCatalogAllocator) {
        1
    } else {
        2
    };
    manifest.next_segment = entries.last().unwrap().id + 1;
    if matches!(shape, Shape::BadInventoryCounter) {
        manifest.next_segment = entries.last().unwrap().id;
    }
    if matches!(shape, Shape::BadInventoryLength) {
        entries[0].length += 4096;
    }
    manifest.segments = entries.into();
    if manifest.format_minor == 4 {
        manifest.prepare_indexed(None).unwrap();
    }
    ulmdb_storage::publish(path, &manifest).unwrap();
    manifest
}

fn fixture(mode: PublicationMode, a_shape: Shape, b_shape: Shape) -> Fixture {
    let path = std::env::temp_dir().join(format!(
        "ulmdb-rollback-validation-{}",
        ulmdb_storage::new_identity().unwrap()
    ));
    drop(Environment::create_with_publication(&path, mode).unwrap());
    let genesis = ulmdb_storage::read_current(&path).unwrap();
    let a = publish(&path, &genesis, a_shape, b"old");
    let a_current = fs::read(path.join("CURRENT")).unwrap();
    publish(&path, &a, b_shape, b"new");
    let b_current = fs::read(path.join("CURRENT")).unwrap();
    Fixture {
        path,
        a,
        a_current,
        b_current,
    }
}

fn pins(fixture: &Fixture) -> usize {
    fs::read_dir(fixture.path.join(".ulmdb-coordination/pins"))
        .unwrap()
        .count()
}

fn rejects_predecessor_without_selection(fixture: &Fixture) {
    // Ordinary B open is healthy and cannot have visited the damaged A graph.
    let env = Environment::open(&fixture.path).unwrap();
    assert_eq!(
        env.begin_read().get(b"kv", b"key").unwrap(),
        Some(&b"new"[..])
    );
    let count = pins(fixture);
    assert!(matches!(
        env.rollback_last_commit(2),
        Err(RollbackError::Engine(Error::Corrupt(_)))
    ));
    assert_eq!(
        fs::read(fixture.path.join("CURRENT")).unwrap(),
        fixture.b_current
    );
    assert_eq!(env.begin_read().sequence(), 2);
    assert_eq!(
        pins(fixture),
        count,
        "preselection failure leaked a physical pin"
    );
    env.begin_write().unwrap().abort(); // Failure does not poison or strand writer ownership.
}

#[test]
fn rollback_preserves_descriptor_inventory_and_catalog_rejections() {
    for mode in MODES {
        for shape in [
            Shape::BadCatalogEncoding,
            Shape::DuplicateCatalogId,
            Shape::BadCatalogAllocator,
            Shape::BadInventoryCounter,
            Shape::BadInventoryLength,
        ] {
            let fixture = fixture(mode, shape, Shape::Valid);
            rejects_predecessor_without_selection(&fixture);
        }
        let fixture = fixture(mode, Shape::Valid, Shape::Valid);
        let path = ulmdb_storage::descriptor_path(&fixture.path, &fixture.a);
        let mut bytes = fs::read(&path).unwrap();
        bytes[20] ^= 1; // Descriptor hash/header authentication must remain strict.
        fs::set_permissions(&path, writable_permissions(&path)).unwrap();
        fs::write(path, bytes).unwrap();
        rejects_predecessor_without_selection(&fixture);
    }
}

fn writable_permissions(path: &std::path::Path) -> fs::Permissions {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o600);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(false);
    permissions
}

#[test]
fn rollback_reauthenticates_a_only_segment_bytes_and_physical_length() {
    for mode in MODES {
        for truncate in [false, true] {
            let fixture = fixture(mode, Shape::Valid, Shape::Valid);
            let path = ulmdb_storage::segment_path(&fixture.path, &fixture.a.segments[0]);
            let mut bytes = fs::read(&path).unwrap();
            if truncate {
                bytes.pop();
            } else {
                bytes[4096] ^= 1;
            }
            fs::set_permissions(&path, writable_permissions(&path)).unwrap();
            fs::write(path, bytes).unwrap();
            rejects_predecessor_without_selection(&fixture);
        }
    }
}

fn assert_checked_user_tree_failure(env: &Environment, path: &std::path::Path) {
    let read = env.begin_read();
    assert!(matches!(read.get(b"kv", b"key"), Err(Error::Corrupt(_))));
    assert!(matches!(read.kv(b"kv"), Err(Error::Corrupt(_))));
    assert!(matches!(
        read.range(b"kv", Bound::Unbounded, Bound::Unbounded, false),
        Err(Error::Corrupt(_))
    ));
    assert!(matches!(backup::verify(env), Err(Error::Corrupt(_))));
    let target = path.join("must-not-export");
    assert!(matches!(
        backup::export(env, &target),
        Err(Error::Corrupt(_))
    ));
    assert!(
        !target.exists(),
        "full audit must reject before export creates a target"
    );
}

#[test]
fn hash_valid_malformed_user_tree_uses_ordinary_open_lazy_checks_after_rollback() {
    for mode in MODES {
        let fixture = fixture(mode, Shape::BadUserTree, Shape::Valid);
        // Prove normal native open already has this lazy user-tree contract.
        // Restore the captured publication bytes only while no owner is alive.
        fs::write(fixture.path.join("CURRENT"), &fixture.a_current).unwrap();
        let ordinary = Environment::open(&fixture.path).unwrap();
        assert_checked_user_tree_failure(&ordinary, &fixture.path);
        drop(ordinary);
        fs::write(fixture.path.join("CURRENT"), &fixture.b_current).unwrap();
        let env = Environment::open(&fixture.path).unwrap();
        assert_eq!(env.rollback_last_commit(2).unwrap().sequence, 1);
        assert_checked_user_tree_failure(&env, &fixture.path);
        drop(env);
        let reopened = Environment::open(&fixture.path).unwrap();
        assert_eq!(reopened.begin_read().sequence(), 1);
        assert_checked_user_tree_failure(&reopened, &fixture.path);
    }
}

#[test]
fn hash_valid_malformed_undone_user_tree_does_not_block_restoring_healthy_a() {
    for mode in MODES {
        let fixture = fixture(mode, Shape::Valid, Shape::BadUserTree);
        let env = Environment::open(&fixture.path).unwrap();
        assert_checked_user_tree_failure(&env, &fixture.path);
        assert_eq!(env.rollback_last_commit(2).unwrap().sequence, 1);
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(&b"old"[..])
        );
        backup::verify(&env).unwrap();
    }
}
