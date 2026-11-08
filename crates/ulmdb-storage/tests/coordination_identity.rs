//! Fresh named binding checks against privately held immutable directory FDs.
#![cfg(any(target_os = "linux", target_os = "macos"))]
use std::{fs, os::unix::fs::symlink, path::PathBuf};
use ulmdb_storage::{
    Error, Manifest, coordination::Coordination, create_layout, new_identity, publish,
};

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Replacement {
    named: PathBuf,
    saved: PathBuf,
    link: bool,
}
impl Replacement {
    fn restore(&self) {
        if self.link {
            fs::remove_file(&self.named).unwrap();
        } else {
            fs::remove_dir(&self.named).unwrap();
        }
        fs::rename(&self.saved, &self.named).unwrap();
    }
}
impl Drop for Replacement {
    fn drop(&mut self) {
        if self.saved.exists() {
            if self.link {
                let _ = fs::remove_file(&self.named);
            } else {
                let _ = fs::remove_dir(&self.named);
            }
            let _ = fs::rename(&self.saved, &self.named);
        }
    }
}

#[test]
fn every_held_directory_rejects_replacement_and_symlink_then_recovers_exact_binding() {
    for format in [2, 3, 4] {
        for relative in ["", ".ulmdb-coordination", ".ulmdb-coordination/pins"] {
            for link in [false, true] {
                let directory = Directory(std::env::temp_dir().join(format!(
                    "coordination-identity-{:032x}",
                    new_identity().unwrap()
                )));
                create_layout(&directory.0).unwrap();
                let mut manifest = Manifest::empty().unwrap();
                manifest.format_minor = format;
                if format == 4 {
                    manifest.prepare_indexed(None).unwrap();
                }
                publish(&directory.0, &manifest).unwrap();
                let coordinator =
                    Coordination::open(&directory.0, manifest.uuid, Default::default()).unwrap();
                let held = coordinator.capture_bound_directory().unwrap();
                let named = if relative.is_empty() {
                    directory.0.clone()
                } else {
                    directory.0.join(relative)
                };
                let saved = named.with_extension("identity-saved");
                fs::rename(&named, &saved).unwrap();
                let replacement = Replacement { named, saved, link };
                if link {
                    symlink(&replacement.saved, &replacement.named).unwrap();
                } else {
                    fs::create_dir(&replacement.named).unwrap();
                }
                assert!(matches!(
                    coordinator.capture_bound_directory(),
                    Err(Error::Corrupt(_))
                ));
                assert!(matches!(held.directory_binding(), Err(Error::Corrupt(_))));
                if !link {
                    assert_eq!(fs::read_dir(&replacement.named).unwrap().count(), 0);
                }
                replacement.restore();
                drop(replacement);
                let (_, capture, pin) = held.capture_current().unwrap();
                assert_eq!(pin.root().manifest().uuid, manifest.uuid);
                drop((capture, pin));
                assert!(coordinator.try_maintenance().is_ok());
            }
        }
    }
}
