//! Actual file/OFD/process journeys for the opt-in Linux storage primitive.
#![cfg(target_os = "linux")]
#![allow(unsafe_code)]
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use ulmdb_storage::prepare_publication;
use ulmdb_storage::{
    Arena, Error, Manifest, SegmentTable, WriteStore, checkpoint_bytes,
    coordination::{Coordination, Limits},
    create_layout, new_identity, publish, read_current,
};

struct Fixture {
    path: PathBuf,
    manifest: Manifest,
}
impl Fixture {
    fn new(inventory: bool) -> Self {
        Self::with_format(inventory, 2)
    }
    fn with_format(inventory: bool, format: u16) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-coordination-{:032x}",
            new_identity().unwrap()
        ));
        create_layout(&path).unwrap();
        let mut manifest = Manifest::empty().unwrap();
        if inventory {
            let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
            arena
                .allocate(b"actual persisted segment contents", 8)
                .unwrap();
            let (table, segment) = arena.seal(&path).unwrap();
            assert!(segment.is_some());
            manifest.segments = table.entries().into();
            manifest.next_segment = 2;
        }
        manifest.format_minor = format;
        if format == 4 {
            manifest.prepare_indexed(None).unwrap();
        }
        publish(&path, &manifest).unwrap();
        Self { path, manifest }
    }
    fn coordination(&self, limits: Limits) -> Coordination {
        Coordination::open(&self.path, self.manifest.uuid, limits).unwrap()
    }
    fn pins(&self) -> PathBuf {
        self.path.join(".ulmdb-coordination/pins")
    }
    fn checkpoint(&self) -> [u8; 64] {
        checkpoint_bytes(&self.manifest).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn stale_path(fixture: &Fixture) -> PathBuf {
    fixture
        .pins()
        .join(format!("pin-{:032x}.lease", new_identity().unwrap()))
}

#[test]
fn live_root_has_exact_persisted_binding_and_survives_capture_release() {
    let fixture = Fixture::new(true);
    let coordinator = fixture.coordination(Limits::default());
    let capture = coordinator.try_capture().unwrap();
    let pin = capture.pin_checkpoint(&fixture.checkpoint()).unwrap();
    assert!(pin.root().matches_manifest(&fixture.manifest).unwrap());
    assert!(
        pin.matches_environment(&fixture.path, &fixture.manifest)
            .unwrap()
    );
    let copied_directory = Fixture::new(false);
    assert!(
        !pin.matches_environment(&copied_directory.path, &fixture.manifest)
            .unwrap()
    );
    let mut changed = fixture.manifest.clone();
    changed.sequence += 1;
    assert!(!pin.root().matches_manifest(&changed).unwrap());
    assert!(
        capture
            .pin_checkpoint(&checkpoint_bytes(&changed).unwrap())
            .is_err()
    );
    assert!(matches!(coordinator.try_maintenance(), Err(Error::Busy)));
    drop(capture);
    let mut maintenance = coordinator.try_maintenance().unwrap();
    assert!(matches!(coordinator.try_capture(), Err(Error::Busy)));
    let scan = maintenance.scan().unwrap();
    assert_eq!(scan.live_roots().len(), 1);
    assert_eq!(scan.live_roots()[0].checkpoint(), &fixture.checkpoint());
    assert_eq!(scan.live_roots()[0].manifest().segments.len(), 1);
    assert_eq!(scan.stale_records(), 0);
    assert_eq!(scan.cleanup_stale().unwrap(), 0);
    drop(pin);
    let scan = maintenance.scan().unwrap();
    assert!(scan.live_roots().is_empty());
    assert_eq!(scan.stale_records(), 0);
}

#[test]
fn legacy_cohort_and_indexed_descriptors_keep_their_exact_typed_checkpoint() {
    for format in [2, 3, 4] {
        let fixture = Fixture::with_format(true, format);
        let coordinator = fixture.coordination(Limits::default());
        let capture = coordinator.try_capture().unwrap();
        let pin = capture.pin_checkpoint(&fixture.checkpoint()).unwrap();
        assert_eq!(pin.root().manifest().format_minor, format);
        assert!(
            pin.matches_environment(&fixture.path, &fixture.manifest)
                .unwrap()
        );
        drop(capture);
        let mut maintenance = coordinator.try_maintenance().unwrap();
        assert_eq!(
            maintenance.scan().unwrap().live_roots()[0].checkpoint(),
            &fixture.checkpoint()
        );
    }
}

#[test]
fn directory_gate_covers_realm_lookup_and_binding_without_unlock_gap() {
    let fixture = Fixture::new(false);
    let known = fixture.coordination(Limits::default());
    let unbound = Coordination::capture_directory(&fixture.path, Limits::default()).unwrap();
    assert!(matches!(known.try_maintenance(), Err(Error::Busy)));
    let manifest = read_current(&fixture.path).unwrap();
    let (bound, capture) = unbound.bind(manifest.uuid).unwrap();
    assert!(matches!(bound.try_maintenance(), Err(Error::Busy)));
    let pin = capture
        .pin_checkpoint(&checkpoint_bytes(&manifest).unwrap())
        .unwrap();
    assert!(pin.coordination().try_maintenance().is_err());
    assert!(capture.coordination().try_maintenance().is_err());
    drop(capture);
    assert_eq!(
        bound
            .try_maintenance()
            .unwrap()
            .scan()
            .unwrap()
            .live_roots()
            .len(),
        1
    );
    let invalid = Coordination::capture_directory(&fixture.path, Limits::default()).unwrap();
    assert!(matches!(invalid.bind([0; 16]), Err(Error::InvalidInput(_))));
    assert!(known.try_maintenance().is_ok());
}

#[test]
fn prepared_pins_bind_real_tokens_without_cold_inventory_decoding() {
    for format in [2, 3, 4] {
        let fixture = Fixture::with_format(true, format);
        let before = fs::read(fixture.path.join("CURRENT")).unwrap();
        let mut old = read_current(&fixture.path).unwrap();
        let mut next = old.clone();
        next.previous = old.generation;
        next.generation = new_identity().unwrap();
        next.sequence = old.sequence + 1;
        next.parent = old.commit;
        next.commit = next.generation;
        let publication = if format == 4 {
            // Use the real admitted incremental arena path, whose new inventory
            // is lazy. Cold prepare_indexed is allowed to materialize its input.
            let table = SegmentTable::load(&fixture.path, &old.segments).unwrap();
            old.certify_indexed_table(&table).unwrap();
            let mut arena = Arena::new(table, old.next_segment).unwrap();
            arena.allocate(b"new prepared physical data", 8).unwrap();
            arena
                .seal_indexed(&fixture.path, &mut next, &old)
                .unwrap()
                .1
        } else {
            prepare_publication(&fixture.path, &next).unwrap()
        };
        let coordinator = fixture.coordination(Limits::default());
        let capture = coordinator.try_capture().unwrap();
        if format == 4 {
            assert!(!next.segments.is_materialized());
        }
        let pin = capture.pin_prepared(&publication, &next).unwrap();
        if format == 4 {
            assert!(!next.segments.is_materialized());
            assert!(!pin.root().manifest().segments.is_materialized());
        }
        assert!(pin.matches_environment(&fixture.path, &next).unwrap());
        let mut changed = next.clone();
        changed.sequence += 1;
        assert!(capture.pin_prepared(&publication, &changed).is_err());
        assert!(capture.pin_prepared(&publication, &old).is_err());
        let other = Fixture::new(false);
        let foreign = Coordination::open(&other.path, next.uuid, Limits::default()).unwrap();
        assert!(
            foreign
                .try_capture()
                .unwrap()
                .pin_prepared(&publication, &next)
                .is_err()
        );
        assert_eq!(fs::read(fixture.path.join("CURRENT")).unwrap(), before);
        // Isolated no-cold-IO control: hide only the unselected descriptor while
        // capture and the publication owner remain held, then restore it before
        // releasing capture or publishing. Cold registration must still fail.
        let descriptor = ulmdb_storage::descriptor_path(&fixture.path, &next);
        let hidden = fixture.path.join("unselected-descriptor-control");
        fs::rename(&descriptor, &hidden).unwrap();
        let no_cold_read = capture.pin_prepared(&publication, &next).unwrap();
        assert!(
            capture
                .pin_checkpoint(&checkpoint_bytes(&next).unwrap())
                .is_err()
        );
        fs::rename(&hidden, &descriptor).unwrap();
        assert_eq!(fs::read(fixture.path.join("CURRENT")).unwrap(), before);
        drop(no_cold_read);
        drop(capture);
        publication.publish().unwrap();
        assert_eq!(
            read_current(&fixture.path).unwrap().generation,
            next.generation
        );
        let mut maintenance = coordinator.try_maintenance().unwrap();
        assert_eq!(
            maintenance.scan().unwrap().live_roots()[0].checkpoint(),
            pin.root().checkpoint()
        );
    }
}

#[test]
fn foreign_realm_or_directory_record_cannot_mark_a_root() {
    let fixture = Fixture::new(false);
    let coordinator = fixture.coordination(Limits::default());
    let foreign = Coordination::open(&fixture.path, [7; 16], Limits::default()).unwrap();
    assert!(matches!(
        foreign
            .try_capture()
            .unwrap()
            .pin_checkpoint(&fixture.checkpoint()),
        Err(Error::Corrupt(_))
    ));
    let capture = coordinator.try_capture().unwrap();
    let pin = capture.pin_checkpoint(&fixture.checkpoint()).unwrap();
    let second = Fixture::new(false);
    let other = second.coordination(Limits::default());
    let copied = second.pins().join(pin.record_path().file_name().unwrap());
    fs::copy(pin.record_path(), &copied).unwrap();
    let lease = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&copied)
        .unwrap();
    lease.try_lock().unwrap();
    let mut maintenance = other.try_maintenance().unwrap();
    assert!(matches!(maintenance.scan(), Err(Error::Corrupt(_))));
    assert!(copied.exists());
    drop(lease);
    assert_eq!(maintenance.scan().unwrap().cleanup_stale().unwrap(), 1);
}

