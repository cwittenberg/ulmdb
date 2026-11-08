//! Facade routing with actual mapped KV and IP ranges, retained old locks.
use std::{fs, path::PathBuf};
use ulmdb::{Address, DatabaseKind, Environment, Prefix, PublicationMode, ResidencyPolicy};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(PathBuf::from("/dev/shm").join(format!(
            "ulmdb-range-facade-admission-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn required_locked_facade_updates_preserve_exact_reports_old_borrows_and_reopen() {
    let d = Dir::new();
    let e = Environment::create_with_residency(
        &d.0,
        PublicationMode::Range,
        ResidencyPolicy::RequiredLocked {
            max_bytes: 4 * 1024 * 1024,
        },
    )
    .unwrap();
    let p = Prefix::new(Address::V4(0xc0000200), 24).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
    w.put(b"kv", b"state", &1u64.to_le_bytes()).unwrap();
    w.insert_prefix(b"ip", p, &1u64.to_le_bytes()).unwrap();
    w.commit().unwrap();
    let old = e.begin_read();
    let borrowed = old.get(b"kv", b"state").unwrap().unwrap();
    for n in 2..=64u64 {
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", b"state", &n.to_le_bytes()).unwrap();
        w.insert_prefix(b"ip", p, &n.to_le_bytes()).unwrap();
        w.commit().unwrap();
        let report = e.residency_report();
        assert_eq!(report.locked_bytes, report.page_rounded_bytes.unwrap());
        assert_eq!(report.locked_bytes, e.residency_status().reserved_bytes);
        assert_eq!(borrowed, 1u64.to_le_bytes());
    }
    ulmdb::backup::verify(&e).unwrap();
    drop(e);
    assert_eq!(borrowed, 1u64.to_le_bytes());
    let reopened = Environment::open(&d.0).unwrap();
    assert_eq!(reopened.stats().sequence, 64);
    assert_eq!(
        reopened.begin_read().get(b"kv", b"state").unwrap(),
        Some(64u64.to_le_bytes().as_slice())
    );
    assert_eq!(
        reopened
            .begin_read()
            .exact_prefix(b"ip", p)
            .unwrap()
            .unwrap()
            .1,
        64u64.to_le_bytes()
    );
    assert_eq!(borrowed, 1u64.to_le_bytes());
}
#[test]
fn real_os_append_failure_worker() {
    let Some(path) = std::env::var_os("ULMDB_RANGE_ADMISSION_OS_DIR") else {
        return;
    };
    let e = Environment::create_with_residency(
        &path,
        PublicationMode::Range,
        ResidencyPolicy::RequiredLocked { max_bytes: 1048576 },
    )
    .unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"state", b"old").unwrap();
    w.commit().unwrap();
    let locked = e.residency_status().live_locked_bytes;
    let limit = format!("--memlock={locked}:{locked}");
    let output = std::process::Command::new("prlimit")
        .args(["--pid", &std::process::id().to_string(), &limit])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let before = e.residency_status();
    let pack = PathBuf::from(&path).join("RANGE.pack");
    let length = fs::metadata(&pack).unwrap().len();
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", b"state", b"new").unwrap();
    let result = w.commit();
    assert!(matches!(result, Err(ulmdb::Error::Io(_))), "{result:?}");
    assert_eq!(e.stats().sequence, 1);
    assert_eq!(
        e.begin_read().get(b"kv", b"state").unwrap(),
        Some(b"old".as_slice())
    );
    assert_eq!(e.residency_status(), before);
    assert_eq!(fs::metadata(&pack).unwrap().len(), length);
    e.begin_write().unwrap().abort();
    drop(e);
    let reopened = Environment::open(path).unwrap();
    assert_eq!(reopened.stats().sequence, 1);
    assert_eq!(
        reopened.begin_read().get(b"kv", b"state").unwrap(),
        Some(b"old".as_slice())
    );
}
#[test]
fn actual_os_lock_failure_after_nonempty_prior_cannot_select_candidate() {
    let d = Dir::new();
    let status = fs::read_to_string("/proc/self/status").unwrap();
    let caps = u64::from_str_radix(
        status
            .lines()
            .find_map(|l| l.strip_prefix("CapEff:\t"))
            .unwrap(),
        16,
    )
    .unwrap();
    let mut cmd = if caps & (1 << 14) != 0 {
        let mut c = std::process::Command::new("setpriv");
        c.args(["--bounding-set=-ipc_lock", "--", "/bin/sh"]);
        c
    } else {
        std::process::Command::new("/bin/sh")
    };
    let output = cmd
        .args([
            "-c",
            "ulimit -l 1024 || exit 99; exec \"$@\"",
            "range-append-lock",
        ])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", "real_os_append_failure_worker", "--nocapture"])
        .env("ULMDB_RANGE_ADMISSION_OS_DIR", &d.0)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
