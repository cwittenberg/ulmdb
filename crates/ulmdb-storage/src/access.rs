//! Per-owner creation policy. The stable LOCK mode records the effective,
//! umask-filtered policy for future owners; business descriptors are unchanged.
use crate::{Error, Result};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};
/// Creation rights for new files. Owner read/write is required; the process
/// umask can further restrict requested bits. Shared groups additionally need a
/// setgid 2770 namespace and an umask preserving group read/write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileAccess {
    mode: u32,
    group: Option<u32>,
}
impl Default for FileAccess {
    fn default() -> Self {
        Self::PRIVATE
    }
}
impl FileAccess {
    pub const PRIVATE: Self = Self {
        mode: 0o600,
        group: None,
    };
    pub fn requested(mode: u32) -> Result<Self> {
        if mode & !0o777 != 0 || mode & 0o600 != 0o600 {
            return Err(Error::InvalidInput(
                "creation mode requires owner read/write",
            ));
        }
        Ok(Self { mode, group: None })
    }
    pub fn shared_group(mode: u32, group: u32) -> Result<Self> {
        #[cfg(not(target_os = "linux"))]
        return Err(Error::InvalidInput("shared file access requires Linux"));
        let mut value = Self::requested(mode)?;
        if mode & 0o060 != 0o060 {
            return Err(Error::InvalidInput(
                "shared group requires group read/write",
            ));
        }
        value.group = Some(group);
        Ok(value)
    }
    pub fn validate_root(self, path: &Path) -> Result<()> {
        let meta = fs::symlink_metadata(path)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(Error::Corrupt("invalid access root"));
        }
        #[cfg(unix)]
        {
            if let Some(gid) = self.group {
                if meta.gid() != gid || meta.mode() & 0o2072 != 0o2070 || !member(gid)? {
                    return Err(denied());
                }
            } else if meta.uid() != effective_uid() {
                return Err(denied());
            }
        }
        Ok(())
    }
    pub fn mode(self) -> u32 {
        self.mode
    }
    pub fn group(self) -> Option<u32> {
        self.group
    }
    pub fn directory_mode(self) -> u32 {
        if self.group.is_some() { 0o2770 } else { 0o700 }
    }
}
#[cfg(unix)]
use std::os::unix::{
    fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    io::AsRawFd,
};
#[cfg(target_os = "linux")]
fn creation_umask() -> Result<u32> {
    use std::io::Read;
    let mut bytes = String::new();
    File::open("/proc/self/status")?
        .take(65536)
        .read_to_string(&mut bytes)?;
    let row = bytes
        .lines()
        .find_map(|line| line.strip_prefix("Umask:"))
        .ok_or(Error::Corrupt("missing process umask"))?;
    u32::from_str_radix(row.trim(), 8).map_err(|_| Error::Corrupt("invalid process umask"))
}
fn denied() -> Error {
    std::io::Error::from(std::io::ErrorKind::PermissionDenied).into()
}
#[cfg(unix)]
#[allow(unsafe_code)]
fn effective_uid() -> u32 {
    unsafe { libc::geteuid() }
}
#[cfg(unix)]
#[allow(unsafe_code)]
fn credentials() -> Result<(u32, u32, Vec<u32>)> {
    // Only credential queries; no global credential/umask state is modified.
    unsafe {
        let uid = libc::geteuid();
        let gid = libc::getegid();
        let count = libc::getgroups(0, std::ptr::null_mut());
        if !(0..=65536).contains(&count) {
            return Err(denied());
        }
        let mut groups = vec![0; count as usize];
        if libc::getgroups(count, groups.as_mut_ptr()) < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok((uid, gid, groups))
    }
}
#[cfg(unix)]
fn member(gid: u32) -> Result<bool> {
    let (uid, primary, groups) = credentials()?;
    Ok(uid == 0 || primary == gid || groups.contains(&gid))
}
#[cfg(unix)]
#[allow(unsafe_code)]
fn set_group(file: &File, gid: u32) -> Result<()> {
    // A held descriptor prevents following a replaced pathname. The kernel checks
    // that an unprivileged owner belongs to the requested group.
    if unsafe { libc::fchown(file.as_raw_fd(), !0, gid) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
#[derive(Clone, Debug)]
pub struct FileAccessContext {
    directory: PathBuf,
    policy: FileAccess,
    creator: u32,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    owner: u32,
    #[cfg(unix)]
    writer: u32,
}
impl FileAccessContext {
    pub fn open(directory: &Path) -> Result<Self> {
        let named = fs::symlink_metadata(directory)?;
        if !named.is_dir() || named.file_type().is_symlink() {
            return Err(Error::Corrupt("invalid access directory"));
        }
        let mode = match fs::symlink_metadata(directory.join("LOCK")) {
            Ok(meta) => {
                if !meta.is_file() || meta.file_type().is_symlink() {
                    return Err(Error::Corrupt("invalid access LOCK"));
                }
                #[cfg(unix)]
                {
                    if meta.nlink() != 1 {
                        return Err(Error::Corrupt("linked access LOCK"));
                    }
                    meta.mode() & 0o777
                }
                #[cfg(not(unix))]
                {
                    0o600
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0o600,
            Err(e) => return Err(e.into()),
        };
        let mut policy = FileAccess::requested(mode)?;
        #[cfg(unix)]
        {
            if named.mode() & 0o2000 != 0 && named.mode() & 0o070 == 0o070 && mode & 0o060 == 0o060
            {
                if mode & 0o002 != 0 || named.mode() & 0o002 != 0 {
                    return Err(denied());
                }
                policy.group = Some(named.gid());
            }
        }
        Ok(Self {
            directory: directory.to_path_buf(),
            policy,
            creator: std::process::id(),
            #[cfg(unix)]
            device: named.dev(),
            #[cfg(unix)]
            inode: named.ino(),
            #[cfg(unix)]
            owner: named.uid(),
            #[cfg(unix)]
            writer: effective_uid(),
        })
    }
    pub fn policy(&self) -> FileAccess {
        self.policy
    }
    pub fn check(&self) -> Result<()> {
        self.checked_root().map(|_| ())
    }
    /// Return only this call's freshly checked namespace metadata. No checked
    /// metadata survives an operation, callback or kernel wait.
    fn checked_root(&self) -> Result<fs::Metadata> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited file access context"));
        }
        let named = fs::symlink_metadata(&self.directory)?;
        if !named.is_dir() || named.file_type().is_symlink() {
            return Err(Error::Corrupt("access directory replaced"));
        }
        #[cfg(unix)]
        {
            if named.dev() != self.device || named.ino() != self.inode || named.uid() != self.owner
            {
                return Err(Error::Corrupt("access namespace changed"));
            }
            if effective_uid() != self.writer {
                return Err(denied());
            }
            if let Some(gid) = self.policy.group {
                if named.gid() != gid || named.mode() & 0o2072 != 0o2070 || !member(gid)? {
                    return Err(denied());
                }
            }
        }
        Ok(named)
    }
    pub fn authorize_create(&self) -> Result<()> {
        self.check()?;
        #[cfg(target_os = "linux")]
        if self.policy.group.is_some() && creation_umask()? & 0o670 != 0 {
            return Err(denied());
        }
        #[cfg(unix)]
        if self.owner != self.writer && self.policy.group.is_none() {
            return Err(denied());
        }
        Ok(())
    }
    pub fn validate_directory(&self, path: &Path) -> Result<()> {
        let root = self.checked_root()?;
        // The root was just checked, including its current ownership and group
        // rights. Reuse that same syscall's result only for the same path;
        // independent descendants still need their own no-follow lookup.
        let meta = if path == self.directory {
            root
        } else {
            fs::symlink_metadata(path)?
        };
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(Error::Corrupt("access directory is not regular"));
        }
        #[cfg(unix)]
        if let Some(gid) = self.policy.group {
            if meta.gid() != gid || meta.mode() & 0o2002 != 0o2000 {
                return Err(denied());
            }
        }
        Ok(())
    }
    pub fn validate_parent(&self, path: &Path) -> Result<()> {
        let mut parent = path
            .parent()
            .ok_or(Error::InvalidInput("entry without parent"))?;
        for _ in 0..4 {
            self.validate_directory(parent)?;
            if parent == self.directory {
                return Ok(());
            }
            parent = parent
                .parent()
                .ok_or(Error::InvalidInput("entry outside namespace"))?;
        }
        Err(Error::InvalidInput("entry outside namespace"))
    }
    pub fn options(&self) -> Result<OpenOptions> {
        self.authorize_create()?;
        let mut options = OpenOptions::new();
        #[cfg(unix)]
        options
            .mode(self.policy.mode)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        Ok(options)
    }
    pub fn directory_builder(&self) -> Result<fs::DirBuilder> {
        self.authorize_create()?;
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        builder.mode(self.policy.directory_mode());
        Ok(builder)
    }
    pub fn configure_imported_file(&self, file: &File) -> Result<()> {
        self.authorize_create()?;
        #[cfg(unix)]
        {
            let meta = file.metadata()?;
            if meta.uid() != self.writer || !meta.is_file() {
                return Err(denied());
            }
            if let Some(gid) = self.policy.group {
                if meta.gid() != gid {
                    set_group(file, gid)?;
                }
            }
            #[cfg(target_os = "linux")]
            let mask = creation_umask()?;
            #[cfg(not(target_os = "linux"))]
            let mask = 0;
            file.set_permissions(fs::Permissions::from_mode(
                self.policy.mode & !mask & !0o222,
            ))?;
        }
        #[cfg(windows)]
        ulmdb_platform::set_private_permissions(file, false)?;
        #[cfg(not(any(unix, windows)))]
        {
            let mut p = file.metadata()?.permissions();
            p.set_readonly(true);
            file.set_permissions(p)?;
        }
        Ok(())
    }
    /// Re-read current policy for this operation without rediscovering its root.
    /// A LOCK chmod may change private/group admission after the FD was opened;
    /// the fresh policy must still belong to the original process and namespace.
    pub fn revalidate_file(&self, file: &File) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited file access context"));
        }
        let current = Self::open(&self.directory)?;
        if current.creator != self.creator {
            return Err(Error::InvalidInput("inherited file access context"));
        }
        #[cfg(unix)]
        {
            if (current.device, current.inode, current.owner)
                != (self.device, self.inode, self.owner)
            {
                return Err(Error::Corrupt("access namespace changed"));
            }
            if current.writer != self.writer {
                return Err(denied());
            }
        }
        current.validate_file(file)
    }
    pub fn validate_file(&self, file: &File) -> Result<()> {
        self.check()?;
        #[cfg(unix)]
        {
            let meta = file.metadata()?;
            if !meta.is_file() || meta.nlink() != 1 {
                return Err(Error::Corrupt("invalid access file"));
            }
            if let Some(gid) = self.policy.group {
                if meta.gid() != gid || meta.mode() & 0o002 != 0 {
                    return Err(denied());
                }
            } else if meta.uid() != self.writer && meta.uid() != self.owner {
                return Err(denied());
            }
        }
        Ok(())
    }
}
/// Locate only the bounded known layout/control ancestry. No global policy map.
pub fn context_for_entry(path: &Path) -> Result<FileAccessContext> {
    let fallback = if path.is_dir() {
        path
    } else {
        path.parent()
            .ok_or(Error::InvalidInput("entry without parent"))?
    };
    let mut at = fallback;
    for _ in 0..4 {
        if at.join("LOCK").exists() || (at.join("segments").is_dir() && at.join("commits").is_dir())
        {
            return FileAccessContext::open(at);
        }
        if let Some(parent) = at.parent() {
            at = parent;
        } else {
            break;
        }
    }
    FileAccessContext::open(fallback)
}
pub fn options_for_entry(path: &Path) -> Result<OpenOptions> {
    let context = context_for_entry(path)?;
    context.validate_parent(path)?;
    context.options()
}
pub fn builder_for_entry(path: &Path) -> Result<fs::DirBuilder> {
    let context = context_for_entry(path)?;
    context.validate_parent(path)?;
    context.directory_builder()
}
pub fn validate_entry(path: &Path, file: &File) -> Result<()> {
    let context = context_for_entry(path)?;
    context.validate_file(file)
}

