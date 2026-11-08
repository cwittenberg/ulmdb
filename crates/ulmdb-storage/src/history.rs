//! Private pointer-publication serialization. Never returned to a public caller.
//! Capture precedes shared history; publishers finish pin preparation before EX.
use crate::{Error, Result};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

#[cfg(windows)]
mod windows;
pub(crate) struct HistoryGate {
    #[cfg(windows)]
    windows: windows::Gate,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    file: File,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    directory: PathBuf,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    directory_file: File,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    area: PathBuf,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    area_file: File,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    creator: u32,
}
impl HistoryGate {
    pub(crate) fn shared(directory: &std::path::Path) -> Result<Self> {
        Self::acquire(directory, true)
    }
    pub(crate) fn exclusive(directory: &std::path::Path) -> Result<Self> {
        Self::acquire(directory, false)
    }
    fn acquire(directory: &std::path::Path, shared: bool) -> Result<Self> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let directory_file = open(directory, true, false)?;
            let directory = directory.canonicalize()?;
            let area = directory.join(".ulmdb-coordination");
            match crate::access::builder_for_entry(&area)?.create(&area) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
            let area_file = open(&area, true, false)?;
            let file = open(&area.join("history"), false, true)?;
            loop {
                let result = if shared {
                    file.lock_shared()
                } else {
                    file.lock()
                };
                match result {
                    Ok(()) => break,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error.into()),
                }
            }
            let gate = Self {
                file,
                directory,
                directory_file,
                area,
                area_file,
                creator: std::process::id(),
            };
            gate.check(&gate.directory)?;
            #[cfg(feature = "fault-injection")]
            crate::fault::history_point(if shared {
                "history.capture.acquired"
            } else {
                "history.publish.acquired"
            })?;
            Ok(gate)
        }
        #[cfg(windows)]
        {
            Ok(Self {
                windows: windows::Gate::acquire(directory, shared)?,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = (directory, shared);
            Ok(Self {})
        }
    }
    pub(crate) fn check(&self, directory: &std::path::Path) -> Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            if self.creator != std::process::id() {
                return Err(Error::InvalidInput("inherited history gate"));
            }
            if directory.canonicalize()? != self.directory {
                return Err(Error::Corrupt("history directory binding"));
            }
            check(&self.directory, &self.directory_file, true)?;
            check(&self.area, &self.area_file, true)?;
            check(&self.area.join("history"), &self.file, false)?;
        }
        #[cfg(windows)]
        self.windows.check(directory)?;
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let _ = directory;
        Ok(())
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn open(path: &Path, directory: bool, create: bool) -> Result<File> {
    let mut options = if create {
        crate::access::options_for_entry(path)?
    } else {
        OpenOptions::new()
    };
    options.read(true).custom_flags(
        libc::O_NOFOLLOW | libc::O_CLOEXEC | if directory { libc::O_DIRECTORY } else { 0 },
    );
    if !directory {
        options.write(true).create(create).truncate(false);
    }
    let file = options.open(path)?;
    check(path, &file, directory)?;
    if !directory {
        crate::access::validate_entry(path, &file)?;
    }
    Ok(file)
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn check(path: &Path, file: &File, directory: bool) -> Result<()> {
    let named = fs::symlink_metadata(path)?;
    let owned = file.metadata()?;
    if named.file_type().is_symlink()
        || (named.dev(), named.ino()) != (owned.dev(), owned.ino())
        || if directory {
            !named.is_dir() || !owned.is_dir()
        } else {
            !named.is_file() || !owned.is_file() || owned.nlink() != 1 || owned.len() != 0
        }
    {
        return Err(Error::Corrupt("invalid history gate inode"));
    }
    Ok(())
}
impl Drop for HistoryGate {
    fn drop(&mut self) {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if self.creator == std::process::id() {
            let _ = self.file.unlock();
        }
        // Foreign fork children close only their copy, never unlock the parent OFD.
    }
}

#[cfg(all(test, target_os = "linux"))]
#[allow(unsafe_code)]
pub(crate) mod tests {
    use super::*;
    use std::{
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::process::CommandExt,
        },
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    const WORKER: &str = "ULMDB_HISTORY_ISOLATED_TEST";
    const WORKER_TIMEOUT: Duration = Duration::from_secs(10);

    // A raw fork in the shared parallel libtest process inherits every test's
    // OFDs, including close-released retirement leases. Exec first so CLOEXEC
    // closes unrelated test authority before any fork-sensitive body runs.
    pub(crate) fn run_isolated(name: &str) -> bool {
        if std::env::var_os(WORKER).as_deref() == Some(std::ffi::OsStr::new(name)) {
            return false;
        }
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env(WORKER, name)
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut worker = Worker { child: Some(child) };
        let start = Instant::now();
        loop {
            // WNOWAIT leaves the PID reserved until group cleanup and reap,
            // including if an unexpected worker exit left a fork child alive.
            if worker.exited() {
                let output = worker.finish();
                assert!(
                    output.status.success(),
                    "isolated worker failed: {output:?}"
                );
                assert!(
                    String::from_utf8_lossy(&output.stdout)
                        .contains(&format!("isolated worker completed: {name}"))
                );
                return true;
            }
            if start.elapsed() >= WORKER_TIMEOUT {
                let output = worker.finish();
                panic!("isolated worker deadline: {output:?}");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    pub(crate) fn worker_completed(name: &str) {
        println!("isolated worker completed: {name}");
    }

    struct Worker {
        child: Option<Child>,
    }
    impl Worker {
        fn exited(&self) -> bool {
            let mut status = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
            let rc = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.child.as_ref().unwrap().id(),
                    status.as_mut_ptr(),
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if rc < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                return false;
            }
            assert_eq!(rc, 0, "observe owned worker exit");
            unsafe { status.assume_init().si_pid() != 0 }
        }
        fn finish(&mut self) -> std::process::Output {
            let child = self.child.take().unwrap();
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            child.wait_with_output().unwrap()
        }
    }
    impl Drop for Worker {
        fn drop(&mut self) {
            if let Some(mut child) = self.child.take() {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.wait();
            }
        }
    }

    struct ForkChild(Option<libc::pid_t>);
    impl ForkChild {
        fn wait_success(&mut self) {
            let start = Instant::now();
            loop {
                let mut status = 0;
                let child = self.0.unwrap();
                let got = unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) };
                if got == child {
                    self.0 = None;
                    assert_eq!(status, 0, "fork child status");
                    return;
                }
                if got < 0
                    && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
                {
                    continue;
                }
                assert_eq!(got, 0, "observe owned fork child");
                assert!(
                    start.elapsed() < Duration::from_secs(3),
                    "fork child deadline"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    impl Drop for ForkChild {
        fn drop(&mut self) {
            if let Some(pid) = self.0.take() {
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                    while libc::waitpid(pid, std::ptr::null_mut(), 0) < 0
                        && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
                    {
                    }
                }
            }
        }
    }
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("history-gate-{}", crate::new_identity().unwrap()));
            crate::create_layout(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn nonempty_symlink_and_hardlinked_gates_are_never_accepted() {
        let d = Directory::new();
        drop(HistoryGate::shared(&d.0).unwrap());
        let p = d.0.join(".ulmdb-coordination/history");
        fs::write(&p, b"bad").unwrap();
        assert!(matches!(HistoryGate::shared(&d.0), Err(Error::Corrupt(_))));
        fs::write(&p, b"").unwrap();
        let other = d.0.join("other");
        fs::hard_link(&p, &other).unwrap();
        assert!(matches!(
            HistoryGate::exclusive(&d.0),
            Err(Error::Corrupt(_))
        ));
        fs::remove_file(&other).unwrap();
        fs::remove_file(&p).unwrap();
        std::os::unix::fs::symlink(&other, &p).unwrap();
        assert!(HistoryGate::shared(&d.0).is_err());
        assert!(!other.exists());
    }
    #[test]
    #[allow(unsafe_code)]
    fn foreign_fork_drop_does_not_unlock_parent_history_lease() {
        const NAME: &str = "history::tests::foreign_fork_drop_does_not_unlock_parent_history_lease";
        if run_isolated(NAME) {
            return;
        }
        let d = Directory::new();
        let gate = HistoryGate::shared(&d.0).unwrap();
        let child = unsafe { libc::fork() };
        assert!(child >= 0);
        if child == 0 {
            let invalid = matches!(gate.check(&d.0), Err(Error::InvalidInput(_)));
            drop(gate);
            unsafe { libc::_exit(if invalid { 0 } else { 1 }) }
        }
        let mut child = ForkChild(Some(child));
        child.wait_success();
        let observer = open(&d.0.join(".ulmdb-coordination/history"), false, false).unwrap();
        assert!(matches!(
            observer.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(gate);
        observer.try_lock().unwrap();
        worker_completed(NAME);
    }
    #[test]
    #[allow(unsafe_code)]
    fn creator_drop_unlocks_even_while_foreign_child_retains_its_ofd_copy() {
        const NAME: &str =
            "history::tests::creator_drop_unlocks_even_while_foreign_child_retains_its_ofd_copy";
        if run_isolated(NAME) {
            return;
        }
        let d = Directory::new();
        let gate = HistoryGate::shared(&d.0).unwrap();
        let mut pipe = [0; 2];
        assert_eq!(
            unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) },
            0
        );
        let receive = unsafe { File::from_raw_fd(pipe[0]) };
        let send = unsafe { File::from_raw_fd(pipe[1]) };
        let child = unsafe { libc::fork() };
        assert!(child >= 0);
        if child == 0 {
            drop(send);
            unsafe {
                let mut event = libc::pollfd {
                    fd: receive.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                if libc::poll(&mut event, 1, 2000) != 1 {
                    libc::_exit(2);
                }
                let mut byte = 0u8;
                if libc::read(receive.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) != 1
                    || byte != b'x'
                {
                    libc::_exit(3);
                }
            }
            drop(gate);
            unsafe { libc::_exit(0) }
        }
        let mut child = ForkChild(Some(child));
        drop(receive);
        drop(gate);
        let observer = open(&d.0.join(".ulmdb-coordination/history"), false, false).unwrap();
        observer.try_lock().unwrap();
        assert_eq!(
            unsafe { libc::write(send.as_raw_fd(), b"x".as_ptr().cast(), 1) },
            1
        );
        drop(send);
        child.wait_success();
        worker_completed(NAME);
    }
}
