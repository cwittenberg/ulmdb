//! Actual Env fixed-change work bounds, not latency qualification.
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::{
    fs,
    ops::Bound,
    path::{Path, PathBuf},
};
use ulmdb::{Address, DatabaseKind, Environment, Error, Prefix, PublicationMode, ResidencyPolicy};
struct Dir(PathBuf);
impl Dir {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-locator-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn p4() -> Prefix {
    Prefix::new(Address::V4(0xc0000200), 24).unwrap()
}
fn p6() -> Prefix {
    Prefix::new(Address::V6(0x20010db8000000000000000000000000), 32).unwrap()
}
fn fill(e: &Environment) {
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
    w.create_database(b"v6", DatabaseKind::Ipv6).unwrap();
    for n in 0..128u64 {
        w.put(b"kv", &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
    }
    w.insert_prefix(b"v4", p4(), &1u64.to_le_bytes()).unwrap();
    w.insert_prefix(b"v6", p6(), &1u64.to_le_bytes()).unwrap();
    w.commit().unwrap();
}
fn growing(parent: &Path, limit: u64) {
    let d = Dir::new(parent);
    let mut e = Environment::create_with_publication(&d.0, PublicationMode::Range).unwrap();
    fill(&e);
    let initial = e.locator_work().unwrap();
    eprintln!(
        "RANGE_LOCATOR_WORK {{\"schema\":1,\"n\":1,\"nodes_written\":{},\"serialization_visits\":{},\"locator_bytes\":{},\"data_bytes\":{},\"peak_nodes\":{},\"peak_visits\":{},\"qualified\":false}}",
        initial.nodes_written,
        initial.nodes_visited,
        initial.node_bytes,
        initial.data_bytes,
        initial.nodes_written,
        initial.nodes_visited
    );
    let mut peak_nodes = e.locator_work().unwrap().nodes_written;
    let mut peak_visits = e.locator_work().unwrap().nodes_visited;
    let old = e.begin_read();
    let old_value = old.get(b"kv", &7u64.to_be_bytes()).unwrap().unwrap();
    for n in 2..=limit {
        let mut w = e.begin_write().unwrap();
        w.put(b"kv", &7u64.to_be_bytes(), &n.to_le_bytes()).unwrap();
        w.insert_prefix(b"v4", p4(), &n.to_le_bytes()).unwrap();
        w.insert_prefix(b"v6", p6(), &n.to_le_bytes()).unwrap();
        assert_eq!(w.commit().unwrap(), n);
        let work = e.locator_work().unwrap();
        assert!(
            work.nodes_written <= 65 && work.nodes_visited <= 129 && work.node_bytes <= 10400,
            "{n}: {work:?}"
        );
        assert!(!work.inventory_materialized);
        peak_nodes = peak_nodes.max(work.nodes_written);
        peak_visits = peak_visits.max(work.nodes_visited);

        assert_eq!(old_value, 7u64.to_le_bytes());
        if [2, 16, 257, 1025, 4097].contains(&n) {
            eprintln!(
                "RANGE_LOCATOR_WORK {{\"schema\":1,\"n\":{n},\"nodes_written\":{},\"serialization_visits\":{},\"locator_bytes\":{},\"data_bytes\":{},\"peak_nodes\":{peak_nodes},\"peak_visits\":{peak_visits},\"qualified\":false}}",
                work.nodes_written, work.nodes_visited, work.node_bytes, work.data_bytes
            );
            assert_eq!(e.stats().segments, n as usize);
            assert_eq!(e.stats().format_minor, 6);
            assert_eq!(e.publication_mode(), PublicationMode::Range);
            ulmdb::backup::verify(&e).unwrap();
            drop(e);
            e = Environment::open(&d.0).unwrap();
            let r = e.begin_read();
            assert_eq!(
                r.get(b"kv", &7u64.to_be_bytes()).unwrap(),
                Some(n.to_le_bytes().as_slice())
            );
            assert_eq!(
                r.exact_prefix(b"v4", p4()).unwrap().unwrap().1,
                n.to_le_bytes()
            );
            assert_eq!(
                r.exact_prefix(b"v6", p6()).unwrap().unwrap().1,
                n.to_le_bytes()
            );
            assert_eq!(old_value, 7u64.to_le_bytes());
        }
    }
    let r = e.begin_read();
    assert_eq!(
        r.range(b"kv", Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .count(),
        128
    );
    for n in 0..128u64 {
        let expected = if n == 7 { limit } else { n };
        assert_eq!(
            r.get(b"kv", &n.to_be_bytes()).unwrap(),
            Some(expected.to_le_bytes().as_slice())
        );
    }
    let len = fs::metadata(d.0.join("RANGE.pack")).unwrap().len();
    assert_eq!(e.begin_write().unwrap().commit().unwrap(), limit);
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", b"aborted", b"x").unwrap();
    w.abort();
    assert_eq!(fs::metadata(d.0.join("RANGE.pack")).unwrap().len(), len);
    for turn in 0..4 {
        let mut w = e.begin_write().unwrap();
        for n in 0..64u64 {
            w.put(b"kv", &n.to_be_bytes(), &[turn as u8; 8]).unwrap();
        }
        w.commit().unwrap();
        assert!(e.locator_work().unwrap().nodes_written <= 65);
    }
    assert!(matches!(e.reclaim(), Err(Error::InvalidInput(_))));
    assert!(matches!(e.compact(), Err(Error::InvalidInput(_))));
    assert!(matches!(e.prune_inventory(), Err(Error::InvalidInput(_))));
    assert!(matches!(e.upgrade_inventory(), Err(Error::InvalidInput(_))));
}
fn disk_fixed_change257_and_reopen_body() {
    growing(&std::env::temp_dir(), 257);
}
fn tmpfs_fixed_change4097_and_reopen_body() {
    growing(Path::new("/dev/shm"), 4097);
}
fn v5_read_and_write_are_preserved_without_upgrade_body() {
    let d = Dir::new(&std::env::temp_dir());
    let e = Environment::create_with_publication(&d.0, PublicationMode::RangeV5).unwrap();
    fill(&e);
    drop(e);
    let e = Environment::open(&d.0).unwrap();
    assert_eq!(e.publication_mode(), PublicationMode::RangeV5);
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", b"after", b"open").unwrap();
    w.commit().unwrap();
    drop(e);
    let e = Environment::open(&d.0).unwrap();
    assert_eq!(e.stats().format_minor, 5);
    assert!(e.locator_work().is_none());
}
fn v6_denied_candidate_is_known_nonpublication_body() {
    let d = Dir::new(Path::new("/dev/shm"));
    let e = Environment::create_with_residency(
        &d.0,
        PublicationMode::Range,
        ResidencyPolicy::Prefault { max_bytes: 0 },
    )
    .unwrap();
    let len = fs::metadata(d.0.join("RANGE.pack")).unwrap().len();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"key", b"value").unwrap();
    assert!(matches!(w.commit(), Err(Error::LimitExceeded)));
    assert_eq!(fs::metadata(d.0.join("RANGE.pack")).unwrap().len(), len);
    e.begin_write().unwrap().abort();
    drop(e);
    assert_eq!(Environment::open(&d.0).unwrap().stats().sequence, 0);
}
fn no_data_metadata_commit_reuses_exact_directory_root_body() {
    let d = Dir::new(Path::new("/dev/shm"));
    let e = Environment::create_with_publication(&d.0, PublicationMode::Range).unwrap();
    fill(&e);
    let old = e.begin_read();
    let value = old.get(b"kv", &7u64.to_be_bytes()).unwrap().unwrap();
    drop(e);
    let controller = ulmdb_storage::ResidencyController::new(ResidencyPolicy::Demand).unwrap();
    let (mut store, state) =
        ulmdb_storage::range::RangeStore::open_checked(&d.0, &controller, |_, _| Ok(())).unwrap();
    let mut manifest = state.manifest.clone_metadata();
    manifest.previous = manifest.generation;
    manifest.parent = manifest.commit;
    manifest.generation = ulmdb_storage::new_identity().unwrap();
    manifest.commit = manifest.generation;
    manifest.sequence += 1;
    let arena =
        ulmdb_storage::Arena::new(state.segments.clone(), state.manifest.next_segment).unwrap();
    let result = store
        .commit_arena(arena, manifest, &state.manifest, &controller)
        .unwrap();
    let work = store.locator_work().unwrap();
    assert_eq!(
        (work.nodes_written, work.node_bytes, work.data_bytes),
        (0, 0, 0)
    );
    drop(result);
    drop(state);
    drop(store);
    let e = Environment::open(&d.0).unwrap();
    assert_eq!(e.stats().sequence, 2);
    assert_eq!(value, 7u64.to_le_bytes());
    ulmdb::backup::verify(&e).unwrap();
}
fn v6_managed_required_lock_pins_actual_mapping_until_old_reader_drops_body() {
    fn locked() -> u64 {
        fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find_map(|l| l.strip_prefix("VmLck:"))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            * 1024
    }
    let before = locked();
    let d = Dir::new(Path::new("/dev/shm"));
    let e = Environment::create_with_residency(
        &d.0,
        PublicationMode::Range,
        ResidencyPolicy::RequiredLocked { max_bytes: 1048576 },
    )
    .unwrap();
    fill(&e);
    let old = e.begin_read();
    let borrowed = old.get(b"kv", &7u64.to_be_bytes()).unwrap().unwrap();
    let report = old.residency_report();
    assert!(report.locked_bytes > 0);
    assert_eq!(report.locked_bytes, report.page_rounded_bytes.unwrap());
    assert!(locked() >= before + report.locked_bytes);
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", &7u64.to_be_bytes(), b"new").unwrap();
    w.commit().unwrap();
    drop(e);
    assert_eq!(borrowed, 7u64.to_le_bytes());
    assert!(locked() >= before + report.locked_bytes);
    drop(old);
    assert_eq!(locked(), before);
}
fn v6_managed_real_os_lock_denial_keeps_initial_head_and_reusable_file_body() {
    let d = Dir::new(Path::new("/dev/shm"));
    let status = fs::read_to_string("/proc/self/status").unwrap();
    let caps = u64::from_str_radix(
        status
            .lines()
            .find_map(|l| l.strip_prefix("CapEff:\t"))
            .unwrap(),
        16,
    )
    .unwrap();
    let mut command = if caps & (1 << 14) != 0 {
        let mut c = std::process::Command::new("setpriv");
        c.args(["--bounding-set=-ipc_lock", "--", "/bin/sh"]);
        c
    } else {
        std::process::Command::new("/bin/sh")
    };
    let output = command
        .args([
            "-c",
            "ulimit -l 0 || exit 99; exec \"$@\"",
            "range-zero-lock",
            env!("CARGO_BIN_EXE_ulmdb"),
            "--residency",
            "locked:1048576",
            "demo-range",
            d.0.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_ne!(output.status.code(), Some(99));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("Operation not permitted") || error.contains("Cannot allocate memory"),
        "{error}"
    );
    eprintln!("RANGE_LOCK_DENIAL {error}");
    let e = Environment::open(&d.0).unwrap();
    assert_eq!(e.stats().sequence, 0);
    assert_eq!(e.stats().databases, 0);
    assert_eq!(fs::metadata(d.0.join("RANGE.pack")).unwrap().len(), 327680);
    let mut w = e.begin_write().unwrap();
    w.create_database(b"after", DatabaseKind::Kv).unwrap();
    w.put(b"after", b"key", b"works").unwrap();
    w.commit().unwrap();
    assert_eq!(
        e.begin_read().get(b"after", b"key").unwrap(),
        Some(b"works".as_slice())
    );
}

// Every parent wrapper owns no Env. Each body runs in its own exec image, so
// a sibling test's fork cannot transiently inherit a writer OFD or change the
// process-wide VmLck baseline being asserted. No engine assertion is retried.
#[cfg(not(unix))]
fn isolated(_name: &str, body: fn()) {
    body();
}
#[cfg(unix)]
fn isolated(name: &str, body: fn()) {
    if std::env::var("ULMDB_RANGE_LOCATOR_CHILD").ok().as_deref() == Some(name) {
        body();
        println!("\nULMDB_RANGE_LOCATOR_COMPLETE {name}");
        return;
    }
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("ULMDB_RANGE_LOCATOR_CHILD", name)
        .process_group(0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let (output, timed_out) = finish_group(child, std::time::Duration::from_secs(120));
    assert!(!timed_out, "{name} child timed out: {output:?}");
    let marker = format!("ULMDB_RANGE_LOCATOR_COMPLETE {name}");
    assert!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line == marker),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // Preserve the existing work/resource evidence without forwarding nested
    // libtest summaries that would falsely change the parent's test tally.
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        if line.starts_with("RANGE_LOCATOR_WORK ") || line.starts_with("RANGE_LOCK_DENIAL ") {
            eprintln!("{line}");
        }
    }
}

// The CLI launched by an original body inherits this group. Kill every member
// on timeout, not just the direct test process. No engine operation is retried.
#[cfg(unix)]
fn finish_group(
    mut child: std::process::Child,
    limit: std::time::Duration,
) -> (std::process::Output, bool) {
    let deadline = std::time::Instant::now() + limit;
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            let group = format!("-{}", child.id());
            let killed = std::process::Command::new("/bin/kill")
                .args(["-KILL", "--", &group])
                .output();
            // Do not drain inherited stdout/stderr if the utility failed:
            // an un-killed descendant could still hold those pipe writers.
            match killed {
                Ok(killed) if killed.status.success() => {}
                failed => {
                    let _ = child.kill();
                    let reaped = child.wait();
                    panic!("process-group cleanup failed: {failed:?}; direct child={reaped:?}");
                }
            }
            return (child.wait_with_output().unwrap(), true);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    (child.wait_with_output().unwrap(), false)
}

#[test]
fn disk_fixed_change257_and_reopen() {
    isolated(
        "disk_fixed_change257_and_reopen",
        disk_fixed_change257_and_reopen_body,
    );
}

#[cfg(target_os = "linux")]
#[test]
fn tmpfs_fixed_change4097_and_reopen() {
    isolated(
        "tmpfs_fixed_change4097_and_reopen",
        tmpfs_fixed_change4097_and_reopen_body,
    );
}

#[test]
fn v5_read_and_write_are_preserved_without_upgrade() {
    isolated(
        "v5_read_and_write_are_preserved_without_upgrade",
        v5_read_and_write_are_preserved_without_upgrade_body,
    );
}

#[test]
fn v6_denied_candidate_is_known_nonpublication() {
    isolated(
        "v6_denied_candidate_is_known_nonpublication",
        v6_denied_candidate_is_known_nonpublication_body,
    );
}

#[test]
fn no_data_metadata_commit_reuses_exact_directory_root() {
    isolated(
        "no_data_metadata_commit_reuses_exact_directory_root",
        no_data_metadata_commit_reuses_exact_directory_root_body,
    );
}

#[cfg(target_os = "linux")]
#[test]
fn v6_managed_required_lock_pins_actual_mapping_until_old_reader_drops() {
    isolated(
        "v6_managed_required_lock_pins_actual_mapping_until_old_reader_drops",
        v6_managed_required_lock_pins_actual_mapping_until_old_reader_drops_body,
    );
}

#[cfg(target_os = "linux")]
#[test]
fn v6_managed_real_os_lock_denial_keeps_initial_head_and_reusable_file() {
    isolated(
        "v6_managed_real_os_lock_denial_keeps_initial_head_and_reusable_file",
        v6_managed_real_os_lock_denial_keeps_initial_head_and_reusable_file_body,
    );
}

#[cfg(target_os = "linux")]
#[test]
fn timeout_cleanup_kills_the_owned_process_group_and_nested_children() {
    const NAME: &str = "timeout_cleanup_kills_the_owned_process_group_and_nested_children";
    if let Some(path) = std::env::var_os("ULMDB_RANGE_TIMEOUT_PIDFILE") {
        let status = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                r#"sleep 60 & child=$!; printf '%s %s\n' "$$" "$child" > "$1"; wait"#,
                "range-timeout",
            ])
            .arg(path)
            .status()
            .unwrap();
        panic!("timeout control unexpectedly completed: {status}");
    }
    let d = Dir::new(&std::env::temp_dir());
    fs::create_dir(&d.0).unwrap();
    let pidfile = d.0.join("children");
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", NAME, "--nocapture"])
        .env("ULMDB_RANGE_TIMEOUT_PIDFILE", &pidfile)
        .process_group(0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let (output, timed_out) = finish_group(child, std::time::Duration::from_secs(3));
    assert!(timed_out, "timeout fixture exited early: {output:?}");
    let pids = fs::read_to_string(pidfile).unwrap();
    let pids: Vec<u32> = pids
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect();
    assert_eq!(pids.len(), 2);
    for pid in pids {
        // A killed orphan can await its external reaper as a zombie; it owns no
        // live FD/CPU. Require every nested process to be exited or dead, not running.
        let mut alive = true;
        for _ in 0..200 {
            alive = fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
                s.rsplit_once(") ")
                    .is_some_and(|(_, tail)| !tail.starts_with('Z') && !tail.starts_with('X'))
            });
            if !alive {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!alive, "nested child {pid} survived process-group cleanup");
    }
}
