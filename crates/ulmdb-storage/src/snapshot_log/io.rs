//! Synchronous, positioned frame writes into the already fenced journal inode.
//!
//! RWF_DSYNC is the per-write equivalent of O_DSYNC. A complete successful
//! return is the durability boundary, including retrieval-critical metadata.
//! It does not promise atomic sectors. A short write or other error leaves the
//! transaction outcome uncertain; the Journal owner must poison itself.

use std::{fs::File, io, os::fd::AsRawFd};
use ulmdb_platform::FileExt;

const MAX_PARTS: usize = 4;

/// Classify the owned inode's actual filesystem once during startup. Path
/// names and caller benchmark labels do not choose a transport. This recognizes
/// TMPFS_MAGIC only; synchronous tmpfs I/O still is not physical-media durability.
pub(super) fn is_ram_backed(file: &File) -> io::Result<bool> {
    Ok(filesystem_type(file)? == libc::TMPFS_MAGIC as u64)
}

#[allow(unsafe_code)]
fn filesystem_type(file: &File) -> io::Result<u64> {
    let mut status = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: status points to writable storage for the complete native statfs
    // ABI object; the owned File keeps its descriptor valid for this call.
    if unsafe { libc::fstatfs(file.as_raw_fd(), status.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fstatfs initialized this ABI object. Only the integral
    // filesystem type is read; its positive tmpfs value fits both32/64-bit words.
    let status = unsafe { status.assume_init() };
    Ok(status.f_type as u64)
}

/// The caller owns the inode exclusively and supplies immutable input buffers;
/// read-only source slices may share backing storage. No mapped reference to
/// the destination may be live while
/// writing. The journal extent and directory entry were fenced at creation.
/// Unsupported kernels/filesystems use ordinary positioned writes followed by
/// fdatasync; a successful return always includes synchronous completion.
pub(super) fn write_sync(file: &File, offset: u64, parts: &[&[u8]]) -> io::Result<()> {
    let total = validate::<MAX_PARTS>(offset, parts)?;
    match positioned_sync::<MAX_PARTS>(file, offset, parts, total) {
        Err(error) if matches!(error.raw_os_error(), Some(libc::ENOSYS | libc::EOPNOTSUPP)) => {
            // Rewrite the same private frontier, so this is idempotent even if
            // a filesystem reports unsupported synchronization after copying
            // some bytes. The fallback fences the complete frame and marker.
            let mut at = offset;
            for part in parts {
                file.write_all_at(part, at)?;
                at += part.len() as u64;
            }
            file.sync_data()
        }
        result => result,
    }
}

/// Packet writes have one more piece than the compact-frame path: prefix,
/// body, unused gap, certificates and optional future zeros. A separate
/// monomorphization leaves the existing compact-frame stack array at four.
pub(super) fn write_packet_sync(file: &File, offset: u64, parts: &[&[u8]]) -> io::Result<()> {
    let total = validate::<5>(offset, parts)?;
    match positioned_sync::<5>(file, offset, parts, total) {
        Err(error) if matches!(error.raw_os_error(), Some(libc::ENOSYS | libc::EOPNOTSUPP)) => {
            let mut at = offset;
            for part in parts {
                file.write_all_at(part, at)?;
                at += part.len() as u64;
            }
            file.sync_data()
        }
        result => result,
    }
}

fn validate<const COUNT: usize>(offset: u64, parts: &[&[u8]]) -> io::Result<usize> {
    if parts.is_empty() || parts.len() > COUNT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "journal write vector count",
        ));
    }
    let total = parts
        .iter()
        .try_fold(0usize, |total, part| total.checked_add(part.len()))
        .filter(|&total| total != 0 && total <= isize::MAX as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "journal write length"))?;
    offset
        .checked_add(total as u64)
        .filter(|&end| end <= libc::off_t::MAX as u64)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "journal write offset"))?;
    Ok(total)
}

