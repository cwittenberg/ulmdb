//! Public C lifecycle and real process/native-retention controls.
use super::*;
use std::{
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};
use ulmdb::PublicationMode;

struct Directory(PathBuf);
impl Directory {
    fn new(mode: PublicationMode) -> Self {
        let path = std::env::temp_dir().join(format!(
            "c-previous-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        drop(Environment::create_with_publication(&path, mode).unwrap());
        std::fs::write(path.join("LMDB-ABI"), b"1.0\n").unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
struct Env(*mut MDB_env);
impl Env {
    fn open(path: &Path, flags: u32) -> Self {
        let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let mut env = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut env), 0);
            assert_eq!(environment::mdb_env_set_maxdbs(env, 8), 0);
            assert_eq!(
                environment::mdb_env_open(env, path.as_ptr(), flags, 0o600),
                0
            );
        }
        Self(env)
    }
    fn begin(&self, flags: u32) -> *mut MDB_txn {
        let mut txn = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(self.0, ptr::null_mut(), flags, &mut txn) },
            0
        );
        let mut main = 0;
        assert_eq!(
            unsafe { operations::mdb_dbi_open(txn, ptr::null(), 0, &mut main) },
            0
        );
        assert_eq!(main, 1);
        txn
    }
    fn flags(&self) -> u32 {
        let mut flags = 0;
        assert_eq!(
            unsafe { environment::mdb_env_get_flags(self.0, &mut flags) },
            0
        );
        flags
    }
    fn info(&self) -> usize {
        let mut info = std::mem::MaybeUninit::<MDB_envinfo>::uninit();
        assert_eq!(
            unsafe { environment::mdb_env_info(self.0, info.as_mut_ptr()) },
            0
        );
        unsafe { info.assume_init() }.me_last_txnid
    }
    fn read(&self, key: &[u8]) -> (usize, Option<Vec<u8>>) {
        let txn = self.begin(RDONLY);
        let id = unsafe { transaction::mdb_txn_id(txn) };
        let value = get(txn, 1, key);
        unsafe { transaction::mdb_txn_abort(txn) };
        (id, value)
    }
}
impl Drop for Env {
    fn drop(&mut self) {
        unsafe { environment::mdb_env_close(self.0) };
    }
}
fn put(txn: *mut MDB_txn, dbi: u32, key: &[u8], value: &[u8]) {
    let mut key = MDB_val {
        mv_size: key.len(),
        mv_data: key.as_ptr().cast_mut().cast(),
    };
    let mut value = MDB_val {
        mv_size: value.len(),
        mv_data: value.as_ptr().cast_mut().cast(),
    };
    assert_eq!(
        unsafe { operations::mdb_put(txn, dbi, &mut key, &mut value, 0) },
        0
    );
}
fn get(txn: *mut MDB_txn, dbi: u32, key: &[u8]) -> Option<Vec<u8>> {
    let mut key = MDB_val {
        mv_size: key.len(),
        mv_data: key.as_ptr().cast_mut().cast(),
    };
    let mut value = MDB_val {
        mv_size: 0,
        mv_data: ptr::null_mut(),
    };
    match unsafe { operations::mdb_get(txn, dbi, &mut key, &mut value) } {
        0 => Some(
            unsafe { std::slice::from_raw_parts(value.mv_data.cast::<u8>(), value.mv_size) }
                .to_vec(),
        ),
        NOTFOUND => None,
        code => panic!("unexpected get code {code}"),
    }
}
fn seed(env: &Env) {
    for value in [b"old".as_slice(), b"new".as_slice()] {
        let txn = env.begin(0);
        put(txn, 1, b"k", value);
        assert_eq!(unsafe { transaction::mdb_txn_commit(txn) }, 0);
    }
    assert_eq!(env.info(), 2);
}

