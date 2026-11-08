//! Fork only after exec into one exact test. A parked helper then deliberately
//! holds process-local state that has no surviving thread in the fork child.
#![allow(unsafe_code)]
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    sync::Mutex,
    thread::JoinHandle,
    time::{Duration, Instant},
};

fn pipe() -> (File, File) {
    let mut fds = [-1; 2];
    assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
    // SAFETY: pipe2 returned unique FDs, now owned exactly once by these Files.
    unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
}

pub(crate) struct DropProbe(File);
pub(crate) struct ProbeReader(File);
impl DropProbe {
    pub(crate) fn new() -> (Self, ProbeReader) {
        let (reader, writer) = pipe();
        (Self(writer), ProbeReader(reader))
    }
}
impl Drop for DropProbe {
    fn drop(&mut self) {
        assert_eq!(
            unsafe { libc::write(self.0.as_raw_fd(), b"D".as_ptr().cast(), 1) },
            1
        );
    }
}
impl ProbeReader {
    pub(crate) fn absent(&self) {
        let mut fd = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        assert_eq!(
            unsafe { libc::poll(&mut fd, 1, 0) },
            0,
            "fork child traversed inherited resources"
        );
    }
    pub(crate) fn once(&mut self) {
        let mut byte = [0];
        self.0.read_exact(&mut byte).unwrap();
        assert_eq!(byte, *b"D");
        // The creator's final destructor closed the last writer.
        assert_eq!(self.0.read(&mut byte).unwrap(), 0);
    }
}

pub(crate) struct HeldMutex {
    release: Option<File>,
    worker: Option<JoinHandle<()>>,
}
impl HeldMutex {
    /// SAFETY: caller keeps the mutex allocation alive until this guard drops.
    /// The helper borrows through a raw pointer, not an Arc: an extra inherited
    /// Arc would prevent the last-owner destructor being tested in the child.
    pub(crate) unsafe fn new<T: Send>(mutex: &Mutex<T>) -> Self {
        let address = mutex as *const Mutex<T> as usize;
        let (mut started, mut ready) = pipe();
        let (mut resume, release) = pipe();
        let worker = std::thread::spawn(move || {
            let guard = unsafe { &*(address as *const Mutex<T>) }
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            ready.write_all(b"R").unwrap();
            // No Rust allocation/other engine locks while the mutex is parked.
            let mut byte = [0];
            resume.read_exact(&mut byte).unwrap();
            drop(guard);
        });
        let mut byte = [0];
        started.read_exact(&mut byte).unwrap();
        assert_eq!(byte, *b"R");
        Self {
            release: Some(release),
            worker: Some(worker),
        }
    }
}
impl Drop for HeldMutex {
    fn drop(&mut self) {
        if let Some(mut release) = self.release.take() {
            let _ = release.write_all(b"R");
        }
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

struct Child(Option<libc::pid_t>);
impl Drop for Child {
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
pub(crate) fn fork_drop<T>(value: T) -> T {
    // On a failed child assertion, leak this inherited test value until the
    // worker exits instead of dropping it while its helper still holds a lock.
    // The caller's guard can then unlock/join during unwinding, and Child below
    // always kills/reaps the raw child before the isolated worker terminates.
    let value = std::mem::ManuallyDrop::new(value);
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        drop(std::mem::ManuallyDrop::into_inner(value));
        // Never run unrelated inherited Rust destructors or libtest cleanup.
        unsafe { libc::_exit(0) };
    }
    let mut child = Child(Some(pid));
    let begin = Instant::now();
    loop {
        let mut status = 0;
        let got = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if got == pid {
            child.0 = None;
            assert_eq!(status, 0, "fork child status");
            return std::mem::ManuallyDrop::into_inner(value);
        }
        if got < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        assert_eq!(got, 0);
        assert!(
            begin.elapsed() < Duration::from_secs(2),
            "fork drop child deadline"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn inherited_container_skips_contents_while_creator_drops_once() {
    const NAME: &str =
        "creator_owned::tests::inherited_container_skips_contents_while_creator_drops_once";
    if crate::history::tests::run_isolated(NAME) {
        return;
    }
    let (probe, mut reader) = DropProbe::new();
    let value = Box::new(super::CreatorOwned::new(Mutex::new(vec![probe])));
    let held = unsafe { HeldMutex::new(&value) };
    let value = fork_drop(value);
    drop(held);
    reader.absent();
    drop(value);
    reader.once();
    crate::history::tests::worker_completed(NAME);
}