#[cfg(all(test, target_os = "linux"))]
mod operation_tests {
    use super::*;
    struct Directory(PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn directory_validation_rechecks_bound_root_and_independent_child() {
        let directory = Directory(std::env::temp_dir().join(format!(
            "access-directory-{:032x}", crate::new_identity().unwrap()
        )));
        crate::create_layout(&directory.0).unwrap();
        let context = FileAccessContext::open(&directory.0).unwrap();
        let child = directory.0.join("child");
        fs::create_dir(&child).unwrap();
        context.validate_directory(&directory.0).unwrap();
        context.validate_parent(&directory.0.join("entry")).unwrap();
        context.validate_directory(&child).unwrap();
        let alias = directory.0.join("alias");
        std::os::unix::fs::symlink(&child, &alias).unwrap();
        assert!(matches!(context.validate_directory(&alias), Err(Error::Corrupt(_))));
        fs::write(directory.0.join("ordinary"), b"not a directory").unwrap();
        assert!(matches!(context.validate_directory(&directory.0.join("ordinary")), Err(Error::Corrupt(_))));

        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o2770)).unwrap();
        fs::set_permissions(directory.0.join("LOCK"), fs::Permissions::from_mode(0o660)).unwrap();
        fs::set_permissions(&child, fs::Permissions::from_mode(0o2770)).unwrap();
        let shared = FileAccessContext::open(&directory.0).unwrap();
        assert!(shared.policy().group().is_some());
        shared.validate_directory(&directory.0).unwrap();
        shared.validate_directory(&child).unwrap();
        fs::set_permissions(&child, fs::Permissions::from_mode(0o2772)).unwrap();
        assert!(shared.validate_directory(&child).is_err(), "child rights stay fresh");
        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o0770)).unwrap();
        assert!(shared.validate_directory(&directory.0).is_err(), "root rights stay fresh");

        let moved = Directory(directory.0.with_extension("moved"));
        fs::rename(&directory.0, &moved.0).unwrap();
        crate::create_layout(&directory.0).unwrap();
        assert!(matches!(context.validate_directory(&directory.0), Err(Error::Corrupt(_))));
        let mut inherited = context;
        inherited.creator = inherited.creator.wrapping_add(1);
        fs::remove_dir_all(&directory.0).unwrap();
        assert!(matches!(inherited.validate_directory(&directory.0), Err(Error::InvalidInput(_))));
    }
    #[test]
    fn operation_local_refresh_checks_new_policy_and_original_namespace() {
        let directory = Directory(std::env::temp_dir().join(format!(
            "access-refresh-{:032x}", crate::new_identity().unwrap()
        )));
        crate::create_layout(&directory.0).unwrap();
        let context = FileAccessContext::open(&directory.0).unwrap();
        assert_eq!(context.policy().group(), None);
        let path = directory.0.join("probe");
        let file = OpenOptions::new().read(true).write(true).create_new(true)
            .open(&path).unwrap();
        // A stale private policy accepts this owner-created FD. A fresh shared
        // policy must reject its world-write bit even if it changed after open.
        file.set_permissions(fs::Permissions::from_mode(0o602)).unwrap();
        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o2770)).unwrap();
        fs::set_permissions(directory.0.join("LOCK"), fs::Permissions::from_mode(0o660)).unwrap();
        assert!(FileAccessContext::open(&directory.0).unwrap().policy().group().is_some());
        assert!(context.validate_file(&file).is_ok(), "stale-private control");
        assert!(context.revalidate_file(&file).is_err(), "new shared policy");
        file.set_permissions(fs::Permissions::from_mode(0o600)).unwrap();
        context.revalidate_file(&file).unwrap();
        let shared = FileAccessContext::open(&directory.0).unwrap();
        fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(directory.0.join("LOCK"), fs::Permissions::from_mode(0o600)).unwrap();
        shared.revalidate_file(&file).unwrap();
        let moved = Directory(directory.0.with_extension("moved"));
        fs::rename(&directory.0, &moved.0).unwrap();
        crate::create_layout(&directory.0).unwrap();
        assert!(matches!(context.revalidate_file(&file), Err(Error::Corrupt(_))));
        let mut inherited = context;
        inherited.creator = inherited.creator.wrapping_add(1);
        fs::remove_dir_all(&directory.0).unwrap();
        assert!(matches!(inherited.revalidate_file(&file), Err(Error::InvalidInput(_))));
    }
}

