use super::*;
use crate::hybrid::redo::Patch;
fn boot() -> Boot {
    Boot {
        uuid: [7; 16],
        epoch: 2,
        commit: 100,
        kv_pages: 4,
        tiny_pages: 1,
    }
}
fn at(slot: u32, generation: u64) -> Reference {
    Reference {
        segment: 1,
        offset: (generation << 28) | ((u64::from(slot) + 1) * PAGE as u64),
    }
}
fn literal(slot: u32, generation: u64, previous_generation: u64) -> Allocation {
    Allocation {
        destination: VersionKey { slot, generation },
        previous_generation,
        ranges: vec![(0, PAGE as u16)],
        body: Body::Literal(vec![slot as u8; PAGE]),
    }
}
fn write(s: &ReplayState, allocations: Vec<Allocation>, root: Reference) -> Record {
    Record {
        uuid: s.boot().uuid,
        epoch: s.boot().epoch,
        lsn: s.selected().lsn + 1,
        sequence: s.selected().sequence + 1,
        commit: s.commit() + 1,
        parent_commit: s.commit(),
        parent: s.selected(),
        root,
        next_database: s.next_database(),
        kind: Kind::Write,
        allocations,
        retired: vec![],
    }
}
fn encoded(s: &ReplayState, r: Record) -> Decoded {
    let (e, d) = encode_owned(s.boot(), r).unwrap();
    let copy = decode(s.boot(), &e.bytes, d.receipt().offset).unwrap();
    assert_eq!(copy.record(), d.record());
    assert_eq!(copy.receipt(), d.receipt());
    assert_eq!(frame_len(&e.bytes[..HEADER]).unwrap(), e.bytes.len());
    d
}
#[test]
fn compact_frame_binds_parent_root_recipes_and_state_without_postimage_expansion() {
    let b = boot();
    assert_ne!(genesis(&b).unwrap(), b.genesis().unwrap());
    let mut s = ReplayState::new(b).unwrap();
    let first = write(&s, vec![literal(0, 1, 0)], at(0, 1));
    s.apply(&encoded(&s, first)).unwrap();
    let compact = Allocation {
        destination: VersionKey {
            slot: 1,
            generation: 7,
        },
        previous_generation: 0,
        ranges: vec![(0, 4096)],
        body: Body::Copy {
            source: VersionKey {
                slot: 0,
                generation: 1,
            },
            patches: vec![Patch {
                offset: 20,
                bytes: vec![42, 43],
            }],
        },
    };
    let r = write(&s, vec![compact], at(1, 7));
    let length = r.encoded_length(b).unwrap();
    assert_eq!(length, 368);
    s.apply(&encoded(&s, r)).unwrap();
    assert_eq!(s.slot(1), Some(SlotState::Live(7)));
}
#[test]
fn late_invalid_event_drop_and_source_retirement_never_change_state() {
    let b = boot();
    let mut s = ReplayState::new(b).unwrap();
    let r = write(&s, vec![literal(0, 1, 0)], at(0, 1));
    s.apply(&encoded(&s, r)).unwrap();
    let compact = Allocation {
        destination: VersionKey {
            slot: 1,
            generation: 2,
        },
        previous_generation: 0,
        ranges: vec![(0, 4096)],
        body: Body::Copy {
            source: VersionKey {
                slot: 0,
                generation: 1,
            },
            patches: vec![],
        },
    };
    let good = encoded(&s, write(&s, vec![compact.clone()], at(1, 2)));
    let before = (s.selected(), s.root(), s.slots().to_vec());
    drop(s.prepare(&good).unwrap());
    assert_eq!((s.selected(), s.root(), s.slots().to_vec()), before);
    let mut bad = write(&s, vec![compact.clone(), literal(2, 2, 1)], at(1, 2));
    let d = encoded(&s, bad.clone());
    assert!(s.apply(&d).is_err());
    assert_eq!((s.selected(), s.root(), s.slots().to_vec()), before);
    bad.allocations.truncate(1);
    bad.retired.push(VersionKey {
        slot: 0,
        generation: 1,
    });
    assert!(encode(b, &bad).is_err());
    let mut bad = compact;
    if let Body::Copy { source, .. } = &mut bad.body {
        source.generation = 99;
    }
    assert!(
        s.apply(&encoded(&s, write(&s, vec![bad], at(1, 2))))
            .is_err()
    );
}
#[test]
fn canonical_patch_shapes_and_family_boundaries_reject() {
    let b = boot();
    let s = ReplayState::new(b).unwrap();
    let mut r = write(&s, vec![literal(0, 1, 0)], at(0, 1));
    r.parent = b.genesis().unwrap();
    assert!(encode(b, &r).is_err());
    let r = write(&s, vec![literal(0, 1, 0)], at(0, 1));
    let e = encode(b, &r).unwrap();
    assert!(super::super::wal_codec::decode(b, &e.bytes, DATA_START).is_err());
    assert!(
        decode(
            b,
            &super::super::wal_codec::encode(
                b,
                &super::super::wal_codec::Record {
                    uuid: b.uuid,
                    epoch: b.epoch,
                    lsn: 1,
                    sequence: 1,
                    commit: 101,
                    parent_commit: 100,
                    parent: b.genesis().unwrap(),
                    root: Reference::NULL,
                    next_database: 1,
                    kind: Kind::Write,
                    allocations: vec![],
                    retired: vec![]
                }
            )
            .unwrap()
            .bytes,
            DATA_START
        )
        .is_err()
    );
    for patches in [
        vec![Patch {
            offset: 0,
            bytes: vec![],
        }],
        vec![Patch {
            offset: 4095,
            bytes: vec![1, 2],
        }],
        vec![
            Patch {
                offset: 0,
                bytes: vec![1],
            },
            Patch {
                offset: 1,
                bytes: vec![2],
            },
        ],
        vec![Patch {
            offset: 0,
            bytes: vec![1; 4096],
        }],
    ] {
        let a = Allocation {
            destination: VersionKey {
                slot: 1,
                generation: 1,
            },
            previous_generation: 0,
            ranges: vec![(0, 4096)],
            body: Body::Copy {
                source: VersionKey {
                    slot: 0,
                    generation: 1,
                },
                patches,
            },
        };
        assert!(encode(b, &write(&s, vec![a], at(1, 1))).is_err());
    }
}

