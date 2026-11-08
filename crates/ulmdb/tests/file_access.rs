#![cfg(target_os = "linux")]
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};
use ulmdb::{DatabaseKind, Environment, PageSize, PublicationMode, ReadAhead, ResidencyPolicy};
use ulmdb_storage::access::{FileAccess, FileAccessContext};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-access-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().mode() & 0o7777
}
#[test]
fn worker() {
    let Ok(bits) = std::env::var("ACCESS_BITS") else {
        return;
    };
    let bits = u32::from_str_radix(&bits, 8).unwrap();
    let mask = u32::from_str_radix(&std::env::var("ACCESS_MASK").unwrap(), 8).unwrap();
    let shared = std::env::var_os("ACCESS_SHARED").is_some();
    for backend in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir::new();
        let copy = Dir::new();
        let identity = Dir::new();
        fs::create_dir(&identity.0).unwrap();
        let group = fs::metadata(&identity.0).unwrap().gid();
        let access = if shared {
            FileAccess::shared_group(bits, group).unwrap()
        } else {
            FileAccess::requested(bits).unwrap()
        };
        let e = Environment::create_with_file_access(
            &d.0,
            backend,
            PageSize::DEFAULT,
            ResidencyPolicy::Demand,
            ReadAhead::Normal,
            None,
            access,
        )
        .unwrap();
        assert_eq!(
            mode(&d.0),
            if shared {
                0o2770 & !mask
            } else {
                0o700 & !mask
            }
        );
        assert_eq!(mode(&d.0.join("LOCK")), bits & !mask);
        let mut w = e.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"k", b"A").unwrap();
        w.commit().unwrap();
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"k", b"B").unwrap();
        w.commit().unwrap();
        let read = ulmdb::ReadTxn::open_coordinated_snapshot(&d.0).unwrap();
        assert_eq!(read.get(b"kv", b"k").unwrap(), Some(b"B".as_slice()));
        assert_eq!(
            mode(&d.0.join(".ulmdb-coordination")),
            if shared {
                0o2770 & !mask
            } else {
                0o700 & !mask
            }
        );
        for path in fs::read_dir(d.0.join("segments"))
            .unwrap()
            .chain(fs::read_dir(d.0.join("cohorts")).unwrap())
        {
            let p = path.unwrap().path();
            if p.extension().is_some_and(|x| x == "seg" || x == "cohort") {
                assert_eq!(mode(&p) & 0o777, (bits & !mask) & !0o222);
            }
        }
        e.compact().unwrap();
        ulmdb::backup::export(&e, &copy.0).unwrap();
        assert_eq!(mode(&copy.0.join("LOCK")), bits & !mask);
        drop(read);
        drop(e);
        let e = Environment::open_with_file_access(&d.0, access).unwrap();
        assert_eq!(
            e.begin_read().get(b"kv", b"k").unwrap(),
            Some(b"B".as_slice())
        );
        let _ = FileAccessContext::open(&d.0).unwrap();
    }
}
#[test]
fn requested_modes_honor_umask_in_isolated_children() {
    for (bits, mask, shared) in [
        (0o600, 0o022, false),
        (0o640, 0o027, false),
        (0o660, 0o007, true),
    ] {
        let exe = std::env::current_exe().unwrap();
        let mut cmd = Command::new("bash");
        cmd.args([
            "-c",
            "umask \"$1\"; exec \"$2\" --exact worker --nocapture",
            "access-worker",
            &format!("{mask:o}"),
            exe.to_str().unwrap(),
        ])
        .env("ACCESS_BITS", format!("{bits:o}"))
        .env("ACCESS_MASK", format!("{mask:o}"));
        if shared {
            cmd.env("ACCESS_SHARED", "1");
        }
        let result = cmd.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
#[test]
fn explicit_group_rejects_bad_namespace_and_replaced_context() {
    let d = Dir::new();
    let e = Environment::create(&d.0).unwrap();
    let ctx = FileAccessContext::open(&d.0).unwrap();
    let gid = fs::metadata(&d.0).unwrap().gid();
    assert!(
        Environment::open_with_file_access(&d.0, FileAccess::shared_group(0o660, gid).unwrap())
            .is_err()
    );
    let moved = d.0.with_extension("moved");
    fs::rename(&d.0, &moved).unwrap();
    fs::create_dir(&d.0).unwrap();
    assert!(ctx.check().is_err());
    drop(e);
    fs::remove_dir_all(&moved).unwrap();
}
#[test]
fn migration_worker() {
    if std::env::var_os("ACCESS_MIGRATE").is_none() {
        return;
    }
    let d = Dir::new();
    let e = Environment::create(&d.0).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"k", b"kept").unwrap();
    w.commit().unwrap();
    let read = e.begin_read();
    let checkpoint = read.file_checkpoint().unwrap();
    let note = d.0.join("user-note");
    fs::write(&note, b"unrelated").unwrap();
    fs::set_permissions(&note, fs::Permissions::from_mode(0o640)).unwrap();
    // Automatic discovery has already moved physical leases into generations.
    // Their existing directories and control bytes must migrate with the root.
    let generations: Vec<_> = fs::read_dir(d.0.join(".ulmdb-coordination/pins"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .as_encoded_bytes()
                .starts_with(b"gen-")
        })
        .collect();
    assert!(!generations.is_empty());
    let generation_files: Vec<_> = generations
        .iter()
        .flat_map(|generation| {
            fs::read_dir(generation).unwrap().map(|entry| {
                let path = entry.unwrap().path();
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
        })
        .collect();
    let gid = fs::metadata(&d.0).unwrap().gid();
    let report = e
        .migrate_file_access(FileAccess::shared_group(0o660, gid).unwrap())
        .unwrap();
    assert!(report.files > 3);
    assert_eq!(mode(&d.0), 0o2770);
    assert_eq!(mode(&d.0.join("LOCK")), 0o660);
    assert_eq!(mode(&note), 0o640);
    for generation in &generations {
        assert_eq!(mode(generation), 0o2770);
    }
    for (path, bytes) in &generation_files {
        assert_eq!(mode(path), 0o660);
        assert_eq!(&fs::read(path).unwrap(), bytes);
    }
    assert_eq!(read.file_checkpoint().unwrap(), checkpoint);
    assert_eq!(read.get(b"kv", b"k").unwrap(), Some(b"kept".as_slice()));
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", b"k", b"after").unwrap();
    w.commit().unwrap();
    assert_eq!(
        e.begin_read().get(b"kv", b"k").unwrap(),
        Some(b"after".as_slice())
    );
}
#[test]
fn explicit_migration_preserves_business_bytes_and_unrelated_files() {
    let exe = std::env::current_exe().unwrap();
    let p = Command::new("bash")
        .args([
            "-c",
            "umask 007; exec \"$1\" --exact migration_worker --nocapture",
            "migration",
            exe.to_str().unwrap(),
        ])
        .env("ACCESS_MIGRATE", "1")
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
}
#[test]
fn linked_assets_reject_permission_migration_before_rights_change() {
    let d = Dir::new();
    let link = Dir::new();
    let e = Environment::create(&d.0).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"k", b"kept").unwrap();
    w.commit().unwrap();
    let segment = fs::read_dir(d.0.join("segments"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::hard_link(&segment, &link.0).unwrap();
    let old = mode(&segment);
    let gid = fs::metadata(&d.0).unwrap().gid();
    assert!(
        e.migrate_file_access(FileAccess::shared_group(0o660, gid).unwrap())
            .is_err()
    );
    assert_eq!(mode(&segment), old);
    assert_eq!(mode(&d.0), 0o700);
    let _ = fs::remove_file(&link.0);
}

#[test]
fn generation_permission_migration_preserves_unrecognized_entries() {
    let d = Dir::new();
    ulmdb_storage::create_layout(&d.0).unwrap();
    let generation = d.0.join(".ulmdb-coordination/pins/gen-0000000000000001");
    fs::create_dir_all(&generation).unwrap();
    let scan = generation.join("_scan");
    let pin = generation.join("pin-00000000000000000000000000000001.lease");
    fs::write(&scan, []).unwrap();
    fs::write(&pin, [7; 160]).unwrap();
    let operator = generation.join("operator-note");
    let operator_directory = generation.join("operator-directory");
    let operator_link = generation.join("operator-link");
    fs::write(&operator, b"unrelated").unwrap();
    fs::set_permissions(&operator, fs::Permissions::from_mode(0o600)).unwrap();
    fs::create_dir(&operator_directory).unwrap();
    fs::set_permissions(&operator_directory, fs::Permissions::from_mode(0o750)).unwrap();
    fs::write(operator_directory.join("kept"), b"nested operator data").unwrap();
    std::os::unix::fs::symlink(&operator, &operator_link).unwrap();
    ulmdb_storage::access::migrate(&d.0, FileAccess::requested(0o644).unwrap(), 100).unwrap();
    assert_eq!(mode(&scan), mode(&d.0.join("LOCK")));
    assert_eq!(mode(&pin), mode(&d.0.join("LOCK")));
    assert_eq!(fs::read(&pin).unwrap(), [7; 160]);
    assert_eq!(mode(&operator), 0o600);
    assert_eq!(fs::read(&operator).unwrap(), b"unrelated");
    assert_eq!(mode(&operator_directory), 0o750);
    assert_eq!(
        fs::read(operator_directory.join("kept")).unwrap(),
        b"nested operator data"
    );
    assert_eq!(fs::read_link(operator_link).unwrap(), operator);
}

#[test]
fn malformed_or_aliased_generation_assets_reject_migration_before_rights_change() {
    for variant in 0..8 {
        let d = Dir::new();
        let foreign = Dir::new();
        ulmdb_storage::create_layout(&d.0).unwrap();
        fs::create_dir(&foreign.0).unwrap();
        let pins = d.0.join(".ulmdb-coordination/pins");
        fs::create_dir_all(&pins).unwrap();
        let generation = pins.join("gen-0000000000000001");
        let scan = generation.join("_scan");
        let pin = generation.join("pin-00000000000000000000000000000001.lease");
        let outside = foreign.0.join("kept");
        fs::write(&outside, b"outside").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
        let recognized = d.0.join("HISTORY_RETENTION");
        fs::write(&recognized, b"recognized control bytes").unwrap();
        fs::set_permissions(&recognized, fs::Permissions::from_mode(0o400)).unwrap();
        if variant == 0 {
            std::os::unix::fs::symlink(&foreign.0, &generation).unwrap();
        } else {
            fs::create_dir(&generation).unwrap();
            if variant != 1 {
                fs::write(&scan, []).unwrap();
            }
            match variant {
                2 => {
                    fs::write(&scan, b"invalid gate").unwrap();
                }
                3 => {
                    fs::write(generation.join("pin-invalid.lease"), b"invalid name").unwrap();
                }
                4 => {
                    fs::create_dir(pins.join("gen-invalid")).unwrap();
                }
                5 => {
                    fs::hard_link(&outside, &pin).unwrap();
                }
                6 => {
                    std::os::unix::fs::symlink(&outside, &pin).unwrap();
                }
                7 => {
                    fs::remove_file(&scan).unwrap();
                    fs::hard_link(&outside, &scan).unwrap();
                }
                _ => {}
            }
        }
        let root_mode = mode(&d.0);
        let lock_mode = mode(&d.0.join("LOCK"));
        assert!(
            ulmdb_storage::access::migrate(&d.0, FileAccess::requested(0o644).unwrap(), 100,)
                .is_err(),
            "variant {variant}"
        );
        assert_eq!(mode(&d.0), root_mode, "variant {variant}");
        assert_eq!(mode(&d.0.join("LOCK")), lock_mode, "variant {variant}");
        assert_eq!(mode(&outside), 0o600, "variant {variant}");
        assert_eq!(fs::read(&outside).unwrap(), b"outside");
        assert_eq!(mode(&recognized), 0o400, "variant {variant}");
        assert_eq!(fs::read(&recognized).unwrap(), b"recognized control bytes");
    }
}
#[test]
fn unsuitable_shared_umask_is_rejected_before_namespace_creation() {
    if std::env::var_os("ACCESS_BAD_MASK").is_some() {
        let identity = Dir::new();
        fs::create_dir(&identity.0).unwrap();
        let gid = fs::metadata(&identity.0).unwrap().gid();
        let d = Dir::new();
        assert!(
            Environment::create_with_file_access(
                &d.0,
                PublicationMode::Legacy,
                PageSize::DEFAULT,
                ResidencyPolicy::Demand,
                ReadAhead::Normal,
                None,
                FileAccess::shared_group(0o660, gid).unwrap()
            )
            .is_err()
        );
        assert!(!d.0.exists());
        return;
    }
    for mask in ["010", "027"] {
        let output=Command::new("bash").args(["-c","umask \"$1\"; exec \"$2\" --exact unsuitable_shared_umask_is_rejected_before_namespace_creation --nocapture","bad-mask",mask,std::env::current_exe().unwrap().to_str().unwrap()]).env("ACCESS_BAD_MASK","1").output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
