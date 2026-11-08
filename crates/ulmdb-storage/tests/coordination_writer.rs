//! Real OFD/thread/process journeys for the common native/C writer lease.
#![cfg(target_os = "linux")]
#![allow(unsafe_code)]

use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{BufRead, BufReader, Write},
    os::unix::fs::{MetadataExt, symlink},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};
use ulmdb_storage::coordination::WriterLease;
use ulmdb_storage::access;
pub use ulmdb_storage::{Error, Result};

// Compile an independent copy, including any runtime-local state. The shared
// on-disk/kernel owner protocol must work across this boundary without TLS.
#[path = "../src/coordination/writer.rs"]
mod second_copy;

const DEADLINE: Duration = Duration::from_secs(8);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-common-writer-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn record(&self) -> PathBuf {
        self.0.join("LMDB-WRITER")
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn file(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .unwrap()
}
fn signal(message: &str) {
    println!("WRITER {message}");
    std::io::stdout().flush().unwrap();
}
fn command(input: &mut impl BufRead) -> String {
    let mut value = String::new();
    assert_ne!(input.read_line(&mut value).unwrap(), 0);
    value.trim().to_owned()
}
struct Worker {
    child: Child,
    input: ChildStdin,
    output: Receiver<String>,
}
impl Worker {
    fn start(directory: &Directory, role: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "writer_process_worker", "--nocapture"])
            .env("ULMDB_WRITER_TEST_PATH", &directory.0)
            .env("ULMDB_WRITER_TEST_ROLE", role)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (send, receive) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if let Some((_, message)) = line.split_once("WRITER ") {
                    if send.send(message.to_owned()).is_err() {
                        break;
                    }
                }
            }
        });
        Self {
            child,
            input,
            output: receive,
        }
    }
    fn expect(&mut self, expected: &str) {
        assert_eq!(self.output.recv_timeout(DEADLINE).unwrap(), expected);
    }
    fn send(&mut self, value: &str) {
        writeln!(self.input, "{value}").unwrap();
        self.input.flush().unwrap();
    }
    fn close(mut self) {
        self.send("close");
        self.expect("closed");
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(start.elapsed() < DEADLINE);
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn kill(mut self) {
        self.child.kill().unwrap();
        assert!(!self.child.wait().unwrap().success());
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
fn writer_process_worker() {
    let Ok(role) = std::env::var("ULMDB_WRITER_TEST_ROLE") else {
        return;
    };
    let path = PathBuf::from(std::env::var_os("ULMDB_WRITER_TEST_PATH").unwrap());
    let mut input = std::io::stdin().lock();
    match role.as_str() {
        "recursion" => {
            let first = WriterLease::acquire(&path).unwrap();
            assert!(matches!(
                second_copy::WriterLease::acquire(&path),
                Err(Error::Busy)
            ));
            signal("recursive-copy-rejected");
            assert_eq!(command(&mut input), "close");
            drop(first);
        }
        "wait" => {
            signal("attempting");
            let _lease = second_copy::WriterLease::acquire(&path).unwrap();
            signal("acquired");
            assert_eq!(command(&mut input), "close");
        }
        "replaced-path" => {
            signal("attempting");
            assert!(matches!(
                WriterLease::acquire(&path),
                Err(Error::Corrupt(_))
            ));
            signal("replaced-path-rejected");
            assert_eq!(command(&mut input), "close");
        }
        "hold" => {
            let _lease = WriterLease::acquire(&path).unwrap();
            signal("holding");
            assert_eq!(command(&mut input), "close");
        }
        "short" | "empty" => {
            let mut selected = file(&path.join("LMDB-WRITER"));
            selected.lock().unwrap();
            if role == "short" {
                selected.write_all(b"partial").unwrap();
            }
            signal("transition");
            assert_eq!(command(&mut input), "release");
            selected.set_len(0).unwrap();
            selected.unlock().unwrap();
            signal("released");
            assert_eq!(command(&mut input), "close");
        }
        "malformed" => {
            let mut selected = file(&path.join("LMDB-WRITER"));
            selected.lock().unwrap();
            selected.write_all(&[0xa5; 32]).unwrap();
            assert!(matches!(
                WriterLease::acquire(&path),
                Err(Error::Corrupt(_))
            ));
            signal("malformed-rejected");
            assert_eq!(command(&mut input), "close");
            selected.set_len(0).unwrap();
        }
        _ => panic!("unknown writer role"),
    }
    signal("closed");
}

#[test]
fn independent_compiled_copy_reentry_fails_without_tls_or_deadlock() {
    let directory = Directory::new();
    let mut worker = Worker::start(&directory, "recursion");
    worker.expect("recursive-copy-rejected");
    worker.close();
    assert!(fs::read(directory.record()).unwrap().is_empty());
}

#[test]
fn another_thread_blocks_until_owner_releases_then_makes_progress() {
    let directory = Directory::new();
    let owner = WriterLease::acquire(&directory.0).unwrap();
    let (send, receive) = mpsc::channel();
    let path = directory.0.clone();
    let worker = thread::spawn(move || {
        send.send("attempting").unwrap();
        let _lease = second_copy::WriterLease::acquire(path).unwrap();
        send.send("acquired").unwrap();
    });
    assert_eq!(receive.recv_timeout(DEADLINE).unwrap(), "attempting");
    assert!(matches!(
        receive.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    drop(owner);
    assert_eq!(receive.recv_timeout(DEADLINE).unwrap(), "acquired");
    worker.join().unwrap();
    assert!(fs::read(directory.record()).unwrap().is_empty());
}

#[test]
fn independent_process_waits_and_progresses_after_creator_release() {
    let directory = Directory::new();
    let owner = WriterLease::acquire(&directory.0).unwrap();
    let mut worker = Worker::start(&directory, "wait");
    worker.expect("attempting");
    assert!(matches!(
        worker.output.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    drop(owner);
    worker.expect("acquired");
    worker.close();
}

#[test]
fn locked_empty_and_short_publication_transitions_wait_without_busy() {
    for role in ["empty", "short"] {
        let directory = Directory::new();
        let mut holder = Worker::start(&directory, role);
        holder.expect("transition");
        let mut waiting = Worker::start(&directory, "wait");
        waiting.expect("attempting");
        assert!(matches!(
            waiting.output.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        holder.send("release");
        holder.expect("released");
        waiting.expect("acquired");
        waiting.close();
        holder.close();
    }
}

#[test]
fn locked_complete_malformed_record_fails_closed_promptly() {
    let directory = Directory::new();
    let mut worker = Worker::start(&directory, "malformed");
    worker.expect("malformed-rejected");
    worker.close();
}

#[test]
fn unlocked_stale_and_partial_state_is_superseded_after_kernel_ownership() {
    let directory = Directory::new();
    let first = WriterLease::acquire(&directory.0).unwrap();
    let stale = fs::read(directory.record()).unwrap();
    assert_eq!(stale.len(), 32);
    drop(first);
    for bytes in [stale.as_slice(), b"partial", b""] {
        fs::write(directory.record(), bytes).unwrap();
        let lease = second_copy::WriterLease::acquire(&directory.0).unwrap();
        assert_eq!(fs::read(directory.record()).unwrap().len(), 32);
        assert!(matches!(
            WriterLease::acquire(&directory.0),
            Err(Error::Busy)
        ));
        drop(lease);
        assert!(fs::read(directory.record()).unwrap().is_empty());
    }
}

#[test]
fn dead_process_releases_kernel_lock_and_stale_owner_record_is_replaced() {
    let directory = Directory::new();
    let mut worker = Worker::start(&directory, "hold");
    worker.expect("holding");
    let stale = fs::read(directory.record()).unwrap();
    assert_eq!(stale.len(), 32);
    worker.kill();
    assert_eq!(fs::read(directory.record()).unwrap(), stale);
    let lease = WriterLease::acquire(&directory.0).unwrap();
    assert_ne!(fs::read(directory.record()).unwrap(), stale);
    drop(lease);
}

#[test]
fn symlinks_hardlinks_nonregular_and_oversized_paths_are_rejected_without_rewrite() {
    let directory = Directory::new();
    let target = directory.0.join("unrelated");
    fs::write(&target, b"preserved").unwrap();
    symlink(&target, directory.record()).unwrap();
    assert!(WriterLease::acquire(&directory.0).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"preserved");
    fs::remove_file(directory.record()).unwrap();
    fs::hard_link(&target, directory.record()).unwrap();
    assert!(matches!(
        WriterLease::acquire(&directory.0),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(fs::read(&target).unwrap(), b"preserved");
    fs::remove_file(directory.record()).unwrap();
    fs::create_dir(directory.record()).unwrap();
    assert!(WriterLease::acquire(&directory.0).is_err());
    fs::remove_dir(directory.record()).unwrap();
    fs::write(directory.record(), [0; 33]).unwrap();
    assert!(matches!(
        WriterLease::acquire(&directory.0),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(fs::read(directory.record()).unwrap(), [0; 33]);
    let alias = directory.0.join("alias");
    symlink(&directory.0, &alias).unwrap();
    assert!(WriterLease::acquire(alias).is_err());
}

#[test]
fn blocked_waiter_rechecks_inode_and_never_rewrites_replacement() {
    let directory = Directory::new();
    let owner = WriterLease::acquire(&directory.0).unwrap();
    let inode = fs::metadata(directory.record()).unwrap().ino();
    let mut waiter = Worker::start(&directory, "replaced-path");
    waiter.expect("attempting");
    let start = Instant::now();
    let pid = waiter.child.id().to_string();
    // Observe the actual kernel wait, rather than guessing with a sleep that
    // the child has already opened the inode which will be replaced.
    loop {
        let locks = fs::read_to_string("/proc/locks").unwrap();
        if locks.lines().any(|line| {
            line.contains("->")
                && line.contains(&format!(":{inode} "))
                && line.split_whitespace().any(|field| field == pid)
        }) {
            break;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "child never entered the kernel lock wait"
        );
        thread::sleep(Duration::from_millis(10));
    }
    fs::remove_file(directory.record()).unwrap();
    fs::write(directory.record(), b"").unwrap();
    assert_ne!(fs::metadata(directory.record()).unwrap().ino(), inode);
    drop(owner);
    waiter.expect("replaced-path-rejected");
    assert!(fs::read(directory.record()).unwrap().is_empty());
    waiter.close();
}

#[test]
fn foreign_fork_drop_preserves_creator_record_and_lock() {
    let directory = Directory::new();
    let lease = WriterLease::acquire(&directory.0).unwrap();
    let record = fs::read(directory.record()).unwrap();
    let mut pipe = [0; 2];
    assert_eq!(
        unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) },
        0
    );
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        // Drop uses only PID query/ftruncate/flock/close; a foreign PID skips
        // ftruncate/flock, and the lease owns no heap allocations to free.
        drop(lease);
        unsafe {
            libc::close(pipe[0]);
            let byte = 1u8;
            let written = libc::write(pipe[1], (&byte as *const u8).cast(), 1);
            libc::_exit(if written == 1 { 0 } else { 2 });
        }
    }
    unsafe {
        libc::close(pipe[1]);
    }
    let mut response = [0; 1];
    assert_eq!(
        unsafe { libc::read(pipe[0], response.as_mut_ptr().cast(), 1) },
        1
    );
    unsafe {
        libc::close(pipe[0]);
    }
    let mut status = 0;
    assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
    assert!(libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0);
    assert_eq!(fs::read(directory.record()).unwrap(), record);
    assert!(matches!(
        file(&directory.record()).try_lock(),
        Err(TryLockError::WouldBlock)
    ));
    drop(lease);
    file(&directory.record()).try_lock().unwrap();
}

#[test]
fn creator_release_progresses_despite_inherited_fd_then_foreign_drop_keeps_new_owner() {
    let directory = Directory::new();
    let lease = WriterLease::acquire(&directory.0).unwrap();
    let mut commands = [0; 2];
    let mut replies = [0; 2];
    assert_eq!(
        unsafe { libc::pipe2(commands.as_mut_ptr(), libc::O_CLOEXEC) },
        0
    );
    assert_eq!(
        unsafe { libc::pipe2(replies.as_mut_ptr(), libc::O_CLOEXEC) },
        0
    );
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        unsafe {
            libc::close(commands[1]);
            libc::close(replies[0]);
            let ready = 1u8;
            if libc::write(replies[1], (&ready as *const u8).cast(), 1) != 1 {
                libc::_exit(2);
            }
            let mut release = 0u8;
            if libc::read(commands[0], (&mut release as *mut u8).cast(), 1) != 1 {
                libc::_exit(2);
            }
        }
        drop(lease);
        unsafe {
            let done = 2u8;
            let written = libc::write(replies[1], (&done as *const u8).cast(), 1);
            libc::_exit(if written == 1 { 0 } else { 2 });
        }
    }
    struct ForkChild {
        pid: i32,
        command: i32,
        reply: i32,
    }
    impl ForkChild {
        fn byte(&self) -> u8 {
            let mut poll = libc::pollfd {
                fd: self.reply,
                events: libc::POLLIN,
                revents: 0,
            };
            assert_eq!(
                unsafe { libc::poll(&mut poll, 1, DEADLINE.as_millis() as i32) },
                1
            );
            let mut byte = 0;
            assert_eq!(
                unsafe { libc::read(self.reply, (&mut byte as *mut u8).cast(), 1) },
                1
            );
            byte
        }
    }
    impl Drop for ForkChild {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.command);
                libc::close(self.reply);
                libc::kill(self.pid, libc::SIGKILL);
                libc::waitpid(self.pid, std::ptr::null_mut(), 0);
            }
        }
    }
    unsafe {
        libc::close(commands[0]);
        libc::close(replies[1]);
    }
    let child = ForkChild {
        pid,
        command: commands[1],
        reply: replies[0],
    };
    assert_eq!(child.byte(), 1);
    drop(lease);
    assert!(fs::read(directory.record()).unwrap().is_empty());
    // The child still holds the inherited old OFD. Explicit creator unlock
    // must nevertheless permit this independently opened replacement owner.
    let next = second_copy::WriterLease::acquire(&directory.0).unwrap();
    let record = fs::read(directory.record()).unwrap();
    let release = 1u8;
    assert_eq!(
        unsafe { libc::write(child.command, (&release as *const u8).cast(), 1) },
        1
    );
    assert_eq!(child.byte(), 2);
    assert_eq!(fs::read(directory.record()).unwrap(), record);
    assert!(matches!(
        file(&directory.record()).try_lock(),
        Err(TryLockError::WouldBlock)
    ));
    drop(next);
}
