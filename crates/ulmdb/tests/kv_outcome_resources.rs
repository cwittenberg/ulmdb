//! Real Linux resource denial in bounded child processes; parent limits are untouched.
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use ulmdb::{DatabaseKind, Environment, Error, ValueLimit};
const MAX_VALUE: usize = ValueLimit::DEFAULT.get();

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn run_worker(parent: &Path, case: &str) {
    let directory = Directory(parent.join(format!(
        "ulmdb-outcome-resource-{}",
        ulmdb_storage::new_identity().unwrap()
    )));
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "outcome_resource_worker", "--nocapture"])
        .env("ULMDB_OUTCOME_RESOURCE_CASE", case)
        .env("ULMDB_OUTCOME_RESOURCE_PATH", &directory.0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "resource worker timed out: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "resource worker {case}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn exact_64mib_boundary_and_actual_previous_copy_resource_failure_disk_and_ramdrive() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for case in ["boundary", "reservation"] {
            run_worker(&parent, case);
        }
    }
}

#[test]
fn outcome_resource_worker() {
    let Ok(case) = std::env::var("ULMDB_OUTCOME_RESOURCE_CASE") else {
        return;
    };
    let path = std::env::var_os("ULMDB_OUTCOME_RESOURCE_PATH").unwrap();
    let env = Environment::create(&path).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    let key = vec![0; 4096];
    let value = vec![0x5a; MAX_VALUE];
    assert_eq!(w.put_with_previous(b"kv", &key, &value).unwrap(), None);
    drop(value);
    w.commit().unwrap();
    let old = env.begin_read();
    // A real mutation can select the 128MiB extension. Its outer boundary
    // remains invalid; the prior-value copy oracle deliberately stays64MiB.
    let oversized = vec![1; ValueLimit::EXTENDED.get() + 1];
    if case == "boundary" {
        let mut w = env.begin_write().unwrap();
        let dirty = w.dirty_bytes();
        assert!(matches!(
            w.put_with_previous(b"kv", &key, &oversized),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(w.dirty_bytes(), dirty);
        assert_eq!(w.value_limit(), ValueLimit::DEFAULT);
        let prior = w.put_with_previous(b"kv", &key, b"next").unwrap().unwrap();
        assert_eq!(prior.len(), MAX_VALUE);
        assert!(prior.iter().all(|b| *b == 0x5a));
        assert!(w.delete_with_presence(b"kv", &key).unwrap());
        assert!(!w.delete_with_presence(b"kv", &key).unwrap());
        assert_eq!(w.put_with_previous(b"kv", &key, b"").unwrap(), None);
        w.commit().unwrap();
        assert_eq!(old.get(b"kv", &key).unwrap().unwrap().len(), MAX_VALUE);
        drop(env);
        let reopened = Environment::open(&path).unwrap();
        assert_eq!(
            reopened.begin_read().get(b"kv", &key).unwrap(),
            Some(b"".as_slice())
        );
    } else {
        assert_eq!(case, "reservation");
        // Set only this subprocess's SOFT address-space limit. The kernel must
        // deny a fresh 64MiB prior-value Vec; leave 4MiB for small normal writes.
        // The oversized argument already exists, so its validation cannot be
        // confused with a failed caller-side argument allocation.
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let kib: u64 = status
            .lines()
            .find_map(|line| line.strip_prefix("VmSize:"))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let limit = kib
            .checked_mul(1024)
            .unwrap()
            .checked_add(4 * 1024 * 1024)
            .unwrap();
        let result = Command::new("prlimit")
            .arg(format!("--pid={}", std::process::id()))
            .arg(format!("--as={limit}:"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "required prlimit facility failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let mut w = env.begin_write().unwrap();
        let dirty = w.dirty_bytes();
        // Validation precedes the otherwise impossible previous-value copy.
        assert!(matches!(
            w.put_with_previous(b"kv", &key, &oversized),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(w.dirty_bytes(), dirty);
        assert_eq!(w.value_limit(), ValueLimit::DEFAULT);
        assert!(matches!(
            w.put_with_previous(b"kv", &key, b"denied"),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(w.dirty_bytes(), dirty);
        let preserved = w.get(b"kv", &key).unwrap().unwrap();
        assert_eq!(preserved.len(), MAX_VALUE);
        assert_eq!((preserved[0], preserved[MAX_VALUE - 1]), (0x5a, 0x5a));
        // A failed prior-copy reservation is pre-mutation and leaves the writer live.
        assert_eq!(
            w.put_with_previous(b"kv", b"small", b"usable").unwrap(),
            None
        );
        assert_eq!(w.get(b"kv", b"small").unwrap(), Some(b"usable".as_slice()));
        assert!(w.delete_with_presence(b"kv", &key).unwrap()); // no large value copy
        w.abort();
        assert_eq!(old.get(b"kv", &key).unwrap().unwrap().len(), MAX_VALUE);
        assert_eq!(
            env.begin_read().get(b"kv", &key).unwrap().unwrap().len(),
            MAX_VALUE
        );
    }
}
