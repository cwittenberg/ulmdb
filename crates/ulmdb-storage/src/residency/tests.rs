use super::*;
use crate::{Arena, Error, SegmentTable, WriteStore};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    fs,
    path::PathBuf,
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-residency-{:032x}",
            crate::new_identity().unwrap()
        ));
        crate::create_layout(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn extend(dir: &Directory, table: SegmentTable, id: u64) -> SegmentTable {
    let mut arena = Arena::new(table, id).unwrap();
    arena.put_payload(b"resident bytes").unwrap();
    arena.seal(&dir.0).unwrap().0
}
fn fixture() -> (Directory, SegmentTable, u64) {
    let dir = Directory::new();
    let table = extend(&dir, SegmentTable::default(), 1);
    let pages = rounded(table.mappings().next().unwrap(), os::page_size().unwrap()).unwrap();
    let table = extend(&dir, table, 2);
    (dir, table, pages)
}
struct Fake {
    calls: Cell<usize>,
    fail: Option<usize>,
    bad_unlock: BTreeSet<u64>,
    events: RefCell<Vec<(bool, u64)>>,
}
impl Fake {
    fn new(fail: Option<usize>, bad_unlock: impl IntoIterator<Item = u64>) -> Self {
        Self {
            calls: Cell::new(0),
            fail,
            bad_unlock: bad_unlock.into_iter().collect(),
            events: RefCell::new(Vec::new()),
        }
    }
}
impl LockOps for Fake {
    fn lock(&self, mapping: &Mapping) -> std::io::Result<()> {
        let call = self.calls.get() + 1;
        self.calls.set(call);
        self.events.borrow_mut().push((true, mapping.entry.id));
        if self.fail == Some(call) {
            Err(std::io::ErrorKind::PermissionDenied.into())
        } else {
            Ok(())
        }
    }
    fn unlock(&self, mapping: &Mapping) -> std::io::Result<()> {
        self.events.borrow_mut().push((false, mapping.entry.id));
        if self.bad_unlock.contains(&mapping.entry.id) {
            Err(std::io::ErrorKind::Other.into())
        } else {
            Ok(())
        }
    }
}
#[test]
fn demand_is_unscanned_and_tokens_bind_controller_and_table_revision() {
    let (dir, table, _) = fixture();
    let controller = ResidencyController::new(ResidencyPolicy::Demand).unwrap();
    let fake = Fake::new(Some(1), []);
    let ready = controller.admit_with(&table, &fake).unwrap();
    assert_eq!(fake.calls.get(), 0);
    assert_eq!(ready.report().mapped_bytes, None);
    assert!(ready.is_for(&controller.clone()));
    assert!(!ready.is_for(&ResidencyController::new(ResidencyPolicy::Demand).unwrap()));
    assert!(ready.is_for_table(&table.clone()));
    let all = table.select(&BTreeSet::from([1, 2])).unwrap();
    assert!(ready.is_for_table(&all));
    let subset = table.select(&BTreeSet::from([1])).unwrap();
    assert!(!ready.is_for_table(&subset));
    let added = extend(&dir, table.clone(), 3);
    assert!(!ready.is_for_table(&added));
    assert_eq!(controller.live_status().reserved_bytes, 0);
}
#[test]
fn prefault_checks_rounded_candidate_budget_and_never_claims_persistent_residency() {
    let (_, table, pages) = fixture();
    let small = ResidencyController::new(ResidencyPolicy::Prefault {
        max_bytes: pages * 2 - 1,
    })
    .unwrap();
    assert!(matches!(small.admit(&table), Err(Error::LimitExceeded)));
    let controller = ResidencyController::new(ResidencyPolicy::Prefault {
        max_bytes: pages * 2,
    })
    .unwrap();
    let report = controller.admit(&table).unwrap().report();
    assert_eq!(report.page_rounded_bytes, Some(pages * 2));
    assert_eq!(report.mapped_bytes, Some(report.prefaulted_bytes));
    assert_eq!(report.locked_bytes, 0);
    assert_eq!(controller.live_status().reserved_bytes, 0);
}
#[test]
fn mapping_arcs_deduplicate_locks_reject_foreign_owner_and_release_after_last_table() {
    let (_, table, pages) = fixture();
    let controller = ResidencyController::new(ResidencyPolicy::RequiredLocked {
        max_bytes: pages * 2,
    })
    .unwrap();
    let fake = Fake::new(None, []);
    let token = controller.admit_with(&table, &fake).unwrap();
    assert_eq!(token.report().locked_bytes, pages * 2);
    assert_eq!(fake.calls.get(), 2);
    controller.admit_with(&table.clone(), &fake).unwrap();
    assert_eq!(fake.calls.get(), 2);
    let foreign = ResidencyController::new(ResidencyPolicy::RequiredLocked {
        max_bytes: pages * 2,
    })
    .unwrap();
    assert!(matches!(
        foreign.admit_with(&table, &fake),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(fake.calls.get(), 2);
    let old = table.clone();
    let selected = table.select(&BTreeSet::from([1])).unwrap();
    drop(table);
    assert_eq!(controller.live_status().reserved_bytes, pages * 2);
    drop(old);
    assert_eq!(controller.live_status().reserved_bytes, pages);
    drop(selected);
    assert_eq!(controller.live_status().reserved_bytes, 0);
    assert_eq!(controller.live_status().live_locked_bytes, 0);
    assert_eq!(
        fake.events
            .borrow()
            .iter()
            .filter(|(lock, _)| !*lock)
            .count(),
        0,
        "destruction unmaps; it never munlocks after unmapping"
    );
}
#[test]
fn overlap_budget_includes_old_readers_and_preflight_performs_no_lock_on_rejection() {
    let (dir, table, pages) = fixture();
    let controller = ResidencyController::new(ResidencyPolicy::RequiredLocked {
        max_bytes: pages * 2,
    })
    .unwrap();
    let fake = Fake::new(None, []);
    controller.admit_with(&table, &fake).unwrap();
    let only = table.select(&BTreeSet::from([1])).unwrap();
    let new = extend(&dir, only, 3);
    assert!(matches!(
        controller.admit_with(&new, &fake),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(fake.calls.get(), 2);
    drop(table);
    controller.admit_with(&new, &fake).unwrap();
    assert_eq!(fake.calls.get(), 3);
    assert_eq!(controller.live_status().reserved_bytes, pages * 2);
}
#[test]
fn partial_failure_rolls_back_only_new_locks_and_preserves_old_admission() {
    let (dir, table, pages) = fixture();
    let old = table.select(&BTreeSet::from([1])).unwrap();
    let controller = ResidencyController::new(ResidencyPolicy::RequiredLocked {
        max_bytes: pages * 3,
    })
    .unwrap();
    let fake = Fake::new(Some(3), []);
    let token = controller.admit_with(&old, &fake).unwrap();
    let candidate = extend(&dir, table, 3);
    assert!(matches!(
        controller.admit_with(&candidate, &fake),
        Err(Error::Io(_))
    ));
    assert_eq!(
        *fake.events.borrow(),
        vec![(true, 1), (true, 2), (true, 3), (false, 3), (false, 2)]
    );
    let status = controller.live_status();
    assert_eq!(status.reserved_bytes, pages);
    assert_eq!(status.live_locked_bytes, pages);
    assert_eq!(status.retained_failed_bytes, 0);
    assert!(token.is_for_table(&old));
}
#[test]
fn failed_unlock_quarantines_every_affected_reservation_until_mapping_unmap() {
    let (_, table, pages) = fixture();
    let controller = ResidencyController::new(ResidencyPolicy::RequiredLocked {
        max_bytes: pages * 2,
    })
    .unwrap();
    let fake = Fake::new(Some(2), [1, 2]);
    assert!(matches!(
        controller.admit_with(&table, &fake),
        Err(Error::Io(_))
    ));
    let status = controller.live_status();
    assert_eq!(
        (
            status.reserved_bytes,
            status.live_locked_bytes,
            status.retained_failed_bytes
        ),
        (pages * 2, 0, pages * 2)
    );
    assert!(matches!(
        controller.admit_with(&table, &fake),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(fake.calls.get(), 2);
    let retained = table.clone();
    drop(table);
    assert_eq!(controller.live_status().reserved_bytes, pages * 2);
    drop(retained);
    assert_eq!(controller.live_status().reserved_bytes, 0);
    assert_eq!(controller.live_status().retained_failed_bytes, 0);
}
#[test]
fn rounded_page_accounting_uses_the_actual_mapping_extent() {
    let (_, table, _) = fixture();
    let page = os::page_size().unwrap();
    for mapping in table.mappings() {
        let bytes = rounded(mapping, page).unwrap();
        assert!(bytes >= mapping.bytes.len() as u64);
        assert!(bytes.is_multiple_of(page as u64));
        assert!(bytes - (mapping.bytes.len() as u64) < page as u64);
    }
    assert_eq!(std::mem::offset_of!(Mapping, bytes), 0);
    assert!(std::mem::offset_of!(Mapping, residency) > std::mem::offset_of!(Mapping, bytes));
}

#[test]
fn default_admission_and_optional_slot_stay_compact_without_a_report_allocation() {
    assert!(std::mem::size_of::<ResidencyAdmission>() <= 32);
    assert!(std::mem::size_of::<Option<ResidencyAdmission>>() <= 32);
    let table = SegmentTable::default();
    let controller = ResidencyController::new(ResidencyPolicy::Demand).unwrap();
    let admission = controller.admit(&table).unwrap();
    assert!(admission.report.is_none());
    assert!(admission.clone().report.is_none());
    assert_eq!(
        admission.report(),
        ResidencyReport {
            policy: ResidencyPolicy::Demand,
            mapped_bytes: None,
            page_rounded_bytes: None,
            page_size: None,
            locked_bytes: 0,
            prefaulted_bytes: 0
        }
    );
    let prefault = ResidencyController::new(ResidencyPolicy::Prefault { max_bytes: 0 }).unwrap();
    let managed = prefault.admit(&table).unwrap();
    assert!(managed.report.is_some());
    assert_eq!(
        managed.report().policy,
        ResidencyPolicy::Prefault { max_bytes: 0 }
    );
}

#[path = "tests/append.rs"]
mod append;

#[cfg(target_os = "linux")]
fn fixed_fixture() -> (
    Directory,
    SegmentTable,
    Arc<crate::fixed_map::FixedMap>,
    u64,
) {
    let (dir, table, _) = fixture();
    let fixed = crate::fixed_map::FixedMap::create(&dir.0, [7; 16], 1, 1 << 20, None).unwrap();
    let declaration = crate::fixed_map::FixedMapDeclaration::open(&dir.0, [7; 16], 1).unwrap();
    let mut manifest = crate::Manifest::empty().unwrap();
    manifest.uuid = [7; 16];
    manifest.epoch = 1;
    manifest.segments = table.entries().into();
    declaration.register_manifest(&manifest).unwrap();
    let table = table.with_fixed_map(&dir.0, fixed.clone()).unwrap();
    let pages = table
        .mappings()
        .map(|m| m.fixed_lease().unwrap().rounded_len() as u64)
        .sum();
    (dir, table, fixed, pages)
}
#[test]
#[cfg(target_os = "linux")]
fn fixed_physical_charges_survive_required_mapping_drop_with_demand_alias_and_release_on_slot_unmap()
 {
    let (dir, demand, fixed, pages) = fixed_fixture();
    let required = demand.with_fixed_map(&dir.0, fixed.clone()).unwrap();
    let controller =
        ResidencyController::new(ResidencyPolicy::RequiredLocked { max_bytes: pages }).unwrap();
    let admission = controller.admit(&required);
    if std::env::var_os("ULMDB_REQUIRED_EXPECT_LOCK_FAIL").is_some() {
        assert!(matches!(admission,Err(Error::Io(ref e))if e.raw_os_error()==Some(libc::ENOMEM)));
        let retained = controller.live_status().reserved_bytes;
        if std::env::var_os("ULMDB_REQUIRED_EXPECT_UNLOCK_FAIL").is_some() {
            assert!(retained > 0);
            assert_eq!(controller.live_status().retained_failed_bytes, retained);
        } else {
            assert_eq!(retained, 0);
        }
        drop(required);
        assert_eq!(controller.live_status().reserved_bytes, retained);
        drop(demand);
        assert_eq!(controller.live_status().reserved_bytes, 0);
        drop(fixed);
        println!("ACTUAL LOCK FAILURE CHECKED");
        return;
    }
    let report = match admission {
        Ok(r) => r,
        Err(Error::Io(e)) if matches!(e.raw_os_error(), Some(libc::EPERM) | Some(libc::ENOMEM)) => {
            assert_eq!(controller.live_status().reserved_bytes, 0);
            eprintln!("host declines actual fixed mlock: {e}");
            return;
        }
        Err(e) => panic!("{e}"),
    };
    assert_eq!(report.report().locked_bytes, pages);
    assert_eq!(controller.live_status().reserved_bytes, pages);
    let again = demand.with_fixed_map(&dir.0, fixed.clone()).unwrap();
    controller.admit(&again).unwrap();
    assert_eq!(
        controller.live_status().reserved_bytes,
        pages,
        "same controller must share its physical charge"
    );
    let foreign =
        ResidencyController::new(ResidencyPolicy::RequiredLocked { max_bytes: pages }).unwrap();
    foreign.admit(&again).unwrap();
    assert_eq!(foreign.live_status().reserved_bytes, pages);
    drop(required);
    drop(again);
    assert_eq!(controller.live_status().live_locked_bytes, pages);
    assert_eq!(foreign.live_status().live_locked_bytes, pages);
    let tiny = ResidencyController::new(ResidencyPolicy::RequiredLocked {
        max_bytes: pages - 1,
    })
    .unwrap();
    assert!(matches!(tiny.admit(&demand), Err(Error::LimitExceeded)));
    assert_eq!(tiny.live_status().reserved_bytes, 0);
    drop(demand);
    assert_eq!(controller.live_status().reserved_bytes, 0);
    assert_eq!(foreign.live_status().reserved_bytes, 0);
    drop(fixed);
}
#[test]
#[cfg(target_os = "linux")]
fn fixed_poisoned_drop_keeps_physical_budget_until_reservation_unmaps() {
    let (_dir, table, fixed, pages) = fixed_fixture();
    let controller =
        ResidencyController::new(ResidencyPolicy::RequiredLocked { max_bytes: pages }).unwrap();
    let fake = Fake::new(None, []);
    controller.admit_with(&table, &fake).unwrap();
    table
        .mappings()
        .next()
        .unwrap()
        .fixed_lease()
        .unwrap()
        .poison_registry_for_test();
    drop(table);
    assert_eq!(controller.live_status().reserved_bytes, pages);
    drop(fixed);
    assert_eq!(controller.live_status().reserved_bytes, 0);
}
#[test]
#[cfg(target_os = "linux")]
fn fixed_failed_admission_rolls_back_or_keeps_exact_failed_charge_with_demand_owner() {
    let (dir, demand, fixed, pages) = fixed_fixture();
    let required = demand.with_fixed_map(&dir.0, fixed.clone()).unwrap();
    let controller =
        ResidencyController::new(ResidencyPolicy::RequiredLocked { max_bytes: pages }).unwrap();
    let fake = Fake::new(Some(2), []);
    assert!(controller.admit_with(&required, &fake).is_err());
    assert_eq!(controller.live_status().reserved_bytes, 0);
    assert_eq!(
        fake.events
            .borrow()
            .iter()
            .filter(|(lock, _)| !*lock)
            .count(),
        2
    );
    let controller =
        ResidencyController::new(ResidencyPolicy::RequiredLocked { max_bytes: pages }).unwrap();
    let fake = Fake::new(Some(1), [1]);
    assert!(controller.admit_with(&required, &fake).is_err());
    assert!(controller.live_status().retained_failed_bytes > 0);
    let retained = controller.live_status().reserved_bytes;
    drop(required);
    assert_eq!(controller.live_status().reserved_bytes, retained);
    drop(demand);
    assert_eq!(controller.live_status().reserved_bytes, 0);
    drop(fixed);
}

#[test]
#[cfg(target_os = "linux")]
fn fixed_decoded_charge_survives_poisoned_slot_until_actual_reservation_unmap() {
    use crate::transform::{Checksum, ChecksumConfig, TransformContext};
    use sha2::{Digest, Sha256};
    struct Sum;
    impl Checksum for Sum {
        fn compute(&self, src: &[u8], dst: &mut [u8], _: Option<&[u8]>) -> crate::Result<()> {
            dst.copy_from_slice(&Sha256::digest(src));
            Ok(())
        }
    }
    let dir = Directory::new();
    let ctx = Arc::new(
        TransformContext::new(
            Some(ChecksumConfig {
                callback: Arc::new(Sum),
                bytes: 32,
            }),
            None,
            1 << 20,
        )
        .unwrap(),
    );
    let base =
        SegmentTable::empty_with_transform(crate::ReadAhead::Normal, Some(ctx.clone())).unwrap();
    let plain = extend(&dir, base, 1);
    let fixed = crate::fixed_map::FixedMap::create(&dir.0, [7; 16], 1, 1 << 20, None).unwrap();
    let declaration = crate::fixed_map::FixedMapDeclaration::open(&dir.0, [7; 16], 1).unwrap();
    let mut manifest = crate::Manifest::empty().unwrap();
    manifest.uuid = [7; 16];
    manifest.epoch = 1;
    manifest.segments = plain.entries().into();
    declaration.register_manifest(&manifest).unwrap();
    let table = plain.with_fixed_map(&dir.0, fixed.clone()).unwrap();
    drop(plain);
    assert!(ctx.decoded_bytes() > 0);
    let charged = ctx.decoded_bytes();
    table
        .mappings()
        .next()
        .unwrap()
        .fixed_lease()
        .unwrap()
        .poison_registry_for_test();
    drop(table);
    assert_eq!(ctx.decoded_bytes(), charged);
    drop(fixed);
    assert_eq!(ctx.decoded_bytes(), 0);
}

#[test]
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn fork_child_lease_drop_never_locks_inherited_ledger() {
    const NAME: &str = "residency::tests::fork_child_lease_drop_never_locks_inherited_ledger";
    if crate::history::tests::run_isolated(NAME) {
        return;
    }
    let state = Arc::new(State {
        creator: std::process::id(),
        id: NonZeroU64::new(1).unwrap(),
        policy: ResidencyPolicy::RequiredLocked { max_bytes: 4096 },
        page_size: 4096,
        budget: 4096,
        ledger: std::sync::Mutex::new(Ledger {
            reserved: 4096,
            locked: 4096,
            ..Ledger::default()
        }),
        serial: std::sync::Mutex::new(()),
    });
    let lease = Lease {
        state: state.clone(),
        bytes: 4096,
        kind: LeaseKind::Locked,
    };
    let held = unsafe { crate::creator_owned::tests::HeldMutex::new(&state.ledger) };
    let lease = crate::creator_owned::tests::fork_drop(lease);
    drop(held);
    assert_eq!(state.ledger.lock().unwrap().locked, 4096);
    drop(lease);
    let ledger = state.ledger.lock().unwrap();
    assert_eq!((ledger.reserved, ledger.locked, ledger.failed), (0, 0, 0));
    crate::history::tests::worker_completed(NAME);
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub(crate) fn fixed_charge_with_held_ledger()
-> (FixedCharge, crate::creator_owned::tests::HeldMutex) {
    let state = Arc::new(State {
        creator: std::process::id(),
        id: NonZeroU64::new(1).unwrap(),
        policy: ResidencyPolicy::RequiredLocked { max_bytes: 4096 },
        page_size: 4096,
        budget: 4096,
        ledger: std::sync::Mutex::new(Ledger {
            reserved: 4096,
            locked: 4096,
            ..Ledger::default()
        }),
        serial: std::sync::Mutex::new(()),
    });
    let held = unsafe { crate::creator_owned::tests::HeldMutex::new(&state.ledger) };
    (
        FixedCharge {
            lease: Lease {
                state,
                bytes: 4096,
                kind: LeaseKind::Locked,
            },
            healthy: true,
        },
        held,
    )
}