fn copy(slot: u32, generation: u64, previous_generation: u64, source: VersionKey) -> Allocation {
    Allocation {
        destination: VersionKey { slot, generation },
        previous_generation,
        ranges: vec![(0, 4096)],
        body: Body::Copy {
            source,
            patches: vec![Patch {
                offset: 20,
                bytes: vec![42, 43],
            }],
        },
    }
}
fn rehash(bytes: &mut [u8]) {
    let end = bytes.len() - FOOTER;
    let mut h = Sha256::new();
    h.update(b"ULMDB-RESIDENT-COMPACT-RECORD-V2\0");
    h.update(&bytes[..end]);
    bytes[end..].copy_from_slice(&h.finalize());
}
#[test]
fn independent_python_golden_and_owned_token_preserve_literal_allocation() {
    let hex = |bytes: &[u8]| bytes.iter().map(|n| format!("{n:02x}")).collect::<String>();
    let b = boot();
    let mut s = ReplayState::new(b).unwrap();
    assert_eq!(
        hex(&genesis(&b).unwrap().digest),
        "7c93fbb4160ac40dad61af68768e9dbee71ba0293dc01b8e2b6406da348f89b2"
    );
    let r = write(&s, vec![literal(0, 1, 0)], at(0, 1));
    let ptr = match &r.allocations[0].body {
        Body::Literal(b) => b.as_ptr(),
        _ => unreachable!(),
    };
    let (e, d) = encode_owned(b, r).unwrap();
    assert_eq!(e.bytes.len(), 4432);
    assert_eq!(
        hex(&e.digest),
        "f98ef12cafda40be9c4ea49ad544d2e2367768d22367555f471a016934feb06b"
    );
    assert_eq!(
        hex(&Sha256::digest(&e.bytes)),
        "6e1962b1b061fcacb86d60b87fc11b5c0404e71614a006b95a0ac2b518fe1e97"
    );
    match &d.record().allocations[0].body {
        Body::Literal(b) => assert_eq!(b.as_ptr(), ptr),
        _ => unreachable!(),
    };
    s.apply(&d).unwrap();
    let r = write(
        &s,
        vec![copy(
            1,
            7,
            0,
            VersionKey {
                slot: 0,
                generation: 1,
            },
        )],
        at(1, 7),
    );
    let e = encode(b, &r).unwrap();
    assert_eq!(e.bytes.len(), 368);
    assert_eq!(
        hex(&e.digest),
        "d88cce863106dd9b04ed591394ffca97e8a5118f9196e7a3c0daf28724bccaa5"
    );
    assert_eq!(
        hex(&Sha256::digest(&e.bytes)),
        "fc5ac63487cf0c54c4bec016438ae5e685ba71f3fff17ae0b2e3c188973ba601"
    );
}
#[test]
fn every_copy_frame_byte_and_rehashed_structure_are_checked() {
    let b = boot();
    let mut s = ReplayState::new(b).unwrap();
    s.apply(&encoded(&s, write(&s, vec![literal(0, 1, 0)], at(0, 1))))
        .unwrap();
    let r = write(
        &s,
        vec![copy(
            1,
            7,
            0,
            VersionKey {
                slot: 0,
                generation: 1,
            },
        )],
        at(1, 7),
    );
    let good = encode(b, &r).unwrap().bytes;
    let offset = r.parent.end().unwrap();
    for i in 0..good.len() {
        let mut bad = good.clone();
        bad[i] ^= 1;
        assert!(decode(b, &bad, offset).is_err(), "byte{i}");
    }
    for (p, bytes) in [
        (11, vec![1]),
        (208, vec![1]),
        (184, u32::MAX.to_le_bytes().to_vec()),
        (256 + 6, vec![3]),
        (256 + 4, 0u16.to_le_bytes().to_vec()),
        (256 + 24, u32::MAX.to_le_bytes().to_vec()),
        (256 + 28, 16u32.to_le_bytes().to_vec()),
        (256 + 48 + 8, 0u64.to_le_bytes().to_vec()),
        (256 + 64, 4095u32.to_le_bytes().to_vec()),
        (256 + 64 + 4, 0u32.to_le_bytes().to_vec()),
        (330, vec![1]),
    ] {
        let mut bad = good.clone();
        bad[p..p + bytes.len()].copy_from_slice(&bytes);
        rehash(&mut bad);
        assert!(decode(b, &bad, offset).is_err(), "offset{p}");
    }
    for n in 0..good.len() {
        assert!(decode(b, &good[..n], offset).is_err());
    }
    assert!(decode(b, &good, offset + 8).is_err());
    assert!(matches!(
        decode_limited(b, &good, offset, good.len() - 1),
        Err(Error::LimitExceeded)
    ));
    assert!(matches!(
        encode_limited(b, &r, good.len() - 1),
        Err(Error::LimitExceeded)
    ));
    let header = &good[..HEADER];
    assert_eq!(frame_len(header).unwrap(), good.len());
    for (p, bytes) in [
        (16, u64::MAX.to_le_bytes().to_vec()),
        (16, ((MAX_RECORD_BYTES + 8) as u64).to_le_bytes().to_vec()),
        (16, 288u64.to_le_bytes().to_vec()),
        (188, u32::MAX.to_le_bytes().to_vec()),
        (192, 0u32.to_le_bytes().to_vec()),
    ] {
        let mut bad = header.to_vec();
        bad[p..p + bytes.len()].copy_from_slice(&bytes);
        assert!(frame_len(&bad).is_err());
    }
}
#[test]
fn collect_retires_exact_version_before_gapped_reuse_and_preserves_business_identity() {
    let b = boot();
    let mut s = ReplayState::new(b).unwrap();
    s.apply(&encoded(&s, write(&s, vec![literal(0, 1, 0)], at(0, 1))))
        .unwrap();
    s.apply(&encoded(
        &s,
        write(
            &s,
            vec![copy(
                1,
                7,
                0,
                VersionKey {
                    slot: 0,
                    generation: 1,
                },
            )],
            at(1, 7),
        ),
    ))
    .unwrap();
    let mut collect = write(&s, vec![], s.root());
    collect.kind = Kind::Collect;
    collect.sequence = s.selected().sequence;
    collect.commit = s.commit();
    collect.retired.push(VersionKey {
        slot: 0,
        generation: 1,
    });
    let before = (
        s.selected().sequence,
        s.commit(),
        s.root(),
        s.next_database(),
    );
    s.apply(&encoded(&s, collect)).unwrap();
    assert_eq!(
        (
            s.selected().sequence,
            s.commit(),
            s.root(),
            s.next_database()
        ),
        before
    );
    assert_eq!(s.slot(0), Some(SlotState::Retired(1)));
    let dead_source = write(
        &s,
        vec![copy(
            2,
            1,
            0,
            VersionKey {
                slot: 0,
                generation: 1,
            },
        )],
        at(2, 1),
    );
    assert!(s.apply(&encoded(&s, dead_source)).is_err());
    let wrong = write(&s, vec![literal(0, 9, 0)], at(0, 9));
    assert!(s.apply(&encoded(&s, wrong)).is_err());
    let r = write(
        &s,
        vec![copy(
            0,
            9,
            1,
            VersionKey {
                slot: 1,
                generation: 7,
            },
        )],
        at(0, 9),
    );
    s.apply(&encoded(&s, r)).unwrap();
    assert_eq!(
        (s.selected().lsn, s.selected().sequence, s.slot(0)),
        (4, 3, Some(SlotState::Live(9)))
    );
}
#[test]
fn tiny_literals_ranges_metadata_counters_and_parent_caps_are_bounded() {
    let b = boot();
    let mut s = ReplayState::new(b).unwrap();
    let mut tiny = literal(4, 1, 0);
    tiny.ranges = vec![(0, 3), (16, 7), (4095, 1)];
    let mut r = write(&s, vec![tiny.clone()], Reference::NULL);
    r.next_database = 9;
    s.apply(&encoded(&s, r)).unwrap();
    let mut wrong = write(&s, vec![], Reference::NULL);
    wrong.next_database = 8;
    assert!(s.apply(&encoded(&s, wrong)).is_err());
    for ranges in [
        vec![],
        vec![(0, 0)],
        vec![(4095, 2)],
        vec![(0, 2), (1, 3)],
        vec![(0, 1); 257],
    ] {
        let mut a = tiny.clone();
        a.ranges = ranges;
        assert!(encode(b, &write(&s, vec![a], Reference::NULL)).is_err());
    }
    let mut a = copy(
        2,
        1,
        0,
        VersionKey {
            slot: 4,
            generation: 1,
        },
    );
    assert!(encode(b, &write(&s, vec![a.clone()], at(2, 1))).is_err());
    a.destination.slot = 4;
    assert!(encode(b, &write(&s, vec![a], Reference::NULL)).is_err());
    let mut wrong = write(&s, vec![], Reference::NULL);
    wrong.parent.length = (MAX_RECORD_BYTES + 8) as u64;
    assert!(encode(b, &wrong).is_err());
    let mut overflow = b;
    overflow.commit = u128::MAX;
    let r = Record {
        uuid: b.uuid,
        epoch: b.epoch,
        lsn: 1,
        sequence: 1,
        commit: 0,
        parent_commit: u128::MAX,
        parent: genesis(&overflow).unwrap(),
        root: Reference::NULL,
        next_database: 1,
        kind: Kind::Write,
        allocations: vec![],
        retired: vec![],
    };
    assert!(matches!(encode(overflow, &r), Err(Error::LimitExceeded)));
}

