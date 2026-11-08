//! Real bounded-history Resident lifecycle, not timing measurements.
use std::{fs, path::PathBuf};
use ulmdb::{
    DatabaseKind, Error,
    resident::{Environment, Options, RedoEncoding, RedoFormat},
};
use ulmdb_storage::resident::Config;
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "resident-cp-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn options() -> Options {
    Options {
        pool: Config {
            kv_pages: 48,
            tiny_pages: 16,
            max_reserved_bytes: 8 << 20,
            max_metadata_bytes: 16 << 20,
            max_views: 64,
        },
        wal_capacity: 256 << 10,
        graph_read_budget: 1_000_000,
        auto_collect_below: 16,
    }
}
fn modes() -> [(RedoFormat, RedoEncoding); 4] {
    [
        (RedoFormat::LiteralV1, RedoEncoding::Owned),
        (RedoFormat::CompactV2, RedoEncoding::Owned),
        (RedoFormat::FullPostimageV2, RedoEncoding::Owned),
        (RedoFormat::CompactV2, RedoEncoding::Borrowed),
    ]
}
fn files(path: &std::path::Path) -> (usize, u64) {
    let mut count = 0;
    let mut bytes = 0;
    for e in fs::read_dir(path).unwrap() {
        let e = e.unwrap();
        if e.file_name().to_string_lossy().starts_with(".resident-") {
            count += 1;
            bytes += e.metadata().unwrap().len();
        }
    }
    (count, bytes)
}
#[cfg(target_os = "linux")]
mod linux_resources {
    use super::Options;
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
    pub fn isolated_parent() -> bool {
        if std::env::var_os("ULMDB_RESIDENT_RESOURCE_WORKER").as_deref() == Some(OsStr::new("1")) {
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
        // Re-exec only this exact existing test; parallel libtest workloads must
        // not become part of its process RSS or inherit its Resident resources.
        let name = std::thread::current().name().unwrap().to_owned();
        let mut child = Owned(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &name, "--nocapture", "--test-threads=1"])
                .env("ULMDB_RESIDENT_RESOURCE_WORKER", "1")
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success(), "Resident resource worker: {status}");
                return true;
            }
            assert!(
                Instant::now() < deadline,
                "Resident resource worker deadline"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[derive(Clone, Copy, Debug, Default)]
    struct Memory {
        rss: u64,
        hwm: u64,
    }
    fn memory() -> Memory {
        let status = fs::read_to_string("/proc/self/status").unwrap();
        let field = |key: &str| {
            let line = status
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .unwrap();
            let mut fields = line.split_whitespace();
            let number = fields.next().unwrap().parse::<u64>().unwrap();
            assert_eq!(fields.next(), Some("kB"));
            number.checked_mul(1024).unwrap()
        };
        Memory {
            rss: field("VmRSS:"),
            hwm: field("VmHWM:"),
        }
    }
    #[derive(Clone, Copy, Debug, Default)]
    struct Disk {
        apparent: u64,
        allocated: u64,
        inodes: usize,
        unlinked: usize,
        open_fds: usize,
    }
    fn asset(path: &Path) -> bool {
        path.file_name().is_some_and(|name| {
            let bytes = name.as_bytes();
            bytes == b"RESIDENT.wal" || bytes.starts_with(b".resident-")
        })
    }
    fn disk(directory: &Path) -> Disk {
        let directory = directory.canonicalize().unwrap();
        let mut files = BTreeMap::<(u64, u64), (u64, u64, bool)>::new();
        let mut insert = |m: fs::Metadata| {
            assert!(m.is_file());
            let value = (
                m.len(),
                m.blocks().checked_mul(512).unwrap(),
                m.nlink() == 0,
            );
            if let Some(old) = files.insert((m.dev(), m.ino()), value) {
                assert_eq!(old, value, "asset changed during stable sample");
            }
        };
        for entry in fs::read_dir(&directory).unwrap() {
            let entry = entry.unwrap();
            if asset(&entry.path()) {
                insert(entry.metadata().unwrap());
            }
        }
        let mut open_fds = 0;
        for entry in fs::read_dir("/proc/self/fd").unwrap() {
            let path = entry.unwrap().path();
            // Other runtime FDs may disappear; never omit a live owned asset
            // merely because its directory name has already been unlinked.
            let target = match fs::read_link(&path) {
                Ok(target) => target,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => panic!("read FD target: {e}"),
            };
            let bytes = target.as_os_str().as_bytes();
            let bytes = bytes.strip_suffix(b" (deleted)").unwrap_or(bytes);
            let target = Path::new(OsStr::from_bytes(bytes));
            if target.parent() == Some(directory.as_path()) && asset(target) {
                let metadata = fs::metadata(path).unwrap();
                insert(metadata);
                open_fds += 1;
            }
        }
        let mut out = Disk {
            inodes: files.len(),
            open_fds,
            ..Default::default()
        };
        for (length, allocated, unlinked) in files.into_values() {
            out.apparent = out.apparent.checked_add(length).unwrap();
            out.allocated = out.allocated.checked_add(allocated).unwrap();
            out.unlinked += usize::from(unlinked);
        }
        out
    }
    pub struct Meter {
        base: Memory,
        apparent_limit: u64,
        allocated_limit: u64,
        memory_growth_limit: u64,
        peak: Disk,
        peak_rss: u64,
        hwm: u64,
        samples: usize,
    }
    impl Meter {
        pub fn new(o: Options) -> Self {
            // Predeclared small-profile envelope, never inferred from results:
            // 64 slots, each with a 16-byte floor plus at most a 4096-byte page
            // and 1024-byte range metadata, and a 256-byte checkpoint header.
            // Allow current plus one retained generation during rotation/reopen.
            let slots =
                u64::try_from(o.pool.kv_pages).unwrap() + u64::try_from(o.pool.tiny_pages).unwrap();
            assert_eq!(slots, 64);
            assert_eq!(o.wal_capacity, 256 << 10);
            let checkpoint = 256 + slots * (16 + 4096 + 1024);
            let apparent_limit = 2 * (o.wal_capacity + checkpoint);
            // Four possible files each get 64KiB filesystem allocation slack.
            // This is an explicit tested-host envelope, not universal packing.
            let allocated_limit = apparent_limit + 4 * (64 << 10);
            // Held reader plus reopened owner, each with the configured pool
            // and metadata admission budgets, plus 16MiB codec/runtime slack.
            let memory_growth_limit = 2
                * (o.pool.max_reserved_bytes as u64 + o.pool.max_metadata_bytes as u64)
                + (16 << 20);
            assert_eq!(memory_growth_limit, 64 << 20);
            let base = memory();
            println!(
                "RESIDENT_RESOURCE_BUDGET apparent={apparent_limit} allocated={allocated_limit} rss_growth={memory_growth_limit} base_rss={} base_hwm={}",
                base.rss, base.hwm
            );
            Self {
                base,
                apparent_limit,
                allocated_limit,
                memory_growth_limit,
                peak: Disk::default(),
                peak_rss: base.rss,
                hwm: base.hwm,
                samples: 0,
            }
        }
        pub fn accounting_control(&self, path: &Path) {
            let before = disk(path);
            let probe = path.join(".resident-checkpoint-accounting");
            let mut held = File::create_new(&probe).unwrap();
            held.write_all(&[0xa7; 8193]).unwrap();
            let metadata = held.metadata().unwrap();
            let length = metadata.len();
            let allocated = metadata.blocks() * 512;
            assert!(allocated > 0, "probe must own real allocated blocks");
            fs::remove_file(probe).unwrap();
            let deleted = disk(path);
            assert_eq!(deleted.apparent, before.apparent + length);
            assert_eq!(deleted.allocated, before.allocated + allocated);
            assert_eq!(deleted.unlinked, before.unlinked + 1);
            assert_eq!(deleted.inodes, before.inodes + 1);
            drop(held);
            let after = disk(path);
            assert_eq!(after.apparent, before.apparent);
            assert_eq!(after.allocated, before.allocated);
            println!("RESIDENT_UNLINKED_ACCOUNTING bytes={length} allocated={allocated}");
        }
        pub fn sample(&mut self, path: &Path, phase: &str) {
            let d = disk(path);
            let m = memory();
            assert!(d.apparent <= self.apparent_limit, "{phase}: {d:?}");
            assert!(d.allocated <= self.allocated_limit, "{phase}: {d:?}");
            assert!(
                m.rss <= self.base.rss + self.memory_growth_limit,
                "{phase}: {m:?}"
            );
            assert!(
                m.hwm <= self.base.hwm + self.memory_growth_limit,
                "{phase}: {m:?}"
            );
            self.peak.apparent = self.peak.apparent.max(d.apparent);
            self.peak.allocated = self.peak.allocated.max(d.allocated);
            self.peak.unlinked = self.peak.unlinked.max(d.unlinked);
            self.peak.inodes = self.peak.inodes.max(d.inodes);
            self.peak.open_fds = self.peak.open_fds.max(d.open_fds);
            self.peak_rss = self.peak_rss.max(m.rss);
            self.hwm = self.hwm.max(m.hwm);
            self.samples += 1;
            if phase == "owner-closed" {
                assert_eq!(d.open_fds, 0, "{d:?}");
            }
            println_if_final(phase, d, m);
        }
        pub fn report(&self, format: super::RedoFormat, encoding: super::RedoEncoding) {
            println!(
                "RESIDENT_RESOURCES cumulative_through_mode={format:?}/{encoding:?} samples={} peak_apparent={} peak_allocated={} peak_owned_inodes={} peak_unlinked={} peak_owned_fds={} sampled_peak_rss={} max_kernel_hwm={} rss_growth={} hwm_growth={} disk_scope=phase-boundary",
                self.samples,
                self.peak.apparent,
                self.peak.allocated,
                self.peak.inodes,
                self.peak.unlinked,
                self.peak.open_fds,
                self.peak_rss,
                self.hwm,
                self.peak_rss.saturating_sub(self.base.rss),
                self.hwm.saturating_sub(self.base.hwm)
            );
        }
    }
    fn println_if_final(phase: &str, d: Disk, m: Memory) {
        if matches!(phase, "reader-released" | "owner-closed") {
            println!(
                "RESIDENT_RELEASE phase={phase} apparent={} allocated={} owned_fds={} unlinked={} rss={} hwm={}",
                d.apparent, d.allocated, d.open_fds, d.unlinked, m.rss, m.hwm
            );
        }
    }
}