#[test]
fn previous_worker() {
    if let Ok(role) = std::env::var("ULMDB_C_PREVIOUS_WORKER") {
        let path = PathBuf::from(std::env::var_os("ULMDB_C_PREVIOUS_PATH").unwrap());
        match role.as_str() {
            "set-and-hold" => {
                let _env = Env::open(&path, RDONLY | visibility::PREVIOUS);
                println!("PREVIOUS_WORKER_READY");
                std::io::stdout().flush().unwrap();
                let mut byte = [0];
                let _ = std::io::stdin().read_exact(&mut byte);
            }
            "fork-owner" => fork_owner(&path),
            "record-fault" => record_fault(&path),
            "resources" => resource_controls(&path),
            "prepublication-denial" => prepublication_denial(&path),
            "normal-open" => {
                println!("PREVIOUS_NORMAL_OPENING");
                std::io::stdout().flush().unwrap();
                let _env = Env::open(&path, RDONLY);
                println!("PREVIOUS_NORMAL_OPENED");
                std::io::stdout().flush().unwrap();
            }
            _ => panic!("invalid previous worker role"),
        }
    }
}

fn peer_is_kernel_blocked(path: &Path) -> bool {
    use std::os::unix::{fs::MetadataExt, process::CommandExt};
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "previous_tests::previous_worker",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("ULMDB_C_PREVIOUS_WORKER", "normal-open")
        .env("ULMDB_C_PREVIOUS_PATH", path)
        .env_remove("ULMDB_FAIL_AT")
        .env_remove("ULMDB_CRASH_AT")
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let output = child.stdout.take().unwrap();
    let (events, received) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in std::io::BufReader::new(output).lines() {
            let line = line.unwrap();
            if line.ends_with("PREVIOUS_NORMAL_OPENING") {
                let _ = events.send(false);
            }
            if line.ends_with("PREVIOUS_NORMAL_OPENED") {
                let _ = events.send(true);
            }
        }
    });
    let opening = received.recv_timeout(Duration::from_secs(8));
    let inode = std::fs::metadata(path.join("LMDB-ENV"))
        .unwrap()
        .ino()
        .to_string();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut blocked = false;
    if opening == Ok(false) {
        loop {
            if received.try_recv() == Ok(true) || child.try_wait().unwrap().is_some() {
                break;
            }
            let locks = std::fs::read_to_string("/proc/locks").unwrap();
            if locks.lines().any(|line| {
                line.contains("->")
                    && line
                        .split_whitespace()
                        .any(|word| word == child.id().to_string())
                    && line.split_whitespace().any(|word| {
                        word.contains(':') && word.rsplit(':').next() == Some(inode.as_str())
                    })
            }) {
                println!("PREVIOUS_BLOCKED_PEER pid={} inode={inode}", child.id());
                blocked = true;
                break;
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    if child.try_wait().unwrap().is_none() {
        unsafe {
            libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
        }
    }
    let status = child.wait().unwrap();
    reader.join().unwrap();
    if opening != Ok(false) {
        let mut error = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut error)
            .unwrap();
        panic!("peer did not reach open: {status}: {error}");
    }
    blocked
}

#[test]
fn failed_pending_rollback_preserves_native_owner_and_actual_environment_exclusion() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let directory = Directory::new(mode);
        {
            let env = Env::open(&directory.0, 0);
            seed(&env);
        }
        let env = Env::open(&directory.0, visibility::PREVIOUS);
        let clean = env.begin(0);
        assert_eq!(unsafe { transaction::mdb_txn_commit(clean) }, 0);
        assert!(
            peer_is_kernel_blocked(&directory.0),
            "pending PREV did not retain initial ENV EX"
        );
        // This is an adapter safety rejection, not an upstream differential:
        // opening PREV already violates rollback's immediate-commit precondition.
        assert_eq!(unsafe { rollback::mdb_env_rollback(env.0, 2) }, libc::EBUSY);
        assert!(
            peer_is_kernel_blocked(&directory.0),
            "failed rollback downgraded retained ENV EX"
        );
        assert!(matches!(Environment::open(&directory.0), Err(Error::Busy)));
        assert_eq!(env.read(b"k"), (1, Some(b"old".to_vec())));
    }
}

