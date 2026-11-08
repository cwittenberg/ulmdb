//! Actual mapped range owners + kernel locks; counter rejects hidden full scans.
use crate::{
    Arena, Error, Manifest, ResidencyController, ResidencyPolicy, WriteStore,
    range::{PriorAdmission, RangeSnapshot, RangeStore},
    table::take_mapping_visits,
};
use std::path::PathBuf;
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "ulmdb-range-admission-{}",
            crate::new_identity().unwrap()
        ));
        crate::create_layout(&p).unwrap();
        Self(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn one_map_pages() -> u64 {
    let page = crate::residency::os::page_size().unwrap() as u64;
    let bytes = 4096 + 16 + b"actual immutable mapped range".len() as u64;
    let covered = 8192 % page + bytes;
    covered.div_ceil(page) * page
}
fn next(m: &Manifest) -> Manifest {
    let mut n = m.clone_metadata();
    n.previous = m.generation;
    n.parent = m.commit;
    n.generation = crate::new_identity().unwrap();
    n.commit = n.generation;
    n.sequence += 1;
    n
}
fn append(
    store: &mut RangeStore,
    base: &RangeSnapshot,
    c: &ResidencyController,
    prior: bool,
) -> crate::Result<RangeSnapshot> {
    let mut a = Arena::new(base.segments.clone(), base.manifest.next_segment)?;
    a.put_payload(b"actual immutable mapped range")?;
    let p = prior.then(|| PriorAdmission::new(&base.segments, &base.admission));
    store.commit_arena_with_admission(a, next(&base.manifest), &base.manifest, c, p)
}
#[test]
fn required_locked257_actual_range_appends_never_enumerate_old_maps() {
    let d = Dir::new();
    let c = ResidencyController::new(ResidencyPolicy::RequiredLocked {
        max_bytes: one_map_pages() * 1024,
    })
    .unwrap();
    let (mut store, mut state) = RangeStore::create(&d.0, &c).unwrap();
    for n in 1..=257 {
        take_mapping_visits();
        let candidate = append(&mut store, &state, &c, true).unwrap();
        let visits = take_mapping_visits();
        if n == 1 {
            assert!(visits >= 1, "canonical empty parent takes full admission");
        } else {
            assert_eq!(visits, 0, "append{n} enumerated old mappings");
        }
        assert!(!candidate.manifest.segments.is_materialized());
        if [1, 17, 257].contains(&n) {
            let expected = c.admit(&candidate.segments).unwrap().report();
            assert!(take_mapping_visits() >= n);
            assert_eq!(candidate.admission.report(), expected);
        }
        state = candidate;
    }
    assert_eq!(
        c.live_status().reserved_bytes,
        state.admission.report().locked_bytes
    );
    // The preserved generic API takes full admission rather than assuming a prior.
    let a = Arena::new(state.segments.clone(), state.manifest.next_segment).unwrap();
    take_mapping_visits();
    let generic = store
        .commit_arena(a, next(&state.manifest), &state.manifest, &c)
        .unwrap();
    assert!(take_mapping_visits() >= 257);
    assert_eq!(generic.admission.report(), state.admission.report());
    drop(state);
    drop(generic);
    drop(store);
    take_mapping_visits();
    let (_store, _state) = RangeStore::open_checked(&d.0, &c, |_, _| Ok(())).unwrap();
    assert!(
        take_mapping_visits() >= 257,
        "reopen must fully admit loaded owners"
    );
}
#[test]
fn prefault_always_retouches_all_candidate_maps_and_demand_remains_unscanned() {
    for policy in [
        ResidencyPolicy::Prefault {
            max_bytes: one_map_pages() * 8,
        },
        ResidencyPolicy::Demand,
    ] {
        let d = Dir::new();
        let c = ResidencyController::new(policy).unwrap();
        let (mut store, mut state) = RangeStore::create(&d.0, &c).unwrap();
        for n in 1..=4 {
            take_mapping_visits();
            let candidate = append(&mut store, &state, &c, true).unwrap();
            let visits = take_mapping_visits();
            if policy == ResidencyPolicy::Demand {
                assert_eq!(visits, 0);
            } else {
                assert!(visits >= n);
                assert_eq!(
                    candidate.admission.report().prefaulted_bytes,
                    candidate.admission.report().mapped_bytes.unwrap()
                );
            }
            state = candidate;
        }
    }
}
#[test]
fn denied_delta_retains_old_locks_length_and_writer_readiness() {
    let d = Dir::new();
    let c = ResidencyController::new(ResidencyPolicy::RequiredLocked {
        max_bytes: one_map_pages(),
    })
    .unwrap();
    let (mut store, initial) = RangeStore::create(&d.0, &c).unwrap();
    let state = append(&mut store, &initial, &c, true).unwrap();
    let length = std::fs::metadata(d.0.join("RANGE.pack")).unwrap().len();
    let before = c.live_status();
    take_mapping_visits();
    assert!(matches!(
        append(&mut store, &state, &c, true),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(take_mapping_visits(), 0);
    assert!(!store.is_poisoned());
    assert_eq!(c.live_status(), before);
    assert_eq!(
        std::fs::metadata(d.0.join("RANGE.pack")).unwrap().len(),
        length
    );
    let a = Arena::new(state.segments.clone(), state.manifest.next_segment).unwrap();
    let result = store
        .commit_arena_with_admission(
            a,
            next(&state.manifest),
            &state.manifest,
            &c,
            Some(PriorAdmission::new(&state.segments, &state.admission)),
        )
        .unwrap();
    assert_eq!(result.manifest.sequence, 2);
    assert_eq!(result.admission.report(), state.admission.report());
}
#[test]
fn foreign_base_is_rejected_before_sealing_and_stale_token_uses_full_fallback() {
    let d = Dir::new();
    let c = ResidencyController::new(ResidencyPolicy::RequiredLocked {
        max_bytes: one_map_pages() * 8,
    })
    .unwrap();
    let (mut store, initial) = RangeStore::create(&d.0, &c).unwrap();
    let state = append(&mut store, &initial, &c, true).unwrap();
    let length = std::fs::metadata(d.0.join("RANGE.pack")).unwrap().len();
    let mut a = Arena::new(state.segments.clone(), state.manifest.next_segment).unwrap();
    a.put_payload(b"candidate").unwrap();
    assert!(matches!(
        store.commit_arena_with_admission(
            a,
            next(&state.manifest),
            &state.manifest,
            &c,
            Some(PriorAdmission::new(&initial.segments, &initial.admission))
        ),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        std::fs::metadata(d.0.join("RANGE.pack")).unwrap().len(),
        length
    );
    assert!(!store.is_poisoned());
    let mut a = Arena::new(state.segments.clone(), state.manifest.next_segment).unwrap();
    a.put_payload(b"candidate").unwrap();
    take_mapping_visits();
    let candidate = store
        .commit_arena_with_admission(
            a,
            next(&state.manifest),
            &state.manifest,
            &c,
            Some(PriorAdmission::new(&state.segments, &initial.admission)),
        )
        .unwrap();
    assert!(
        take_mapping_visits() >= 2,
        "stale evidence must not use append shortcut"
    );
    assert_eq!(
        candidate.admission.report(),
        c.admit(&candidate.segments).unwrap().report()
    );
}