#[test]
fn automatic_rotation_survives_ten_wal_capacities_and_old_borrows() {
    #[cfg(target_os = "linux")]
    if linux_resources::isolated_parent() {
        return;
    }
    #[cfg(target_os = "linux")]
    let mut resources = linux_resources::Meter::new(options());
    for (format, encoding) in modes() {
        let d = Dir::new();
        let o = options();
        let mut env = Environment::create_with_encoding(&d.0, o, format, encoding).unwrap();
        {
            let mut w = env.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            w.put(b"kv", b"key", b"old").unwrap();
            assert_eq!(w.commit().unwrap(), 1);
        }
        let old = env.begin_read();
        let borrowed = old.get(b"kv", b"key").unwrap().unwrap();
        let identity = old.commit_identity();
        #[cfg(target_os = "linux")]
        {
            resources.accounting_control(&d.0);
            resources.sample(&d.0, "seed");
        }
        let mut appended = 0;
        let mut rotations = 0;
        let mut last_lsn = env.physical_lsn();
        let steps = if format == RedoFormat::CompactV2 {
            6400
        } else {
            640
        };
        for i in 2u64..=steps {
            let before = env.wal_bytes();
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"key", &i.to_le_bytes()).unwrap();
            if i % 3 == 0 {
                w.put(b"kv", b"extra", b"present").unwrap();
            } else {
                w.delete(b"kv", b"extra").unwrap();
            }
            assert_eq!(w.commit().unwrap(), i);
            let after = env.wal_bytes();
            if after < before {
                rotations += 1;
                appended += after - 196608;
            } else {
                appended += after - before;
            }
            assert_eq!(
                env.begin_read().get(b"kv", b"key").unwrap(),
                Some(i.to_le_bytes().as_slice())
            );
            assert_eq!(borrowed, b"old");
            assert_eq!(old.commit_identity(), identity);
            assert!(env.physical_lsn() > last_lsn);
            last_lsn = env.physical_lsn();
            let (count, bytes) = files(&d.0);
            assert!(count <= 1, "{count}");
            assert!(bytes <= 64 * 5136 + 256);
            #[cfg(target_os = "linux")]
            resources.sample(&d.0, "commit");
            if i % 80 == 0 {
                drop(env);
                env = Environment::open_with_encoding(&d.0, o, format, encoding).unwrap();
                #[cfg(target_os = "linux")]
                resources.sample(&d.0, "reopen");
                assert_eq!(env.sequence(), i);
                assert_eq!(
                    env.begin_read().get(b"kv", b"key").unwrap(),
                    Some(i.to_le_bytes().as_slice())
                );
            }
        }
        assert!(
            appended > 10 * o.wal_capacity,
            "appended={appended}, format={format:?}"
        );
        assert!(rotations > 10);
        assert_eq!(env.pool_stats().unwrap().maps_created, 64);
        drop(old);
        env.collect().unwrap();
        let cp = env.checkpoint().unwrap();
        assert_eq!(cp.sequence, steps);
        assert_eq!(cp.tail_bytes_after, 196608);
        assert_eq!(
            env.begin_read().get(b"kv", b"key").unwrap(),
            Some(steps.to_le_bytes().as_slice())
        );
        #[cfg(target_os = "linux")]
        resources.sample(&d.0, "reader-released");
        drop(env);
        #[cfg(target_os = "linux")]
        {
            resources.sample(&d.0, "owner-closed");
            resources.report(format, encoding);
        }
    }
}
#[test]
fn empty_checkpoint_real_growth_and_corruption_keep_exact_identity() {
    for (format, encoding) in modes() {
        let d = Dir::new();
        let mut o = options();
        let env = Environment::create_with_encoding(&d.0, o, format, encoding).unwrap();
        let cp = env.checkpoint().unwrap();
        assert_eq!(cp.sequence, 0);
        assert_eq!(cp.physical_lsn, 1);
        let held = env.begin_write().unwrap();
        assert!(matches!(env.checkpoint(), Err(Error::Busy)));
        assert!(matches!(env.grow_wal(512 << 10), Err(Error::Busy)));
        drop(held);
        let cp = env.grow_wal(512 << 10).unwrap();
        assert_eq!(cp.sequence, 0);
        assert_eq!(env.wal_capacity(), 512 << 10);
        assert_eq!(
            fs::metadata(d.0.join("RESIDENT.wal")).unwrap().len(),
            512 << 10
        );
        o.wal_capacity = 512 << 10;
        {
            let mut w = env.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            w.put(b"kv", b"k", b"v").unwrap();
            assert_eq!(w.commit().unwrap(), 1);
        }
        let identity = env.begin_read().commit_identity();
        env.checkpoint().unwrap();
        drop(env);
        let env = Environment::open_with_encoding(&d.0, o, format, encoding).unwrap();
        assert_eq!(env.sequence(), 1);
        assert_eq!(env.begin_read().commit_identity(), identity);
        drop(env);
        let checkpoint = fs::read_dir(&d.0)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".resident-checkpoint-")
            })
            .unwrap();
        let selected = fs::read(d.0.join("RESIDENT.wal")).unwrap();
        let mut damaged = fs::read(&checkpoint).unwrap();
        let n = damaged.len();
        damaged[n - 1] ^= 1;
        fs::write(&checkpoint, &damaged).unwrap();
        assert!(matches!(
            Environment::open_with_encoding(&d.0, o, format, encoding),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(fs::read(d.0.join("RESIDENT.wal")).unwrap(), selected);
    }
}
#[test]
fn prefix_tiny_pages_recycle_after_checkpoint_but_old_borrow_stays_owned() {
    use ulmdb::{Address, Prefix};
    for (format, encoding) in modes() {
        let d = Dir::new();
        let mut o = options();
        o.pool.tiny_pages = 8;
        let mut env = Environment::create_with_encoding(&d.0, o, format, encoding).unwrap();
        let prefix = Prefix::new(Address::V4(0), 0).unwrap();
        {
            let mut w = env.begin_write().unwrap();
            w.create_database(b"ip", DatabaseKind::Ipv4).unwrap();
            w.insert_prefix(b"ip", prefix, b"old").unwrap();
            assert_eq!(w.commit().unwrap(), 1);
        }
        let old = env.begin_read();
        let borrowed = old.longest_match(b"ip", Address::V4(1)).unwrap().unwrap().1;
        for n in 2u64..=240 {
            let mut w = env.begin_write().unwrap();
            w.insert_prefix(b"ip", prefix, &n.to_le_bytes()).unwrap();
            assert_eq!(w.commit().unwrap(), n);
            assert_eq!(
                env.begin_read()
                    .longest_match(b"ip", Address::V4(1))
                    .unwrap()
                    .unwrap()
                    .1,
                n.to_le_bytes()
            );
            assert_eq!(borrowed, b"old");
            if n % 40 == 0 {
                drop(env);
                env = Environment::open_with_encoding(&d.0, o, format, encoding).unwrap();
            }
        }
        drop(old);
        env.collect().unwrap();
        env.checkpoint().unwrap();
        assert_eq!(env.pool_stats().unwrap().maps_created, 56);
        assert!(env.pool_stats().unwrap().unused_tiny_pages >= 6);
    }
}
#[test]
fn checkpoint_rebinds_incremental_certificate_without_changing_private_source() {
    use ulmdb::resident::IncrementalLimits;
    for encoding in [RedoEncoding::Owned, RedoEncoding::Borrowed] {
        let d = Dir::new();
        let env =
            Environment::create_with_encoding(&d.0, options(), RedoFormat::CompactV2, encoding)
                .unwrap();
        {
            let mut w = env.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            w.put(b"kv", b"k", b"old").unwrap();
            w.commit().unwrap();
        }
        env.enable_incremental(IncrementalLimits::default())
            .unwrap();
        let old = env.begin_read();
        let held = old.get(b"kv", b"k").unwrap().unwrap();
        for i in 2u64..=96 {
            if i % 8 == 0 {
                assert_eq!(env.checkpoint().unwrap().sequence, i - 1);
            }
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", b"k", &i.to_le_bytes()).unwrap();
            assert_eq!(w.commit().unwrap(), i);
            assert!(env.incremental_metrics().unwrap().is_some());
            assert_eq!(held, b"old");
        }
    }
}
#[test]
fn exhausted_tail_can_grow_and_retry_without_a_fake_commit() {
    for (format, encoding) in modes() {
        let d = Dir::new();
        let mut o = options();
        o.wal_capacity = 196608;
        let env = Environment::create_with_encoding(&d.0, o, format, encoding).unwrap();
        {
            let mut w = env.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            w.put(b"kv", b"k", b"v").unwrap();
            assert!(matches!(w.commit(), Err(Error::LimitExceeded)));
        }
        assert_eq!(env.sequence(), 0);
        assert_eq!(env.begin_read().database_count(), 0);
        env.grow_wal(512 << 10).unwrap();
        o.wal_capacity = 512 << 10;
        {
            let mut w = env.begin_write().unwrap();
            w.create_database(b"kv", DatabaseKind::Kv).unwrap();
            w.put(b"kv", b"k", b"v").unwrap();
            assert_eq!(w.commit().unwrap(), 1);
        }
        drop(env);
        let env = Environment::open_with_encoding(&d.0, o, format, encoding).unwrap();
        assert_eq!(env.sequence(), 1);
        assert_eq!(
            env.begin_read().get(b"kv", b"k").unwrap(),
            Some(b"v".as_slice())
        );
    }
}