fn fork_owner(path: &Path) {
    let env = Env::open(path, visibility::PREVIOUS);
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        let mut txn = ptr::null_mut();
        let denied =
            unsafe { transaction::mdb_txn_begin(env.0, ptr::null_mut(), RDONLY, &mut txn) };
        let mut info = std::mem::MaybeUninit::<MDB_envinfo>::uninit();
        let metadata = unsafe { environment::mdb_env_info(env.0, info.as_mut_ptr()) };
        unsafe {
            environment::mdb_env_close(env.0);
            libc::_exit(
                if denied == libc::EINVAL && txn.is_null() && metadata == libc::EINVAL {
                    0
                } else {
                    1
                },
            );
        }
    }
    let mut status = 0;
    assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
    assert_eq!(status, 0);
    assert!(
        matches!(Environment::open(path), Err(Error::Busy)),
        "foreign Drop released native owner"
    );
    assert_eq!(env.read(b"k"), (1, Some(b"old".to_vec())));
    let mut pipe = [0; 2];
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        unsafe {
            libc::close(pipe[1]);
            let mut byte = 0u8;
            libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1);
            environment::mdb_env_close(env.0);
            libc::_exit(0);
        }
    }
    unsafe {
        libc::close(pipe[0]);
    }
    drop(env); // Creator must explicitly unlock OFDs retained in the child.
    {
        let _native = Environment::open(path).unwrap();
    }
    let fresh = Env::open(path, RDONLY);
    assert_eq!(fresh.read(b"k"), (2, Some(b"new".to_vec())));
    assert_eq!(unsafe { libc::write(pipe[1], b"x".as_ptr().cast(), 1) }, 1);
    unsafe {
        libc::close(pipe[1]);
    }
    assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
    assert_eq!(status, 0);
    assert_eq!(fresh.read(b"k"), (2, Some(b"new".to_vec())));
    println!("PREVIOUS_FORK_PASS");
}

fn record_fault(path: &Path) {
    let env = Env::open(path, visibility::PREVIOUS);
    let txn = env.begin(0);
    put(txn, 1, b"first", b"selected");
    if std::env::var("ULMDB_C_VIEW_PREPARED").as_deref() == Ok("yes") {
        assert_eq!(unsafe { transaction::mdb_txn_prepare(txn) }, 0);
    }
    let point = std::env::var("ULMDB_C_VIEW_POINT").unwrap();
    let trace = path.join("view-fault.trace");
    unsafe {
        std::env::set_var("ULMDB_C_VIEW_FAIL_AT", &point);
        std::env::set_var("ULMDB_C_VIEW_TRACE", &trace);
    }
    let result = unsafe { transaction::mdb_txn_commit(txn) };
    unsafe {
        std::env::remove_var("ULMDB_C_VIEW_FAIL_AT");
        std::env::remove_var("ULMDB_C_VIEW_TRACE");
    }
    assert_eq!(
        std::fs::read_to_string(trace).unwrap().lines().last(),
        Some(point.as_str())
    );
    assert_eq!(
        result, -30795,
        "record failure follows real durable selection"
    );
    let mut denied = ptr::null_mut();
    assert_eq!(
        unsafe { transaction::mdb_txn_begin(env.0, ptr::null_mut(), 0, &mut denied) },
        -30795
    );
    assert!(denied.is_null());
    assert!(
        matches!(Environment::open(path), Err(Error::Busy)),
        "uncertain first publication released exclusion"
    );
    drop(env);
    let fresh = Env::open(path, RDONLY);
    assert_eq!(fresh.read(b"first"), (2, Some(b"selected".to_vec())));
    println!("PREVIOUS_RECORD_FAULT_PASS point={point}");
}

