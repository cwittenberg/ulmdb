//! Actual C/native ownership and snapshot journeys, optionally across a dlopen boundary.
#![cfg(target_os = "linux")]

use super::{MDB_env, MDB_txn, MDB_val};
use std::{
    cell::Cell,
    collections::BTreeSet,
    ffi::{CStr, CString, c_char, c_int, c_void},
    fs::{self, File},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    ptr, thread,
    time::{Duration, Instant},
};
use ulmdb::{CoordinatedEnvironment, DatabaseKind, Environment, Error, PublicationMode};

const DEADLINE: Duration = Duration::from_secs(8);
const READONLY: u32 = 0x20000;
const NOTLS: u32 = 0x200000;

struct Library(*mut c_void);
impl Library {
    unsafe fn symbol<T: Copy>(&self, name: &CStr) -> T {
        // Function-pointer signatures below are the public C ABI. No Rust
        // handle fields or private types are inspected across this boundary.
        let symbol = unsafe { libc::dlsym(self.0, name.as_ptr()) };
        assert!(!symbol.is_null(), "missing C symbol {name:?}");
        assert_eq!(std::mem::size_of::<T>(), std::mem::size_of_val(&symbol));
        unsafe { std::mem::transmute_copy(&symbol) }
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        // Api is owned by CEnvironment, whose Drop closes its handle first.
        // CTransaction borrows CEnvironment and therefore must already be gone.
        unsafe {
            libc::dlclose(self.0);
        }
    }
}

struct Api {
    _library: Option<Library>,
    env_create: unsafe extern "C" fn(*mut *mut MDB_env) -> c_int,
    env_set_maxdbs: unsafe extern "C" fn(*mut MDB_env, u32) -> c_int,
    env_open: unsafe extern "C" fn(*mut MDB_env, *const c_char, u32, libc::mode_t) -> c_int,
    env_close: unsafe extern "C" fn(*mut MDB_env),
    txn_begin: unsafe extern "C" fn(*mut MDB_env, *mut MDB_txn, u32, *mut *mut MDB_txn) -> c_int,
    txn_commit: unsafe extern "C" fn(*mut MDB_txn) -> c_int,
    txn_abort: unsafe extern "C" fn(*mut MDB_txn),
    dbi_open: unsafe extern "C" fn(*mut MDB_txn, *const c_char, u32, *mut u32) -> c_int,
    get: unsafe extern "C" fn(*mut MDB_txn, u32, *mut MDB_val, *mut MDB_val) -> c_int,
    put: unsafe extern "C" fn(*mut MDB_txn, u32, *mut MDB_val, *mut MDB_val, u32) -> c_int,
}
impl Api {
    fn load() -> Self {
        if let Some(path) = std::env::var_os("ULMDB_COORDINATION_DYLIB") {
            let path = PathBuf::from(path);
            assert!(
                path.is_absolute() && path.is_file(),
                "candidate must be an absolute shared-library path"
            );
            let name = CString::new(path.as_os_str().as_bytes()).unwrap();
            // Match normal application library selection: Rust TLS callbacks
            // may outlive the last C environment on this worker thread. Keep
            // the image mapped until process exit; this fixture qualifies no
            // dynamic Rust-library unloading contract.
            let handle = unsafe {
                libc::dlopen(
                    name.as_ptr(),
                    libc::RTLD_NOW | libc::RTLD_LOCAL | libc::RTLD_NODELETE,
                )
            };
            if handle.is_null() {
                let message = unsafe { libc::dlerror() };
                let message = if message.is_null() {
                    "unknown loader error".into()
                } else {
                    unsafe { CStr::from_ptr(message) }.to_string_lossy()
                };
                panic!("candidate dlopen failed: {message}");
            }
            let library = Library(handle);
            println!("mixed-provider=cdylib:{}", path.display());
            let api = unsafe {
                Self {
                    env_create: library.symbol(c"mdb_env_create"),
                    env_set_maxdbs: library.symbol(c"mdb_env_set_maxdbs"),
                    env_open: library.symbol(c"mdb_env_open"),
                    env_close: library.symbol(c"mdb_env_close"),
                    txn_begin: library.symbol(c"mdb_txn_begin"),
                    txn_commit: library.symbol(c"mdb_txn_commit"),
                    txn_abort: library.symbol(c"mdb_txn_abort"),
                    dbi_open: library.symbol(c"mdb_dbi_open"),
                    get: library.symbol(c"mdb_get"),
                    put: library.symbol(c"mdb_put"),
                    _library: Some(library),
                }
            };
            assert_ne!(
                api.txn_begin as usize,
                super::transaction::mdb_txn_begin as *const () as usize,
                "candidate resolved to the directly linked C implementation"
            );
            let mut provenance: libc::Dl_info = unsafe { std::mem::zeroed() };
            assert_ne!(
                unsafe { libc::dladdr((api.txn_begin as *const ()).cast(), &mut provenance) },
                0
            );
            assert!(!provenance.dli_fname.is_null());
            let loaded = unsafe { CStr::from_ptr(provenance.dli_fname) };
            let loaded = Path::new(std::ffi::OsStr::from_bytes(loaded.to_bytes()));
            assert_eq!(
                loaded.canonicalize().unwrap(),
                path.canonicalize().unwrap(),
                "C function pointer came from a different shared library"
            );
            api
        } else {
            println!("mixed-provider=direct");
            Self {
                _library: None,
                env_create: super::environment::mdb_env_create,
                env_set_maxdbs: super::environment::mdb_env_set_maxdbs,
                env_open: super::environment::mdb_env_open,
                env_close: super::environment::mdb_env_close,
                txn_begin: super::transaction::mdb_txn_begin,
                txn_commit: super::transaction::mdb_txn_commit,
                txn_abort: super::transaction::mdb_txn_abort,
                dbi_open: super::operations::mdb_dbi_open,
                get: super::operations::mdb_get,
                put: super::operations::mdb_put,
            }
        }
    }
}
fn ok(result: c_int, operation: &str) {
    assert_eq!(result, 0, "C operation {operation} failed");
}

