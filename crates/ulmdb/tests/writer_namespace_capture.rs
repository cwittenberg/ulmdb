#![cfg(target_os = "linux")]
use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};
use ulmdb::{
    DatabaseKind, Environment, Error, OwnedFileCapture, PublicationMode, ReadTxn, ResidencyPolicy,
};
use ulmdb_storage::coordination::{Coordination, Limits};
#[path = "support/pin_records.rs"]
mod pin_records;
const MODES: [PublicationMode; 3] = [
    PublicationMode::Legacy,
    PublicationMode::Cohort,
    PublicationMode::Indexed,
];
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "writer-namespace-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn seed(p: &Path, m: PublicationMode) -> Environment {
    seed_with_maintenance(p, m, ulmdb::MaintenancePolicy::AUTOMATIC)
}
fn seed_with_maintenance(
    p: &Path,
    m: PublicationMode,
    policy: ulmdb::MaintenancePolicy,
) -> Environment {
    let e = Environment::create_with_publication(p, m).unwrap();
    e.set_maintenance_policy(policy).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"A").unwrap();
    w.commit().unwrap();
    e
}
fn pins(p: &Path) -> usize {
    pin_records::paths(p).len()
}
fn cap(c: &Coordination) -> ulmdb_storage::coordination::DirectoryCapture {
    c.capture_bound_directory().unwrap()
}
#[test]
fn existing_read_namespace_binding_charges_no_pin_and_retains_two_record_limit() {
    for mode in MODES {
        let d = Dir::new();
        let e = seed(&d.0, mode);
        let uuid = e.begin_read().file_manifest().unwrap().uuid;
        drop(e);
        let c = Coordination::open(
            &d.0,
            uuid,
            Limits {
                max_pin_records: 2,
                ..Default::default()
            },
        )
        .unwrap();
        {
            let mut maintenance = c.try_maintenance().unwrap();
            maintenance.scan().unwrap().cleanup_stale().unwrap();
        }
        let (read, _) = OwnedFileCapture::current(&d.0, cap(&c))
            .unwrap()
            .admit(ResidencyPolicy::Demand)
            .unwrap();
        let second = OwnedFileCapture::current(&d.0, cap(&c)).unwrap();
        assert_eq!(pins(&d.0), 2);
        let pre = cap(&c);
        OwnedFileCapture::check_directory(&d.0, &pre).unwrap();
        drop(pre);
        let lease = read.bind_capture_namespace(&d.0, cap(&c)).unwrap();
        assert_eq!(pins(&d.0), 2);
        assert!(matches!(
            c.try_maintenance(),
            Err(ulmdb_storage::Error::Busy)
        ));
        assert!(matches!(
            lease.pin_current(),
            Err(ulmdb_storage::Error::LimitExceeded)
        ));
        drop(lease);
        drop(second);
        drop(read);
        assert_eq!(pins(&d.0), 0);
        let mut maintenance = c.try_maintenance().unwrap();
        let scan = maintenance.scan().unwrap();
        assert!(scan.live_roots().is_empty());
        assert_eq!(scan.stale_records(), 0);
    }
}
#[test]
fn namespace_binding_is_not_currentness_and_rejects_unpinned_or_foreign_clone_reads() {
    for mode in MODES {
        let d = Dir::new();
        let e = seed(&d.0, mode);
        let read = e.begin_read();
        let c = Coordination::open(&d.0, read.file_manifest().unwrap().uuid, Default::default())
            .unwrap();
        let baseline = pins(&d.0);
        let pure = ReadTxn::open_snapshot(&d.0).unwrap();
        assert!(matches!(
            pure.bind_capture_namespace(&d.0, cap(&c)),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(pins(&d.0), baseline);
        let clone = Dir::new();
        ulmdb::backup::export(&e, &clone.0).unwrap();
        let (foreign, _) = OwnedFileCapture::current(
            &clone.0,
            Coordination::capture_directory(&clone.0, Default::default()).unwrap(),
        )
        .unwrap()
        .admit(ResidencyPolicy::Demand)
        .unwrap();
        assert_eq!(
            foreign.file_checkpoint().unwrap(),
            read.file_checkpoint().unwrap()
        );
        assert!(foreign.bind_capture_namespace(&d.0, cap(&c)).is_err());
        assert_eq!(pins(&d.0), baseline);
        fs::rename(d.0.join("CURRENT"), d.0.join("CURRENT.saved")).unwrap();
        let pre = cap(&c);
        OwnedFileCapture::check_directory(&d.0, &pre).unwrap();
        drop(pre);
        let lease = read.bind_capture_namespace(&d.0, cap(&c)).unwrap();
        assert_eq!(pins(&d.0), baseline);
        drop(lease);
        fs::rename(d.0.join("CURRENT.saved"), d.0.join("CURRENT")).unwrap();
        assert_eq!(read.get(b"kv", b"key").unwrap(), Some(b"A".as_slice()));
    }
}
fn copy_assets(from: &Path, to: &Path) {
    for e in fs::read_dir(from).unwrap() {
        let p = e.unwrap().path();
        let n = p.file_name().unwrap();
        if n == "CURRENT" || n == "PREVIOUS" || n == "LOCK" || n == ".ulmdb-coordination" {
            continue;
        }
        let dst = to.join(n);
        if p.is_dir() {
            fs::create_dir_all(&dst).unwrap();
            copy_assets(&p, &dst);
        } else if dst.exists() {
            assert_eq!(
                fs::read(&p).unwrap(),
                fs::read(&dst).unwrap(),
                "same immutable asset path must contain identical bytes"
            );
        } else {
            fs::copy(p, dst).unwrap();
        }
    }
}
#[test]
fn real_foreign_uuid_owner_in_same_inode_cannot_bind_original_cached_realm() {
    for mode in MODES {
        let d = Dir::new();
        // Keep the flat registry unbound so this isolates namespace binding's
        // realm check; a generation profile independently rejects foreign UUIDs.
        let e = seed_with_maintenance(&d.0, mode, ulmdb::MaintenancePolicy::Disabled);
        let identity = ulmdb_platform::path_identity(&d.0).unwrap();
        let original = e.begin_read();
        let c = Coordination::open(
            &d.0,
            original.file_manifest().unwrap().uuid,
            Default::default(),
        )
        .unwrap();
        let foreign = Dir::new();
        let f = seed_with_maintenance(&foreign.0, mode, ulmdb::MaintenancePolicy::Disabled);
        assert_ne!(
            original.file_manifest().unwrap().uuid,
            f.begin_read().file_manifest().unwrap().uuid
        );
        let saved = fs::read(d.0.join("CURRENT")).unwrap();
        copy_assets(&foreign.0, &d.0);
        fs::write(
            d.0.join("CURRENT"),
            fs::read(foreign.0.join("CURRENT")).unwrap(),
        )
        .unwrap();
        let (actual, _) = OwnedFileCapture::current(
            &d.0,
            Coordination::capture_directory(&d.0, Default::default()).unwrap(),
        )
        .unwrap()
        .admit(ResidencyPolicy::Demand)
        .unwrap();
        assert!(identity.same_object(&ulmdb_platform::path_identity(&d.0).unwrap()));
        assert_eq!(
            actual.file_manifest().unwrap().uuid,
            f.begin_read().file_manifest().unwrap().uuid
        );
        let baseline = pins(&d.0);
        assert!(matches!(
            actual.bind_capture_namespace(&d.0, cap(&c)),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(pins(&d.0), baseline);
        drop(actual);
        fs::write(d.0.join("CURRENT"), saved).unwrap();
        assert!(original.bind_capture_namespace(&d.0, cap(&c)).is_ok());
    }
}
#[test]
fn static_namespace_damage_and_ancestor_alias_fail_preflight_without_replacement_metadata() {
    for mode in MODES {
        let parent = Dir::new();
        fs::create_dir(&parent.0).unwrap();
        let path = parent.0.join("db");
        let e = seed(&path, mode);
        let read = e.begin_read();
        let c = Coordination::open(
            &path,
            read.file_manifest().unwrap().uuid,
            Default::default(),
        )
        .unwrap();
        for name in [".ulmdb-coordination/pins", ".ulmdb-coordination"] {
            let held = cap(&c);
            let p = path.join(name);
            let saved = p.with_extension("saved");
            fs::rename(&p, &saved).unwrap();
            assert!(c.capture_bound_directory().is_err());
            assert!(OwnedFileCapture::check_directory(&path, &held).is_err());
            assert!(!p.exists(), "preflight did not recreate missing namespace");
            drop(held);
            fs::rename(saved, p).unwrap();
            assert!(read.bind_capture_namespace(&path, cap(&c)).is_ok());
        }
        let held = cap(&c);
        let moved = parent.0.with_extension("moved");
        fs::rename(&parent.0, &moved).unwrap();
        symlink(&moved, &parent.0).unwrap();
        assert!(matches!(
            OwnedFileCapture::check_directory(&path, &held),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            read.bind_capture_namespace(&path, cap(&c)),
            Err(Error::InvalidInput(_))
        ));
        fs::remove_file(&parent.0).unwrap();
        fs::rename(moved, &parent.0).unwrap();
        drop(held);
        assert!(read.bind_capture_namespace(&path, cap(&c)).is_ok());
    }
}