#[test]
#[ignore = "subprocess real C record I/O faults; run the explicit lifecycle fault stage"]
fn previous_record_fault_controls() {
    use std::os::unix::process::CommandExt;
    let mut count = 0;
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        for prepared in [false, true] {
            for point in [
                "view.before_write",
                "view.after_write",
                "view.before_file_sync",
                "view.after_file_sync",
            ] {
                let directory = Directory::new(mode);
                {
                    let env = Env::open(&directory.0, 0);
                    seed(&env);
                }
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "previous_tests::previous_worker",
                        "--nocapture",
                        "--test-threads=1",
                    ])
                    .env("ULMDB_C_PREVIOUS_WORKER", "record-fault")
                    .env("ULMDB_C_PREVIOUS_PATH", &directory.0)
                    .env("ULMDB_C_VIEW_POINT", point)
                    .env("ULMDB_C_VIEW_PREPARED", if prepared { "yes" } else { "no" })
                    .env_remove("ULMDB_FAIL_AT")
                    .env_remove("ULMDB_CRASH_AT")
                    .env_remove("ULMDB_C_VIEW_FAIL_AT")
                    .env_remove("ULMDB_C_VIEW_TRACE")
                    .process_group(0)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let deadline = std::time::Instant::now() + Duration::from_secs(8);
                while child.try_wait().unwrap().is_none() {
                    if std::time::Instant::now() >= deadline {
                        unsafe {
                            libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
                        }
                        let result = child.wait_with_output().unwrap();
                        panic!(
                            "record fault worker deadline: {point}: {}",
                            String::from_utf8_lossy(&result.stderr)
                        );
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                let result = child.wait_with_output().unwrap();
                assert!(
                    result.status.success(),
                    "{mode:?}/{point}/prepared{prepared}: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
                assert_eq!(
                    String::from_utf8(result.stdout)
                        .unwrap()
                        .matches("PREVIOUS_RECORD_FAULT_PASS ")
                        .count(),
                    1
                );
                count += 1;
            }
        }
    }
    assert_eq!(count, 24);
}

#[test]
fn foreign_fork_cleanup_preserves_parent_and_creator_close_releases_inherited_ofds() {
    use std::os::unix::process::CommandExt;
    let directory = Directory::new(PublicationMode::Legacy);
    {
        let env = Env::open(&directory.0, 0);
        seed(&env);
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "previous_tests::previous_worker",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("ULMDB_C_PREVIOUS_WORKER", "fork-owner")
        .env("ULMDB_C_PREVIOUS_PATH", &directory.0)
        .env_remove("ULMDB_FAIL_AT")
        .env_remove("ULMDB_CRASH_AT")
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            let result = child.wait_with_output().unwrap();
            panic!(
                "fork cleanup worker deadline: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8(result.stdout)
            .unwrap()
            .matches("PREVIOUS_FORK_PASS")
            .count(),
        1
    );
}

#[test]
fn setter_death_reclaim_and_same_sequence_native_publication_never_open_stale_a() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let directory = Directory::new(mode);
        let peer = Env::open(&directory.0, 0);
        seed(&peer);
        let a = ulmdb_storage::read_previous(&directory.0).unwrap().unwrap();
        let a_path = ulmdb_storage::descriptor_path(&directory.0, &a);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "previous_tests::previous_worker",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("ULMDB_C_PREVIOUS_WORKER", "set-and-hold")
            .env("ULMDB_C_PREVIOUS_PATH", &directory.0)
            .env_remove("ULMDB_FAIL_AT")
            .env_remove("ULMDB_CRASH_AT")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        let (ready, received) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut lines = Vec::new();
            for line in std::io::BufReader::new(output).lines() {
                let line = line.unwrap();
                if line.ends_with("PREVIOUS_WORKER_READY") {
                    ready.send(()).unwrap();
                }
                lines.push(line);
            }
            lines
        });
        if received.recv_timeout(Duration::from_secs(8)).is_err() {
            let _ = child.kill();
            let result = child.wait_with_output().unwrap();
            panic!(
                "previous setter failed to become ready: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        child.kill().unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success());
        reader.join().unwrap();
        // No A read transaction has ever been issued by this surviving peer.
        // Only the durable B/A pair can retain A after the setter's death.
        let native = Environment::open(&directory.0).unwrap();
        native.reclaim().unwrap();
        assert!(a_path.exists());
        drop(native);
        assert_eq!(peer.read(b"k"), (1, Some(b"old".to_vec())));
        let native = Environment::open(&directory.0).unwrap();
        assert_eq!(native.begin_read().sequence(), 2);
        native.compact().unwrap(); // New physical D, same logical ID2/data new.
        // In cohort formats A's descriptor shares its data inode. B's retained
        // inventory can still reference that inode after the first compaction.
        // A second compaction leaves only compacted D/E origins in the pair.
        native.compact().unwrap();
        assert_eq!(native.begin_read().sequence(), 2);
        native.reclaim().unwrap();
        assert!(!a_path.exists());
        drop(native);
        assert_eq!(
            peer.read(b"k"),
            (2, Some(b"new".to_vec())),
            "a stale checkpoint was opened or invalidated using only its ID"
        );
        assert_eq!(
            std::fs::metadata(directory.0.join("LMDB-VIEW"))
                .unwrap()
                .len(),
            256
        );
    }
}

