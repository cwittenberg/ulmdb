use super::*;
use crate::resident::{Config, Pool};
fn setup() -> (std::sync::Arc<Pool>, View, ReplayState, Reference) {
    let p = Pool::create(Config {
        kv_pages: 8,
        tiny_pages: 2,
        max_reserved_bytes: 1 << 20,
        max_metadata_bytes: 64 << 20,
        max_views: 32,
    })
    .unwrap();
    let empty = p.empty().unwrap();
    let mut a = p.begin(empty).unwrap();
    let mut root = Reference::NULL;
    for k in 0..32u64 {
        root = ulmdb_kv::put(&mut a, root, &k.to_be_bytes(), &k.to_le_bytes()).unwrap();
    }
    let prepared = a.freeze().unwrap();
    let boot = Boot {
        uuid: [7; 16],
        epoch: 1,
        commit: 1,
        kv_pages: 8,
        tiny_pages: 2,
    };
    let mut state = ReplayState::new(boot).unwrap();
    let r = wal_codec::Record {
        uuid: boot.uuid,
        epoch: 1,
        lsn: 1,
        sequence: 1,
        commit: 2,
        parent_commit: 1,
        parent: state.selected(),
        root,
        next_database: 1,
        kind: wal_codec::Kind::Write,
        allocations: prepared.literals(&state).unwrap(),
        retired: vec![],
    };
    let (_, decoded) = wal_codec::encode_owned(boot, r).unwrap();
    state.apply(&decoded).unwrap();
    (p, prepared.activate(), state, root)
}
fn source_key(root: Reference) -> VersionKey {
    VersionKey {
        slot: ((root.offset & wal_codec::PHYSICAL_MASK) / PAGE as u64 - 1) as u32,
        generation: root.offset >> 28,
    }
}
fn after(
    p: &std::sync::Arc<Pool>,
    view: &View,
    state: &ReplayState,
    root: Reference,
) -> (super::super::Prepared, Reference) {
    let mut a = p.begin(view.clone()).unwrap();
    let root = ulmdb_kv::put(&mut a, root, &12u64.to_be_bytes(), &999u64.to_le_bytes()).unwrap();
    let prepared = a.freeze().unwrap();
    assert_eq!(prepared.literals(state).unwrap().len(), 1);
    (prepared, root)
}
#[test]
fn actual_kv_copy_roundtrip_matches_literal_and_keeps_old_borrow() {
    let (p, view, state, root) = setup();
    let old = ulmdb_kv::get(&view, root, &12u64.to_be_bytes())
        .unwrap()
        .unwrap();
    let sources = Sources::from_selected_materialization(&view, &state).unwrap();
    let (prepared, newroot) = after(&p, &view, &state, root);
    let literals = prepared.literals(&state).unwrap();
    let allocation = sources
        .choose(&literals[0], Some(source_key(root)))
        .unwrap();
    assert!(matches!(allocation.body, Body::Copy { .. }));
    let r = Recipes {
        authority: sources.authority,
        allocations: vec![allocation],
    };
    let encoded = encode(&r).unwrap();
    let decoded = decode(&encoded).unwrap();
    assert_eq!(decoded, r);
    assert_eq!(encode(&decoded).unwrap(), encoded);
    let materialized = sources.materialize(&decoded).unwrap();
    assert_eq!(materialized.allocations(), literals);
    let full = Recipes {
        authority: r.authority,
        allocations: vec![Allocation {
            body: Body::Literal(literals[0].bytes.clone()),
            ..r.allocations[0].clone()
        }],
    };
    assert!(encoded.len() < encode(&full).unwrap().len() / 8);
    println!(
        "RP2_MECHANISM compact={} literal={} actual_mapped_kv_keys=32 changed_keys=1",
        encoded.len(),
        encode(&full).unwrap().len()
    );
    let new = prepared.activate();
    assert_eq!(
        ulmdb_kv::get(&new, newroot, &12u64.to_be_bytes()).unwrap(),
        Some(999u64.to_le_bytes().as_slice())
    );
    assert_eq!(old, 12u64.to_le_bytes());
}
#[test]
fn randomized_diff_and_full_postimage_fallback_preserve_every_byte() {
    let (p, view, state, root) = setup();
    let sources = Sources::from_selected_materialization(&view, &state).unwrap();
    let (prepared, _) = after(&p, &view, &state, root);
    let mut literal = prepared.literals(&state).unwrap().remove(0);
    let base = sources.page(source_key(root)).unwrap();
    let mut seed = 73u64;
    for trial in 0..1000 {
        literal.bytes.copy_from_slice(base);
        for _ in 0..trial % 65 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            literal.bytes[seed as usize % PAGE] ^= (seed >> 32) as u8;
        }
        let a = sources.choose(&literal, Some(source_key(root))).unwrap();
        let r = Recipes {
            authority: sources.authority,
            allocations: vec![a],
        };
        let encoded = encode(&r).unwrap();
        let back = decode(&encoded).unwrap();
        assert_eq!(
            sources.materialize(&back).unwrap().allocations()[0],
            literal
        );
    }
    literal
        .bytes
        .iter_mut()
        .zip(base)
        .for_each(|(a, b)| *a = !*b);
    let a = sources.choose(&literal, Some(source_key(root))).unwrap();
    assert!(matches!(a.body, Body::Literal(_)));
}
#[test]
fn stale_parent_generation_private_source_and_destination_aba_reject() {
    let (p, view, state, root) = setup();
    let sources = Sources::from_selected_materialization(&view, &state).unwrap();
    let (prepared, _) = after(&p, &view, &state, root);
    let a = sources
        .choose(
            &prepared.literals(&state).unwrap()[0],
            Some(source_key(root)),
        )
        .unwrap();
    let r = Recipes {
        authority: sources.authority,
        allocations: vec![a],
    };
    for case in 0..7 {
        let mut bad = r.clone();
        match case {
            0 => bad.authority.parent.digest[0] ^= 1,
            1 => bad.authority.boot.epoch += 1,
            2 => {
                if let Body::Copy { source, .. } = &mut bad.allocations[0].body {
                    source.generation += 1;
                }
            }
            3 => {
                let dest = bad.allocations[0].destination;
                if let Body::Copy { source, .. } = &mut bad.allocations[0].body {
                    *source = dest;
                }
            }
            4 => bad.allocations[0].previous_generation = 1,
            5 => bad.allocations[0].destination = source_key(root),
            _ => bad.allocations[0].destination.generation = 0,
        }
        assert!(sources.materialize(&bad).is_err(), "case {case}");
    }
}
#[test]
fn all_wire_bytes_reserved_padding_and_count_mutations_reject() {
    let (p, view, state, root) = setup();
    let sources = Sources::from_selected_materialization(&view, &state).unwrap();
    let (prepared, _) = after(&p, &view, &state, root);
    let r = Recipes {
        authority: sources.authority,
        allocations: vec![
            sources
                .choose(
                    &prepared.literals(&state).unwrap()[0],
                    Some(source_key(root)),
                )
                .unwrap(),
        ],
    };
    let encoded = encode(&r).unwrap();
    for i in 0..encoded.len() {
        let mut b = encoded.clone();
        b[i] ^= 1;
        assert!(decode(&b).is_err());
    }
    for at in [10, 156, 160 + 7, 160 + 32, 152] {
        let mut b = encoded.clone();
        b[at] = 255;
        let end = b.len() - 32;
        let mut h = sha2::Sha256::new();
        use sha2::Digest;
        h.update(b"ULMDB-RESIDENT-RECIPE-BLOCK-V2\0");
        h.update(&b[..end]);
        b[end..].copy_from_slice(&h.finalize());
        assert!(decode(&b).is_err());
    }
}
#[test]
fn patches_are_exact_nonoverlapping_minimal_and_cannot_cross_page() {
    let (p, view, state, root) = setup();
    let sources = Sources::from_selected_materialization(&view, &state).unwrap();
    let (prepared, _) = after(&p, &view, &state, root);
    let a = sources
        .choose(
            &prepared.literals(&state).unwrap()[0],
            Some(source_key(root)),
        )
        .unwrap();
    let base = sources.page(source_key(root)).unwrap();
    let mut r = Recipes {
        authority: sources.authority,
        allocations: vec![a],
    };
    for patches in [
        vec![Patch {
            offset: 4096,
            bytes: vec![1],
        }],
        vec![
            Patch {
                offset: 5,
                bytes: vec![1],
            },
            Patch {
                offset: 6,
                bytes: vec![2],
            },
        ],
        vec![Patch {
            offset: 5,
            bytes: vec![],
        }],
        vec![Patch {
            offset: 5,
            bytes: vec![base[5]],
        }],
    ] {
        r.allocations[0].body = Body::Copy {
            source: source_key(root),
            patches,
        };
        assert!(sources.materialize(&r).is_err());
    }
}
#[test]
fn old_reader_cannot_authorize_retired_source_or_reused_generation() {
    let (p, old, mut state, root) = setup();
    let oldkey = source_key(root);
    let borrowed = ulmdb_kv::get(&old, root, &12u64.to_be_bytes())
        .unwrap()
        .unwrap();
    let (prepared, newroot) = after(&p, &old, &state, root);
    let boot = state.boot();
    let r = wal_codec::Record {
        uuid: boot.uuid,
        epoch: boot.epoch,
        lsn: 2,
        sequence: 2,
        commit: 3,
        parent_commit: 2,
        parent: state.selected(),
        root: newroot,
        next_database: 1,
        kind: wal_codec::Kind::Write,
        allocations: prepared.literals(&state).unwrap(),
        retired: vec![],
    };
    let (_, d) = wal_codec::encode_owned(boot, r).unwrap();
    state.apply(&d).unwrap();
    let mut view = prepared.activate();
    let collect = wal_codec::Record {
        uuid: boot.uuid,
        epoch: boot.epoch,
        lsn: 3,
        sequence: 2,
        commit: 3,
        parent_commit: 3,
        parent: state.selected(),
        root: newroot,
        next_database: 1,
        kind: wal_codec::Kind::Collect,
        allocations: vec![],
        retired: vec![oldkey],
    };
    let (_, d) = wal_codec::encode_owned(boot, collect).unwrap();
    state.apply(&d).unwrap();
    // Still readable through both old pinned owners; logical retirement blocks issuance.
    let sources = Sources::from_selected_materialization(&view, &state).unwrap();
    assert!(sources.page(oldkey).is_err());
    assert_eq!(borrowed, 12u64.to_le_bytes());
    let keep = std::collections::BTreeSet::from([source_key(newroot)]);
    view = view.retain(&keep).unwrap();
    drop(old);
    let sources = Sources::from_selected_materialization(&view, &state).unwrap();
    assert!(sources.page(oldkey).is_err());
}
#[test]
fn correctly_rehashed_oversized_parent_receipt_is_rejected_before_allocation() {
    let (_, view, state, _) = setup();
    let sources = Sources::from_selected_materialization(&view, &state).unwrap();
    let r = Recipes {
        authority: sources.authority,
        allocations: vec![],
    };
    let mut b = encode(&r).unwrap();
    b[96..104].copy_from_slice(&((wal_codec::MAX_RECORD_BYTES as u64) + 8).to_le_bytes());
    let end = b.len() - 32;
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(b"ULMDB-RESIDENT-RECIPE-BLOCK-V2\0");
    h.update(&b[..end]);
    b[end..].copy_from_slice(&h.finalize());
    assert!(decode(&b).is_err());
}