pub fn create_layout(directory: &Path, access: FileAccess) -> Result<()> {
    #[cfg(target_os = "linux")]
    if access.group.is_some() && creation_umask()? & 0o670 != 0 {
        return Err(denied());
    }
    #[cfg(not(unix))]
    if access != FileAccess::PRIVATE {
        return Err(Error::InvalidInput("shared file policy requires Unix"));
    }
    #[cfg(unix)]
    if access.group.is_some_and(|g| !member(g).unwrap_or(false)) {
        return Err(denied());
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    builder.mode(access.directory_mode());
    builder.create(directory)?;
    #[cfg(unix)]
    if let Some(gid) = access.group {
        let root = File::open(directory)?;
        set_group(&root, gid)?;
        root.set_permissions(fs::Permissions::from_mode(root.metadata()?.mode() | 0o2000))?;
    }
    let mut options = OpenOptions::new();
    options.create_new(true).read(true).write(true);
    #[cfg(unix)]
    options
        .mode(access.mode)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let lock = options.open(directory.join("LOCK"))?;
    let context = FileAccessContext::open(directory)?;
    if let Some(gid) = access.group {
        if context.policy.group != Some(gid) {
            return Err(denied());
        }
    }
    context.validate_file(&lock)?;
    for name in ["segments", "commits", "transactions", "cohorts"] {
        context.directory_builder()?.create(directory.join(name))?;
    }
    ulmdb_platform::sync_path(directory)?;
    ulmdb_platform::sync_path(
        directory
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub struct AccessMigrationReport {
    pub files: usize,
    pub directories: usize,
    pub policy: FileAccess,
}
#[cfg(unix)]
fn hex_name(name: &str, digits: usize, suffix: &str) -> bool {
    name.strip_suffix(suffix)
        .is_some_and(|v| v.len() == digits && v.bytes().all(|b| b.is_ascii_hexdigit()))
}
#[cfg(unix)]
fn pin_generation_name(name: &str) -> bool {
    name.strip_prefix("gen-").is_some_and(|hex| {
        hex.len() == 16
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
#[cfg(unix)]
fn pin_record_name(name: &str) -> bool {
    name.strip_prefix("pin-")
        .and_then(|hex| hex.strip_suffix(".lease"))
        .is_some_and(|hex| {
            hex.len() == 32
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && hex.bytes().any(|b| b != b'0')
        })
}
/// Explicit one-pass permission migration. Caller holds native writer, C-env
/// exclusion and physical maintenance. Business bytes/checkpoints do not change.
/// Every recognized inode is checked and held before the first permission edit;
/// linked/nonregular/foreign-owned assets fail before mutation.
#[cfg(unix)]
pub fn migrate(
    directory: &Path,
    policy: FileAccess,
    max_entries: usize,
) -> Result<AccessMigrationReport> {
    let context = FileAccessContext::open(directory)?;
    context.check()?;
    let (uid, _, _) = credentials()?;
    if context.owner != uid && uid != 0 {
        return Err(denied());
    }
    let mask = {
        #[cfg(target_os = "linux")]
        {
            creation_umask()?
        }
        #[cfg(not(target_os = "linux"))]
        {
            0
        }
    };
    let effective = policy.mode & !mask;
    if effective & 0o600 != 0o600
        || policy
            .group
            .is_some_and(|g| mask & 0o670 != 0 || !member(g).unwrap_or(false))
    {
        return Err(denied());
    }
    let mut directories = Vec::new();
    let mut files = Vec::new();
    let known_root = [
        "HISTORY_RETENTION",
        "LOCK",
        "CURRENT",
        "PREVIOUS",
        "TRANSFORM",
        "FIXEDMAP",
        "FIXEDMAP-LEASES",
        "REPLICA",
        "LMDB-ABI",
        "LMDB-ENV",
        "LMDB-ENV-GATE",
        "LMDB-WRITER",
        "LMDB-READERS",
        "LMDB-MAPSIZE",
        "LMDB-VIEW",
        "LMDB-VIEW-GATE",
    ];
    let mut pending = vec![(directory.to_path_buf(), 0usize)];
    while let Some((parent, depth)) = pending.pop() {
        if directories.len() + files.len() >= max_entries {
            return Err(Error::LimitExceeded);
        }
        let meta = fs::symlink_metadata(&parent)?;
        if !meta.is_dir() || meta.file_type().is_symlink() || meta.uid() != uid && uid != 0 {
            return Err(denied());
        }
        let fd = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&parent)?;
        let held = fd.metadata()?;
        if held.dev() != meta.dev() || held.ino() != meta.ino() {
            return Err(Error::Corrupt("migration directory changed"));
        }
        directories.push((parent.clone(), fd));
        let parent_relative = parent
            .strip_prefix(directory)
            .map_err(|_| Error::Corrupt("migration namespace"))?;
        let pins = Path::new(".ulmdb-coordination/pins");
        let in_generation = parent_relative.parent() == Some(pins)
            && parent_relative
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(pin_generation_name);
        let mut has_scan = false;
        for item in fs::read_dir(&parent)? {
            let item = item?;
            let path = item.path();
            let name = item.file_name();
            let Some(name) = name.to_str() else { continue };
            let relative = path
                .strip_prefix(directory)
                .map_err(|_| Error::Corrupt("migration namespace"))?;
            let parent_name = parent.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let generation = parent_relative == pins && name.starts_with("gen-");
            if generation && !pin_generation_name(name) {
                return Err(Error::Corrupt("invalid migration pin generation"));
            }
            let is_dir = depth == 0
                && [
                    "segments",
                    "commits",
                    "cohorts",
                    "transactions",
                    ".ulmdb-coordination",
                    "lmdb-readers",
                ]
                .contains(&name)
                || depth == 1 && parent_name == ".ulmdb-coordination" && name == "pins"
                || generation;
            if is_dir {
                pending.push((path, depth + 1));
                continue;
            }
            let recognized = if in_generation {
                if name.starts_with("pin-") && !pin_record_name(name) {
                    return Err(Error::Corrupt("invalid migration generation pin name"));
                }
                has_scan |= name == "_scan";
                name == "_scan" || pin_record_name(name)
            } else if depth == 0 {
                known_root.contains(&name)
            } else {
                match parent_name {
                    "segments" => hex_name(name, 64, ".seg"),
                    "commits" => hex_name(name, 32, ".root") || hex_name(name, 32, ".history"),
                    "cohorts" => hex_name(name, 32, ".cohort"),
                    ".ulmdb-coordination" => ["capture", "registry", "history"].contains(&name),
                    "pins" => name
                        .strip_prefix("pin-")
                        .is_some_and(|n| hex_name(n, 32, ".lease")),
                    "lmdb-readers" => name == "registry.lock" || hex_name(name, 8, ".slot"),
                    _ => false,
                }
            };
            if !recognized {
                continue;
            }
            if directories.len() + files.len() >= max_entries {
                return Err(Error::LimitExceeded);
            }
            let meta = fs::symlink_metadata(&path)?;
            if !meta.is_file()
                || meta.file_type().is_symlink()
                || meta.nlink() != 1
                || meta.uid() != uid && uid != 0
            {
                return Err(denied());
            }
            let fd = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
                .open(&path)?;
            let actual = fd.metadata()?;
            if actual.dev() != meta.dev()
                || actual.ino() != meta.ino()
                || !actual.is_file()
                || actual.nlink() != 1
                || actual.uid() != uid && uid != 0
            {
                return Err(Error::Corrupt("migration file changed"));
            }
            if in_generation && name == "_scan" && actual.len() != 0 {
                return Err(Error::Corrupt("nonempty migration generation scan gate"));
            }
            let immutable = relative.starts_with("segments")
                || relative.starts_with("cohorts")
                || (parent_name == "commits" && name.ends_with(".history"));
            files.push((path, fd, immutable));
        }
        if in_generation && !has_scan {
            return Err(Error::Corrupt("missing migration generation scan gate"));
        }
    }
    context.check()?;
    // All recognized descendants were opened without following links. Recheck
    // their names against the held inodes before changing the first right.
    for (path, file) in &directories {
        let named = fs::symlink_metadata(path)?;
        let held = file.metadata()?;
        if !named.is_dir()
            || named.file_type().is_symlink()
            || named.dev() != held.dev()
            || named.ino() != held.ino()
            || named.uid() != uid && uid != 0
        {
            return Err(Error::Corrupt("migration directory changed"));
        }
    }
    for (path, file, _) in &files {
        let named = fs::symlink_metadata(path)?;
        let held = file.metadata()?;
        if !named.is_file()
            || named.file_type().is_symlink()
            || named.nlink() != 1
            || named.dev() != held.dev()
            || named.ino() != held.ino()
            || named.uid() != uid && uid != 0
        {
            return Err(Error::Corrupt("migration file changed"));
        }
    }
    // LOCK activates the new creation policy only after all data and directories.
    for (path, file, immutable) in &files {
        if path.file_name().is_some_and(|s| s == "LOCK") {
            continue;
        }
        if let Some(gid) = policy.group {
            set_group(file, gid)?;
        }
        file.set_permissions(fs::Permissions::from_mode(if *immutable {
            effective & !0o222
        } else {
            effective
        }))?;
        ulmdb_platform::sync_file(&file)?;
    }
    directories.sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
    for (_, file) in &directories {
        if let Some(gid) = policy.group {
            set_group(file, gid)?;
        }
        file.set_permissions(fs::Permissions::from_mode(policy.directory_mode() & !mask))?;
        ulmdb_platform::sync_directory_file(&file)?;
    }
    let (_, lock, _) = files
        .iter()
        .find(|(p, _, _)| p.file_name().is_some_and(|s| s == "LOCK"))
        .ok_or(Error::Corrupt("migration LOCK absent"))?;
    if let Some(gid) = policy.group {
        set_group(lock, gid)?;
    }
    lock.set_permissions(fs::Permissions::from_mode(effective))?;
    ulmdb_platform::sync_file(&lock)?;
    ulmdb_platform::sync_path(directory)?;
    Ok(AccessMigrationReport {
        files: files.len(),
        directories: directories.len(),
        policy: FileAccess {
            mode: effective,
            group: policy.group,
        },
    })
}
#[cfg(not(unix))]
pub fn migrate(
    _directory: &Path,
    _policy: FileAccess,
    _max_entries: usize,
) -> Result<AccessMigrationReport> {
    Err(Error::InvalidInput("file access migration requires Unix"))
}