#[test]
fn pending_previous_retains_owner_and_locked_second_commit_has_real_a_base() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let directory = Directory::new(mode);
        {
            let env = Env::open(&directory.0, 0);
            seed(&env);
        }
        let env = Env::open(&directory.0, visibility::PREVIOUS);
        assert_eq!(env.read(b"k"), (1, Some(b"old".to_vec())));
        let clean = env.begin(0);
        assert_eq!(unsafe { transaction::mdb_txn_commit(clean) }, 0);
        assert_ne!(env.flags() & visibility::PREVIOUS, 0);
        assert!(matches!(Environment::open(&directory.0), Err(Error::Busy)));
        let pending = env.begin(0);
        put(pending, 1, b"aborted", b"never");
        assert_eq!(unsafe { transaction::mdb_txn_prepare(pending) }, 0);
        unsafe { transaction::mdb_txn_abort(pending) };
        assert!(matches!(Environment::open(&directory.0), Err(Error::Busy)));
        assert_eq!(env.read(b"aborted"), (1, None));
        let first = env.begin(0);
        put(first, 1, b"first", b"first");
        assert_eq!(unsafe { transaction::mdb_txn_id(first) }, 2);
        assert_eq!(unsafe { transaction::mdb_txn_commit(first) }, 0);
        assert_eq!(env.flags() & visibility::PREVIOUS, 0);
        assert_eq!(env.info(), 2);
        assert_eq!(env.read(b"first"), (1, None));
        {
            let native = Environment::open(&directory.0).unwrap();
            assert_eq!(
                native.begin_read().get(MAIN, b"first").unwrap(),
                Some(b"first".as_slice())
            );
        }
        let second = env.begin(0);
        assert_eq!(unsafe { transaction::mdb_txn_id(second) }, 2);
        assert_eq!(get(second, 1, b"first"), None);
        put(second, 1, b"second", b"second");
        assert_eq!(unsafe { transaction::mdb_txn_commit(second) }, 0);
        assert_eq!(env.read(b"first"), (2, None));
        assert_eq!(env.read(b"second"), (2, Some(b"second".to_vec())));
        drop(env);
        let reopened = Env::open(&directory.0, RDONLY);
        assert_eq!(reopened.read(b"second"), (2, Some(b"second".to_vec())));
    }
}

