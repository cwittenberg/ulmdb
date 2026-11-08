use super::*;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ulmdb-platform-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn native_handle_width_and_duplicate_stream_offset_ownership() {
    let d = Dir::new();
    let mut f = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(d.0.join("data"))
        .unwrap();
    f.write_all(b"abcd").unwrap();
    f.seek(SeekFrom::Start(1)).unwrap();
    let mut other = duplicate_file_handle(raw_file_handle(&f)).unwrap();
    let mut byte = [0];
    other.read_exact(&mut byte).unwrap();
    assert_eq!(byte, b"b"[0..1]);
    assert_eq!(f.stream_position().unwrap(), 2);
    drop(other);
    f.write_all(b"Z").unwrap();
    let mut all = [0; 4];
    f.read_exact_at(&mut all, 0).unwrap();
    assert_eq!(&all, b"abZd");
    #[cfg(windows)]
    assert_eq!(
        std::mem::size_of::<RawFileHandle>(),
        std::mem::size_of::<usize>()
    );
    #[cfg(unix)]
    assert_eq!(
        std::mem::size_of::<RawFileHandle>(),
        std::mem::size_of::<i32>()
    );
    assert!(duplicate_file_handle(INVALID_FILE_HANDLE).is_err());
}
#[test]
fn positioned_io_preserves_shared_cursor_and_u64_offsets() {
    let d = Dir::new();
    let mut f = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(d.0.join("data"))
        .unwrap();
    f.write_all(b"abc").unwrap();
    f.seek(SeekFrom::Start(1)).unwrap();
    #[cfg(windows)]
    crate::windows::mark_sparse(&f).unwrap();
    let at = (1u64 << 32) + 4097;
    f.write_all_at(b"wide", at).unwrap();
    assert_eq!(f.stream_position().unwrap(), 1);
    let mut bytes = [0; 4];
    f.read_exact_at(&mut bytes, at).unwrap();
    assert_eq!(&bytes, b"wide");
    assert_eq!(f.stream_position().unwrap(), 1);
    assert!(f.read_exact_at(&mut bytes, u64::MAX).is_err());
}
#[test]
fn identity_uses_real_object_id_across_hardlinks_and_directory_handles() {
    let d = Dir::new();
    let p = d.0.join("a");
    let f = File::create(&p).unwrap();
    fs::hard_link(&p, d.0.join("b")).unwrap();
    let a = file_identity(&f).unwrap();
    let b = file_identity(&File::open(d.0.join("b")).unwrap()).unwrap();
    assert_eq!((a.volume, a.file_id), (b.volume, b.file_id));
    assert_eq!(a.links, 2);
    assert!(
        file_identity(&open_directory(&d.0).unwrap())
            .unwrap()
            .directory
    );
    assert!(file_identity(&f).unwrap().directory == false);
}
#[test]
fn relative_file_open_is_rooted_and_rejects_parent_traversal() {
    let d = Dir::new();
    fs::write(d.0.join("a"), b"a").unwrap();
    let root = open_directory(&d.0).unwrap();
    let file = open_relative(&root, std::ffi::OsStr::new("a"), RelativeKind::File).unwrap();
    assert_eq!(file_identity(&file).unwrap().length, 1);
    assert!(open_relative(&root, std::ffi::OsStr::new("../a"), RelativeKind::File).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(d.0.join("a"), d.0.join("link")).unwrap();
        assert!(open_relative(&root, std::ffi::OsStr::new("link"), RelativeKind::File).is_err());
    }
}
#[test]
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn real_byte_range_locks_conflict_and_release_at_high_offsets() {
    let d = Dir::new();
    let path = d.0.join("lock");
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
    let high = 1u64 << 40;
    let a = RangeLock::acquire(a, LockMode::Shared, high, 1, false).unwrap();
    assert_eq!(
        RangeLock::acquire(b, LockMode::Exclusive, high, 1, false)
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::WouldBlock
    );
    drop(a);
    let b = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let _b = RangeLock::acquire(b, LockMode::Exclusive, high, 1, false).unwrap();
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn independent_range_owners_survive_other_owners_and_unrelated_descriptor_close() {
    let d = Dir::new();
    let path = d.0.join("owned-ranges");
    File::create(&path).unwrap();
    let open = || OpenOptions::new().read(true).write(true).open(&path).unwrap();
    let high = 1u64 << 40;
    let first = RangeLock::acquire(open(), LockMode::Shared, high, 3, false).unwrap();
    let second = RangeLock::acquire(open(), LockMode::Shared, high, 3, false).unwrap();
    let blocked = || {
        assert_eq!(
            RangeLock::acquire(open(), LockMode::Exclusive, high + 1, 1, false)
                .err().unwrap().kind(),
            io::ErrorKind::WouldBlock,
        );
    };
    blocked();
    drop(first);
    blocked();
    drop(open());
    blocked();
    drop(second);
    let _exclusive = RangeLock::acquire(open(), LockMode::Exclusive, high, 3, false).unwrap();
}

#[test]
fn private_artifacts_are_exclusive_nofollow_and_really_sealed() {
    let d = Dir::new();
    let directory = d.0.join("private");
    create_private_directory(&directory).unwrap();
    let path = directory.join("data");
    let mut file = create_private_file(&path, true).unwrap();
    file.write_all(b"owned").unwrap();
    assert!(create_private_file(&path, true).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"owned");
    set_private_permissions(&file, false).unwrap();
    assert_eq!(
        path_identity(&path).unwrap().file_id,
        file_identity(&file).unwrap().file_id
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o400);
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        std::os::unix::fs::symlink(&path, directory.join("alias")).unwrap();
        assert!(open_readonly(&directory.join("alias")).is_err());
    }
    #[cfg(windows)]
    {
        assert!(OpenOptions::new().write(true).open(&path).is_err());
    }
    // The creating owner still holds genuine permission authority.
    set_private_permissions(&file, true).unwrap();
    OpenOptions::new().write(true).open(&path).unwrap();
}

