//! Kernel range gate on the WAL inode. Lock ownership follows the file object;
//! every acquisition uses an independent File and Drop only closes it.
//! Windows reserves a high range: mandatory locks must never cover user bytes
//! or overlap the distinct writer-ownership byte. Linux retains its OFD range.
use crate::{Error, Result};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
fn lock(file: &File, write: bool, start: i64, length: i64) -> Result<()> {
    if start < 0 || length <= 0 || start.checked_add(length).is_none() {
        return Err(Error::InvalidInput("invalid WAL lease range"));
    }
    ulmdb_platform::lock_file_range(
        file,
        if write {
            ulmdb_platform::LockMode::Exclusive
        } else {
            ulmdb_platform::LockMode::Shared
        },
        {
            #[cfg(windows)]
            {
                (1u64 << 61)
                    .checked_add(start as u64)
                    .ok_or(Error::LimitExceeded)?
            }
            #[cfg(not(windows))]
            {
                start as u64
            }
        },
        length as u64,
        false,
    )
    .map_err(|error| {
        if error.kind() == std::io::ErrorKind::WouldBlock {
            Error::Busy
        } else {
            error.into()
        }
    })
}
pub(crate) fn shared(file: &File) -> Result<()> {
    lock(file, false, 0, 1)
}
/// Exclusive non-destructive readiness lease. It exposes no writable FD, no
/// floor publication and no punch operation. It conflicts with every anchor OFD.
pub struct ExclusiveLease {
    file: File,
}
impl ExclusiveLease {
    pub fn try_acquire(path: &Path, expected: &File) -> Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let a = ulmdb_platform::file_identity(expected)?;
        let b = ulmdb_platform::file_identity(&file)?;
        if !a.same_object(&b) {
            return Err(Error::Corrupt("retirement lease WAL inode mismatch"));
        }
        lock(&file, true, 0, 1)?;
        Ok(Self { file })
    }
    pub fn inode(&self) -> Result<(u64, u64)> {
        let m = ulmdb_platform::file_identity(&self.file)?;
        // Preserve the legacy API without silently truncating a128-bit FileId.
        Ok((
            m.volume,
            u64::try_from(m.file_id).map_err(|_| Error::LimitExceeded)?,
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_ofds_conflict_and_64bit_ranges_do_not_truncate() {
        #[cfg(target_os = "linux")]
        const NAME: &str = "hybrid::retirement::lease::tests::independent_ofds_conflict_and_64bit_ranges_do_not_truncate";
        #[cfg(target_os = "linux")]
        if crate::history::tests::run_isolated(NAME) {
            return;
        }
        let path =
            std::env::temp_dir().join(format!("ulmdb-ofd-{}", crate::new_identity().unwrap()));
        let a = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let b = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let high = 1i64 << 40;
        lock(&a, false, high, 1).unwrap();
        assert!(matches!(lock(&b, true, high, 1), Err(Error::Busy)));
        lock(&b, true, 0, 1).unwrap();
        drop(a);
        lock(&b, true, high, 1).unwrap();
        drop(b);
        std::fs::remove_file(path).unwrap();
        #[cfg(target_os = "linux")]
        crate::history::tests::worker_completed(NAME);
    }
    #[test]
    fn leases_survive_dup_and_match_hardlink_inode() {
        const NAME: &str =
            "hybrid::retirement::lease::tests::leases_survive_dup_and_match_hardlink_inode";
        if let Some(path) = std::env::var_os("ULMDB_OFD_CHILD") {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .unwrap();
            let result = lock(&file, true, 0, 1);
            if std::env::var_os("ULMDB_OFD_BUSY").is_some() {
                assert!(matches!(result, Err(Error::Busy)));
            } else {
                result.unwrap();
            }
            println!("OFD child checked");
            return;
        }
        let path =
            std::env::temp_dir().join(format!("ulmdb-ofd-{}", crate::new_identity().unwrap()));
        let alias = path.with_extension("hardlink");
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        std::fs::hard_link(&path, &alias).unwrap();
        shared(&file).unwrap();
        let duplicate = file.try_clone().unwrap();
        drop(file);
        let child = |busy: bool| {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", NAME, "--nocapture"])
                .env("ULMDB_OFD_CHILD", &alias);
            if busy {
                command.env("ULMDB_OFD_BUSY", "1");
            } else {
                command.env_remove("ULMDB_OFD_BUSY");
            }
            command
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let mut process = command.spawn().unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                if process.try_wait().unwrap().is_some() {
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    process.kill().unwrap();
                    process.wait().unwrap();
                    panic!("OFD child timed out");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let output = process.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("OFD child checked"));
        };
        child(true);
        drop(duplicate); // The last duplicate close releases the shared gate.
        child(false);
        std::fs::remove_file(alias).unwrap();
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn same_ofd_conversion_is_not_an_independent_conflict_gate() {
        let path =
            std::env::temp_dir().join(format!("ulmdb-ofd-{}", crate::new_identity().unwrap()));
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        shared(&file).unwrap();
        let duplicate = file.try_clone().unwrap();
        lock(&duplicate, true, 0, 1).unwrap(); // negative control: same OFD converts.
        assert!(matches!(
            ExclusiveLease::try_acquire(&path, &file),
            Err(Error::Busy)
        ));
        drop(duplicate);
        drop(file);
        std::fs::remove_file(path).unwrap();
    }
}