#[test]
fn corrupt_live_pin_blocks_all_cleanup_but_corrupt_stale_pins_are_cleanable() {
    let fixture = Fixture::new(false);
    let coordinator = fixture.coordination(Limits::default());
    let capture = coordinator.try_capture().unwrap();
    let pin = capture.pin_checkpoint(&fixture.checkpoint()).unwrap();
    let stale = stale_path(&fixture);
    fs::write(&stale, b"interrupted malformed record").unwrap();
    let oversized = stale_path(&fixture);
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&oversized)
        .unwrap();
    file.set_len(1 << 30).unwrap();
    let corrupt = OpenOptions::new()
        .write(true)
        .open(pin.record_path())
        .unwrap();
    corrupt.set_len(1 << 30).unwrap();
    drop(capture);
    let mut maintenance = coordinator.try_maintenance().unwrap();
    assert!(matches!(maintenance.scan(), Err(Error::Corrupt(_))));
    assert!(
        stale.exists() && oversized.exists(),
        "nothing unlinked before every live record validates"
    );
    drop(pin);
    let scan = maintenance.scan().unwrap();
    assert!(scan.live_roots().is_empty());
    assert_eq!(scan.stale_records(), 2);
    assert_eq!(scan.cleanup_stale().unwrap(), 2);
    assert!(!stale.exists() && !oversized.exists());
    assert_eq!(
        read_current(&fixture.path).unwrap().generation,
        fixture.manifest.generation
    );
}

