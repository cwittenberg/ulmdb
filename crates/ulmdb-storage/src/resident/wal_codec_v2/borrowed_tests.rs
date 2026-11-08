use super::*;
use crate::{
    WriteStore,
    hybrid::redo::final_patches,
    resident::{Config, Pool},
};
fn boot() -> Boot {
    Boot {
        uuid: [4; 16],
        epoch: 2,
        commit: 19,
        kv_pages: 8,
        tiny_pages: 2,
    }
}
fn header(state: &ReplayState) -> Record {
    let b = state.boot();
    Record {
        uuid: b.uuid,
        epoch: b.epoch,
        lsn: state.selected().lsn + 1,
        sequence: state.selected().sequence + 1,
        commit: state.commit() + 1,
        parent_commit: state.commit(),
        parent: state.selected(),
        root: Reference::NULL,
        next_database: 1,
        kind: Kind::Write,
        allocations: Vec::new(),
        retired: Vec::new(),
    }
}
fn fixture() -> (
    std::sync::Arc<Pool>,
    crate::resident::View,
    Reference,
    ReplayState,
) {
    let b = boot();
    let p = Pool::create(Config {
        kv_pages: 8,
        tiny_pages: 2,
        max_reserved_bytes: 1 << 20,
        max_metadata_bytes: 16 << 20,
        max_views: 32,
    })
    .unwrap();
    let mut a = p.begin(p.empty().unwrap()).unwrap();
    let at = a.allocate(&[0; PAGE], PAGE).unwrap();
    let ready = a.freeze().unwrap();
    let f = ready.slot(0).unwrap();
    let mut state = ReplayState::new(b).unwrap();
    let mut r = header(&state);
    r.allocations.push(Allocation {
        destination: f.key,
        previous_generation: 0,
        ranges: f.ranges.to_vec(),
        body: Body::Literal(f.bytes.to_vec()),
    });
    let (_, d) = encode_owned(b, r).unwrap();
    state.apply(&d).unwrap();
    let view = ready.activate();
    (p, view, at, state)
}
#[test]
fn owned_borrowed_canonical_bytes_tokens_and_minimal_patch_boundaries_match() {
    let (p, base, _at, mut state) = fixture();
    let source = base.members().next().unwrap();
    let mut workspace = RecipeWorkspace::default();
    let mut buffer = Vec::new();
    for case in 0..(70usize + 256 * 5) {
        let mut after = [0; PAGE];
        if case < 64 {
            after[case * 63] = 1;
            after[(case * 63 + 31).min(PAGE - 1)] = 2;
        } else if case >= 70 {
            let mask = (case - 70) % 256;
            let start = [0, 25, 31, 61, 4088][(case - 70) / 256];
            for i in 0..8 {
                if mask & (1 << i) != 0 {
                    after[start + i] = 7;
                }
            }
        } else {
            for (i, b) in after.iter_mut().enumerate() {
                *b = if (i + case).is_multiple_of(case - 63) {
                    7
                } else {
                    0
                };
            }
        }
        let mut a = p.begin(base.clone()).unwrap();
        a.allocate(&after, PAGE).unwrap();
        let candidate = a.freeze().unwrap();
        let h = header(&state);
        let mut builder = RecipeBuilder::new(h.clone(), &candidate, &mut workspace).unwrap();
        builder.committed(0, 0, source, &[0; PAGE]).unwrap();
        let borrowed = builder.finish();
        let f = candidate.slot(0).unwrap();
        let patches = final_patches(&[0; PAGE], &after).unwrap();
        let cost = patches
            .iter()
            .map(|p| 8 + p.bytes.len().next_multiple_of(8))
            .sum::<usize>()
            + 16;
        let mut owned = h;
        owned.allocations.push(Allocation {
            destination: f.key,
            previous_generation: 0,
            ranges: f.ranges.to_vec(),
            body: if cost < PAGE {
                Body::Copy { source, patches }
            } else {
                Body::Literal(after.to_vec())
            },
        });
        let encoded = encode(boot(), &owned).unwrap();
        let checked =
            borrowed::encode_into(boot(), &borrowed, MAX_RECORD_BYTES, &mut buffer).unwrap();
        assert_eq!(buffer, encoded.bytes);
        assert_eq!(checked.receipt.digest, encoded.digest);
        let old = state.selected();
        drop(state.prepare_borrowed(&checked).unwrap());
        assert_eq!(state.selected(), old);
        assert!(workspace_bound(&borrowed));
        drop(borrowed);
        assert!(workspace.retained_bytes() <= MAX_RECORD_BYTES);
    }
}
fn workspace_bound(r: &BorrowedRecord<'_, '_>) -> bool {
    use view::RecordView;
    r.allocation_count() <= MAX_SLOTS
}
#[test]
fn borrowed_token_checks_boot_collect_budget_and_drops_before_selection() {
    let (p, base, at, mut state) = fixture();
    let source = base.members().next().unwrap();
    let mut a = p.begin(base).unwrap();
    a.rewrite(at, &[3; PAGE], PAGE).unwrap();
    let candidate = a.freeze().unwrap();
    let mut w = RecipeWorkspace::default();
    let mut r = header(&state);
    r.kind = Kind::Collect;
    r.sequence = r.parent.sequence;
    r.commit = r.parent_commit;
    let mut builder = RecipeBuilder::new(r, &candidate, &mut w).unwrap();
    builder.literal(0, 0).unwrap();
    let view = builder.finish();
    let mut b = vec![99; 64];
    assert!(borrowed::encode_into(boot(), &view, MAX_RECORD_BYTES, &mut b).is_err());
    assert_eq!(b, vec![99; 64]);
    drop(view);
    let mut builder = RecipeBuilder::new(header(&state), &candidate, &mut w).unwrap();
    builder.committed(0, 0, source, &[0; PAGE]).unwrap();
    let view = builder.finish();
    assert!(matches!(
        borrowed::encode_into(boot(), &view, 1, &mut b),
        Err(Error::LimitExceeded)
    ));
    let mut foreign = boot();
    foreign.tiny_pages += 1;
    let token = borrowed::encode_into(foreign, &view, MAX_RECORD_BYTES, &mut b).unwrap();
    assert!(state.prepare_borrowed(&token).is_err());
    assert_eq!(state.selected().lsn, 1);
}
