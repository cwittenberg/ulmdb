//! Public adapter calls at real preparation/publication I/O boundaries.
//! Each fault runs in a separate process; this is not power-loss evidence.
use super::*;
use std::{
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};
use ulmdb::PublicationMode;

struct Fixture {
    path: PathBuf,
    env: *mut MDB_env,
    flags: u32,
}

impl Fixture {
    fn new(mode: PublicationMode, flags: u32) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-c-prepare-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        drop(Environment::create_with_publication(&path, mode).unwrap());
        let env = Self::open(&path, flags);
        Self { path, env, flags }
    }

    fn open(path: &std::path::Path, flags: u32) -> *mut MDB_env {
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let mut env = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut env), 0);
            assert_eq!(environment::mdb_env_set_maxdbs(env, 8), 0);
            assert_eq!(
                environment::mdb_env_open(env, name.as_ptr(), flags, 0o600),
                0
            );
        }
        env
    }

    fn begin(&self, flags: u32) -> *mut MDB_txn {
        let mut txn = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(self.env, ptr::null_mut(), flags, &mut txn) },
            0
        );
        txn
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { environment::mdb_env_close(self.env) };
        if !std::thread::panicking() {
            std::fs::remove_dir_all(&self.path).unwrap();
        }
    }
}

fn head(env: *mut MDB_env) -> usize {
    let mut info = std::mem::MaybeUninit::<MDB_envinfo>::uninit();
    assert_eq!(
        unsafe { environment::mdb_env_info(env, info.as_mut_ptr()) },
        0
    );
    unsafe { info.assume_init() }.me_last_txnid
}

fn database(txn: *mut MDB_txn, name: &CStr, flags: u32) -> u32 {
    let mut slot = 0;
    assert_eq!(
        unsafe { operations::mdb_dbi_open(txn, name.as_ptr(), flags, &mut slot) },
        0
    );
    slot
}

fn put(txn: *mut MDB_txn, slot: u32, bytes: &[u8]) {
    let mut key = MDB_val {
        mv_size: 1,
        mv_data: b"k".as_ptr().cast_mut().cast(),
    };
    let mut value = MDB_val {
        mv_size: bytes.len(),
        mv_data: bytes.as_ptr().cast_mut().cast(),
    };
    assert_eq!(
        unsafe { operations::mdb_put(txn, slot, &mut key, &mut value, 0) },
        0
    );
}

fn expect_value(txn: *mut MDB_txn, slot: u32, expected: &[u8]) {
    let mut key = MDB_val {
        mv_size: 1,
        mv_data: b"k".as_ptr().cast_mut().cast(),
    };
    let mut value = MDB_val {
        mv_size: 0,
        mv_data: ptr::null_mut(),
    };
    assert_eq!(
        unsafe { operations::mdb_get(txn, slot, &mut key, &mut value) },
        0
    );
    assert_eq!(
        unsafe { std::slice::from_raw_parts(value.mv_data.cast::<u8>(), value.mv_size) },
        expected
    );
}

fn run_fault(mode: PublicationMode, flags: u32, preparing: bool, point: &str) {
    let f = Fixture::new(mode, flags);
    let seed = f.begin(0);
    let values = database(seed, c"values", CREATE);
    put(seed, values, b"old");
    assert_eq!(unsafe { transaction::mdb_txn_commit(seed) }, 0);
    assert_eq!(head(f.env), 1);

    let txn = f.begin(0);
    let created = database(txn, c"created", CREATE);
    put(txn, created, b"new");
    put(txn, values, b"new");
    if !preparing {
        assert_eq!(unsafe { transaction::mdb_txn_prepare(txn) }, 0);
    }
    // This process owns the only test and sets the fault after fixture setup.
    // The trace must reach the requested boundary, so a disabled injector fails.
    let trace = f.path.join("fault.trace");
    unsafe {
        std::env::set_var("ULMDB_TRACE_FILE", &trace);
        std::env::set_var("ULMDB_FAIL_AT", point);
    }
    let result = if preparing {
        unsafe { transaction::mdb_txn_prepare(txn) }
    } else {
        unsafe { transaction::mdb_txn_commit(txn) }
    };
    unsafe {
        std::env::remove_var("ULMDB_FAIL_AT");
        std::env::remove_var("ULMDB_TRACE_FILE");
    }
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap().lines().last(),
        Some(point)
    );

    if preparing {
        assert_eq!(result, libc::EIO, "known unpublished preparation failure");
        assert_eq!(head(f.env), 1);
        let read = f.begin(RDONLY);
        let mut absent = 0;
        assert_eq!(
            unsafe { operations::mdb_dbi_open(read, c"created".as_ptr(), 0, &mut absent) },
            NOTFOUND,
            "failed preparation must release provisional aliases"
        );
        expect_value(read, values, b"old");
        unsafe { transaction::mdb_txn_abort(read) };

        // An actual retry proves the writer lease was released and the owner
        // was not poisoned by a failure known to precede publication.
        let retry = f.begin(0);
        let recreated = database(retry, c"created", CREATE);
        put(retry, recreated, b"retry");
        assert_eq!(unsafe { transaction::mdb_txn_commit(retry) }, 0);
        assert_eq!(head(f.env), 2);
        let read = f.begin(RDONLY);
        expect_value(read, recreated, b"retry");
        unsafe { transaction::mdb_txn_abort(read) };
    } else {
        assert_eq!(result, -30795, "uncertain final commit poisons its owner");
        let mut denied = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(f.env, ptr::null_mut(), 0, &mut denied) },
            -30795
        );
        assert!(denied.is_null());

        let fresh = Fixture::open(&f.path, f.flags);
        let new_selected = point.starts_with("pointer.CURRENT.")
            && [
                "after_rename",
                "before_directory_sync",
                "after_directory_sync",
            ]
            .iter()
            .any(|suffix| point.ends_with(suffix));
        assert_eq!(head(fresh), if new_selected { 2 } else { 1 });
        let mut read = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(fresh, ptr::null_mut(), RDONLY, &mut read) },
            0
        );
        let current_values = database(read, c"values", 0);
        expect_value(
            read,
            current_values,
            if new_selected { b"new" } else { b"old" },
        );
        let mut slot = 0;
        let created_result =
            unsafe { operations::mdb_dbi_open(read, c"created".as_ptr(), 0, &mut slot) };
        assert_eq!(created_result, if new_selected { 0 } else { NOTFOUND });
        if new_selected {
            expect_value(read, slot, b"new");
        }
        unsafe { transaction::mdb_txn_abort(read) };
        let mut writer = ptr::null_mut();
        assert_eq!(
            unsafe { transaction::mdb_txn_begin(fresh, ptr::null_mut(), 0, &mut writer) },
            0,
            "consumed pending owner must release the writer lease"
        );
        unsafe {
            transaction::mdb_txn_abort(writer);
            environment::mdb_env_close(fresh);
        }
    }
    println!("PREPARE_FAULT_PASS mode={mode:?} flags={flags} preparing={preparing} point={point}");
}

