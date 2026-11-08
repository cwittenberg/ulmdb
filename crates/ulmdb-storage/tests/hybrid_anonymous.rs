//! Anonymous ownership and accounting controls; no latency claims.
use std::{
    fs::{File, OpenOptions},
    sync::Arc,
};
use ulmdb_storage::{
    ReadStore, Reference, WriteStore,
    hybrid::{ChunkLimits, ChunkWriter, ImageMode, ImageTable},
};
fn limits() -> ChunkLimits {
    ChunkLimits {
        capacity: 1024 * 1024,
        reserve_quantum: 65536,
        max_burned_bytes: 512 * 1024,
    }
}
#[test]
fn anonymous_same_sid_copy_freeze_burn_and_exact_borrow() {
    let mut writer = ChunkWriter::create_anonymous(7, limits()).unwrap();
    assert_eq!(writer.image_mode(), ImageMode::Anonymous);
    let status = writer.status();
    assert_eq!(status.file_reserved_bytes, 0);
    assert_eq!(status.anonymous_virtual_bytes, limits().capacity);
    assert_eq!(status.reserved_bytes, limits().capacity);
    let mut table = ImageTable::default()
        .with_active(writer.read_view())
        .unwrap();
    let mut a = writer.begin(table.clone()).unwrap();
    let at = a.allocate(&[1; 64], 16).unwrap();
    let f = a.freeze().unwrap();
    table = table.with_active(f.view()).unwrap();
    f.accept();
    let old = table.clone();
    let held = old.read(at, 64).unwrap();
    assert_eq!(held, &[1; 64]);
    let mut a = writer.begin(table.clone()).unwrap();
    let abandoned = a.rewrite(at, &[2; 64], 16).unwrap();
    assert_ne!(abandoned, at);
    let f = a.freeze().unwrap();
    let escaped = f.view();
    assert_eq!(escaped.image_mode(), ImageMode::Anonymous);
    let borrowed = escaped.read(abandoned.offset as usize, 64).unwrap();
    let burned_end = escaped.end();
    drop(f);
    let mut a = writer.begin(table.clone()).unwrap();
    let next = a.rewrite(at, &[3; 64], 16).unwrap();
    assert!(next.offset >= burned_end as u64);
    let private = a.patch(next, 64, 4, &[4; 8], 16).unwrap();
    assert_eq!(private, next);
    let f = a.freeze().unwrap();
    table = table.with_active(f.view()).unwrap();
    f.accept();
    assert_eq!(held, &[1; 64]);
    assert_eq!(borrowed, &[2; 64]);
    assert_eq!(&table.read(next, 64).unwrap()[4..12], &[4; 8]);
    assert!(old.read(next, 64).is_err());
    assert_eq!(writer.status().burned_bytes, 64);
    drop(writer);
    assert_eq!(held, &[1; 64]);
    assert_eq!(borrowed, &[2; 64]);
}
fn anonymous_fence_child() {
    let dir = std::env::temp_dir().join(format!(
        "ulmdb-anon-fence-{:032x}",
        ulmdb_storage::new_identity().unwrap()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("LOCK");
    let fence = Arc::new(
        ulmdb_storage::WriterFence::try_lock(
            OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap(),
        )
        .unwrap(),
    );
    let mut writer =
        ChunkWriter::create_anonymous_with_fence(1, limits(), Arc::clone(&fence)).unwrap();
    drop(fence);
    let mut a = writer.begin(ImageTable::default()).unwrap();
    let at = a.allocate(&[8; 16], 16).unwrap();
    let frozen = a.freeze().unwrap();
    let span = frozen.view();
    drop(writer);
    let other = File::open(&path).unwrap();
    assert!(matches!(
        other.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    frozen.accept();
    other.try_lock().unwrap();
    assert_eq!(span.read(at.offset as usize, 16).unwrap(), &[8; 16]);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    drop(other);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn anonymous_invalid_capacity_is_not_a_file_fallback() {
    let mut bad = limits();
    bad.capacity = 0;
    assert!(matches!(
        ChunkWriter::create_anonymous(1, bad),
        Err(ulmdb_storage::Error::InvalidInput(_))
    ));
    assert!(ChunkWriter::create_anonymous(0, limits()).is_err());
    let mut w = ChunkWriter::create_anonymous(1, limits()).unwrap();
    let a = w.begin(ImageTable::default()).unwrap();
    assert!(
        a.read(
            Reference {
                segment: 1,
                offset: 4096
            },
            1
        )
        .is_err()
    );
}
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn anonymous_map_failure_child() {
    let mut writer = ChunkWriter::create_anonymous(1, limits()).unwrap();
    let mut a = writer.begin(ImageTable::default()).unwrap();
    let at = a.allocate(&[0x7a; 64], 16).unwrap();
    let f = a.freeze().unwrap();
    let view = f.accept();
    let held = view.read(at.offset as usize, 64).unwrap();
    let text = std::fs::read_to_string("/proc/self/status").unwrap();
    let kib: u64 = text
        .lines()
        .find(|l| l.starts_with("VmSize:"))
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let mut old = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: process-local rlimit API, valid initialized structure; this entire
    // test runs in a dedicated child and restores the limit before assertions.
    assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_AS, &mut old) }, 0);
    let target = libc::rlimit {
        rlim_cur: libc::rlim_t::try_from(kib * 1024 + 2 * 1024 * 1024)
            .unwrap()
            .min(old.rlim_cur),
        rlim_max: old.rlim_max,
    };
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_AS, &target) }, 0);
    let mut large = limits();
    large.capacity = 128 * 1024 * 1024;
    let result = ChunkWriter::create_anonymous(2, large);
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_AS, &old) }, 0);
    assert!(
        matches!(result,Err(ulmdb_storage::Error::Io(e))if e.raw_os_error()==Some(libc::ENOMEM))
    );
    assert_eq!(held, &[0x7a; 64]);
    assert_eq!(writer.read_view().end(), view.end());
}
// A concurrent fork can inherit an OFD until exec closes its CLOEXEC fd.
// Run each process-wide resource/fence control in its own exec image so another
// parallel test's fork cannot transiently retain the lock being examined.
fn isolated_test(name: &str, body: fn()) {
    if std::env::var("ULMDB_ANON_TEST_CHILD").ok().as_deref() == Some(name) {
        body();
        println!("\nULMDB_ANON_TEST_CHILD_COMPLETE {name}");
        return;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("ULMDB_ANON_TEST_CHILD", name)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > std::time::Duration::from_secs(30) {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!("{name} child timed out: {output:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let output = child.wait_with_output().unwrap();
    let marker = format!("ULMDB_ANON_TEST_CHILD_COMPLETE {name}");
    assert!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line == marker),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[cfg(target_os = "linux")]
#[test]
fn actual_anonymous_map_enomem_preserves_existing_owner() {
    isolated_test(
        "actual_anonymous_map_enomem_preserves_existing_owner",
        anonymous_map_failure_child,
    );
}
#[test]
fn anonymous_permit_retains_global_fence_but_read_span_does_not() {
    isolated_test(
        "anonymous_permit_retains_global_fence_but_read_span_does_not",
        anonymous_fence_child,
    );
}
