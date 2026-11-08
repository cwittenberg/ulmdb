use super::*;
use std::{sync::mpsc, thread, time::Duration};

struct Fixture {
    path: PathBuf,
    coordination: Coordination,
    checkpoint: [u8; 64],
    writer: crate::WriterFence,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-pin-release-{}-{}",
            std::process::id(),
            new_identity().unwrap()
        ));
        crate::create_layout(&path).unwrap();
        let manifest = Manifest::empty().unwrap();
        crate::publish(&path, &manifest).unwrap();
        let coordination = Coordination::open(&path, manifest.uuid, Default::default()).unwrap();
        let writer = crate::WriterFence::try_lock(
            crate::access::options_for_entry(&path.join("LOCK"))
                .unwrap()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path.join("LOCK"))
                .unwrap(),
        )
        .unwrap();
        Self {
            path,
            coordination,
            checkpoint: checkpoint_bytes(&manifest).unwrap(),
            writer,
        }
    }
    fn negotiate(&self) -> PinDiscovery {
        let mut owner = self.coordination.try_maintenance().unwrap();
        let mut discovery = owner.begin_pin_discovery(&self.writer).unwrap();
        assert!(
            discovery
                .advance(&mut owner, PinDiscoveryBudget { records: 64 })
                .unwrap()
                .complete
        );
        discovery
    }
    fn pin(&self) -> PhysicalPin {
        self.coordination
            .try_capture()
            .unwrap()
            .pin_checkpoint(&self.checkpoint)
            .unwrap()
    }
    fn pending(&self) -> PendingPhysicalPin {
        self.coordination
            .try_maintenance()
            .unwrap()
            .protect_checkpoint_pending(&self.checkpoint, &self.writer)
            .unwrap()
    }
    fn counts(&self) -> (u64, u64) {
        let registry = self.coordination.inner.registry().unwrap();
        let p = profile(&self.coordination.inner, &registry.file)
            .unwrap()
            .unwrap();
        (p.records, p.pending_records)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn ordinary_pin_release_removes_active_and_sealed_members_and_updates_counters() {
    let fixture = Fixture::new();
    let mut discovery = fixture.negotiate();
    let active = fixture.pin();
    let path = active.record_path().to_owned();
    assert_eq!(fixture.counts(), (1, 0));
    drop(active);
    assert!(!path.exists());
    assert_eq!(fixture.counts(), (0, 0));

    let sealed = fixture.pin();
    let path = sealed.record_path().to_owned();
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    assert!(discovery.refresh(&mut owner).unwrap());
    drop(owner);
    drop(sealed);
    assert!(!path.exists());
    assert_eq!(fixture.counts(), (0, 0));
}

#[test]
fn ordinary_pin_release_preserves_membership_during_a_resumable_scan() {
    let fixture = Fixture::new();
    let mut discovery = fixture.negotiate();
    let pin = fixture.pin();
    let path = pin.record_path().to_owned();
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    assert!(discovery.refresh(&mut owner).unwrap());
    let progress = discovery
        .advance(&mut owner, PinDiscoveryBudget { records: 1 })
        .unwrap();
    assert!(!progress.complete);
    assert!(discovery.walk.is_some());
    drop(owner);
    drop(pin);
    assert!(
        path.exists(),
        "membership changed while ReadDir retained generation SH"
    );
    assert_eq!(fixture.counts(), (1, 0));
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    loop {
        if discovery
            .advance(&mut owner, PinDiscoveryBudget { records: 2 })
            .unwrap()
            .complete
        {
            break;
        }
    }
    drop(owner);
    assert!(!path.exists());
    assert_eq!(fixture.counts(), (0, 0));
}

#[test]
fn ordinary_pin_release_never_waits_for_registry_during_drop() {
    let fixture = Fixture::new();
    let _discovery = fixture.negotiate();
    let pin = fixture.pin();
    let path = pin.record_path().to_owned();
    let registry = fixture.coordination.inner.registry().unwrap();
    let (sent, received) = mpsc::channel();
    let dropping = thread::spawn(move || {
        drop(pin);
        sent.send(()).unwrap();
    });
    let completed = received.recv_timeout(Duration::from_secs(2));
    // Release exclusion even if the assertion fails, so a faulty blocking Drop
    // cannot strand a background thread for the remainder of the test process.
    drop(registry);
    dropping.join().unwrap();
    completed.expect("Drop waited for a registry lease its caller can already own");
    assert!(
        path.exists(),
        "contended release must leave a conservative stale record"
    );
    assert_eq!(fixture.counts(), (1, 0));
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    assert_eq!(owner.scan().unwrap().cleanup_stale().unwrap(), 1);
    drop(owner);
    assert!(!path.exists());
    assert_eq!(fixture.counts(), (0, 0));
}

#[test]
fn ordinary_pin_release_rejects_replaced_generation_scan_and_record_paths() {
    for replace in ["generation", "scan", "record"] {
        let fixture = Fixture::new();
        let _discovery = fixture.negotiate();
        let pin = fixture.pin();
        let path = pin.record_path().to_owned();
        let parent = path.parent().unwrap();
        match replace {
            "generation" => {
                let saved = parent.with_extension("saved");
                fs::rename(parent, &saved).unwrap();
                fs::create_dir(parent).unwrap();
                fs::write(parent.join(SCAN), []).unwrap();
                fs::rename(saved.join(path.file_name().unwrap()), &path).unwrap();
            }
            "scan" => {
                fs::rename(parent.join(SCAN), parent.join("saved-scan")).unwrap();
                fs::write(parent.join(SCAN), []).unwrap();
            }
            "record" => {
                fs::rename(&path, path.with_extension("saved")).unwrap();
                fs::write(&path, b"replacement").unwrap();
            }
            _ => unreachable!(),
        }
        drop(pin);
        assert!(path.exists(), "removed a replaced {replace} path");
        assert_eq!(fixture.counts(), (1, 0));
    }
}

#[test]
fn ordinary_pin_release_of_an_unmigrated_flat_member_does_not_decrement_profile() {
    let fixture = Fixture::new();
    let pin = fixture.pin();
    let path = pin.record_path().to_owned();
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    let mut discovery = owner.begin_pin_discovery(&fixture.writer).unwrap();
    assert_eq!(fixture.counts(), (0, 0));
    drop(pin);
    assert!(!path.exists());
    assert_eq!(fixture.counts(), (0, 0));
    assert!(
        discovery
            .advance(&mut owner, PinDiscoveryBudget { records: 64 })
            .unwrap()
            .complete
    );
}

#[test]
fn pending_pin_release_removes_active_records_and_updates_pending_counters() {
    let fixture = Fixture::new();
    let _discovery = fixture.negotiate();
    let pending = fixture.pending();
    let path = pending.path.clone();
    assert_eq!(fixture.counts(), (1, 1));
    drop(pending);
    assert!(!path.exists());
    assert_eq!(fixture.counts(), (0, 0));
}

#[test]
fn dropped_discovery_copies_do_not_create_a_new_frontier() {
    let fixture = Fixture::new();
    let mut discovery = fixture.negotiate();
    let source = fixture.pin();
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    assert!(discovery.refresh(&mut owner).unwrap());
    let progress = discovery
        .advance(&mut owner, PinDiscoveryBudget { records: 64 })
        .unwrap();
    assert!(progress.complete);
    assert_eq!(progress.pins.len(), 1);
    let copy = &progress.pins[0];
    assert_eq!(copy.checkpoint(), Some(&fixture.checkpoint));
    let path = copy.path.clone();
    assert_eq!(fixture.counts(), (2, 1));
    drop(progress);
    assert!(!path.exists());
    assert_eq!(fixture.counts(), (1, 0));
    assert!(!discovery.refresh(&mut owner).unwrap());
    assert!(source.matches_directory(&fixture.path).unwrap());
}

#[test]
fn pending_pin_release_preserves_membership_during_a_resumable_scan() {
    let fixture = Fixture::new();
    let mut discovery = fixture.negotiate();
    let pending = fixture.pending();
    let path = pending.path.clone();
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    assert!(discovery.refresh(&mut owner).unwrap());
    assert!(
        !discovery
            .advance(&mut owner, PinDiscoveryBudget { records: 1 })
            .unwrap()
            .complete
    );
    assert!(discovery.walk.is_some());
    drop(owner);
    drop(pending);
    assert!(path.exists(), "pending release changed sealed membership");
    assert_eq!(fixture.counts(), (1, 1));
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    loop {
        if discovery
            .advance(&mut owner, PinDiscoveryBudget { records: 2 })
            .unwrap()
            .complete
        {
            break;
        }
    }
    assert!(!path.exists());
    assert_eq!(fixture.counts(), (0, 0));
}

#[test]
fn released_copy_bookkeeping_is_bounded_before_another_record_is_reserved() {
    let fixture = Fixture::new();
    let mut discovery = fixture.negotiate();
    let _source = fixture.pin();
    for n in 0..MAX_PENDING_RECORDS {
        discovery.own_records.insert(PathBuf::from(format!("old-{n}")));
    }
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    assert!(discovery.refresh(&mut owner).unwrap());
    assert!(matches!(
        discovery.advance(&mut owner, PinDiscoveryBudget { records: 64 }),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(discovery.own_records.len(), MAX_PENDING_RECORDS as usize);
    assert_eq!(fixture.counts(), (1, 0));
}