fn fd_count() -> usize {
    // The iterator's own descriptor is included consistently at each sample.
    std::fs::read_dir("/proc/self/fd").unwrap().count()
}
fn file_names(root: &Path) -> std::collections::BTreeSet<PathBuf> {
    fn visit(root: &Path, at: &Path, out: &mut std::collections::BTreeSet<PathBuf>) {
        for entry in std::fs::read_dir(at).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, out);
            } else {
                out.insert(path.strip_prefix(root).unwrap().to_owned());
            }
        }
    }
    let mut out = Default::default();
    visit(root, root, &mut out);
    out
}
fn resource_controls(path: &Path) {
    // Existing native identity generation retains one process-wide urandom FD.
    // Initialize that unrelated baseline resource before comparing owner close.
    ulmdb_storage::new_identity().unwrap();
    let original_fds = fd_count();
    let original_targets: Vec<_> = std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            std::fs::read_link(e.path()).ok()
        })
        .collect();
    let env = Env::open(path, 0x200000); // Transaction-owned readers, no TLS cache.
    {
        let setter = Env::open(path, RDONLY | visibility::PREVIOUS);
        assert_eq!(setter.read(b"k"), (1, Some(b"old".to_vec())));
    }
    assert_eq!(env.read(b"k"), (1, Some(b"old".to_vec())));
    // Reader slot high-water files predate G03 and legitimately grow once per
    // simultaneous slot. Warm the same concurrency before testing steady state.
    let warm: Vec<_> = (0..4).map(|_| env.begin(RDONLY)).collect();
    for reader in warm {
        unsafe { transaction::mdb_txn_abort(reader) };
    }
    let settled = fd_count();
    let names = file_names(path);
    let record = std::fs::read(path.join("LMDB-VIEW")).unwrap();
    let current = std::fs::read(path.join("CURRENT")).unwrap();
    let mut observed_max = settled;
    for _ in 0..32 {
        let readers: Vec<_> = (0..4).map(|_| env.begin(RDONLY)).collect();
        for &reader in &readers {
            assert_eq!(get(reader, 1, b"k"), Some(b"old".to_vec()));
            unsafe { transaction::mdb_txn_reset(reader) };
            assert_eq!(unsafe { transaction::mdb_txn_renew(reader) }, 0);
            assert_eq!(get(reader, 1, b"k"), Some(b"old".to_vec()));
        }
        observed_max = observed_max.max(fd_count());
        for reader in readers {
            unsafe { transaction::mdb_txn_abort(reader) };
        }
        assert_eq!(
            fd_count(),
            settled,
            "reader begin/renew leaked a descriptor"
        );
        assert_eq!(
            file_names(path),
            names,
            "readers left new persistent files or pins"
        );
        assert_eq!(std::fs::read(path.join("LMDB-VIEW")).unwrap(), record);
        assert_eq!(std::fs::read(path.join("CURRENT")).unwrap(), current);
    }
    // A subprocess owns this process-wide limit; cap it before filling so the
    // control cannot allocate a host's million-descriptor default allowance.
    let mut original = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, original.as_mut_ptr()) },
        0
    );
    let original = unsafe { original.assume_init() };
    let limited = libc::rlimit {
        rlim_cur: original.rlim_cur.min((fd_count() + 32) as _),
        rlim_max: original.rlim_max,
    };
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limited) }, 0);
    let mut filler = Vec::new();
    loop {
        match std::fs::File::open("/dev/null") {
            Ok(file) => filler.push(file),
            Err(error) => {
                assert_eq!(error.raw_os_error(), Some(libc::EMFILE));
                break;
            }
        }
    }
    let mut rejected = ptr::null_mut();
    let result =
        unsafe { transaction::mdb_txn_begin(env.0, ptr::null_mut(), RDONLY, &mut rejected) };
    drop(filler);
    assert_eq!(
        unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &original) },
        0
    );
    assert_eq!(
        result,
        libc::EMFILE,
        "FD admission failure lost its concrete cause"
    );
    assert!(rejected.is_null());
    assert_eq!(fd_count(), settled);
    assert_eq!(file_names(path), names);
    assert_eq!(std::fs::read(path.join("LMDB-VIEW")).unwrap(), record);
    assert_eq!(std::fs::read(path.join("CURRENT")).unwrap(), current);
    assert_eq!(env.read(b"k"), (1, Some(b"old".to_vec())));
    let writer = env.begin(0);
    put(writer, 1, b"progress", b"after-emfile");
    assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
    assert_eq!(env.read(b"progress"), (2, Some(b"after-emfile".to_vec())));
    drop(env);
    let final_targets: Vec<_> = std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            std::fs::read_link(e.path()).ok()
        })
        .collect();
    assert_eq!(
        fd_count(),
        original_fds,
        "environment close retained visibility/admission FDs: before={original_targets:?} after={final_targets:?}"
    );
    println!(
        "PREVIOUS_RESOURCE_PASS settled={settled} observed_max={observed_max} observed_peak_is_lower_bound=yes cycles=32 simultaneous=4"
    );
}

