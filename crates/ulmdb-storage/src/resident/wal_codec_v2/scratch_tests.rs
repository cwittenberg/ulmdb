use super::*;
fn decoded(state: &ReplayState) -> Decoded {
    let record = Record {
        uuid: state.boot.uuid,
        epoch: state.boot.epoch,
        lsn: state.selected.lsn + 1,
        sequence: state.selected.sequence + 1,
        commit: state.commit + 1,
        parent_commit: state.commit,
        parent: state.selected,
        root: Reference::NULL,
        next_database: 1,
        kind: Kind::Write,
        allocations: vec![Allocation {
            destination: VersionKey {
                slot: 0,
                generation: 1,
            },
            previous_generation: 0,
            ranges: vec![(0, 4096)],
            body: Body::Literal(vec![0; 4096]),
        }],
        retired: vec![],
    };
    encode_owned(state.boot, record).unwrap().1
}
#[test]
fn transition_capacity_reuses_on_rejection_and_ack_without_changing_abort_state() {
    let boot = Boot {
        uuid: [9; 16],
        epoch: 1,
        commit: 1,
        kv_pages: 8,
        tiny_pages: 0,
    };
    let mut state = ReplayState::new(boot).unwrap();
    let d = decoded(&state);
    let parent = state.selected;
    let mut pointer = None;
    for _ in 0..64 {
        let transition = state.prepare(&d).unwrap();
        if let Some(p) = pointer {
            assert_eq!(p, transition.changes.as_ptr());
        } else {
            pointer = Some(transition.changes.as_ptr());
        }
        assert_eq!(transition.changes.len(), 1);
        drop(transition);
        assert_eq!(state.selected, parent);
        assert_eq!(state.slot(0), Some(SlotState::Free));
        assert!(state.scratch.is_empty());
        assert!(state.retained_transition_bytes() <= 8 * std::mem::size_of::<(usize, SlotState)>());
    }
    state.prepare(&d).unwrap().activate();
    assert_eq!(state.slot(0), Some(SlotState::Live(1)));
    assert!(state.scratch.is_empty());
    assert_eq!(Some(state.scratch.as_ptr()), pointer);
    assert!(state.prepare(&d).is_err()); // stale parent rejected before scratch lease
    assert!(state.scratch.is_empty());
}
