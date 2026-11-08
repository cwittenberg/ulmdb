//! Private same-directory files: one process salt, unique checked counters and
//! exclusive creation. Names have no role in published content identities.
use crate::{Error, Result};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
static SALT: OnceLock<u128> = OnceLock::new();
static SERIAL: AtomicU64 = AtomicU64::new(0);
pub(crate) fn create(directory: &Path, purpose: &str) -> Result<(PathBuf, File)> {
    if SALT.get().is_none() {
        let candidate = crate::new_identity()?;
        let _ = SALT.set(candidate);
    }
    create_with(
        directory,
        purpose,
        *SALT.get().ok_or(Error::LimitExceeded)?,
        &SERIAL,
    )
}
fn create_with(
    directory: &Path,
    purpose: &str,
    salt: u128,
    serial: &AtomicU64,
) -> Result<(PathBuf, File)> {
    for _ in 0..128 {
        let number = serial
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::LimitExceeded)?;
        let path = directory.join(format!(".prepare-{salt:032x}-{number:016x}-{purpose}.tmp"));
        let mut options = crate::access::options_for_entry(&path)?;
        options.create_new(true).write(true);
        #[cfg(windows)]
        options.read(true); // Owned staging mappings/identity queries need this access.
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(Error::LimitExceeded)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, io::Write};
    #[test]
    fn orphan_collision_never_overwrites_and_counter_exhaustion_precedes_creation() {
        let directory =
            std::env::temp_dir().join(format!("ulmdb-temp-{}", crate::new_identity().unwrap()));
        std::fs::create_dir(&directory).unwrap();
        let (first, mut file) = create_with(&directory, "test", 7, &AtomicU64::new(0)).unwrap();
        file.write_all(b"kept").unwrap();
        drop(file);
        let (second, file) = create_with(&directory, "test", 7, &AtomicU64::new(0)).unwrap();
        assert_ne!(first, second);
        drop(file);
        assert_eq!(std::fs::read(first).unwrap(), b"kept");
        assert!(matches!(
            create_with(&directory, "test", 7, &AtomicU64::new(u64::MAX)),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn concurrent_private_creators_have_distinct_paths() {
        let directory = std::env::temp_dir().join(format!(
            "ulmdb-temp-parallel-{}",
            crate::new_identity().unwrap()
        ));
        std::fs::create_dir(&directory).unwrap();
        let paths = std::thread::scope(|scope| {
            let mut workers = Vec::new();
            for _ in 0..8 {
                let directory = &directory;
                workers.push(scope.spawn(move || {
                    (0..16)
                        .map(|_| {
                            let (path, file) = create(directory, "parallel").unwrap();
                            drop(file);
                            path
                        })
                        .collect::<Vec<_>>()
                }));
            }
            workers
                .into_iter()
                .flat_map(|worker| worker.join().unwrap())
                .collect::<BTreeSet<_>>()
        });
        assert_eq!(paths.len(), 128);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