#[test]
fn opaque_token_cannot_transfer_between_equal_uuid_but_different_pool_geometry() {
    let b = boot();
    let mut state = ReplayState::new(b).unwrap();
    state
        .apply(&encoded(
            &state,
            write(&state, vec![literal(0, 1, 0)], at(0, 1)),
        ))
        .unwrap();
    // Non-genesis parent receipts alone do not encode their Boot geometry. The
    // private token must also retain the exact Boot used to hash its header.
    let record = write(&state, vec![], state.root());
    let different = Boot { kv_pages: 3, ..b };
    let (_, foreign) = encode_owned(different, record).unwrap();
    let selected = state.selected();
    assert!(state.apply(&foreign).is_err());
    assert_eq!(state.selected(), selected);
}

#[test]
fn owned_budget_encoder_preserves_canonical_tokens_and_rejects_invalid_records() {
    let b = boot();
    let mut state = ReplayState::new(b).unwrap();
    let record = write(&state, vec![literal(0, 1, 0)], at(0, 1));
    let expected = encode(b, &record).unwrap();
    for limit in [0, HEADER + FOOTER, expected.bytes.len() - 1] {
        assert!(matches!(
            encode_owned_limited(b, record.clone(), limit),
            Err(Error::LimitExceeded)
        ));
    }
    let (encoded, token) = encode_owned_limited(b, record.clone(), expected.bytes.len()).unwrap();
    assert_eq!(encoded.bytes, expected.bytes);
    assert_eq!(encoded.digest, expected.digest);
    let parsed = decode(b, &encoded.bytes, DATA_START).unwrap();
    assert_eq!(parsed.record(), token.record());
    assert_eq!(parsed.receipt(), token.receipt());
    let selected = state.selected();
    drop(state.prepare(&token).unwrap());
    assert_eq!(state.selected(), selected);
    state.apply(&token).unwrap();
    assert_eq!(state.selected(), token.receipt());
    let mut malformed = record;
    malformed.allocations[0].ranges = vec![(4095, 2)];
    assert!(encode_owned_limited(b, malformed, MAX_RECORD_BYTES).is_err());
}

