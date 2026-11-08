#![cfg(target_os = "linux")]
use std::{fs, path::PathBuf};
use ulmdb::{
    Address, DatabaseKind, Environment, MaintenancePolicy, Prefix, PublicationMode, ReadTxn,
};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-auto-maint-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn modes() -> [PublicationMode; 3] {
    [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ]
}
#[test]
fn automatic_cleanup_resumes_bounded_name_scans_and_preserves_unknown_files() {
    for mode in modes() {
        let dir = Dir::new();
        let env = Environment::create_with_publication(&dir.0, mode).unwrap();
        seed(&env);
        // Unknown spellings consume discovery work but never become garbage.
        for n in 0..1000 {
            fs::write(
                dir.0.join("segments").join(format!("operator-{n:04}.keep")),
                b"keep",
            )
            .unwrap();
        }
        env.set_maintenance_policy(MaintenancePolicy::Bounded {
            segment_threshold: 1,
            rewrite_budget_bytes: 4096,
            reclaim_budget_files: 1,
        })
        .unwrap();
        let old = env.begin_read();
        for n in 1..=48u8 {
            let mut w = env.begin_write().unwrap();
            w.put(b"data", b"key", &[n; 24_000]).unwrap();
            w.commit().unwrap();
            let status = env.maintenance_status().unwrap();
            assert!(
                status.last_scanned_directory_entries <= 80,
                "{mode:?}: {status:?}"
            );
            assert_eq!(value(&old), vec![0; 24_000]);
        }
        let status = env.maintenance_status().unwrap();
        assert!(
            status.scanned_directory_entries >= 1000,
            "{mode:?}: {status:?}"
        );
        assert!(
            status.incomplete_directory_scans > 0,
            "{mode:?}: {status:?}"
        );
        assert!(status.reclaimed_files > 0, "{mode:?}: {status:?}");
        assert_eq!(status.last_reclaim_error, None);
        for n in 0..1000 {
            assert_eq!(
                fs::read(dir.0.join("segments").join(format!("operator-{n:04}.keep"))).unwrap(),
                b"keep"
            );
        }
        drop(old);
        assert_eq!(value(&env.begin_read()), vec![48; 24_000]);
        drop(env);
        assert_eq!(
            value(&Environment::open(&dir.0).unwrap().begin_read()),
            vec![48; 24_000]
        );
    }
}
fn policy(threshold: usize) -> MaintenancePolicy {
    MaintenancePolicy::Bounded {
        segment_threshold: threshold,
        rewrite_budget_bytes: 16 << 20,
        reclaim_budget_files: 16,
    }
}
fn disk(dir: &Dir) -> (usize, u64) {
    let mut n = 0;
    let mut bytes = 0;
    for sub in ["segments", "cohorts", "commits"] {
        if let Ok(entries) = fs::read_dir(dir.0.join(sub)) {
            for e in entries {
                let m = e.unwrap().metadata().unwrap();
                if m.is_file() {
                    n += 1;
                    bytes += m.len();
                }
            }
        }
    }
    (n, bytes)
}

// Local to the existing churn case: no production instrumentation or unsafe maps.
mod churn_resources {
    use std::{
        collections::BTreeMap,
        ffi::OsStr,
        fs::{self, File},
        io::Write,
        os::unix::{ffi::OsStrExt, fs::MetadataExt},
        path::Path,
        process::{Child, Command},
        time::{Duration, Instant},
    };
    use ulmdb_storage::{ReadStore, SegmentBuilder, WriteStore};

    const REWRITE_BYTES: u64 = 16 << 20;
    const PINNED_FILES: usize = 22;
    const PINNED_BYTES: u64 = 2_500_000;
    const RELEASED_FILES: usize = 18;
    const RELEASED_BYTES: u64 = 2_000_000;
    const ALLOCATION_SLACK: u64 = 64 << 10;
    // One writer arena, the policy's documented 4x output allocation allowance,
    // three small immutable snapshot scopes, and 16MiB codec/runtime slack.
    // Fixed before execution, not fitted to observed RSS. Includes all three
    // modes in one worker, so closing/reopening owners cannot reset the baseline.
    const MEMORY_GROWTH: u64 = 5 * REWRITE_BYTES + 3 * PINNED_BYTES + (16 << 20);