#[test]
fn loaded_current_module_is_discovered_without_cwd_assumptions() {
    // SAFETY: this running executable cannot unload itself during the call.
    #[allow(unsafe_code)]
    let actual = unsafe {
        module_path(
            loaded_current_module_is_discovered_without_cwd_assumptions as *const ()
                as *const std::ffi::c_void,
        )
    }
    .unwrap();
    assert_eq!(
        actual.canonicalize().unwrap(),
        std::env::current_exe().unwrap().canonicalize().unwrap()
    );
}
#[cfg(windows)]
#[test]
fn lmdb_windows_utf8_path_conversion_uses_real_system_replacement_rules() {
    use std::os::windows::ffi::OsStrExt;
    assert_eq!(
        path_from_utf8(b"A\xffB")
            .unwrap()
            .as_os_str()
            .encode_wide()
            .collect::<Vec<_>>(),
        vec![65, 0xfffd, 66]
    );
    assert_eq!(
        path_from_utf8("caf\u{e9}".as_bytes())
            .unwrap()
            .as_os_str()
            .encode_wide()
            .collect::<Vec<_>>(),
        vec![99, 97, 102, 233]
    );
}

#[test]
fn zeroed_private_backing_and_sealed_file_fences_keep_physical_bytes() {
    let d = Dir::new();
    let path = d.0.join("zero");
    let file = create_private_file(&path, true).unwrap();
    initialize_zeroed_file(&file, 1 << 20).unwrap();
    let mut bytes = [1; 4096];
    file.read_exact_at(&mut bytes, (1 << 20) - 4096).unwrap();
    assert_eq!(bytes, [0; 4096]);
    file.write_all_at(b"owned", 1024).unwrap();
    assert!(initialize_zeroed_file(&file, 0).is_err());
    set_private_permissions(&file, false).unwrap();
    drop(file);
    let read = open_readonly(&path).unwrap();
    let before = file_identity(&read).unwrap();
    sync_file(&read).unwrap();
    assert_eq!(file_identity(&read).unwrap(), before);
    let mut value = [0; 5];
    read.read_exact_at(&mut value, 1024).unwrap();
    assert_eq!(&value, b"owned");
    #[cfg(windows)]
    assert!(sync_directory_file(&read).is_err());
}
#[test]
fn namespace_replace_retains_old_open_object_and_fences_held_directory() {
    let d = Dir::new();
    let a = d.0.join("new");
    let b = d.0.join("current");
    fs::write(&a, b"new-value").unwrap();
    fs::write(&b, b"old-value").unwrap();
    let held = File::open(&b).unwrap();
    let directory = open_directory(&d.0).unwrap();
    rename_replace(&a, &b).unwrap();
    sync_directory_file(&directory).unwrap();
    assert_eq!(fs::read(&b).unwrap(), b"new-value");
    assert!(!a.exists());
    let mut old = [0; 9];
    held.read_exact_at(&mut old, 0).unwrap();
    assert_eq!(&old, b"old-value");
    let error = rename_replace(&a, &b).unwrap_err();
    assert!(!rename_outcome_unknown(&error));
    assert_eq!(fs::read(&b).unwrap(), b"new-value");
    #[cfg(windows)]
    assert!(sync_file(&directory).is_err());
}