#[test]
fn expanded_capacity_high_slots_copy_sources_and_boundary_replay() {
    let b = Boot {
        kv_pages: MAX_SLOTS as u32,
        tiny_pages: 0,
        ..boot()
    };
    let mut state = ReplayState::new(b).unwrap();
    let source = MAX_SLOTS as u32 - 2;
    let target = MAX_SLOTS as u32 - 1;
    state
        .apply(&encoded(
            &state,
            write(
                &state,
                vec![literal(source, MAX_GENERATION, 0)],
                at(source, MAX_GENERATION),
            ),
        ))
        .unwrap();
    let copy = Allocation {
        destination: VersionKey {
            slot: target,
            generation: 1,
        },
        previous_generation: 0,
        ranges: vec![(0, PAGE as u16)],
        body: Body::Copy {
            source: VersionKey {
                slot: source,
                generation: MAX_GENERATION,
            },
            patches: vec![Patch {
                offset: 4095,
                bytes: vec![77],
            }],
        },
    };
    let token = encoded(&state, write(&state, vec![copy], at(target, 1)));
    state.apply(&token).unwrap();
    assert_eq!(state.slot(target), Some(SlotState::Live(1)));
    assert_eq!(state.slot(source), Some(SlotState::Live(MAX_GENERATION)));
    assert_eq!(state.slot(MAX_SLOTS as u32), None);
    assert!(catalog_key(b, at(MAX_SLOTS as u32, 1)).is_err());
}