#[allow(unsafe_code)]
fn positioned_sync<const COUNT: usize>(
    file: &File,
    offset: u64,
    parts: &[&[u8]],
    total: usize,
) -> io::Result<()> {
    let mut vectors = [libc::iovec {
        iov_base: std::ptr::null_mut(),
        iov_len: 0,
    }; COUNT];
    for (vector, part) in vectors.iter_mut().zip(parts) {
        vector.iov_base = part.as_ptr().cast_mut().cast();
        vector.iov_len = part.len();
    }
    loop {
        // SAFETY: validate bounds the offset, aggregate length and stack array.
        // All source slices and this owned File outlive the synchronous call;
        // pwritev2 only reads the vectors. O_APPEND is never used by Journal.
        let written = unsafe {
            libc::pwritev2(
                file.as_raw_fd(),
                vectors.as_ptr(),
                parts.len() as i32,
                offset as libc::off_t,
                libc::RWF_DSYNC,
            )
        };
        if written == total as isize {
            return Ok(());
        }
        if written >= 0 {
            // Do not acknowledge or retry the remaining fragment of a frame.
            // Its already transferred prefix may be durable and visible.
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "short synchronous journal write",
            ));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
        // Retrying this exact private frontier is idempotent, including when
        // interruption follows a data copy but precedes durable completion.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};

    #[test]
    fn vectored_frame_is_visible_to_existing_mapping_and_reopen() {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-sync-write-{:032x}",
            crate::new_identity().unwrap()
        ));
        let file = ulmdb_platform::create_private_file(&path, true).unwrap();
        file.set_len(8192).unwrap();
        file.sync_data().unwrap();
        #[allow(unsafe_code)]
        // SAFETY: this test owns the fixed-extent file; no mapping reference is
        // held over write_sync. The read-only mapping observes kernel writes.
        let map = unsafe { memmap2::MmapOptions::new().map(&file).unwrap() };
        write_sync(&file, 4091, &[b"header", b"payload", b"footer", &[0; 8]]).unwrap();
        assert_eq!(&map[4091..4118], b"headerpayloadfooter\0\0\0\0\0\0\0\0");
        drop(map);
        drop(file);
        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[4091..4118], b"headerpayloadfooter\0\0\0\0\0\0\0\0");
        assert!(bytes[..4091].iter().all(|&byte| byte == 0));
        assert!(bytes[4118..].iter().all(|&byte| byte == 0));
        let read_only = OpenOptions::new().read(true).open(&path).unwrap();
        assert_eq!(
            write_sync(&read_only, 4091, &[b"replacement"])
                .unwrap_err()
                .raw_os_error(),
            Some(libc::EBADF)
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn invalid_vector_and_offset_cannot_modify_file() {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-sync-write-limits-{:032x}",
            crate::new_identity().unwrap()
        ));
        let file = ulmdb_platform::create_private_file(&path, true).unwrap();
        for (offset, parts) in [
            (0, Vec::new()),
            (0, vec![&b""[..]]),
            (0, vec![&b"x"[..]; 5]),
            (u64::MAX, vec![&b"x"[..]]),
        ] {
            assert_eq!(
                write_sync(&file, offset, &parts).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert_eq!(file.metadata().unwrap().len(), 0);
        drop(file);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn packet_five_vectors_include_shared_zero_sources_and_bound_the_offset() {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-packet-write-{:032x}",
            crate::new_identity().unwrap()
        ));
        let file = ulmdb_platform::create_private_file(&path, true).unwrap();
        file.set_len(16384).unwrap();
        file.sync_data().unwrap();
        let zeros = [0; 4096];
        write_packet_sync(
            &file,
            4090,
            &[b"prefix", b"body", &zeros[..37], b"certs", &zeros[..4000]],
        )
        .unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(bytes[..4090].iter().all(|&byte| byte == 0));
        assert_eq!(&bytes[4090..4100], b"prefixbody");
        assert!(bytes[4100..4137].iter().all(|&byte| byte == 0));
        assert_eq!(&bytes[4137..4142], b"certs");
        assert!(bytes[4142..].iter().all(|&byte| byte == 0));
        for (offset, parts) in [
            (0, vec![&b"x"[..]; 6]),
            (libc::off_t::MAX as u64 - 2, vec![&b"x"[..]; 5]),
        ] {
            assert_eq!(
                write_packet_sync(&file, offset, &parts).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert_eq!(fs::read(&path).unwrap(), bytes);
        drop(file);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn short_synchronous_write_cannot_be_acknowledged() {
        const CHILD: &str = "ULMDB_SHORT_SYNC_WRITE_TEST";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "snapshot_log::io::tests::short_synchronous_write_cannot_be_acknowledged",
                    "--test-threads=1",
                ])
                .env(CHILD, "1")
                // Pipes keep libtest's own output outside RLIMIT_FSIZE even
                // when the parent test output is redirected to a regular log.
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "child status {:?}; stdout {}; stderr {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let path = std::env::temp_dir().join(format!(
            "ulmdb-short-sync-{:032x}",
            crate::new_identity().unwrap()
        ));
        let file = ulmdb_platform::create_private_file(&path, true).unwrap();
        file.set_len(8192).unwrap();
        file.sync_data().unwrap();
        // Isolated child process: force a real short regular-file write without
        // changing any resource limit or signal handler in the test runner.
        #[allow(unsafe_code)]
        unsafe {
            assert_ne!(libc::signal(libc::SIGXFSZ, libc::SIG_IGN), libc::SIG_ERR);
            let limit = libc::rlimit {
                rlim_cur: 1024,
                rlim_max: 1024,
            };
            assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &limit), 0);
        }
        let body = [42; 2048];
        let error = write_sync(&file, 0, &[b"header", &body, b"footer"]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WriteZero);
        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[..6], b"header");
        assert!(bytes[6..1024].iter().all(|&byte| byte == 42));
        assert!(bytes[1024..].iter().all(|&byte| byte == 0));

        // A reservation suffix can fail after every record byte was already
        // transferred. The full vectored call must still fail: its caller
        // cannot acknowledge the record or assume it did not reach storage.
        let prefix = b"record-prefix";
        let footer = b"record-footer";
        let zero_reservation = [0; 2048];
        let error =
            write_sync(&file, 0, &[prefix, &body[..64], footer, &zero_reservation]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WriteZero);
        let bytes = fs::read(&path).unwrap();
        let end = prefix.len() + 64 + footer.len();
        assert_eq!(&bytes[..prefix.len()], prefix);
        assert!(
            bytes[prefix.len()..prefix.len() + 64]
                .iter()
                .all(|&byte| byte == 42)
        );
        assert_eq!(&bytes[prefix.len() + 64..end], footer);
        assert!(bytes[end..].iter().all(|&byte| byte == 0));

        let error = write_packet_sync(
            &file,
            0,
            &[prefix, &body[..64], &[0; 3], footer, &zero_reservation],
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WriteZero);
        let bytes = fs::read(&path).unwrap();
        let footer_at = prefix.len() + 64 + 3;
        assert_eq!(&bytes[..prefix.len()], prefix);
        assert!(
            bytes[prefix.len()..prefix.len() + 64]
                .iter()
                .all(|&byte| byte == 42)
        );
        assert_eq!(&bytes[prefix.len() + 64..footer_at], &[0; 3]);
        assert_eq!(&bytes[footer_at..footer_at + footer.len()], footer);
        assert!(
            bytes[footer_at + footer.len()..]
                .iter()
                .all(|&byte| byte == 0)
        );
        drop(file);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn actual_tmpfs_inode_is_classified_without_changing_contents() {
        let directory = std::path::Path::new("/dev/shm");
        if !directory.exists() {
            eprintln!("tmpfs classification fixture unavailable: /dev/shm does not exist");
            return;
        }
        let directory_file = File::open(directory).unwrap();
        if filesystem_type(&directory_file).unwrap() != libc::TMPFS_MAGIC as u64 {
            eprintln!("tmpfs classification fixture unavailable: /dev/shm is not TMPFS_MAGIC");
            return;
        }
        drop(directory_file);
        let path = directory.join(format!(
            "ulmdb-tmpfs-type-{:032x}",
            crate::new_identity().unwrap()
        ));
        let file = ulmdb_platform::create_private_file(&path, true).unwrap();
        file.write_all_at(b"owned tmpfs bytes", 0).unwrap();
        file.sync_data().unwrap();
        assert!(is_ram_backed(&file).unwrap());
        let read_only = File::open(&path).unwrap();
        assert!(is_ram_backed(&read_only).unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"owned tmpfs bytes");
        drop(read_only);
        drop(file);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn owned_ext4_inode_is_not_tmpfs_even_with_a_ram_label() {
        let directory = std::env::current_dir().unwrap();
        let directory_file = File::open(&directory).unwrap();
        if filesystem_type(&directory_file).unwrap() != libc::EXT4_SUPER_MAGIC as u64 {
            eprintln!(
                "ext4 classification fixture unavailable: current directory is not EXT4_SUPER_MAGIC"
            );
            return;
        }
        drop(directory_file);
        let path = directory.join(format!(
            "ulmdb-ram-labeled-type-{:032x}",
            crate::new_identity().unwrap()
        ));
        let file = ulmdb_platform::create_private_file(&path, true).unwrap();
        file.write_all_at(b"owned ext4 bytes", 0).unwrap();
        file.sync_data().unwrap();
        assert!(!is_ram_backed(&file).unwrap());
        let read_only = File::open(&path).unwrap();
        assert!(!is_ram_backed(&read_only).unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"owned ext4 bytes");
        drop(read_only);
        drop(file);
        fs::remove_file(path).unwrap();
    }
}