#[test]
fn bounded_readers_and_emfile_preserve_selection_pins_and_writer_progress() {
    use std::os::unix::process::CommandExt;
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let directory = Directory::new(mode);
        {
            let env = Env::open(&directory.0, 0);
            seed(&env);
        }
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "previous_tests::previous_worker",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("ULMDB_C_PREVIOUS_WORKER", "resources")
            .env("ULMDB_C_PREVIOUS_PATH", &directory.0)
            .env_remove("ULMDB_FAIL_AT")
            .env_remove("ULMDB_CRASH_AT")
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline {
                unsafe {
                    libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
                }
                let result = child.wait_with_output().unwrap();
                panic!(
                    "resource worker deadline: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{mode:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let output = String::from_utf8(result.stdout).unwrap();
        assert_eq!(output.matches("PREVIOUS_RESOURCE_PASS ").count(), 1);
        print!("{output}");
    }
}

fn prepublication_denial(path: &Path) {
    let pending = std::env::var("ULMDB_C_VIEW_PENDING").as_deref() == Ok("yes");
    let env = Env::open(path, if pending { visibility::PREVIOUS } else { 0 });
    let current = std::fs::read(path.join("CURRENT")).unwrap();
    let txn = env.begin(0);
    put(txn, 1, b"denied", b"never-selected");
    if std::env::var("ULMDB_C_VIEW_PREPARED").as_deref() == Ok("yes") {
        assert_eq!(unsafe { transaction::mdb_txn_prepare(txn) }, 0);
    }
    let mut damaged = std::fs::read(path.join("LMDB-VIEW")).unwrap();
    damaged[160] ^= 1;
    std::fs::write(path.join("LMDB-VIEW"), &damaged).unwrap();
    assert_eq!(unsafe { transaction::mdb_txn_commit(txn) }, -30796);
    assert_eq!(std::fs::read(path.join("CURRENT")).unwrap(), current);
    assert!(
        !unsafe { (*env.0).shared.as_ref().unwrap() }
            .poisoned
            .load(std::sync::atomic::Ordering::Acquire),
        "known preselection failure incorrectly poisoned the environment"
    );
    let mut denied = ptr::null_mut();
    assert_eq!(
        unsafe { transaction::mdb_txn_begin(env.0, ptr::null_mut(), RDONLY, &mut denied) },
        -30796
    );
    assert!(denied.is_null());
    assert_eq!(
        std::fs::read(path.join("LMDB-VIEW")).unwrap(),
        damaged,
        "live SH automatically repaired damage"
    );
    if pending {
        assert!(matches!(Environment::open(path), Err(Error::Busy)));
    } else {
        let native = Environment::open(path).unwrap();
        assert_eq!(native.begin_read().get(MAIN, b"denied").unwrap(), None);
    }
    drop(env);
    let fresh = Env::open(path, 0); // Genuine fresh ENV EX may reset the record.
    assert_eq!(fresh.read(b"denied"), (2, None));
    let writer = fresh.begin(0);
    put(writer, 1, b"progress", b"selected");
    assert_eq!(unsafe { transaction::mdb_txn_commit(writer) }, 0);
    assert_eq!(fresh.read(b"progress"), (3, Some(b"selected".to_vec())));
    println!("PREVIOUS_PREPUBLICATION_DENIAL_PASS pending={pending}");
}

#[test]
fn record_damage_before_selection_aborts_preparation_without_poison_or_publication() {
    use std::os::unix::process::CommandExt;
    let mut count = 0;
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        for pending in [false, true] {
            for prepared in [false, true] {
                let directory = Directory::new(mode);
                {
                    let env = Env::open(&directory.0, 0);
                    seed(&env);
                }
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "previous_tests::previous_worker",
                        "--nocapture",
                        "--test-threads=1",
                    ])
                    .env("ULMDB_C_PREVIOUS_WORKER", "prepublication-denial")
                    .env("ULMDB_C_PREVIOUS_PATH", &directory.0)
                    .env("ULMDB_C_VIEW_PENDING", if pending { "yes" } else { "no" })
                    .env("ULMDB_C_VIEW_PREPARED", if prepared { "yes" } else { "no" })
                    .env_remove("ULMDB_FAIL_AT")
                    .env_remove("ULMDB_CRASH_AT")
                    .process_group(0)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let deadline = std::time::Instant::now() + Duration::from_secs(8);
                while child.try_wait().unwrap().is_none() {
                    if std::time::Instant::now() >= deadline {
                        unsafe {
                            libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
                        }
                        let result = child.wait_with_output().unwrap();
                        panic!(
                            "prepublication worker deadline: {}",
                            String::from_utf8_lossy(&result.stderr)
                        );
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                let result = child.wait_with_output().unwrap();
                assert!(
                    result.status.success(),
                    "{mode:?}/pending{pending}/prepared{prepared}: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
                assert_eq!(
                    String::from_utf8(result.stdout)
                        .unwrap()
                        .matches("PREVIOUS_PREPUBLICATION_DENIAL_PASS ")
                        .count(),
                    1
                );
                count += 1;
            }
        }
    }
    assert_eq!(count, 12);
}