    pub fn isolated_parent() -> bool {
        const WORKER: &str = "ULMDB_AUTO_CHURN_RESOURCE_WORKER";
        if std::env::var_os(WORKER).as_deref() == Some(OsStr::new("1")) {
            return false;
        }
        struct Owned(Child);
        impl Drop for Owned {
            fn drop(&mut self) {
                if self.0.try_wait().ok().flatten().is_none() {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
        }
        let name = std::thread::current().name().unwrap().to_owned();
        // No other libtest case contributes RSS/HWM or inherited file mappings.
        // This exact worker does not spawn descendants.
        let mut child = Owned(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &name, "--nocapture", "--test-threads=1"])
                .env(WORKER, "1")
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "automatic maintenance resource worker: {status}"
                );
                return true;
            }
            assert!(
                Instant::now() < deadline,
                "automatic maintenance resource deadline"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct Memory {
        rss: u64,
        hwm: u64,
    }
    fn memory() -> Memory {
        let status = fs::read_to_string("/proc/self/status").unwrap();
        let field = |key: &str| {
            let mut words = status
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .unwrap()
                .split_whitespace();
            let kb: u64 = words.next().unwrap().parse().unwrap();
            assert_eq!(words.next(), Some("kB"));
            kb.checked_mul(1024).unwrap()
        };
        Memory {
            rss: field("VmRSS:"),
            hwm: field("VmHWM:"),
        }
    }
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Totals {
        apparent: u64,
        allocated: u64,
        inodes: usize,
        unlinked: usize,
    }
    #[derive(Clone, Copy, Debug, Default)]
    struct Disk {
        payload: Totals,
        history: Totals,
        open_fds: usize,
        mapped_ranges: usize,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Asset {
        length: u64,
        allocated: u64,
        history: bool,
        unlinked: bool,
    }
    type Key = (u64, u64);
    fn kind(directory: &Path, path: &Path) -> Option<bool> {
        let relative = path.strip_prefix(directory).ok()?;
        let mut components = relative.components();
        let parent = components.next()?.as_os_str();
        let name = components.next()?.as_os_str();
        if components.next().is_some()
            || !["segments", "cohorts", "commits"]
                .iter()
                .any(|s| parent == OsStr::new(s))
        {
            return None;
        }
        let bytes = name.as_bytes();
        let history = parent == OsStr::new("commits")
            && bytes.len() == 40
            && bytes[..32].iter().all(|b| b.is_ascii_hexdigit())
            && &bytes[32..] == b".history";
        Some(history)
    }
    fn insert(files: &mut BTreeMap<Key, Asset>, metadata: fs::Metadata, history: bool) {
        assert!(metadata.is_file());
        let item = Asset {
            length: metadata.len(),
            allocated: metadata.blocks().checked_mul(512).unwrap(),
            history,
            unlinked: metadata.nlink() == 0,
        };
        if let Some(old) = files.insert((metadata.dev(), metadata.ino()), item) {
            assert_eq!(
                old, item,
                "sealed asset changed at a stable resource barrier"
            );
        }
    }
    fn clean_path(bytes: &[u8]) -> (&Path, bool) {
        let deleted = bytes.ends_with(b" (deleted)");
        (
            Path::new(OsStr::from_bytes(
                bytes.strip_suffix(b" (deleted)").unwrap_or(bytes),
            )),
            deleted,
        )
    }
    fn disk(directory: &Path, known: &mut BTreeMap<Key, Asset>) -> Disk {
        let directory = directory.canonicalize().unwrap();
        let mut files = BTreeMap::new();
        for sub in ["segments", "cohorts", "commits"] {
            for entry in fs::read_dir(directory.join(sub)).unwrap() {
                let entry = entry.unwrap();
                let metadata = entry.metadata().unwrap();
                if metadata.is_file() {
                    insert(
                        &mut files,
                        metadata,
                        kind(&directory, &entry.path()).unwrap(),
                    );
                }
            }
        }
        let mut out = Disk::default();
        for entry in fs::read_dir("/proc/self/fd").unwrap() {
            let path = entry.unwrap().path();
            let target = match fs::read_link(&path) {
                Ok(target) => target,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => panic!("read owned FD: {e}"),
            };
            let (target, _) = clean_path(target.as_os_str().as_bytes());
            if let Some(history) = kind(&directory, target) {
                insert(&mut files, fs::metadata(path).unwrap(), history);
                out.open_fds += 1;
            }
        }
        // File::open is not retained by ordinary immutable mmap owners. Count
        // their deleted-only VMAs too; /proc/self/fd alone would miss them.
        for line in fs::read("/proc/self/maps").unwrap().split(|b| *b == b'\n') {
            let Some(start) = line.iter().position(|b| *b == b'/') else {
                continue;
            };
            let (path, deleted) = clean_path(&line[start..]);
            let Some(history) = kind(&directory, path) else {
                continue;
            };
            let text = std::str::from_utf8(&line[..start]).unwrap();
            let mut fields = text.split_ascii_whitespace();
            let range = fields.next().unwrap();
            fields.next().unwrap(); // permissions
            fields.next().unwrap(); // offset
            let (major, minor) = fields.next().unwrap().split_once(':').unwrap();
            let dev = libc::makedev(
                u32::from_str_radix(major, 16).unwrap(),
                u32::from_str_radix(minor, 16).unwrap(),
            );
            let key = (dev, fields.next().unwrap().parse().unwrap());
            if let Some(item) = files.get(&key) {
                assert_eq!(item.history, history);
                assert_eq!(item.unlinked, deleted);
            } else {
                // map_files stat may require CAP_CHECKPOINT_RESTORE even for
                // our own process. Previously observed sealed inode geometry
                // is exact after unlink: these committed files cannot change.
                // Unknown unlinked VMAs fail closed rather than undercount.
                let map_file = Path::new("/proc/self/map_files").join(range);
                if let Ok(metadata) = fs::metadata(map_file) {
                    assert_eq!((metadata.dev(), metadata.ino()), key);
                    insert(&mut files, metadata, history);
                } else {
                    assert!(
                        deleted,
                        "linked mapped asset missing from directory inventory"
                    );
                    let mut item = *known
                        .get(&key)
                        .expect("unobserved deleted mapped asset; cannot account allocated blocks");
                    assert_eq!(item.history, history);
                    item.unlinked = true;
                    files.insert(key, item);
                }
            }
            out.mapped_ranges += 1;
        }
        for (&key, &item) in &files {
            known.insert(key, item);
            let totals = if item.history {
                &mut out.history
            } else {
                &mut out.payload
            };
            totals.apparent = totals.apparent.checked_add(item.length).unwrap();
            totals.allocated = totals.allocated.checked_add(item.allocated).unwrap();
            totals.inodes += 1;
            totals.unlinked += usize::from(item.unlinked);
        }
        assert!(known.len() <= 4096, "bounded 72-commit observation ledger");
        out
    }

    pub struct Meter {
        base: Memory,
        known: BTreeMap<Key, Asset>,
        peak_payload: Totals,
        peak_history: Totals,
        peak_rss: u64,
        hwm: u64,
        samples: usize,
    }
    impl Meter {
        pub fn new(policy: super::MaintenancePolicy) -> Self {
            assert!(matches!(
                policy,
                super::MaintenancePolicy::Bounded {
                    rewrite_budget_bytes: REWRITE_BYTES,
                    ..
                }
            ));
            let base = memory();
            println!(
                "AUTO_RESOURCE_BUDGET rewrite={REWRITE_BYTES} pinned_payload_files={PINNED_FILES} pinned_payload_bytes={PINNED_BYTES} released_payload_files={RELEASED_FILES} released_payload_bytes_lt={RELEASED_BYTES} allocation_slack_per_inode={ALLOCATION_SLACK} rss_hwm_growth={MEMORY_GROWTH} base={base:?} history_policy=KeepAll-separately-reported"
            );
            Self {
                base,
                known: BTreeMap::new(),
                peak_payload: Totals::default(),
                peak_history: Totals::default(),
                peak_rss: base.rss,
                hwm: base.hwm,
                samples: 0,
            }
        }
        pub fn accounting_control(&self) {
            let probe = super::Dir::new();
            ulmdb_storage::create_layout(&probe.0).unwrap();
            let mut known = BTreeMap::new();
            let before = disk(&probe.0, &mut known);
            let path = probe.0.join("segments/accounting-control");
            let alias = probe.0.join("segments/accounting-hardlink");
            let mut file = File::create_new(&path).unwrap();
            file.write_all(&[0x71; 8193]).unwrap();
            let metadata = file.metadata().unwrap();
            let bytes = metadata.len();
            let allocated = metadata.blocks() * 512;
            assert!(allocated > 0);
            let linked = disk(&probe.0, &mut known);
            fs::hard_link(&path, &alias).unwrap();
            assert_eq!(
                disk(&probe.0, &mut known).payload,
                linked.payload,
                "hard links must not double count"
            );
            fs::remove_file(path).unwrap();
            fs::remove_file(alias).unwrap();
            let unlinked = disk(&probe.0, &mut known);
            assert_eq!(unlinked.payload.apparent, before.payload.apparent + bytes);
            assert_eq!(
                unlinked.payload.allocated,
                before.payload.allocated + allocated
            );
            assert_eq!(unlinked.payload.unlinked, 1);
            assert_eq!(unlinked.open_fds, 1);
            drop(file);
            assert_eq!(disk(&probe.0, &mut known).payload, before.payload);
            // Genuine safe sealed-storage mapping, not a raw mmap in a test.
            let mut builder = SegmentBuilder::new(&probe.0, 1).unwrap();
            let at = builder.allocate(&[0x57; 8193], 8).unwrap();
            let (table, _) = builder.finish().unwrap();
            assert_eq!(table.read(at, 8193).unwrap(), &[0x57; 8193]);
            let mapped = disk(&probe.0, &mut known);
            assert_eq!(mapped.payload.inodes, 1);
            assert_eq!(mapped.open_fds, 0);
            assert!(mapped.mapped_ranges > 0);
            let path = fs::read_dir(probe.0.join("segments"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            fs::remove_file(path).unwrap();
            let deleted_map = disk(&probe.0, &mut known);
            assert_eq!(deleted_map.payload.apparent, mapped.payload.apparent);
            assert_eq!(deleted_map.payload.allocated, mapped.payload.allocated);
            assert_eq!(deleted_map.payload.unlinked, 1);
            assert_eq!(deleted_map.open_fds, 0);
            assert!(deleted_map.mapped_ranges > 0);
            assert_eq!(table.read(at, 8193).unwrap(), &[0x57; 8193]);
            drop(table);
            let released = disk(&probe.0, &mut known);
            assert_eq!(released.payload, before.payload);
            assert_eq!(released.mapped_ranges, 0);
            println!(
                "AUTO_UNLINKED_ACCOUNTING open_fd_bytes={bytes} open_fd_allocated={allocated} mapped_only_bytes={} mapped_only_allocated={} inode_dedup=ok",
                mapped.payload.apparent, mapped.payload.allocated
            );
        }
        pub fn sample(
            &mut self,
            directory: &Path,
            mode: super::PublicationMode,
            phase: &str,
            pinned: bool,
        ) {
            if phase == "seed" {
                self.known.clear();
            }
            let d = disk(directory, &mut self.known);
            let m = memory();
            let (files, bytes) = if pinned {
                (PINNED_FILES, PINNED_BYTES)
            } else {
                (RELEASED_FILES, RELEASED_BYTES)
            };
            assert!(d.payload.inodes <= files, "{mode:?}/{phase}: {d:?}");
            assert!(
                if pinned {
                    d.payload.apparent <= bytes
                } else {
                    d.payload.apparent < bytes
                },
                "{mode:?}/{phase}: {d:?}"
            );
            assert!(
                d.payload.allocated <= bytes + files as u64 * ALLOCATION_SLACK,
                "{mode:?}/{phase}: {d:?}"
            );
            assert!(
                m.rss <= self.base.rss + MEMORY_GROWTH,
                "{mode:?}/{phase}: {m:?}"
            );
            assert!(
                m.hwm <= self.base.hwm + MEMORY_GROWTH,
                "{mode:?}/{phase}: {m:?}"
            );
            if phase == "owner-closed" {
                assert_eq!((d.open_fds, d.mapped_ranges), (0, 0), "{mode:?}: {d:?}");
            }
            self.peak_payload.apparent = self.peak_payload.apparent.max(d.payload.apparent);
            self.peak_payload.allocated = self.peak_payload.allocated.max(d.payload.allocated);
            self.peak_payload.inodes = self.peak_payload.inodes.max(d.payload.inodes);
            self.peak_payload.unlinked = self.peak_payload.unlinked.max(d.payload.unlinked);
            self.peak_history.apparent = self.peak_history.apparent.max(d.history.apparent);
            self.peak_history.allocated = self.peak_history.allocated.max(d.history.allocated);
            self.peak_history.inodes = self.peak_history.inodes.max(d.history.inodes);
            self.peak_history.unlinked = self.peak_history.unlinked.max(d.history.unlinked);
            self.peak_rss = self.peak_rss.max(m.rss);
            self.hwm = self.hwm.max(m.hwm);
            self.samples += 1;
            if matches!(
                phase,
                "seed" | "readers-released" | "owner-closed" | "reopened"
            ) {
                println!(
                    "AUTO_RESOURCES {mode:?}/{phase} payload={:?} KeepAll_history={:?} open_fds={} mapped_ranges={} memory={m:?} samples={} cumulative_peak_payload={:?} cumulative_peak_history={:?} sampled_peak_rss={} kernel_hwm={} rss_growth={} hwm_growth={} disk_scope=phase-boundary",
                    d.payload,
                    d.history,
                    d.open_fds,
                    d.mapped_ranges,
                    self.samples,
                    self.peak_payload,
                    self.peak_history,
                    self.peak_rss,
                    self.hwm,
                    self.peak_rss.saturating_sub(self.base.rss),
                    self.hwm.saturating_sub(self.base.hwm)
                );
            }
        }
    }
}

fn value(read: &ReadTxn) -> Vec<u8> {
    read.kv(b"data")
        .unwrap()
        .get(b"key")
        .unwrap()
        .unwrap()
        .to_vec()
}
fn seed(env: &Environment) {
    let mut w = env.begin_write().unwrap();
    w.create_database(b"data", DatabaseKind::Kv).unwrap();
    w.create_database(b"dups", DatabaseKind::Duplicates { width: Some(4) })
        .unwrap();
    w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
    w.put(b"data", b"key", &[0; 24_000]).unwrap();
    for n in 0..128u32 {
        w.put(b"data", &n.to_be_bytes(), &[0x76; 64]).unwrap();
        w.insert_duplicate(b"dups", b"k", &n.to_be_bytes()).unwrap();
    }
    w.insert_prefix(b"ip", Prefix::new(Address::V4(0), 0).unwrap(), b"default")
        .unwrap();
    assert_eq!(w.commit().unwrap(), 1);
}
#[test]
fn constant_size_churn_preserves_real_history_and_pins_then_bounds_files_without_manual_maintenance()
 {
    if churn_resources::isolated_parent() {
        return;
    }
    let mut resources = churn_resources::Meter::new(policy(4));
    resources.accounting_control();
    for mode in modes() {
        let dir = Dir::new();
        let env = Environment::create_with_publication(&dir.0, mode).unwrap();
        assert_eq!(
            env.maintenance_policy().unwrap(),
            MaintenancePolicy::AUTOMATIC
        );
        seed(&env);
        env.set_maintenance_policy(policy(4)).unwrap();
        assert_eq!(
            env.history_availability().unwrap().policy,
            ulmdb_storage::HistoryPolicy::KeepAll
        );
        let old = env.begin_read();
        // Independent admitted reader exercises the cross-owner physical pin.
        let peer = ReadTxn::open_snapshot(&dir.0).unwrap();
        let a = ulmdb_storage::read_current(&dir.0).unwrap();
        let old_kv = old.kv(b"data").unwrap();
        let old_borrow = old_kv.get(b"key").unwrap().unwrap();
        let peer_kv = peer.kv(b"data").unwrap();
        let peer_borrow = peer_kv.get(b"key").unwrap().unwrap();
        resources.sample(&dir.0, mode, "seed", true);
        let mut max_with_pin = (0, 0);
        for n in 1..=48u8 {
            let before = ulmdb_storage::read_current(&dir.0).unwrap();
            let mut w = env.begin_write().unwrap();
            w.put(b"data", b"key", &[n; 24_000]).unwrap();
            assert_eq!(w.commit().unwrap(), u64::from(n) + 1);
            let (current, previous) = ulmdb_storage::read_roots(&dir.0).unwrap();
            let previous = previous.unwrap();
            assert_eq!(current.previous, before.generation);
            assert_eq!(
                (previous.generation, previous.sequence, previous.commit),
                (before.generation, before.sequence, before.commit)
            );
            assert_eq!(current.parent, before.commit);
            assert_eq!(value(&old), vec![0; 24_000]);
            assert_eq!(value(&peer), vec![0; 24_000]);
            assert_eq!(old_borrow, &[0; 24_000]);
            assert_eq!(peer_borrow, &[0; 24_000]);
            assert_eq!(value(&env.begin_read()), vec![n; 24_000]);
            resources.sample(&dir.0, mode, "pinned-commit", true);
            let used = disk(&dir);
            max_with_pin.0 = max_with_pin.0.max(used.0);
            max_with_pin.1 = max_with_pin.1.max(used.1);
            assert!(used.0 <= 22, "{mode:?} files grew: {used:?}");
            assert!(used.1 <= 2_500_000, "{mode:?} bytes grew: {used:?}");
        }
        assert_eq!(a.sequence, old.stats().sequence);
        drop(old_kv);
        drop(peer_kv);
        drop(old);
        drop(peer);
        resources.sample(&dir.0, mode, "readers-released", true);
        for n in 49..=72u8 {
            let mut w = env.begin_write().unwrap();
            w.put(b"data", b"key", &[n; 24_000]).unwrap();
            w.commit().unwrap();
            resources.sample(&dir.0, mode, "released-commit", true);
        }
        resources.sample(&dir.0, mode, "settled", false);
        let bounded = disk(&dir);
        assert!(
            bounded.0 <= 18 && bounded.1 < 2_000_000,
            "{mode:?} {bounded:?}"
        );
        assert!(env.stats().segments <= 3);
        let status = env.maintenance_status().unwrap();
        assert!(
            status.rebuilt_candidates >= 18 && status.reclaimed_files > 60,
            "{mode:?} {status:?}"
        );
        assert_eq!(status.last_reclaim_error, None);
        println!(
            "{mode:?} pinned_max={max_with_pin:?} after_release={bounded:?} status={status:?}"
        );
        assert_eq!(
            env.begin_read()
                .duplicates(b"dups")
                .unwrap()
                .count(b"k")
                .unwrap(),
            128
        );
        assert_eq!(
            env.begin_read()
                .prefix_index(b"ip")
                .unwrap()
                .longest_match(Address::V4(17))
                .unwrap()
                .unwrap()
                .1,
            b"default"
        );
        drop(env);
        resources.sample(&dir.0, mode, "owner-closed", false);
        let reopened = Environment::open(&dir.0).unwrap();
        assert_eq!(value(&reopened.begin_read()), vec![72; 24_000]);
        resources.sample(&dir.0, mode, "reopened", false);
        assert_eq!(
            reopened.maintenance_policy().unwrap(),
            MaintenancePolicy::AUTOMATIC
        );
        drop(reopened);
        resources.sample(&dir.0, mode, "owner-closed", false);
    }
}
#[test]
fn capture_contention_defers_only_cleanup_and_budget_caps_deletion() {
    for mode in modes() {
        let dir = Dir::new();
        let env = Environment::create_with_publication(&dir.0, mode).unwrap();
        seed(&env);
        env.set_maintenance_policy(policy(2)).unwrap();
        let coordinator = ulmdb_storage::coordination::Coordination::open(
            &dir.0,
            ulmdb_storage::read_current(&dir.0).unwrap().uuid,
            Default::default(),
        )
        .unwrap();
        let capture = coordinator.try_capture().unwrap();
        for n in 1..=5u8 {
            let mut w = env.begin_write().unwrap();
            w.put(b"data", b"key", &[n; 24_000]).unwrap();
            w.commit().unwrap();
        }
        let blocked = env.maintenance_status().unwrap();
        assert_eq!(blocked.deferred_reclaims, 5);
        assert!(blocked.last_reclaim_error.is_some());
        drop(capture);
        env.set_maintenance_policy(MaintenancePolicy::Bounded {
            segment_threshold: 2,
            rewrite_budget_bytes: 16 << 20,
            reclaim_budget_files: 1,
        })
        .unwrap();
        let mut w = env.begin_write().unwrap();
        w.put(b"data", b"key", b"last").unwrap();
        w.commit().unwrap();
        assert_eq!(env.maintenance_status().unwrap().reclaimed_files, 1);
        assert_eq!(value(&env.begin_read()), b"last");
    }
}
#[test]
fn rebuild_defers_above_budget_and_noop_does_not_publish() {
    let dir = Dir::new();
    let env = Environment::create(&dir.0).unwrap();
    seed(&env);
    env.set_maintenance_policy(MaintenancePolicy::Bounded {
        segment_threshold: 1,
        rewrite_budget_bytes: 4096,
        reclaim_budget_files: 16,
    })
    .unwrap();
    let before = fs::read(dir.0.join("CURRENT")).unwrap();
    assert_eq!(env.begin_write().unwrap().commit().unwrap(), 1);
    assert_eq!(before, fs::read(dir.0.join("CURRENT")).unwrap());
    let mut w = env.begin_write().unwrap();
    w.put(b"data", b"key", b"ok").unwrap();
    assert_eq!(w.commit().unwrap(), 2);
    let status = env.maintenance_status().unwrap();
    assert_eq!(status.deferred_rebuilds, 1);
    assert_eq!(status.rebuilt_candidates, 0);
}
#[test]
fn builtin_order_and_nondefault_geometry_survive_streamed_rebuild() {
    use ulmdb::ordering::{DatabaseOrdering, OrderingSpec};
    for mode in modes() {
        let dir = Dir::new();
        let env =
            Environment::create_with_page_size(&dir.0, mode, ulmdb::PageSize::new(512).unwrap())
                .unwrap();
        env.set_maintenance_policy(policy(1)).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database_ordered(
            b"rev",
            DatabaseKind::Kv,
            DatabaseOrdering {
                keys: OrderingSpec::ReverseBytes,
                duplicates: OrderingSpec::Bytewise,
            },
        )
        .unwrap();
        for n in 0..300u32 {
            w.put(b"rev", &n.to_le_bytes(), &n.to_be_bytes()).unwrap();
        }
        w.commit().unwrap();
        assert_eq!(env.stats().page_size, 512);
        let r = env.begin_read();
        assert_eq!(
            r.kv(b"rev").unwrap().get(&29u32.to_le_bytes()).unwrap(),
            Some(29u32.to_be_bytes().as_slice())
        );
        drop(r);
        drop(env);
        let e = Environment::open(&dir.0).unwrap();
        assert_eq!(e.stats().page_size, 512);
        assert_eq!(
            e.begin_read()
                .kv(b"rev")
                .unwrap()
                .get(&299u32.to_le_bytes())
                .unwrap(),
            Some(299u32.to_be_bytes().as_slice())
        );
    }
}
#[test]
fn custom_order_and_transformed_files_survive_without_callback_during_rebuild() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use ulmdb::ordering::{DatabaseComparators, DatabaseOrdering, OrderingSpec};
    use ulmdb_storage::transform::{Checksum, ChecksumConfig, TransformContext};
    struct Sum;
    impl Checksum for Sum {
        fn compute(&self, src: &[u8], dst: &mut [u8], _: Option<&[u8]>) -> ulmdb::Result<()> {
            use sha2::{Digest, Sha256};
            dst.copy_from_slice(&Sha256::digest(src));
            Ok(())
        }
    }
    for mode in modes() {
        let dir = Dir::new();
        let context = Arc::new(
            TransformContext::new(
                Some(ChecksumConfig {
                    callback: Arc::new(Sum),
                    bytes: 32,
                }),
                None,
                64 << 20,
            )
            .unwrap(),
        );
        let env = Environment::create_with_transform(&dir.0, mode, context.clone()).unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        let cmp = DatabaseComparators {
            keys: Some(Arc::new(move |a, b| {
                c.fetch_add(1, Ordering::Relaxed);
                a.iter()
                    .map(u8::to_ascii_lowercase)
                    .cmp(b.iter().map(u8::to_ascii_lowercase))
            })),
            duplicates: None,
        };
        let mut w = env.begin_write().unwrap();
        w.create_database_ordered(
            b"custom",
            DatabaseKind::Kv,
            DatabaseOrdering {
                keys: OrderingSpec::Custom,
                duplicates: OrderingSpec::Bytewise,
            },
        )
        .unwrap();
        w.create_database(b"state", DatabaseKind::Kv).unwrap();
        for key in [b"a", b"B", b"c", b"D"] {
            w.put_with_ordering(b"custom", key, key, &cmp).unwrap();
        }
        w.commit().unwrap();
        env.set_maintenance_policy(policy(1)).unwrap();
        let old = env.begin_read();
        counter.store(0, Ordering::Relaxed);
        for n in 0..12u8 {
            let mut w = env.begin_write().unwrap();
            w.put(b"state", b"k", &[n; 4096]).unwrap();
            w.commit().unwrap();
        }
        assert_eq!(counter.load(Ordering::Relaxed), 0);
        assert_eq!(
            env.begin_read()
                .kv_with_ordering(b"custom", &cmp)
                .unwrap()
                .get(b"b")
                .unwrap(),
            Some(b"B".as_slice())
        );
        assert_eq!(
            old.kv_with_ordering(b"custom", &cmp)
                .unwrap()
                .get(b"d")
                .unwrap(),
            Some(b"D".as_slice())
        );
        assert_eq!(env.maintenance_status().unwrap().last_reclaim_error, None);
        assert!(disk(&dir).0 <= 10);
        drop(old);
        drop(env);
        let reopened = Environment::open_with_transform(&dir.0, context).unwrap();
        assert_eq!(
            reopened
                .begin_read()
                .kv_with_ordering(b"custom", &cmp)
                .unwrap()
                .get(b"b")
                .unwrap(),
            Some(b"B".as_slice())
        );
    }
}
#[test]
fn decode_quota_defers_rebuild_and_keeps_original_candidate_valid() {
    use std::sync::Arc;
    use ulmdb_storage::transform::{Checksum, ChecksumConfig, TransformContext};
    struct Sum;
    impl Checksum for Sum {
        fn compute(&self, src: &[u8], dst: &mut [u8], _: Option<&[u8]>) -> ulmdb::Result<()> {
            use sha2::{Digest, Sha256};
            dst.copy_from_slice(&Sha256::digest(src));
            Ok(())
        }
    }
    for mode in modes() {
        let dir = Dir::new();
        let context = Arc::new(
            TransformContext::new(
                Some(ChecksumConfig {
                    callback: Arc::new(Sum),
                    bytes: 32,
                }),
                None,
                128 << 10,
            )
            .unwrap(),
        );
        let env = Environment::create_with_transform(&dir.0, mode, context.clone()).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"data", DatabaseKind::Kv).unwrap();
        w.put(b"data", b"big", &[9; 64_000]).unwrap();
        w.commit().unwrap();
        let old = env.begin_read();
        env.set_maintenance_policy(policy(1)).unwrap();
        let mut w = env.begin_write().unwrap();
        w.put(b"data", b"k", b"small").unwrap();
        w.commit().unwrap();
        assert_eq!(env.maintenance_status().unwrap().deferred_rebuilds, 1);
        assert_eq!(env.maintenance_status().unwrap().rebuilt_candidates, 0);
        assert_eq!(
            old.kv(b"data").unwrap().get(b"big").unwrap().unwrap(),
            [9; 64_000]
        );
        assert_eq!(
            env.begin_read().kv(b"data").unwrap().get(b"k").unwrap(),
            Some(b"small".as_slice())
        );
        drop(old);
        drop(env);
        assert_eq!(context.decoded_bytes(), 0);
        let reopened = Environment::open_with_transform(&dir.0, context).unwrap();
        assert_eq!(
            reopened
                .begin_read()
                .kv(b"data")
                .unwrap()
                .get(b"k")
                .unwrap(),
            Some(b"small".as_slice())
        );
    }
}
#[test]
fn weak_heads_defer_reclaim_until_a_full_business_commit_repairs_debt() {
    use ulmdb::Durability;
    for mode in modes() {
        let dir = Dir::new();
        let env = Environment::create_with_publication(&dir.0, mode).unwrap();
        seed(&env);
        env.set_maintenance_policy(policy(1)).unwrap();
        env.set_durability(Durability::NoSync).unwrap();
        for n in 1..=4u8 {
            let mut w = env.begin_write().unwrap();
            w.put(b"data", b"key", &[n; 24_000]).unwrap();
            w.commit().unwrap();
        }
        let deferred = env.maintenance_status().unwrap();
        assert_eq!(deferred.deferred_reclaims, 4);
        assert_eq!(deferred.reclaimed_files, 0);
        env.set_durability(Durability::Full).unwrap();
        let mut w = env.begin_write().unwrap();
        w.put(b"data", b"key", b"durable").unwrap();
        w.commit().unwrap();
        let status = env.maintenance_status().unwrap();
        assert!(status.reclaimed_files > 0);
        assert_eq!(status.last_reclaim_error, None);
        drop(env);
        let e = Environment::open(&dir.0).unwrap();
        assert_eq!(value(&e.begin_read()), b"durable");
    }
}
#[cfg(feature = "fault-injection")]
#[test]
fn failed_rebuild_worker() {
    let Ok(which) = std::env::var("ULMDB_MAINT_WORKER_MODE") else {
        return;
    };
    let mode = match which.as_str() {
        "Legacy" => PublicationMode::Legacy,
        "Cohort" => PublicationMode::Cohort,
        _ => PublicationMode::Indexed,
    };
    let dir = Dir::new();
    let env = Environment::create_with_publication(&dir.0, mode).unwrap();
    seed(&env);
    env.set_maintenance_policy(policy(1)).unwrap();
    let before = fs::read(dir.0.join("CURRENT")).unwrap();
    let old = env.begin_read();
    let mut w = env.begin_write().unwrap();
    w.put(b"data", b"key", b"unpublished").unwrap();
    assert!(matches!(w.prepare(), Err(ulmdb::Error::Io(_))));
    assert_eq!(before, fs::read(dir.0.join("CURRENT")).unwrap());
    assert_eq!(value(&old), vec![0; 24_000]);
    assert_eq!(value(&env.begin_read()), vec![0; 24_000]);
    env.set_maintenance_policy(MaintenancePolicy::Disabled)
        .unwrap();
    let mut w = env.begin_write().unwrap();
    w.put(b"data", b"key", b"recovered").unwrap();
    assert_eq!(w.commit().unwrap(), 2);
    drop(old);
    drop(env);
    let e = Environment::open(&dir.0).unwrap();
    assert_eq!(value(&e.begin_read()), b"recovered");
}
#[cfg(feature = "fault-injection")]
#[test]
fn rebuild_io_failures_before_selection_preserve_head_and_release_writer() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for mode in modes() {
        for point in ["maintenance.before_rebuild", "maintenance.after_rebuild"] {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "failed_rebuild_worker", "--nocapture"])
                .env("ULMDB_MAINT_WORKER_MODE", format!("{mode:?}"))
                .env("ULMDB_FAIL_AT", point)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("maintenance worker deadline {mode:?}/{point}");
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            let output = child.wait_with_output().unwrap();
            assert!(
                status.success(),
                "{mode:?}/{point}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            println!("preserved head and writer progress {mode:?}/{point}");
        }
    }
}

#[test]
fn setting_policy_with_owned_writer_or_prepared_returns_busy() {
    let dir = Dir::new();
    let env = Environment::create(&dir.0).unwrap();
    let writer = env.begin_write().unwrap();
    assert!(matches!(
        env.set_maintenance_policy(policy(1)),
        Err(ulmdb::Error::Busy)
    ));
    let prepared = writer.prepare().unwrap();
    assert!(matches!(
        env.set_maintenance_policy(policy(1)),
        Err(ulmdb::Error::Busy)
    ));
    drop(prepared);
    env.set_maintenance_policy(policy(1)).unwrap();
}
