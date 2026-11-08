//! Linux OS boundary: deterministic fork inheritance, no sleeps or reopen retry.
#![cfg(target_os = "linux")]
#![allow(unsafe_code)]
use std::{
    fs::{self, File, OpenOptions},
    os::fd::{AsRawFd, RawFd},
    path::{Path, PathBuf},
    sync::Arc,
};
use ulmdb_storage::WriterFence;
fn busy(path: &Path) -> bool {
    let file = File::open(path).unwrap();
    match file.try_lock() {
        Ok(()) => {
            file.unlock().unwrap();
            false
        }
        Err(fs::TryLockError::WouldBlock) => true,
        Err(e) => panic!("{e:?}"),
    }
}
struct Child {
    pid: i32,
    send: RawFd,
    receive: RawFd,
}
impl Child {
    fn command(&self, b: u8) {
        unsafe {
            assert_eq!(
                libc::write(self.send, &b as *const _ as *const libc::c_void, 1),
                1
            );
        }
        assert_eq!(self.reply(), b);
    }
    fn reply(&self) -> u8 {
        unsafe {
            let mut event = libc::pollfd {
                fd: self.receive,
                events: libc::POLLIN,
                revents: 0,
            };
            assert_eq!(
                libc::poll(&mut event, 1, 5000),
                1,
                "bounded child handshake"
            );
            let mut b = 0;
            assert_eq!(
                libc::read(self.receive, &mut b as *mut _ as *mut libc::c_void, 1),
                1
            );
            b
        }
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.pid, libc::SIGKILL);
            let mut status = 0;
            libc::waitpid(self.pid, &mut status, 0);
            libc::close(self.send);
            libc::close(self.receive);
        }
    }
}
fn pipes() -> ([i32; 2], [i32; 2]) {
    unsafe {
        let mut a = [0; 2];
        let mut b = [0; 2];
        assert_eq!(libc::pipe2(a.as_mut_ptr(), libc::O_CLOEXEC), 0);
        assert_eq!(libc::pipe2(b.as_mut_ptr(), libc::O_CLOEXEC), 0);
        (a, b)
    }
}
unsafe fn child_loop(commands: [i32; 2], acks: [i32; 2], close: RawFd) -> ! {
    unsafe {
        libc::close(commands[1]);
        libc::close(acks[0]);
        let ready = b'R';
        libc::write(acks[1], &ready as *const _ as *const libc::c_void, 1);
        loop {
            let mut b = 0;
            if libc::read(commands[0], &mut b as *mut _ as *mut libc::c_void, 1) != 1 {
                libc::_exit(2)
            }
            if b == b'C' && libc::close(close) != 0 {
                libc::_exit(3)
            }
            libc::write(acks[1], &b as *const _ as *const libc::c_void, 1);
        }
    }
}
fn parent(pid: i32, commands: [i32; 2], acks: [i32; 2]) -> Child {
    unsafe {
        libc::close(commands[0]);
        libc::close(acks[1]);
    }
    let c = Child {
        pid,
        send: commands[1],
        receive: acks[0],
    };
    assert_eq!(c.reply(), b'R');
    c
}
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn path(root: &str, name: &str) -> Temp {
    Temp(Path::new(root).join(format!("ulmdb-file-fence-{}-{name}", std::process::id())))
}
fn fence(path: &Path) -> WriterFence {
    WriterFence::try_lock(
        OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(path)
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn creator_last_arc_unlocks_despite_inherited_fd_and_child_drop_cannot_unlock_parent() {
    for root in ["/tmp", "/dev/shm"] {
        let temp = path(root, "arc");
        let guard = Arc::new(fence(&temp.0));
        let lease = Arc::clone(&guard);
        let fd = guard.as_raw_fd();
        assert_ne!(
            unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        let (commands, acks) = pipes();
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            unsafe { child_loop(commands, acks, fd) }
        }
        let child = parent(pid, commands, acks);
        drop(guard);
        assert!(busy(&temp.0), "last private-capability owner retains fence");
        drop(lease);
        assert!(
            !busy(&temp.0),
            "creator final owner explicitly unlocks despite child FD"
        );
        let replacement = fence_reopen(&temp.0);
        child.command(b'C');
        assert!(
            busy(&temp.0),
            "closing old inherited OFD cannot release new owner"
        );
        drop(replacement);
        drop(child);

        let temp = path(root, "foreign-drop");
        let guard = fence(&temp.0);
        let (commands, acks) = pipes();
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            // The actual guard Drop performs only PID query/close in the foreign child.
            // No allocator, panic, Rust IO or lock acquisition occurs in this child branch.
            drop(guard);
            unsafe { child_loop(commands, acks, -1) }
        }
        let child = parent(pid, commands, acks);
        assert!(
            busy(&temp.0),
            "foreign child destructor must not unlock creator"
        );
        drop(guard);
        assert!(!busy(&temp.0));
        drop(child);
    }
}
fn fence_reopen(path: &Path) -> WriterFence {
    WriterFence::try_lock(
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap(),
    )
    .unwrap()
}
