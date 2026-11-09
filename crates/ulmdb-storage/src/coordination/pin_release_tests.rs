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
    fn drain(&self, discovery: &mut PinDiscovery, budget: usize) -> usize {
        let mut owner = self.coordination.try_maintenance().unwrap();
        let mut units = 0;
        for _ in 0..10_000 {
            let progress = discovery
                .advance(&mut owner, PinDiscoveryBudget { records: budget })
                .unwrap();
            assert!(progress.work_units <= budget);
            units += progress.work_units;
            if progress.complete {
                return units;
            }
        }
        panic!("pin discovery did not finish");
    }
    fn directories(&self) -> usize {
        fs::read_dir(&self.coordination.inner.pins).unwrap().count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn singleton_scan_still_rejects_bad_links_and_unknown_generations_before_cleanup() {
    for damage in ["active_links", "extra_generation"] {
        let fixture = Fixture::new();
        let _discovery = fixture.negotiate();
        let inner = &fixture.coordination.inner;
        let registry = inner.registry().unwrap();
        let p = profile(inner, &registry.file).unwrap().unwrap();
        assert!(p.indexed);
        assert_eq!(p.generations, 1);
        assert_eq!(p.first, p.active);
        assert_eq!(p.records, 0);
        assert!(cleanup_leases(inner, &registry.file).unwrap().is_empty());
        let active = generation(inner, p.active);
        let stale = active.join("pin-00000000000000000000000000000001.lease");
        fs::write(&stale, b"stale record must survive a rejected scan").unwrap();
        match damage {
            "active_links" => {
                let path = active.join(LINKS);
                let mut bytes = fs::read(&path).unwrap();
                bytes[88..96].copy_from_slice(&0u64.to_le_bytes());
                let crc = crate::publication::crc(&bytes[..120]);
                bytes[120..124].copy_from_slice(&crc.to_le_bytes());
                fs::write(path, bytes).unwrap();
            }
            "extra_generation" => create_generation(inner, 0).unwrap(),
            _ => unreachable!(),
        }
        drop(registry);
        let mut owner = fixture.coordination.try_maintenance().unwrap();
        assert!(matches!(owner.scan(), Err(Error::Corrupt(_))));
        assert!(stale.exists());
    }
}

#[test]
fn registry_churn_retires_gaps_while_an_ancient_reader_remains_live() {
    let fixture = Fixture::new();
    let mut discovery = fixture.negotiate();
    assert_eq!(fixture.directories(), 1);
    let ancient = fixture.pin();
    let ancient_path = ancient.record_path().to_owned();
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    assert!(discovery.refresh(&mut owner).unwrap());
    drop(owner);
    fixture.drain(&mut discovery, 1);
    for _ in 0..256 {
        let pin = fixture.pin();
        let retired = pin.record_path().parent().unwrap().to_owned();
        let mut owner = fixture.coordination.try_maintenance().unwrap();
        assert!(discovery.refresh(&mut owner).unwrap());
        drop(owner);
        drop(pin);
        fixture.drain(&mut discovery, 1);
        assert!(!retired.exists());
        assert_eq!(fixture.directories(), 2);
        assert!(ancient_path.exists());
    }
    assert_eq!(fixture.counts(), (1, 0));
    drop(ancient);
    assert_eq!(fixture.directories(), 1);
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    let mut fresh = owner.begin_pin_discovery(&fixture.writer).unwrap();
    drop(owner);
    assert_eq!(
        fixture.drain(&mut fresh, 1),
        0,
        "fresh discovery scanned retired history"
    );
}

#[test]
fn registry_continuation_rebases_after_a_second_owner_retires_its_cleanup() {
    let fixture = Fixture::new();
    let mut first = fixture.negotiate();
    let pin = fixture.pin();
    let generation_path = pin.record_path().parent().unwrap().to_owned();
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    assert!(first.refresh(&mut owner).unwrap());
    let mut second = owner.begin_pin_discovery(&fixture.writer).unwrap();
    assert!(
        !first
            .advance(&mut owner, PinDiscoveryBudget { records: 1 })
            .unwrap()
            .complete
    );
    assert!(
        !second
            .advance(&mut owner, PinDiscoveryBudget { records: 1 })
            .unwrap()
            .complete
    );
    drop(owner);
    drop(pin); // Both SH walkers force a conservative stale member.
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    assert!(
        !first
            .advance(&mut owner, PinDiscoveryBudget { records: 64 })
            .unwrap()
            .complete
    );
    assert!(first.cleanup.as_ref().unwrap().exclusion.is_none());
    drop(owner);
    fixture.drain(&mut second, 1);
    assert!(!generation_path.exists());
    fixture.drain(&mut first, 1);
    assert_eq!(fixture.directories(), 1);
    assert_eq!(fixture.counts(), (0, 0));
}

#[test]
fn registry_rg2_upgrade_indexes_one_generation_per_work_unit_and_is_explicit() {
    let fixture = Fixture::new();
    let inner = &fixture.coordination.inner;
    let registry = inner.registry().unwrap();
    let mut p = Profile {
        active: 32,
        migrating: false,
        records: 0,
        pending_records: 0,
        indexed: false,
        legacy: true,
        dirty: false,
        first: 0,
        generations: 0,
        index_cursor: 0,
    };
    for id in 0..=p.active {
        create_generation(inner, id).unwrap();
    }
    store(inner, &registry.file, p).unwrap();
    drop(registry);
    let ordinary = fixture.pin();
    let registry = inner.registry().unwrap();
    assert!(profile(inner, &registry.file).unwrap().unwrap().legacy);
    drop(registry);
    drop(ordinary);
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    let mut discovery = owner.begin_pin_discovery(&fixture.writer).unwrap();
    let registry = inner.registry().unwrap();
    p = profile(inner, &registry.file).unwrap().unwrap();
    assert!(!p.legacy);
    assert!(!p.indexed);
    drop(registry);
    for cursor in 1..=32 {
        let progress = discovery
            .advance(&mut owner, PinDiscoveryBudget { records: 1 })
            .unwrap();
        assert_eq!(progress.work_units, 1);
        let registry = inner.registry().unwrap();
        p = profile(inner, &registry.file).unwrap().unwrap();
        assert_eq!(p.index_cursor, cursor);
        assert!(!p.indexed);
    }
    drop(owner);
    fixture.drain(&mut discovery, 1);
    assert_eq!(fixture.directories(), 1);
}

#[test]
fn registry_interrupted_retirement_rejects_all_future_admission_at_each_boundary() {
    for stop in 1..=4 {
        let fixture = Fixture::new();
        let mut discovery = fixture.negotiate();
        let pin = fixture.pin();
        let path = pin.record_path().to_owned();
        let mut owner = fixture.coordination.try_maintenance().unwrap();
        assert!(discovery.refresh(&mut owner).unwrap());
        drop(owner);
        let inner = &fixture.coordination.inner;
        let registry = inner.registry().unwrap();
        drop(pin); // Leave stale name while Registry is unavailable to Drop.
        let record = opened(&path, false, false).unwrap();
        assert!(remove_owned(&path, &record).unwrap());
        removed(inner, &registry.file, &record).unwrap();
        let id = number(
            path.parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
        )
        .unwrap();
        let directory = opened(path.parent().unwrap(), false, true).unwrap();
        let exclusion = gate(&path.parent().unwrap().join(SCAN), false).unwrap();
        let result =
            index::retire_with_fault(inner, &registry.file, id, &directory, &exclusion, |stage| {
                if stage == stop {
                    Err(Error::InvalidInput("injected retirement interruption"))
                } else {
                    Ok(())
                }
            });
        assert!(result.is_err());
        assert!(matches!(
            profile(inner, &registry.file),
            Err(Error::Corrupt("interrupted pin registry index mutation"))
        ));
        drop(registry);
        assert!(
            fixture
                .coordination
                .try_capture()
                .unwrap()
                .pin_checkpoint(&fixture.checkpoint)
                .is_err()
        );
    }
}

#[test]
fn registry_discovery_rejects_a_valid_crc_but_nonreciprocal_link() {
    let fixture = Fixture::new();
    let mut discovery = fixture.negotiate();
    let first = fixture.pin();
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    assert!(discovery.refresh(&mut owner).unwrap());
    drop(owner);
    fixture.drain(&mut discovery, 64);
    let second = fixture.pin();
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    assert!(discovery.refresh(&mut owner).unwrap());
    let mut fresh = owner.begin_pin_discovery(&fixture.writer).unwrap();
    let link = generation(&fixture.coordination.inner, 1).join(LINKS);
    let mut bytes = fs::read(&link).unwrap();
    bytes[96..104].copy_from_slice(&3u64.to_le_bytes());
    let crc = crate::publication::crc(&bytes[..120]);
    bytes[120..124].copy_from_slice(&crc.to_le_bytes());
    fs::write(link, bytes).unwrap();
    assert!(
        fresh
            .advance(&mut owner, PinDiscoveryBudget { records: 1 })
            .is_err()
    );
    drop(owner);
    drop(first);
    drop(second);
}

#[test]
fn registry_discovery_rejects_a_missing_neighbor_of_a_saved_candidate() {
    let fixture = Fixture::new();
    let mut discovery = fixture.negotiate();
    let first = fixture.pin();
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    assert!(discovery.refresh(&mut owner).unwrap());
    drop(owner);
    fixture.drain(&mut discovery, 64);
    let second = fixture.pin();
    let mut owner = fixture.coordination.try_maintenance().unwrap();
    assert!(discovery.refresh(&mut owner).unwrap());
    let mut fresh = owner.begin_pin_discovery(&fixture.writer).unwrap();
    fresh.next = 2;
    fresh.seek = Some(2);
    fs::remove_file(generation(&fixture.coordination.inner, 3).join(LINKS)).unwrap();
    assert!(matches!(
        fresh.advance(&mut owner, PinDiscoveryBudget { records: 1 }),
        Err(Error::Corrupt("pin generation index neighbor missing"))
    ));
    drop(owner);
    drop(first);
    drop(second);
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
        discovery
            .own_records
            .insert(PathBuf::from(format!("old-{n}")));
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