#[test]
fn symlink_nonregular_and_unknown_records_fail_closed() {
    let fixture = Fixture::new(false);
    let coordinator = fixture.coordination(Limits::default());
    let before = fs::read(fixture.path.join("CURRENT")).unwrap();
    let symlink_path = stale_path(&fixture);
    symlink(fixture.path.join("CURRENT"), &symlink_path).unwrap();
    let mut maintenance = coordinator.try_maintenance().unwrap();
    assert!(maintenance.scan().is_err());
    fs::remove_file(&symlink_path).unwrap();
    let directory = stale_path(&fixture);
    fs::create_dir(&directory).unwrap();
    assert!(maintenance.scan().is_err());
    fs::remove_dir(&directory).unwrap();
    let unknown = fixture.pins().join("unrecognized-file");
    fs::write(&unknown, b"foreign").unwrap();
    assert!(maintenance.scan().is_err());
    assert!(unknown.exists());
    assert_eq!(fs::read(fixture.path.join("CURRENT")).unwrap(), before);
}

#[test]
fn registry_and_live_inventory_budgets_fail_before_removal() {
    let fixture = Fixture::new(true);
    let limited = fixture.coordination(Limits {
        max_pin_records: 1,
        ..Limits::default()
    });
    let capture = limited.try_capture().unwrap();
    let pin = capture.pin_checkpoint(&fixture.checkpoint()).unwrap();
    assert!(matches!(
        capture.pin_checkpoint(&fixture.checkpoint()),
        Err(Error::LimitExceeded)
    ));
    drop(capture);
    drop(pin);
    let larger = fixture.coordination(Limits::default());
    let capture = larger.try_capture().unwrap();
    let first = capture.pin_checkpoint(&fixture.checkpoint()).unwrap();
    let second = capture.pin_checkpoint(&fixture.checkpoint()).unwrap();
    drop(capture);
    let mut maintenance = limited.try_maintenance().unwrap();
    assert!(matches!(maintenance.scan(), Err(Error::LimitExceeded)));
    drop(maintenance);
    let inventory_limited = fixture.coordination(Limits {
        max_inventory_entries: 1,
        ..Limits::default()
    });
    let mut maintenance = inventory_limited.try_maintenance().unwrap();
    assert!(matches!(maintenance.scan(), Err(Error::LimitExceeded)));
    assert!(first.record_path().exists() && second.record_path().exists());
    drop(first);
    assert_eq!(maintenance.scan().unwrap().live_roots().len(), 1);
}