#[test]
fn worker() {
    if let Ok(role) = std::env::var("ULMDB_C_PREPARE_FAULT") {
        let fields: Vec<_> = role.split('|').collect();
        assert_eq!(fields.len(), 4);
        let mode = match fields[0] {
            "legacy" => PublicationMode::Legacy,
            "cohort" => PublicationMode::Cohort,
            "indexed" => PublicationMode::Indexed,
            _ => panic!("invalid mode"),
        };
        run_fault(
            mode,
            fields[1].parse().unwrap(),
            fields[2] == "prepare",
            fields[3],
        );
    }
}

#[test]
#[ignore = "requires cargo test -p ulmdb-c --features ulmdb/fault-injection -- --ignored"]
fn preparation_c_fault_controls() {
    let segment = [
        "before_write",
        "after_write",
        "before_file_sync",
        "after_file_sync",
        "before_rename",
        "after_rename",
        "before_segments_dir_sync",
        "after_segments_dir_sync",
    ];
    let descriptor = [
        "before_write",
        "after_write",
        "before_file_sync",
        "after_file_sync",
        "before_directory_sync",
        "after_directory_sync",
    ];
    let current = [
        "before_write",
        "after_write",
        "before_file_sync",
        "after_file_sync",
        "before_rename",
        "after_rename",
        "before_directory_sync",
        "after_directory_sync",
    ];
    let previous = [
        "before_write",
        "after_write",
        "before_file_sync",
        "after_file_sync",
        "before_rename",
        "after_rename",
    ];
    let mut count = 0;
    for mode in ["legacy", "cohort", "indexed"] {
        let mut points = Vec::new();
        if mode == "legacy" {
            points.extend(segment.iter().map(|s| ("prepare", format!("segment.{s}"))));
        }
        points.extend(descriptor.iter().map(|s| {
            (
                "prepare",
                format!(
                    "{}.{s}",
                    if mode == "legacy" {
                        "descriptor"
                    } else {
                        "cohort"
                    }
                ),
            )
        }));
        points.extend(
            current
                .iter()
                .map(|s| ("commit", format!("pointer.CURRENT.{s}"))),
        );
        if mode == "legacy" {
            points.extend(
                previous
                    .iter()
                    .map(|s| ("commit", format!("pointer.PREVIOUS.{s}"))),
            );
        }
        for flags in [0, 0x200000] {
            for (phase, point) in &points {
                let role = format!("{mode}|{flags}|{phase}|{point}");
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "prepared_tests::worker",
                        "--nocapture",
                        "--test-threads=1",
                    ])
                    .env("ULMDB_C_PREPARE_FAULT", &role)
                    .env("ULMDB_LMDB_ABI", "1.0")
                    .env_remove("ULMDB_FAIL_AT")
                    .env_remove("ULMDB_CRASH_AT")
                    .env_remove("ULMDB_TRACE_FILE")
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap();
                let deadline = Instant::now() + Duration::from_secs(8);
                loop {
                    if child.try_wait().unwrap().is_some() {
                        break;
                    }
                    if Instant::now() >= deadline {
                        child.kill().unwrap();
                        let output = child.wait_with_output().unwrap();
                        panic!(
                            "preparation fault worker deadline: {role}\n{}\n{}",
                            String::from_utf8_lossy(&output.stdout),
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                let output = child.wait_with_output().unwrap();
                assert!(
                    output.status.success(),
                    "{role}\n{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let stdout = String::from_utf8(output.stdout).unwrap();
                assert_eq!(stdout.matches("PREPARE_FAULT_PASS ").count(), 1);
                print!("{stdout}");
                count += 1;
            }
        }
    }
    assert_eq!(count, 112);
}
