#![cfg(target_os = "linux")]
#![allow(unsafe_code)]
use std::{
    fs,
    os::unix::fs::{MetadataExt, symlink},
    path::{Path, PathBuf},
};
use ulmdb_storage::{
    Error, Manifest,
    coordination::{Coordination, Limits},
    create_layout, new_identity, publish, read_current,
};
struct Fixture {
    path: PathBuf,
    manifest: Manifest,
}
impl Fixture {
    fn new(format: u16) -> Self {
        let path =
            std::env::temp_dir().join(format!("cached-directory-{:032x}", new_identity().unwrap()));
        create_layout(&path).unwrap();
        let mut manifest = Manifest::empty().unwrap();
        manifest.format_minor = format;
        if format == 4 {
            manifest.prepare_indexed(None).unwrap();
        }
        publish(&path, &manifest).unwrap();
        Self { path, manifest }
    }
    fn coordinator(&self, limit: usize) -> Coordination {
        Coordination::open(
            &self.path,
            self.manifest.uuid,
            Limits {
                max_pin_records: limit,
                ..Default::default()
            },
        )
        .unwrap()
    }
    fn pins(&self) -> usize {
        fs::read_dir(self.path.join(".ulmdb-coordination/pins"))
            .unwrap()
            .count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn source_fds(path: &Path) -> usize {
    fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|x| fs::read_link(x.ok()?.path()).ok())
        .filter(|x| x.starts_with(path))
        .count()
}
#[test]
fn bound_capture_reuses_three_held_directories_with_live_clones_and_nonblocking_exclusion() {
    let f = Fixture::new(2);
    let c = f.coordinator(2);
    let clone = c.clone();
    let baseline = source_fds(&f.path);
    let mut captures = Vec::new();
    for _ in 0..32 {
        let capture = c.capture_bound_directory().unwrap();
        let (directory, identity) = capture.directory_binding().unwrap();
        let metadata = fs::symlink_metadata(&f.path).unwrap();
        assert_eq!(directory, f.path);
        assert_eq!(identity, (metadata.dev(), metadata.ino()));
        captures.push(capture);
    }
    assert_eq!(
        source_fds(&f.path),
        baseline + 32,
        "one gate FD per capture; directory FDs are shared"
    );
    assert!(matches!(clone.try_maintenance(), Err(Error::Busy)));
    for capture in captures {
        let (bound, lease) = capture.bind(f.manifest.uuid).unwrap();
        drop(bound);
        drop(lease);
    }
    assert_eq!(source_fds(&f.path), baseline);
    let maintenance = clone.try_maintenance().unwrap();
    assert!(matches!(c.capture_bound_directory(), Err(Error::Busy)));
    drop(maintenance);
    assert!(c.capture_bound_directory().is_ok());
    assert_eq!(f.pins(), 0);
}
#[test]
fn bound_bind_rejects_null_and_foreign_uuid_without_pin_and_original_owner_remains_usable() {
    for format in [2, 3, 4] {
        let f = Fixture::new(format);
        let c = f.coordinator(2);
        let mut foreign = f.manifest.uuid;
        foreign[0] ^= 1;
        assert!(matches!(
            c.capture_bound_directory().unwrap().bind([0; 16]),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            c.capture_bound_directory().unwrap().bind(foreign),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(f.pins(), 0);
        let (_, lease, pin) = c
            .capture_bound_directory()
            .unwrap()
            .capture_current()
            .unwrap();
        assert_eq!(pin.root().manifest().uuid, f.manifest.uuid);
        drop(pin);
        drop(lease);
        assert_eq!(f.pins(), 0);
        let (_, lease) = Coordination::capture_directory(&f.path, Default::default())
            .unwrap()
            .bind(f.manifest.uuid)
            .unwrap();
        drop(lease);
    }
}
#[test]
fn bound_capture_keeps_custom_two_record_limit_even_with_existing_pins_and_shared_inner() {
    for format in [2, 3, 4] {
        let f = Fixture::new(format);
        let c = f.coordinator(2);
        let clone = c.clone();
        let (_, one, pin1) = c
            .capture_bound_directory()
            .unwrap()
            .capture_current()
            .unwrap();
        let (_, two, pin2) = clone
            .capture_bound_directory()
            .unwrap()
            .capture_current()
            .unwrap();
        assert_eq!(f.pins(), 2);
        assert!(matches!(
            c.capture_bound_directory().unwrap().capture_current(),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(f.pins(), 2);
        drop((one, pin1));
        let (_, three, pin3) = c
            .capture_bound_directory()
            .unwrap()
            .capture_current()
            .unwrap();
        assert_eq!(f.pins(), 2);
        drop((two, pin2, three, pin3));
        assert_eq!(f.pins(), 0);
    }
}
fn copy_assets(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap() {
        let p = entry.unwrap().path();
        let name = p.file_name().unwrap();
        if name == "CURRENT" || name == "PREVIOUS" || name == ".ulmdb-coordination" {
            continue;
        }
        let dst = to.join(name);
        if p.is_dir() {
            fs::create_dir_all(&dst).unwrap();
            copy_assets(&p, &dst);
        } else {
            fs::copy(&p, dst).unwrap();
        }
    }
}
#[test]
fn actual_valid_foreign_current_in_same_inode_cannot_retag_cached_realm() {
    for format in [2, 3, 4] {
        let f = Fixture::new(format);
        let foreign = Fixture::new(format);
        let c = f.coordinator(2);
        let identity = (
            fs::metadata(&f.path).unwrap().dev(),
            fs::metadata(&f.path).unwrap().ino(),
        );
        let saved = fs::read(f.path.join("CURRENT")).unwrap();
        copy_assets(&foreign.path, &f.path);
        fs::write(
            f.path.join("CURRENT"),
            fs::read(foreign.path.join("CURRENT")).unwrap(),
        )
        .unwrap();
        assert_eq!(read_current(&f.path).unwrap().uuid, foreign.manifest.uuid);
        assert_eq!(
            (
                fs::metadata(&f.path).unwrap().dev(),
                fs::metadata(&f.path).unwrap().ino()
            ),
            identity
        );
        assert!(matches!(
            c.capture_bound_directory().unwrap().capture_current(),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(f.pins(), 0);
        fs::write(f.path.join("CURRENT"), saved).unwrap();
        let (_, lease, pin) = c
            .capture_bound_directory()
            .unwrap()
            .capture_current()
            .unwrap();
        assert_eq!(pin.root().manifest().uuid, f.manifest.uuid);
        drop((lease, pin));
        assert_eq!(f.pins(), 0);
    }
}
#[test]
fn cached_and_already_acquired_captures_reject_replaced_or_symlinked_namespace() {
    for replace_with_symlink in [false, true] {
        let f = Fixture::new(2);
        let c = f.coordinator(2);
        let held = c.capture_bound_directory().unwrap();
        let moved = f.path.with_extension("moved");
        fs::rename(&f.path, &moved).unwrap();
        if replace_with_symlink {
            symlink(&moved, &f.path).unwrap();
        } else {
            create_layout(&f.path).unwrap();
        }
        assert!(c.capture_bound_directory().is_err());
        assert!(held.directory_binding().is_err());
        assert!(held.bind(f.manifest.uuid).is_err());
        assert!(
            fs::read_dir(moved.join(".ulmdb-coordination/pins"))
                .unwrap()
                .next()
                .is_none()
        );
        if replace_with_symlink {
            fs::remove_file(&f.path).unwrap();
        }
        fs::remove_dir_all(moved).unwrap();
    }
}
#[test]
fn cached_capture_fork_worker() {
    if std::env::var_os("ULMDB_CACHED_FORK_BODY").is_none() {
        return;
    }
    for exclusive in [false, true] {
        let f = Fixture::new(2);
        let c = f.coordinator(2);
        let capture = if exclusive {
            None
        } else {
            Some(c.capture_bound_directory().unwrap())
        };
        let maintenance = if exclusive {
            Some(c.try_maintenance().unwrap())
        } else {
            None
        };
        let mut pipe = [0; 2];
        assert_eq!(
            unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) },
            0
        );
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            let binding_rejected = capture.as_ref().is_none_or(|capture| {
                matches!(capture.directory_binding(), Err(Error::InvalidInput(_)))
            });
            let rejected = binding_rejected
                && matches!(c.capture_bound_directory(), Err(Error::InvalidInput(_)));
            drop(capture);
            drop(maintenance);
            let byte = u8::from(rejected);
            unsafe {
                libc::close(pipe[0]);
                libc::write(pipe[1], (&byte as *const u8).cast(), 1);
                libc::_exit(0);
            }
        }
        unsafe {
            libc::close(pipe[1]);
        }
        let mut poll = libc::pollfd {
            fd: pipe[0],
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll, 1, 5000) };
        if ready != 1 {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
        let mut byte = 0u8;
        let received = if ready == 1 {
            unsafe { libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1) }
        } else {
            0
        };
        let mut status = 0;
        unsafe {
            libc::waitpid(pid, &mut status, 0);
            libc::close(pipe[0]);
        }
        assert_eq!((ready, received, byte, status), (1, 1, 1, 0));
        if exclusive {
            assert!(matches!(c.capture_bound_directory(), Err(Error::Busy)));
        } else {
            assert!(matches!(c.try_maintenance(), Err(Error::Busy)));
        }
        drop(capture);
        drop(maintenance);
        assert!(c.try_maintenance().is_ok());
    }
}

struct IsolatedWorker(Option<std::process::Child>);
impl IsolatedWorker {
    fn exited(&self) -> bool {
        let mut status = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        let rc = unsafe {
            libc::waitid(
                libc::P_PID,
                self.0.as_ref().unwrap().id(),
                status.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if rc < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            return false;
        }
        assert_eq!(rc, 0);
        unsafe { status.assume_init().si_pid() != 0 }
    }
    fn finish(&mut self) -> std::process::Output {
        let child = self.0.take().unwrap();
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        child.wait_with_output().unwrap()
    }
}
impl Drop for IsolatedWorker {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
        }
    }
}
#[test]
fn foreign_fork_use_precedes_busy_lock_and_drop_preserves_parent_capture_and_maintenance() {
    use std::{
        os::unix::process::CommandExt,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cached_capture_fork_worker", "--nocapture"])
        .env("ULMDB_CACHED_FORK_BODY", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let mut worker = IsolatedWorker(Some(child));
    let deadline = Instant::now() + Duration::from_secs(15);
    while !worker.exited() {
        if Instant::now() >= deadline {
            let output = worker.finish();
            panic!("isolated fork worker deadline: {output:?}");
        }
        thread::sleep(Duration::from_millis(5));
    }
    let output = worker.finish();
    assert!(
        output.status.success(),
        "worker failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