struct CEnvironment {
    api: Api,
    pointer: *mut MDB_env,
}
impl CEnvironment {
    fn open(path: &Path) -> Self {
        let mut env = Self {
            api: Api::load(),
            pointer: ptr::null_mut(),
        };
        let name = CString::new(path.as_os_str().as_bytes()).unwrap();
        unsafe {
            ok((env.api.env_create)(&mut env.pointer), "env_create");
            ok((env.api.env_set_maxdbs)(env.pointer, 8), "env_set_maxdbs");
            ok(
                (env.api.env_open)(env.pointer, name.as_ptr(), NOTLS, 0o600),
                "env_open",
            );
        }
        env
    }
    fn begin(&self, readonly: bool) -> CTransaction<'_> {
        let mut pointer = ptr::null_mut();
        ok(
            unsafe {
                (self.api.txn_begin)(
                    self.pointer,
                    ptr::null_mut(),
                    if readonly { READONLY } else { 0 },
                    &mut pointer,
                )
            },
            "txn_begin",
        );
        CTransaction { env: self, pointer }
    }
}
impl Drop for CEnvironment {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            unsafe {
                (self.api.env_close)(self.pointer);
            }
        }
    }
}
struct CTransaction<'env> {
    env: &'env CEnvironment,
    pointer: *mut MDB_txn,
}
impl CTransaction<'_> {
    fn database(&self) -> u32 {
        let mut dbi = u32::MAX;
        ok(
            unsafe { (self.env.api.dbi_open)(self.pointer, c"records".as_ptr(), 0, &mut dbi) },
            "dbi_open",
        );
        dbi
    }
    fn get(&self, dbi: u32) -> &[u8] {
        let mut key = MDB_val {
            mv_size: 3,
            mv_data: b"key".as_ptr().cast_mut().cast(),
        };
        let mut value = MDB_val {
            mv_size: 0,
            mv_data: ptr::null_mut(),
        };
        ok(
            unsafe { (self.env.api.get)(self.pointer, dbi, &mut key, &mut value) },
            "get",
        );
        if value.mv_size == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(value.mv_data.cast(), value.mv_size) }
        }
    }
    fn put(&mut self, dbi: u32, bytes: &[u8]) {
        let mut key = MDB_val {
            mv_size: 3,
            mv_data: b"key".as_ptr().cast_mut().cast(),
        };
        let mut value = MDB_val {
            mv_size: bytes.len(),
            mv_data: bytes.as_ptr().cast_mut().cast(),
        };
        ok(
            unsafe { (self.env.api.put)(self.pointer, dbi, &mut key, &mut value, 0) },
            "put",
        );
    }
    fn commit(mut self) {
        let pointer = std::mem::replace(&mut self.pointer, ptr::null_mut());
        ok(unsafe { (self.env.api.txn_commit)(pointer) }, "txn_commit");
    }
}
impl Drop for CTransaction<'_> {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            unsafe {
                (self.env.api.txn_abort)(self.pointer);
            }
        }
    }
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-mixed-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        if thread::panicking() {
            eprintln!("mixed failure fixtures retained at {}", self.0.display());
            return;
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct ChildOwner(Child);
impl Drop for ChildOwner {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn run_worker(path: &Path, role: &str) {
    let native = CoordinatedEnvironment::open(path).unwrap();
    let c = CEnvironment::open(path);
    match role {
        "native-to-c" => {
            native
                .update(|write| {
                    write.put(b"records", b"key", b"native-private")?;
                    let mut rejected = ptr::null_mut();
                    let result =
                        unsafe { (c.api.txn_begin)(c.pointer, ptr::null_mut(), 0, &mut rejected) };
                    if !rejected.is_null() {
                        unsafe {
                            (c.api.txn_abort)(rejected);
                        }
                    }
                    assert_eq!(
                        result,
                        libc::EBUSY,
                        "recursive C writer must fail before blocking"
                    );
                    assert!(rejected.is_null());
                    Ok(())
                })
                .unwrap();
            let mut write = c.begin(false);
            let dbi = write.database();
            assert_eq!(write.get(dbi), b"native-private");
            write.put(dbi, b"C-after-native");
            write.commit();
            assert_eq!(
                native
                    .begin_read()
                    .unwrap()
                    .get(b"records", b"key")
                    .unwrap(),
                Some(b"C-after-native".as_slice())
            );
        }
        "c-to-native" => {
            let old = native.begin_read().unwrap();
            let old_borrow = old.get(b"records", b"key").unwrap().unwrap();
            let mut write = c.begin(false);
            let dbi = write.database();
            write.put(dbi, b"C-private");
            let called = Cell::new(false);
            assert!(matches!(
                native.update(|_| {
                    called.set(true);
                    Ok(())
                }),
                Err(Error::Busy)
            ));
            assert!(
                !called.get(),
                "rejected recursive writer entered its callback"
            );
            assert_eq!(
                native
                    .begin_read()
                    .unwrap()
                    .get(b"records", b"key")
                    .unwrap(),
                Some(b"old".as_slice())
            );
            write.commit();
            assert_eq!(
                native
                    .begin_read()
                    .unwrap()
                    .get(b"records", b"key")
                    .unwrap(),
                Some(b"C-private".as_slice())
            );
            assert_eq!(old_borrow, b"old");
            native
                .update(|write| write.put(b"records", b"key", b"native-after-C"))
                .unwrap();
            let read = c.begin(true);
            assert_eq!(read.get(read.database()), b"native-after-C");
        }
        "snapshot" => {
            let selected = ulmdb_storage::read_current(path).unwrap();
            let files: BTreeSet<_> = selected
                .segments
                .iter()
                .map(|entry| ulmdb_storage::segment_path(path, entry))
                .collect();
            assert!(!files.is_empty());
            let read = c.begin(true);
            let dbi = read.database();
            let borrowed = read.get(dbi);
            assert_eq!(borrowed, b"old");
            for _ in 0..3 {
                native
                    .update(|write| write.put(b"records", b"key", b"native-new"))
                    .unwrap();
            }
            native.compact().unwrap();
            native.prune_inventory().unwrap();
            native.compact().unwrap();
            native.reclaim().unwrap();
            assert_eq!(borrowed, b"old");
            assert_eq!(read.get(dbi), b"old");
            assert!(files.iter().all(|file| file.is_file()));
            assert_eq!(
                native
                    .begin_read()
                    .unwrap()
                    .get(b"records", b"key")
                    .unwrap(),
                Some(b"native-new".as_slice())
            );
            drop(read);
            assert!(native.reclaim().unwrap().removed_bytes > 0);
            assert!(files.iter().all(|file| !file.exists()));
            let fresh = c.begin(true);
            assert_eq!(fresh.get(fresh.database()), b"native-new");
        }
        _ => panic!("unknown mixed C/native role {role}"),
    }
}

#[test]
fn mixed_native_c_and_snapshot_lifetimes() {
    if let Ok(role) = std::env::var("ULMDB_MIXED_COORDINATION_ROLE") {
        let path = PathBuf::from(std::env::var_os("ULMDB_MIXED_COORDINATION_PATH").unwrap());
        run_worker(&path, &role);
        return;
    }
    println!(
        "mixed-parent-provider={}",
        std::env::var_os("ULMDB_COORDINATION_DYLIB").map_or_else(
            || "direct".to_owned(),
            |path| PathBuf::from(path).display().to_string()
        )
    );
    let directory = Directory::new();
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        for role in ["native-to-c", "c-to-native", "snapshot"] {
            let path = directory.0.join(format!("{mode:?}-{role}"));
            let initial = Environment::create_with_publication(&path, mode).unwrap();
            let mut write = initial.begin_write().unwrap();
            write.create_database(b"records", DatabaseKind::Kv).unwrap();
            write.put(b"records", b"key", b"old").unwrap();
            write.commit().unwrap();
            drop(initial);
            let log = directory.0.join(format!("{mode:?}-{role}.log"));
            let output = File::create(&log).unwrap();
            let mut child = ChildOwner(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "coordination_tests::mixed_native_c_and_snapshot_lifetimes",
                        "--nocapture",
                    ])
                    .env("ULMDB_MIXED_COORDINATION_ROLE", role)
                    .env("ULMDB_MIXED_COORDINATION_PATH", &path)
                    .stdout(Stdio::from(output.try_clone().unwrap()))
                    .stderr(Stdio::from(output))
                    .spawn()
                    .unwrap(),
            );
            let start = Instant::now();
            loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    assert!(
                        status.success(),
                        "mixed {mode:?}/{role} failed ({status}):\n{}",
                        fs::read_to_string(&log).unwrap()
                    );
                    break;
                }
                assert!(
                    start.elapsed() < DEADLINE,
                    "mixed {mode:?}/{role} deadlocked:\n{}",
                    fs::read_to_string(&log).unwrap()
                );
                thread::sleep(Duration::from_millis(10));
            }
        }
        let backend = match mode {
            PublicationMode::Legacy => "file-legacy",
            PublicationMode::Cohort => "file-cohort",
            PublicationMode::Indexed => "file-indexed",
            _ => unreachable!(),
        };
        println!(
            "CASE_PASS test=coordination_tests::mixed_native_c_and_snapshot_lifetimes backend={backend}"
        );
    }
}

