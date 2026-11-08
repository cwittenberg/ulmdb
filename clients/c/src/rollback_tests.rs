//! Adapter-only misuse, mixed-API and fault controls. These are deliberately
//! distinct from the unchanged LMDB rollback oracle and its caller contract.
use super::*;
use std::{
    ffi::CString,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use ulmdb::PublicationMode;
struct Fixture {
    env: *mut MDB_env,
    path: PathBuf,
}
impl Fixture {
    fn new(mode: PublicationMode) -> Self {
        Self::new_flags(mode, 0)
    }
    fn new_flags(mode: PublicationMode, flags: u32) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-c-rollback-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        drop(Environment::create_with_publication(&path, mode).unwrap());
        Self::open(path, flags)
    }
    fn open(path: PathBuf, flags: u32) -> Self {
        let mut env = ptr::null_mut();
        let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut env), 0);
            assert_eq!(environment::mdb_env_set_maxdbs(env, 16), 0);
            assert_eq!(
                environment::mdb_env_open(env, name.as_ptr(), flags, 0o600),
                0
            );
        }
        Self { env, path }
    }
    fn begin(&self, parent: *mut MDB_txn, flags: u32) -> *mut MDB_txn {
        let mut t = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(self.env, parent, flags, &mut t) },
            0
        );
        t
    }
    fn commit(&self, slot: Option<u32>, name: &CStr, value: &[u8]) -> (u32, usize) {
        let t = self.begin(ptr::null_mut(), 0);
        let slot = slot.unwrap_or_else(|| open(t, name));
        put(t, slot, value);
        let id = unsafe { transaction::mdb_txn_id(t) };
        assert_eq!(unsafe { transaction::mdb_txn_commit(t) }, 0);
        (slot, id)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { environment::mdb_env_close(self.env) };
        if std::env::var_os("ULMDB_C_ROLLBACK_KEEP").is_none() {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
fn open(t: *mut MDB_txn, name: &CStr) -> u32 {
    let mut d = 0;
    assert_eq!(
        unsafe { operations::mdb_dbi_open(t, name.as_ptr(), CREATE, &mut d) },
        0
    );
    d
}
fn put(t: *mut MDB_txn, d: u32, v: &[u8]) {
    let mut k = MDB_val {
        mv_size: 1,
        mv_data: b"k".as_ptr().cast_mut().cast(),
    };
    let mut v = MDB_val {
        mv_size: v.len(),
        mv_data: v.as_ptr().cast_mut().cast(),
    };
    assert_eq!(unsafe { operations::mdb_put(t, d, &mut k, &mut v, 0) }, 0);
}
fn get(t: *mut MDB_txn, d: u32) -> Result<Vec<u8>, i32> {
    let mut k = MDB_val {
        mv_size: 1,
        mv_data: b"k".as_ptr().cast_mut().cast(),
    };
    let mut v = MDB_val {
        mv_size: 0,
        mv_data: ptr::null_mut(),
    };
    let rc = unsafe { operations::mdb_get(t, d, &mut k, &mut v) };
    if rc == 0 {
        Ok(unsafe { std::slice::from_raw_parts(v.mv_data.cast::<u8>(), v.mv_size) }.to_vec())
    } else {
        Err(rc)
    }
}
fn run(role: &str) {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let f = Fixture::new(mode);
        if role == "initial" {
            let mut initial = MDB_envinfo {
                me_mapaddr: ptr::null_mut(),
                me_mapsize: 0,
                me_last_pgno: 0,
                me_last_txnid: 999,
                me_maxreaders: 0,
                me_numreaders: 0,
            };
            let empty = |info: &mut MDB_envinfo| {
                assert_eq!(unsafe { environment::mdb_env_info(f.env, info) }, 0);
                assert_eq!(info.me_last_txnid, 0);
            };
            empty(&mut initial);
            let t = f.begin(ptr::null_mut(), 0);
            assert_eq!(unsafe { transaction::mdb_txn_commit(t) }, 0);
            empty(&mut initial);
            let t = f.begin(ptr::null_mut(), 0);
            unsafe { transaction::mdb_txn_abort(t) };
            empty(&mut initial);
            let t = f.begin(ptr::null_mut(), 0);
            assert_eq!(unsafe { transaction::mdb_txn_prepare(t) }, 0);
            empty(&mut initial);
            let t = f.begin(ptr::null_mut(), 0);
            let aborted = open(t, c"not-published");
            put(t, aborted, b"never");
            assert_eq!(unsafe { transaction::mdb_txn_prepare(t) }, 0);
            unsafe { transaction::mdb_txn_abort(t) };
            empty(&mut initial);
        }
        let (a, id) = f.commit(None, c"a", b"old");
        assert_eq!(id, 1);
        match role {
            "active" => {
                let t = f.begin(ptr::null_mut(), 0);
                assert_eq!(
                    unsafe { rollback::mdb_env_rollback(f.env, id) },
                    libc::EBUSY
                );
                let child = f.begin(t, 0);
                assert_eq!(
                    unsafe { rollback::mdb_env_rollback(f.env, id) },
                    libc::EBUSY
                );
                unsafe {
                    transaction::mdb_txn_abort(child);
                    transaction::mdb_txn_abort(t)
                };
                let t = f.begin(ptr::null_mut(), 0);
                put(t, a, b"new");
                assert_eq!(unsafe { transaction::mdb_txn_prepare(t) }, 0);
                assert_eq!(unsafe { rollback::mdb_env_rollback(f.env, 2) }, libc::EBUSY);
                assert_eq!(unsafe { transaction::mdb_txn_commit(t) }, 0);
                assert_eq!(unsafe { rollback::mdb_env_rollback(f.env, 2) }, 0);
                assert_eq!(
                    unsafe { rollback::mdb_env_rollback(f.env, 1) },
                    CANT_ROLLBACK
                );
            }
            "blocked-read-setup" | "blocked-renew-setup" => {
                #[cfg(unix)]
                use std::os::fd::AsRawFd;
                let renew = role == "blocked-renew-setup";
                let (ready_tx, ready_rx) = std::sync::mpsc::channel();
                let mut starts = Vec::new();
                let mut workers = Vec::new();
                for _ in 0..2 {
                    let (start_tx, start_rx) = std::sync::mpsc::channel();
                    starts.push(start_tx);
                    let ready = ready_tx.clone();
                    let env = f.env as usize;
                    workers.push(std::thread::spawn(move || {
                        let mut reader = ptr::null_mut();
                        if renew {
                            assert_eq!(
                                unsafe {
                                    transaction::mdb_txn_begin(
                                        env as *mut MDB_env,
                                        ptr::null_mut(),
                                        RDONLY,
                                        &mut reader,
                                    )
                                },
                                0
                            );
                            unsafe { transaction::mdb_txn_reset(reader) };
                        }
                        ready.send(()).unwrap();
                        start_rx.recv().unwrap();
                        if renew {
                            assert_eq!(unsafe { transaction::mdb_txn_renew(reader) }, 0);
                        } else {
                            assert_eq!(
                                unsafe {
                                    transaction::mdb_txn_begin(
                                        env as *mut MDB_env,
                                        ptr::null_mut(),
                                        RDONLY,
                                        &mut reader,
                                    )
                                },
                                0
                            );
                        }
                        assert_eq!(get(reader, a).unwrap(), b"old");
                        unsafe { transaction::mdb_txn_abort(reader) };
                    }));
                }
                for _ in 0..2 {
                    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                }
                // Block the actual kernel registry gate used by snapshot pin
                // admission. Both public setups must enter before either IO
                // operation can finish, without holding the lifecycle mutex.
                let registry = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(f.path.join(".ulmdb-coordination/registry"))
                    .unwrap();
                #[cfg(unix)]
                assert_eq!(
                    unsafe { libc::flock(registry.as_raw_fd(), libc::LOCK_EX) },
                    0
                );
                // Same physical byte as Windows Coordination::registry's
                // LockFileEx gate; real competing handles block admission.
                #[cfg(windows)]
                let registry_guard = ulmdb_platform::RangeLock::acquire(
                    registry,
                    ulmdb_platform::LockMode::Exclusive,
                    1 << 62,
                    1,
                    true,
                )
                .unwrap();
                for start in starts {
                    start.send(()).unwrap();
                }
                let end = Instant::now() + Duration::from_secs(2);
                let mut simultaneous = false;
                while Instant::now() < end {
                    if let Ok(count) = unsafe { (*f.env).shared.as_ref().unwrap() }
                        .lifecycle
                        .try_lock()
                        && *count == 2
                    {
                        simultaneous = true;
                        break;
                    }
                    std::thread::yield_now();
                }
                let started = Instant::now();
                let blocked = unsafe { rollback::mdb_env_rollback(f.env, 1) };
                let promptly = started.elapsed() < Duration::from_millis(500);
                // Unblock and join before assertions to preserve env lifetime
                // even when this schedule exposes a deficient implementation.
                #[cfg(unix)]
                assert_eq!(
                    unsafe { libc::flock(registry.as_raw_fd(), libc::LOCK_UN) },
                    0
                );
                #[cfg(windows)]
                drop(registry_guard);
                for worker in workers {
                    worker.join().unwrap();
                }
                assert!(
                    simultaneous,
                    "both reader setups must be in flight together"
                );
                assert!(promptly, "rollback must not wait for blocked setup IO");
                assert_eq!(blocked, libc::EBUSY);
                assert_eq!(
                    *unsafe { (*f.env).shared.as_ref().unwrap() }
                        .lifecycle
                        .lock()
                        .unwrap(),
                    0
                );
                let (_, id) = f.commit(Some(a), c"a", b"new");
                assert_eq!(unsafe { rollback::mdb_env_rollback(f.env, id) }, 0);
            }
            "waiting-writer" => {
                let t = f.begin(ptr::null_mut(), 0);
                put(t, a, b"new");
                let env = f.env as usize;
                let waiter = std::thread::spawn(move || {
                    let mut next = ptr::null_mut();
                    assert_eq!(
                        unsafe {
                            transaction::mdb_txn_begin(
                                env as *mut MDB_env,
                                ptr::null_mut(),
                                0,
                                &mut next,
                            )
                        },
                        0
                    );
                    put(next, a, b"after");
                    assert_eq!(unsafe { transaction::mdb_txn_commit(next) }, 0);
                });
                let end = Instant::now() + Duration::from_secs(2);
                while *unsafe { (*f.env).shared.as_ref().unwrap() }
                    .lifecycle
                    .lock()
                    .unwrap()
                    == 0
                {
                    assert!(Instant::now() < end);
                    std::thread::yield_now();
                }
                let reader = f.begin(ptr::null_mut(), RDONLY);
                assert_eq!(get(reader, a).unwrap(), b"old");
                unsafe { transaction::mdb_txn_abort(reader) };
                assert_eq!(unsafe { rollback::mdb_env_rollback(f.env, 2) }, libc::EBUSY);
                assert_eq!(unsafe { transaction::mdb_txn_commit(t) }, 0);
                waiter.join().unwrap();
            }
            "process-peer" => {
                let ready = f.path.join("peer.ready");
                let release = f.path.join("peer.release");
                let mut cmd = Command::new(std::env::current_exe().unwrap());
                cmd.args(["--exact", "rollback_tests::worker", "--nocapture"])
                    .env("ULMDB_C_ROLLBACK_ROLE", "peer-hold")
                    .env("ULMDB_C_ROLLBACK_PATH", &f.path)
                    .env("ULMDB_C_ROLLBACK_KEEP", "1")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                let mut child = cmd.spawn().unwrap();
                let end = Instant::now() + Duration::from_secs(2);
                while !ready.exists() {
                    if Instant::now() > end {
                        let _ = child.kill();
                        panic!("peer did not open")
                    };
                    std::thread::sleep(Duration::from_millis(5));
                }
                let (_, id) = f.commit(Some(a), c"a", b"new");
                assert_eq!(
                    unsafe { rollback::mdb_env_rollback(f.env, id) },
                    libc::EBUSY
                );
                std::fs::write(release, b"release").unwrap();
                let out = child.wait_with_output().unwrap();
                assert!(
                    out.status.success(),
                    "{}",
                    String::from_utf8_lossy(&out.stderr)
                );
                assert_eq!(unsafe { rollback::mdb_env_rollback(f.env, id) }, 0);
            }
            "initial" => {
                let mut info = MDB_envinfo {
                    me_mapaddr: ptr::null_mut(),
                    me_mapsize: 0,
                    me_last_pgno: 0,
                    me_last_txnid: 0,
                    me_maxreaders: 0,
                    me_numreaders: 0,
                };
                assert_eq!(unsafe { environment::mdb_env_info(f.env, &mut info) }, 0);
                assert_eq!(info.me_last_txnid, 1);
                // First C business commit above was exactly1 for every physical format.
                let t = f.begin(ptr::null_mut(), 0);
                unsafe { transaction::mdb_txn_abort(t) };
                let t = f.begin(ptr::null_mut(), 0);
                assert_eq!(unsafe { transaction::mdb_txn_prepare(t) }, 0);
                assert_eq!(unsafe { environment::mdb_env_info(f.env, &mut info) }, 0);
                assert_eq!(info.me_last_txnid, 1);
            }
            "peer" => {
                let peer = Fixture::open(f.path.clone(), RDONLY);
                let (_, id) = f.commit(Some(a), c"a", b"new");
                assert_eq!(
                    unsafe { rollback::mdb_env_rollback(f.env, id) },
                    libc::EBUSY
                );
                unsafe { environment::mdb_env_close(peer.env) };
                std::mem::forget(peer);
                assert_eq!(unsafe { rollback::mdb_env_rollback(f.env, id) }, 0);
                let native = Environment::open(&f.path).unwrap();
                assert_eq!(unsafe { rollback::mdb_env_rollback(f.env, 1) }, libc::EBUSY);
                drop(native);
            }
            "mixed" => {
                let t = f.begin(ptr::null_mut(), 0);
                let stale = open(t, c"undone");
                put(t, stale, b"undone");
                let stale_id = unsafe { (*t).peek_handle(stale).unwrap().native_id().value() };
                assert_eq!(unsafe { transaction::mdb_txn_commit(t) }, 0);
                assert_eq!(unsafe { rollback::mdb_env_rollback(f.env, 2) }, 0);
                // A wholly separate native wrapper reopens R; it must see the persisted
                // allocator floor, rather than reusing the undone C alias's identity.
                let native = ulmdb::CoordinatedEnvironment::open(&f.path).unwrap();
                let fresh_id = native
                    .update(|w| {
                        w.create_database(b"native", DatabaseKind::Kv)?;
                        w.put(b"native", b"k", b"native")?;
                        Ok(w.open_database(b"native")?.id())
                    })
                    .unwrap();
                assert!(fresh_id > stale_id);
                drop(native);
                let t = f.begin(ptr::null_mut(), 0);
                let fresh = open(t, c"fresh");
                let id = unsafe { (*t).peek_handle(fresh).unwrap().native_id().value() };
                assert!(id > fresh_id);
                put(t, fresh, b"fresh");
                assert_eq!(get(t, stale), Err(BAD_DBI));
                assert_eq!(unsafe { transaction::mdb_txn_commit(t) }, 0);
                let t = f.begin(ptr::null_mut(), 0);
                let aborted = open(t, c"aborted");
                let aborted_id = unsafe { (*t).peek_handle(aborted).unwrap().id() };
                unsafe { transaction::mdb_txn_abort(t) };
                let t = f.begin(ptr::null_mut(), 0);
                let replacement = open(t, c"replacement");
                assert!(unsafe { (*t).peek_handle(replacement).unwrap().id() } > aborted_id);
                unsafe { transaction::mdb_txn_abort(t) };
            }
            "writemap-drop" => {
                // The pinned reference asserts before rollback for these legal sequences.
                // The adapter must remain defined; no crash parity is required.
                for update in [false, true] {
                    let peer = Fixture::new_flags(mode, 0x80000);
                    let (d, _) = peer.commit(None, c"named", b"old");
                    let t = peer.begin(ptr::null_mut(), 0);
                    if update {
                        put(t, d, b"new")
                    };
                    assert_eq!(unsafe { operations::mdb_drop(t, d, 1) }, 0);
                    assert_eq!(unsafe { transaction::mdb_txn_commit(t) }, 0);
                    assert_eq!(unsafe { rollback::mdb_env_rollback(peer.env, 2) }, 0);
                    let t = peer.begin(ptr::null_mut(), RDONLY);
                    let reopened = open(t, c"named");
                    assert_eq!(get(t, reopened).unwrap(), b"old");
                    unsafe { transaction::mdb_txn_abort(t) };
                }
            }
            role if role.starts_with("fault:") => {
                let (_, id) = f.commit(Some(a), c"a", b"new");
                let point = role.strip_prefix("fault:").unwrap();
                let point = if point == "metadata" {
                    if mode == PublicationMode::Legacy {
                        "descriptor.before_write"
                    } else {
                        "cohort.before_write"
                    }
                } else {
                    point
                };
                // Worker process runs this one test; no concurrent environment mutation.
                unsafe { std::env::set_var("ULMDB_FAIL_AT", point) };
                let rc = unsafe { rollback::mdb_env_rollback(f.env, id) };
                unsafe { std::env::remove_var("ULMDB_FAIL_AT") };
                let known = point == "rollback.before_publish"
                    || point == "descriptor.before_write"
                    || point == "cohort.before_write";
                assert_eq!(rc, if known { libc::EIO } else { -30795 }, "{point}");
                let mut t = ptr::null_mut();
                let rc = unsafe { transaction::mdb_txn_begin(f.env, ptr::null_mut(), 0, &mut t) };
                if known {
                    assert_eq!(rc, 0);
                    assert_eq!(get(t, a).unwrap(), b"new");
                    unsafe { transaction::mdb_txn_abort(t) }
                } else {
                    assert_eq!(rc, -30795);
                    assert!(t.is_null())
                };
                let read = ReadTxn::open_snapshot(&f.path).unwrap();
                assert!([1, 2].contains(&read.sequence()));
                let name = unsafe {
                    (*f.env).shared.as_ref().unwrap().handles.lock().unwrap()[a as usize]
                        .as_ref()
                        .unwrap()
                        .name()
                        .to_vec()
                };
                let value = read.get(&name, b"k").unwrap().unwrap();
                assert_eq!(value, if read.sequence() == 1 { b"old" } else { b"new" });
            }
            _ => panic!("unknown role {role}"),
        }
        println!("CASE_PASS role={role} backend={mode:?}");
    }
}
#[test]
fn worker() {
    if let Ok(role) = std::env::var("ULMDB_C_ROLLBACK_ROLE") {
        if role == "peer-hold" {
            let path = PathBuf::from(std::env::var_os("ULMDB_C_ROLLBACK_PATH").unwrap());
            let _peer = Fixture::open(path.clone(), RDONLY);
            std::fs::write(path.join("peer.ready"), b"ready").unwrap();
            let end = Instant::now() + Duration::from_secs(4);
            while !path.join("peer.release").exists() {
                assert!(Instant::now() < end, "peer release deadline");
                std::thread::sleep(Duration::from_millis(5));
            }
        } else {
            run(&role)
        }
    }
}
fn deadline(mut command: Command) -> std::process::Output {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let end = Instant::now() + Duration::from_secs(8);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() > end {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "8s rollback worker deadline\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn rollback_ownership_and_mixed_allocator_controls() {
    for role in [
        "active",
        "blocked-read-setup",
        "blocked-renew-setup",
        "waiting-writer",
        "peer",
        "process-peer",
        "mixed",
        "writemap-drop",
        "initial",
    ] {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "rollback_tests::worker", "--nocapture"])
            .env("ULMDB_C_ROLLBACK_ROLE", role);
        let out = deadline(cmd);
        assert!(
            out.status.success(),
            "role={role}\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        print!("{}", String::from_utf8_lossy(&out.stdout));
    }
}
#[test]
#[ignore = "requires cargo test -p ulmdb-c --features ulmdb/fault-injection -- --ignored"]
fn rollback_c_fault_controls() {
    for point in [
        "metadata",
        "rollback.before_publish",
        "pointer.CURRENT.before_write",
        "pointer.CURRENT.before_rename",
        "pointer.CURRENT.after_rename",
        "rollback.after_publish",
        "rollback.after_activate",
    ] {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "rollback_tests::worker", "--nocapture"])
            .env("ULMDB_C_ROLLBACK_ROLE", format!("fault:{point}"));
        let out = deadline(cmd);
        assert!(
            out.status.success(),
            "point={point}\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        print!("{}", String::from_utf8_lossy(&out.stdout));
    }
}