#[test]
fn expanded_capacity_rl2_forged_counts_and_out_of_pool_events_fail_closed() {
    let b = Boot {
        kv_pages: MAX_SLOTS as u32,
        tiny_pages: 0,
        ..boot()
    };
    let state = ReplayState::new(b).unwrap();
    let r = write(
        &state,
        vec![literal(MAX_SLOTS as u32 - 1, 1, 0)],
        at(MAX_SLOTS as u32 - 1, 1),
    );
    let bytes = encode(b, &r).unwrap().bytes;
    for (field, value) in [
        (192, MAX_SLOTS as u32 + 1),
        (196, 1),
        (184, u32::MAX),
        (188, MAX_SLOTS as u32 + 1),
    ] {
        let mut forged = bytes.clone();
        forged[field..field + 4].copy_from_slice(&value.to_le_bytes());
        assert!(frame_len(&forged[..HEADER]).is_err());
        rehash(&mut forged);
        assert!(decode(b, &forged, DATA_START).is_err());
    }
    assert!(
        encode(
            b,
            &write(&state, vec![literal(MAX_SLOTS as u32, 1, 0)], state.root())
        )
        .is_err()
    );
    assert!(ReplayState::new(Boot { tiny_pages: 1, ..b }).is_err());
    assert!(
        ReplayState::new(Boot {
            kv_pages: u32::MAX,
            tiny_pages: 1,
            ..b
        })
        .is_err()
    );
    assert_eq!(state.slot(MAX_SLOTS as u32 - 1), Some(SlotState::Free));
}