#[test]
fn admission_cache_raw_fork_drop_while_parent_reads_is_bounded() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    struct StopReads<'a>(&'a AtomicBool);
    impl Drop for StopReads<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    const PAYLOAD: usize = 8 * 1024 * 1024;
    const FORKS: usize = 16;
    let directory = Directory::new();
    let path = directory.0.join("native-cache-fork-drop");
    let environment = Environment::create(&path).unwrap();
    let mut write = environment.begin_write().unwrap();
    write.create_database(b"records", DatabaseKind::Kv).unwrap();
    write.put(b"records", b"key", &vec![7; PAYLOAD]).unwrap();
    write.commit().unwrap();
    drop(environment);

    let owner = Arc::new(CoordinatedEnvironment::open(&path).unwrap());
    let first = owner.begin_read().unwrap();
    let second = owner.begin_read().unwrap();
    let pointer = first.get(b"records", b"key").unwrap().unwrap().as_ptr();
    assert_eq!(
        pointer,
        second.get(b"records", b"key").unwrap().unwrap().as_ptr()
    );
    // Retain a genuine reader while the child destroys its sole coordinator.
    // The old requirement that a certificate keep mappings after every reader
    // ends is obsolete: metadata-only reuse deliberately permits those unmaps.
    // mincore below checks only this still-live reader's mapping, without
    // dereferencing it. Ending the reader is covered by weak-owner tests.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    assert!(page > 0);
    let page = page as usize;
    let mapped_page = pointer as usize / page * page;
    drop(second);

    let stop = AtomicBool::new(false);
    let active = AtomicBool::new(false);
    let completed = AtomicUsize::new(0);
    thread::scope(|scope| {
        // Borrow the native object; an extra owner Arc would make child Drop
        // merely decrement an inherited refcount instead of destroying it.
        let native = owner.as_ref();
        let worker = scope.spawn(|| {
            while !stop.load(Ordering::Acquire) {
                active.store(true, Ordering::Release);
                let read = native.begin_read().unwrap();
                let value = read.get(b"records", b"key").unwrap().unwrap();
                assert_eq!(value.len(), PAYLOAD);
                assert_eq!((value[0], value[PAYLOAD - 1]), (7, 7));
                drop(read);
                active.store(false, Ordering::Release);
                completed.fetch_add(1, Ordering::Release);
            }
        });
        let stop_on_exit = StopReads(&stop);
        let deadline = Instant::now() + DEADLINE;
        for iteration in 0..FORKS {
            while completed.load(Ordering::Acquire) <= iteration || !active.load(Ordering::Acquire)
            {
                assert!(
                    Instant::now() < deadline,
                    "parent reader stopped making progress"
                );
                thread::yield_now();
            }
            assert_eq!(Arc::strong_count(&owner), 1);
            // SAFETY: the child continues only this thread. Its inherited
            // scoped borrowers have vanished; the parent's VM remains separate.
            let child = unsafe { libc::fork() };
            assert!(child >= 0);
            if child == 0 {
                // SAFETY: move the child's sole owner out of its inherited
                // stack copy. _exit prevents a second Drop or scoped-thread
                // cleanup. No Rust reference into the coordinator is used here.
                let inherited = unsafe { ptr::read(&owner) };
                let native = match Arc::try_unwrap(inherited) {
                    Ok(native) => native,
                    Err(_) => unsafe { libc::_exit(3) },
                };
                drop(native); // Exercise native Cache Drop directly, not MDB_env close.
                let mut residency = 0u8;
                // SAFETY: mincore accepts an address/range and an output byte;
                // it returns ENOMEM for an unmapped address rather than reading it.
                let mapped =
                    unsafe { libc::mincore(mapped_page as *mut c_void, page, &mut residency) } == 0;
                unsafe { libc::_exit(if mapped { 0 } else { 4 }) };
            }
            let mut status = 0;
            loop {
                let result = unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) };
                if result == child {
                    break;
                }
                if result < 0
                    && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
                {
                    unsafe {
                        libc::kill(child, libc::SIGKILL);
                        libc::waitpid(child, &mut status, 0);
                    }
                    panic!("could not reap native cache fork child");
                }
                if Instant::now() >= deadline {
                    unsafe {
                        libc::kill(child, libc::SIGKILL);
                        libc::waitpid(child, &mut status, 0);
                    }
                    panic!("native coordinator Drop blocked after raw fork");
                }
                thread::sleep(Duration::from_millis(2));
            }
            assert!(
                libc::WIFEXITED(status),
                "fork child failed with status {status}"
            );
            assert_eq!(libc::WEXITSTATUS(status), 0);
        }
        drop(stop_on_exit);
        worker.join().unwrap();
    });
    assert!(completed.load(Ordering::Acquire) >= FORKS);
    let held = first.get(b"records", b"key").unwrap().unwrap();
    assert_eq!(held.len(), PAYLOAD);
    assert_eq!((held[0], held[PAYLOAD - 1]), (7, 7));
    drop(first);
    let final_read = owner.begin_read().unwrap();
    assert_eq!(
        final_read.get(b"records", b"key").unwrap().unwrap().len(),
        PAYLOAD
    );
}
