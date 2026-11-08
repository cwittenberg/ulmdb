#![cfg(feature = "fault-injection")]
//! Real Environment interruption tests; process exits retain the OS cache.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix, PublicationMode};
const POINTS: &[&str] = &[
    "packed.commit.before_header",
    "packed.commit.half_header",
    "packed.commit.after_header",
    "packed.commit.half_payload",
    "packed.commit.after_payload",
    "packed.commit.before_footer",
    "packed.commit.half_footer",
    "packed.commit.after_footer",
    "packed.commit.before_frame_sync",
    "packed.commit.after_frame_sync",
    "packed.commit.before_head",
    "packed.commit.half_head",
    "packed.commit.after_head",
    "packed.commit.before_head_sync",
    "packed.commit.after_head_sync",
    "packed.commit.before_ack",
];
struct Dir(PathBuf);
impl Dir {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-range-fault-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn net4() -> Prefix {
    Prefix::new(Address::V4(0xc0000200), 24).unwrap()
}
fn net6() -> Prefix {
    Prefix::new(Address::V6(0x20010db8000000000000000000000000), 32).unwrap()
}
fn initialize(path: &Path) {
    let mode = if std::env::var_os("ULMDB_TEST_RANGE_LOCATORS").is_some() {
        PublicationMode::Range
    } else {
        PublicationMode::RangeV5
    };
    let e = Environment::create_with_publication(path, mode).unwrap();
    let mut w = e.begin_write().unwrap();
    for (name, kind) in [
        (b"kv".as_slice(), DatabaseKind::Kv),
        (b"v4", DatabaseKind::Ipv4),
        (b"v6", DatabaseKind::Ipv6),
        (b"dups", DatabaseKind::Duplicates { width: None }),
    ] {
        w.create_database(name, kind).unwrap();
    }
    w.put(b"kv", b"state", b"old").unwrap();
    w.insert_prefix(b"v4", net4(), b"old").unwrap();
    w.insert_prefix(b"v6", net6(), b"old").unwrap();
    w.insert_duplicate(b"dups", b"state", b"old").unwrap();
    w.commit().unwrap();
}
fn update(e: &Environment) -> ulmdb::Result<u64> {
    let mut w = e.begin_write()?;
    w.put(b"kv", b"state", b"new")?;
    w.insert_prefix(b"v4", net4(), b"new")?;
    w.insert_prefix(b"v6", net6(), b"new")?;
    w.delete_duplicates(b"dups", b"state")?;
    w.insert_duplicate(b"dups", b"state", b"new")?;
    w.commit()
}
#[test]
fn range_worker() {
    let Some(p) = std::env::var_os("ULMDB_RANGE_WORKER") else {
        return;
    };
    let e = Environment::open(p).unwrap();
    let result = update(&e);
    if let Ok(point) = std::env::var("ULMDB_PACKED_FAIL_AT") {
        let head = POINTS.iter().position(|p| *p == point).unwrap() >= 10;
        if head {
            assert!(
                matches!(result, Err(Error::OutcomeUnknown)),
                "{point}: {result:?}"
            );
            assert!(matches!(e.begin_write(), Err(Error::OutcomeUnknown)));
        } else {
            assert!(matches!(result, Err(Error::Io(_))), "{point}: {result:?}");
            e.begin_write().unwrap().abort();
        }
        assert_eq!(e.begin_read().sequence(), 1);
        assert_eq!(
            e.begin_read().get(b"kv", b"state").unwrap(),
            Some(b"old".as_slice())
        );
    } else {
        assert_eq!(result.unwrap(), 2);
    }
}
fn check(path: &Path, new: bool) {
    let e = Environment::open(path).unwrap();
    let r = e.begin_read();
    let expected: &[u8] = if new { b"new" } else { b"old" };
    assert_eq!(r.sequence(), if new { 2 } else { 1 });
    assert_eq!(r.get(b"kv", b"state").unwrap(), Some(expected));
    assert_eq!(
        r.exact_prefix(b"v4", net4()).unwrap(),
        Some((net4(), expected))
    );
    assert_eq!(
        r.exact_prefix(b"v6", net6()).unwrap(),
        Some((net6(), expected))
    );
    let d = r.duplicates(b"dups").unwrap();
    assert_eq!(d.count(b"state").unwrap(), 1);
    assert!(d.contains(b"state", expected).unwrap());
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", b"continued", b"yes").unwrap();
    assert_eq!(w.commit().unwrap(), r.sequence() + 1);
    assert_eq!(r.get(b"kv", b"continued").unwrap(), None);
}
fn matrix(parent: &Path) {
    for returned in [false, true] {
        for (index, point) in POINTS.iter().enumerate() {
            let d = Dir::new(parent);
            initialize(&d.0);
            let before = fs::metadata(d.0.join("RANGE.pack")).unwrap().len();
            let status = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "range_worker", "--nocapture"])
                .env("ULMDB_RANGE_WORKER", &d.0)
                .env(
                    if returned {
                        "ULMDB_PACKED_FAIL_AT"
                    } else {
                        "ULMDB_PACKED_CRASH_AT"
                    },
                    point,
                )
                .output()
                .unwrap();
            assert_eq!(
                status.status.code(),
                Some(if returned { 0 } else { 86 }),
                "{point}: {}",
                String::from_utf8_lossy(&status.stderr)
            );
            if returned && index < 10 {
                assert_eq!(fs::metadata(d.0.join("RANGE.pack")).unwrap().len(), before);
            }
            // Before_head has not written. A torn head has exactly one complete successor;
            // strict recovery adopts that successor, and complete heads select it directly.
            check(&d.0, index >= 11);
        }
    }
}
#[test]
fn disk_all_16_cuts_and_16_returned_errors() {
    matrix(&std::env::temp_dir());
}
#[test]
fn tmpfs_all_16_cuts_and_16_returned_errors() {
    matrix(Path::new("/dev/shm"));
}
#[test]
fn commit_hook_trace_matches_declared_matrix() {
    let d = Dir::new(&std::env::temp_dir());
    initialize(&d.0);
    let trace = d.0.join("trace.txt");
    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "range_worker"])
        .env("ULMDB_RANGE_WORKER", &d.0)
        .env("ULMDB_PACKED_TRACE_FILE", &trace)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = fs::read_to_string(trace).unwrap();
    assert_eq!(
        text.lines()
            .filter(|s| s.starts_with("packed.commit."))
            .collect::<Vec<_>>(),
        POINTS
    );
    check(&d.0, true);
}