struct Worker {
    child: Child,
    root: PathBuf,
}
impl Worker {
    fn start(fixture: &Fixture) -> Self {
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "process_worker", "--nocapture"])
            .env("ULMDB_COORDINATION_WORKER", &fixture.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut worker = Self {
            child,
            root: fixture.path.clone(),
        };
        worker.wait_marker("worker-ready");
        worker
    }
    fn start_registration(fixture: &Fixture, id: u32) -> Self {
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "registration_worker", "--nocapture"])
            .env("ULMDB_REGISTRATION_WORKER", &fixture.path)
            .env("ULMDB_REGISTRATION_ID", id.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut worker = Self {
            child,
            root: fixture.path.clone(),
        };
        worker.wait_marker(&format!("registration-ready-{id}"));
        worker
    }
    fn wait_marker(&mut self, name: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.root.join(name).exists() {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "worker exited before {name}"
            );
            assert!(Instant::now() < deadline, "worker deadline for {name}");
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn release_capture(&mut self) {
        self.child.stdin.as_mut().unwrap().write_all(b"r").unwrap();
        self.wait_marker("worker-released");
    }
    fn kill(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[test]
fn process_worker() {
    let Some(path) = std::env::var_os("ULMDB_COORDINATION_WORKER") else {
        return;
    };
    let directory = Path::new(&path);
    let unbound = Coordination::capture_directory(directory, Limits::default()).unwrap();
    let manifest = read_current(directory).unwrap();
    let (_coordinator, capture) = unbound.bind(manifest.uuid).unwrap();
    let mut capture = Some(capture);
    let _pin = capture
        .as_ref()
        .unwrap()
        .pin_checkpoint(&checkpoint_bytes(&manifest).unwrap())
        .unwrap();
    fs::write(directory.join("worker-ready"), b"ready").unwrap();
    let mut command = [0];
    while std::io::stdin().read_exact(&mut command).is_ok() {
        if command[0] == b'r' {
            capture.take();
            fs::write(directory.join("worker-released"), b"released").unwrap();
        }
    }
}

#[test]
fn registration_worker() {
    let Some(path) = std::env::var_os("ULMDB_REGISTRATION_WORKER") else {
        return;
    };
    let id = std::env::var("ULMDB_REGISTRATION_ID").unwrap();
    let directory = Path::new(&path);
    let unbound = Coordination::capture_directory(directory, Limits::default()).unwrap();
    let manifest = read_current(directory).unwrap();
    let (_coordinator, capture) = unbound.bind(manifest.uuid).unwrap();
    let mut capture = Some(capture);
    fs::write(directory.join(format!("registration-ready-{id}")), b"ready").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !directory.join("registration-go").exists() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    let mut pins = Vec::new();
    for _ in 0..32 {
        pins.push(
            capture
                .as_ref()
                .unwrap()
                .pin_checkpoint(&checkpoint_bytes(&manifest).unwrap())
                .unwrap(),
        );
    }
    fs::write(directory.join(format!("registration-done-{id}")), b"done").unwrap();
    let mut command = [0];
    while std::io::stdin().read_exact(&mut command).is_ok() {
        if command[0] == b'r' {
            capture.take();
            fs::write(
                directory.join(format!("registration-released-{id}")),
                b"released",
            )
            .unwrap();
        }
    }
    drop(pins);
}

#[test]
fn simultaneous_process_registration_serializes_without_busy_or_lost_pins() {
    let fixture = Fixture::new(true);
    let coordinator = fixture.coordination(Limits::default());
    let mut first = Worker::start_registration(&fixture, 1);
    let mut second = Worker::start_registration(&fixture, 2);
    fs::write(fixture.path.join("registration-go"), b"go").unwrap();
    first.wait_marker("registration-done-1");
    second.wait_marker("registration-done-2");
    assert!(matches!(coordinator.try_maintenance(), Err(Error::Busy)));
    first.child.stdin.as_mut().unwrap().write_all(b"r").unwrap();
    second
        .child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"r")
        .unwrap();
    first.wait_marker("registration-released-1");
    second.wait_marker("registration-released-2");
    let mut maintenance = coordinator.try_maintenance().unwrap();
    let scan = maintenance.scan().unwrap();
    assert_eq!(scan.live_roots().len(), 64);
    assert!(
        scan.live_roots()
            .iter()
            .all(|root| root.checkpoint() == &fixture.checkpoint())
    );
    assert_eq!(scan.cleanup_stale().unwrap(), 0);
    first.kill();
    second.kill();
    let scan = maintenance.scan().unwrap();
    assert!(scan.live_roots().is_empty());
    assert_eq!(scan.cleanup_stale().unwrap(), 64);
}

#[test]
fn independent_process_capture_exclusion_then_owner_death_releases_stale_pin() {
    let fixture = Fixture::new(true);
    let coordinator = fixture.coordination(Limits::default());
    let mut worker = Worker::start(&fixture);
    assert!(matches!(coordinator.try_maintenance(), Err(Error::Busy)));
    worker.release_capture();
    let mut maintenance = coordinator.try_maintenance().unwrap();
    let scan = maintenance.scan().unwrap();
    assert_eq!(scan.live_roots().len(), 1);
    assert_eq!(scan.live_roots()[0].checkpoint(), &fixture.checkpoint());
    assert_eq!(scan.cleanup_stale().unwrap(), 0);
    worker.kill();
    let scan = maintenance.scan().unwrap();
    assert!(scan.live_roots().is_empty());
    assert_eq!(scan.stale_records(), 1);
    assert_eq!(scan.cleanup_stale().unwrap(), 1);
    assert_eq!(fs::read_dir(fixture.pins()).unwrap().count(), 0);
}

#[test]
fn forked_drop_cannot_unlock_or_unlink_creators_capture_or_root_pin() {
    let fixture = Fixture::new(false);
    let coordinator = fixture.coordination(Limits::default());
    let capture = coordinator.try_capture().unwrap();
    let pin = capture.pin_checkpoint(&fixture.checkpoint()).unwrap();
    let proof = pin.root().clone();
    let record = pin.record_path().to_owned();
    let mut pipe = [0i32; 2];
    assert_eq!(
        unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) },
        0
    );
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        // Neither destructor may unlock/unlink its creator's OFD/path.
        drop(capture);
        drop(pin);
        let invalid = matches!(coordinator.try_capture(), Err(Error::InvalidInput(_)));
        let response = u8::from(invalid);
        unsafe {
            libc::close(pipe[0]);
            libc::write(pipe[1], (&response as *const u8).cast(), 1);
            libc::_exit(0);
        }
    }
    unsafe {
        libc::close(pipe[1]);
    }
    let mut poll = libc::pollfd {
        fd: pipe[0],
        events: libc::POLLIN,
        revents: 0,
    };
    let ready = unsafe { libc::poll(&mut poll, 1, 5000) };
    if ready != 1 {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    let mut response = 0u8;
    let received = if ready == 1 {
        unsafe { libc::read(pipe[0], (&mut response as *mut u8).cast(), 1) }
    } else {
        0
    };
    let mut status = 0;
    unsafe {
        libc::waitpid(pid, &mut status, 0);
        libc::close(pipe[0]);
    }
    assert_eq!((ready, received, response, status), (1, 1, 1, 0));
    assert!(record.exists());
    assert!(matches!(coordinator.try_maintenance(), Err(Error::Busy)));
    drop(capture);
    let mut maintenance = coordinator.try_maintenance().unwrap();
    assert_eq!(
        maintenance.scan().unwrap().live_roots()[0].checkpoint(),
        proof.checkpoint()
    );
    drop(pin);
    assert!(!record.exists());
    assert!(maintenance.scan().unwrap().live_roots().is_empty());
}
