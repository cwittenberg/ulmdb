use super::*;
use crate::table::take_mapping_visits;
fn locked_fixture(
    multiplier: u64,
) -> (
    Directory,
    SegmentTable,
    u64,
    ResidencyController,
    Fake,
    ResidencyAdmission,
) {
    let (directory, table, pages) = fixture();
    let controller = ResidencyController::new(ResidencyPolicy::RequiredLocked {
        max_bytes: pages * multiplier,
    })
    .unwrap();
    let fake = Fake::new(None, []);
    let prior = controller.admit_with(&table, &fake).unwrap();
    (directory, table, pages, controller, fake, prior)
}
#[test]
fn one_insert_reuses_exact_old_report_and_inspects_only_new_mapping() {
    let (directory, base, pages, controller, fake, prior) = locked_fixture(8);
    let candidate = extend(&directory, base.clone(), 3);
    fake.events.borrow_mut().clear();
    take_mapping_visits();
    let ready = controller
        .admit_append_with(&candidate, &base, &prior, &fake)
        .unwrap();
    assert_eq!(
        take_mapping_visits(),
        0,
        "append must not enumerate old mappings"
    );
    assert_eq!(*fake.events.borrow(), vec![(true, 3)]);
    assert_eq!(
        ready.report().mapped_bytes,
        Some(prior.report().mapped_bytes.unwrap() / 2 * 3)
    );
    assert_eq!(ready.report().page_rounded_bytes, Some(pages * 3));
    assert_eq!(ready.report().locked_bytes, pages * 3);
    assert_eq!(ready.report().prefaulted_bytes, 0);
    assert!(ready.is_for(&controller) && ready.is_for_table(&candidate));
    assert_eq!(controller.live_status().reserved_bytes, pages * 3);
    take_mapping_visits();
    let full = controller.admit_with(&candidate, &fake).unwrap();
    assert!(take_mapping_visits() >= 3);
    assert_eq!(ready.report(), full.report());
    assert_eq!(fake.calls.get(), 3);
}
#[test]
fn two_inserts_select_reload_reuse_and_wrong_token_take_full_admission() {
    let (directory, base, _, controller, fake, prior) = locked_fixture(32);
    let middle = extend(&directory, base.clone(), 3);
    let candidate = extend(&directory, middle.clone(), 4);
    take_mapping_visits();
    let ready = controller
        .admit_append_with(&candidate, &base, &prior, &fake)
        .unwrap();
    assert!(
        take_mapping_visits() >= 4,
        "two inserts are not a one-insert certificate"
    );
    assert!(ready.is_for_table(&candidate));
    let selected = middle.select(&BTreeSet::from([1, 2, 3])).unwrap();
    take_mapping_visits();
    controller
        .admit_append_with(&selected, &base, &prior, &fake)
        .unwrap();
    assert!(
        take_mapping_visits() >= 3,
        "even full selection revokes lineage"
    );
    let reloaded = SegmentTable::load(&directory.0, &middle.entries()).unwrap();
    take_mapping_visits();
    controller
        .admit_append_with(&reloaded, &base, &prior, &fake)
        .unwrap();
    assert!(take_mapping_visits() >= 3);
    let mut reused = SegmentTable::default();
    for entry in base.entries() {
        assert!(reused.reuse_from(&base, &entry).unwrap());
    }
    take_mapping_visits();
    controller
        .admit_append_with(&reused, &base, &prior, &fake)
        .unwrap();
    assert!(take_mapping_visits() >= 2);
    let wrong = controller
        .admit_with(&SegmentTable::default(), &fake)
        .unwrap();
    take_mapping_visits();
    controller
        .admit_append_with(&middle, &base, &wrong, &fake)
        .unwrap();
    assert!(take_mapping_visits() >= 3);
}
#[test]
fn foreign_prior_and_foreign_new_mapping_never_bypass_lock_ownership() {
    let (directory, base, pages, controller, fake, prior) = locked_fixture(8);
    let candidate = extend(&directory, base.clone(), 3);
    let foreign = ResidencyController::new(ResidencyPolicy::RequiredLocked {
        max_bytes: pages * 8,
    })
    .unwrap();
    take_mapping_visits();
    assert!(matches!(
        foreign.admit_append_with(&candidate, &base, &prior, &fake),
        Err(Error::InvalidInput(_))
    ));
    assert!(
        take_mapping_visits() > 0,
        "foreign evidence uses generic validation"
    );
    let added = candidate.select(&BTreeSet::from([3])).unwrap();
    foreign.admit_with(&added, &fake).unwrap();
    let calls = fake.calls.get();
    assert!(matches!(
        controller.admit_append_with(&candidate, &base, &prior, &fake),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(fake.calls.get(), calls);
    assert_eq!(controller.live_status().reserved_bytes, pages * 2);
    assert_eq!(foreign.live_status().reserved_bytes, pages);
}
#[test]
fn global_retained_budget_denies_before_lock_and_recovers_after_old_owner_drop() {
    let (directory, retained, pages, controller, fake, _) = locked_fixture(2);
    let base = retained.select(&BTreeSet::from([1])).unwrap();
    let prior = controller.admit_with(&base, &fake).unwrap();
    let candidate = extend(&directory, base.clone(), 3);
    let calls = fake.calls.get();
    assert!(matches!(
        controller.admit_append_with(&candidate, &base, &prior, &fake),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(fake.calls.get(), calls);
    assert_eq!(controller.live_status().reserved_bytes, pages * 2);
    drop(retained);
    take_mapping_visits();
    let ready = controller
        .admit_append_with(&candidate, &base, &prior, &fake)
        .unwrap();
    assert_eq!(take_mapping_visits(), 0);
    assert_eq!(ready.report().locked_bytes, pages * 2);
    assert_eq!(controller.live_status().reserved_bytes, pages * 2);
}
#[test]
fn failed_append_cleanup_quarantines_only_new_mapping_and_revokes_old_fast_evidence() {
    let (directory, base, pages, controller, _, prior) = locked_fixture(8);
    let candidate = extend(&directory, base.clone(), 3);
    let failure = Fake::new(Some(1), [3]);
    assert!(matches!(
        controller.admit_append_with(&candidate, &base, &prior, &failure),
        Err(Error::Io(_))
    ));
    assert_eq!(*failure.events.borrow(), vec![(true, 3), (false, 3)]);
    let status = controller.live_status();
    assert_eq!(
        (
            status.reserved_bytes,
            status.live_locked_bytes,
            status.retained_failed_bytes
        ),
        (pages * 3, pages * 2, pages)
    );
    take_mapping_visits();
    assert!(matches!(
        controller.admit_append_with(&candidate, &base, &prior, &failure),
        Err(Error::InvalidInput(_))
    ));
    assert!(
        take_mapping_visits() > 0,
        "revoked epoch must use generic validation"
    );
    assert_eq!(failure.calls.get(), 1);
    drop(candidate);
    assert_eq!(controller.live_status().retained_failed_bytes, 0);
    assert_eq!(controller.live_status().reserved_bytes, pages * 2);
    let candidate = extend(&directory, base.clone(), 4);
    let healthy = Fake::new(None, []);
    take_mapping_visits();
    let recovered = controller
        .admit_append_with(&candidate, &base, &prior, &healthy)
        .unwrap();
    assert!(take_mapping_visits() > 0);
    let next = extend(&directory, candidate.clone(), 5);
    take_mapping_visits();
    controller
        .admit_append_with(&next, &candidate, &recovered, &healthy)
        .unwrap();
    assert_eq!(take_mapping_visits(), 0);
}
#[test]
fn successful_partial_cleanup_releases_reservation_and_full_retry_recertifies() {
    let (directory, base, pages, controller, _, prior) = locked_fixture(8);
    let candidate = extend(&directory, base.clone(), 3);
    let fake = Fake::new(Some(1), []);
    assert!(matches!(
        controller.admit_append_with(&candidate, &base, &prior, &fake),
        Err(Error::Io(_))
    ));
    assert_eq!(controller.live_status().reserved_bytes, pages * 2);
    assert_eq!(controller.live_status().retained_failed_bytes, 0);
    take_mapping_visits();
    controller
        .admit_append_with(&candidate, &base, &prior, &fake)
        .unwrap();
    assert!(take_mapping_visits() > 0);
    assert_eq!(
        *fake.events.borrow(),
        vec![(true, 3), (false, 3), (true, 3)]
    );
}
#[test]
fn prefault_always_uses_full_touch_path_and_demand_stays_unscanned() {
    let (directory, base, pages) = fixture();
    let prefault = ResidencyController::new(ResidencyPolicy::Prefault {
        max_bytes: pages * 8,
    })
    .unwrap();
    let fake = Fake::new(Some(1), []);
    let prior = prefault.admit_with(&base, &fake).unwrap();
    let candidate = extend(&directory, base.clone(), 3);
    take_mapping_visits();
    let ready = prefault
        .admit_append_with(&candidate, &base, &prior, &fake)
        .unwrap();
    assert!(
        take_mapping_visits() >= 6,
        "prefault preflight plus full retouch enumerate all mappings"
    );
    assert_eq!(
        ready.report().prefaulted_bytes,
        ready.report().mapped_bytes.unwrap()
    );
    assert_eq!(ready.report().locked_bytes, 0);
    assert_eq!(fake.calls.get(), 0);
    let demand = ResidencyController::new(ResidencyPolicy::Demand).unwrap();
    let prior = demand.admit_with(&base, &fake).unwrap();
    take_mapping_visits();
    let ready = demand
        .admit_append_with(&candidate, &base, &prior, &fake)
        .unwrap();
    assert_eq!(take_mapping_visits(), 0);
    assert_eq!(ready.report().mapped_bytes, None);
}
#[test]
fn epoch_exhaustion_disables_shortcut_and_serial_poison_never_admits() {
    let (directory, base, _, controller, fake, _) = locked_fixture(8);
    let state = Arc::clone(
        &base
            .mappings()
            .next()
            .unwrap()
            .residency
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .lease
            .state,
    );
    state.ledger.lock().unwrap().health_epoch = NonZeroU64::new(u64::MAX);
    let prior = controller.admit_with(&base, &fake).unwrap();
    let candidate = extend(&directory, base.clone(), 3);
    let failure = Fake::new(Some(1), []);
    assert!(
        controller
            .admit_append_with(&candidate, &base, &prior, &failure)
            .is_err()
    );
    assert_eq!(state.ledger.lock().unwrap().health_epoch, None);
    take_mapping_visits();
    let ready = controller
        .admit_append_with(&candidate, &base, &prior, &fake)
        .unwrap();
    assert!(take_mapping_visits() > 0);
    assert_eq!(ready.report.as_ref().unwrap().health_epoch, None);
    struct Panic;
    impl LockOps for Panic {
        fn lock(&self, _: &Mapping) -> std::io::Result<()> {
            panic!("controlled lock adapter panic")
        }
        fn unlock(&self, _: &Mapping) -> std::io::Result<()> {
            Ok(())
        }
    }
    let next = extend(&directory, candidate.clone(), 4);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || controller.admit_append_with(&next, &candidate, &ready, &Panic)
        ))
        .is_err()
    );
    assert!(matches!(
        controller.admit_append_with(&next, &candidate, &ready, &fake),
        Err(Error::Busy)
    ));
}
