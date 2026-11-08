use super::*;

fn boot() -> Boot {
    Boot {
        uuid: [7; 16],
        epoch: 9,
        commit: 100,
        kv_pages: 3,
        tiny_pages: 1,
    }
}
fn at(slot: u32, generation: u64) -> Reference {
    Reference {
        segment: 1,
        offset: (generation << 28) | ((u64::from(slot) + 1) * PAGE as u64),
    }
}
fn write(s: &ReplayState, allocations: Vec<LiteralSlot>, root: Reference) -> Record {
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
        retired: Vec::new(),
    }
}
fn literal(slot: u32, generation: u64, expected: u64) -> LiteralSlot {
    LiteralSlot {
        key: VersionKey { slot, generation },
        expected_previous_generation: expected,
        ranges: vec![(0, PAGE as u16)],
        bytes: vec![generation as u8; PAGE],
    }
}
fn decoded(b: Boot, r: &Record) -> Decoded {
    let e = encode(b, r).unwrap();
    decode(b, &e.bytes, r.parent.end().unwrap()).unwrap()
}
fn rehash(bytes: &mut [u8]) {
    let end = bytes.len() - 32;
    let mut h = Sha256::new();
    h.update(b"ULMDB-RESIDENT-LITERAL-RECORD-V1\0");
    h.update(&bytes[..end]);
    bytes[end..].copy_from_slice(&h.finalize());
}
#[test]
fn exact_roundtrip_collect_preserves_business_commit_and_reuse_requires_prior_retire() {
    let b = boot();
    let mut s = ReplayState::new(b).unwrap();
    let first = write(&s, vec![literal(0, 1, 0)], at(0, 1));
    let d = decoded(b, &first);
    assert_eq!(first, d.record);
    s.apply(&d).unwrap();
    let second = write(&s, vec![literal(1, 2, 0)], at(1, 2));
    s.apply(&decoded(b, &second)).unwrap();
    let collect = Record {
        uuid: b.uuid,
        epoch: b.epoch,
        lsn: s.selected().lsn + 1,
        sequence: s.selected().sequence,
        commit: s.commit(),
        parent_commit: s.commit(),
        parent: s.selected(),
        root: s.root(),
        next_database: 1,
        kind: Kind::Collect,
        allocations: vec![],
        retired: vec![VersionKey {
            slot: 0,
            generation: 1,
        }],
    };
    let old_commit = s.commit();
    let old_sequence = s.selected().sequence;
    s.apply(&decoded(b, &collect)).unwrap();
    assert_eq!(s.commit(), old_commit);
    assert_eq!(s.selected().sequence, old_sequence);
    assert_eq!(s.slot(0), Some(SlotState::Retired(1)));
    let reuse = write(&s, vec![literal(0, 7, 1)], at(0, 7));
    s.apply(&decoded(b, &reuse)).unwrap();
    assert_eq!(s.slot(0), Some(SlotState::Live(7)));
    assert_eq!(s.selected().lsn, 4);
    assert_eq!(s.selected().sequence, 3);
}
#[test]
fn prepared_transition_drop_and_all_invalid_changes_leave_state_identical() {
    let b = boot();
    let mut s = ReplayState::new(b).unwrap();
    let r = write(&s, vec![literal(0, 1, 0)], at(0, 1));
    let d = decoded(b, &r);
    let before = s.selected();
    {
        let t = s.prepare(&d).unwrap();
        assert_eq!(t.slot(0), Some(SlotState::Live(1)));
    }
    assert_eq!(s.selected(), before);
    assert_eq!(s.slot(0), Some(SlotState::Free));
    s.apply(&d).unwrap();
    let snapshot = (s.selected(), s.commit(), s.root(), s.slots().to_vec());
    for kind in 0..5 {
        let mut wrong = write(&s, vec![literal(1, 2, 0)], at(1, 2));
        match kind {
            0 => wrong.allocations[0] = literal(0, 2, 1), // Live is never reusable.
            1 => wrong.allocations[0].expected_previous_generation = 1,
            2 => wrong.retired.push(VersionKey {
                slot: 2,
                generation: 1,
            }),
            3 => wrong.root = at(2, 4),
            _ => wrong.next_database = 0,
        }
        let e = encode(b, &wrong);
        if let Ok(e) = e {
            let x = decode(b, &e.bytes, wrong.parent.end().unwrap()).unwrap();
            assert!(s.apply(&x).is_err());
        }
        assert_eq!(
            (s.selected(), s.commit(), s.root(), s.slots().to_vec()),
            snapshot
        );
    }
}
#[test]
fn no_same_frame_retirement_reuse_or_collect_root_changes() {
    let b = boot();
    let mut s = ReplayState::new(b).unwrap();
    s.apply(&decoded(b, &write(&s, vec![literal(0, 1, 0)], at(0, 1))))
        .unwrap();
    let mut r = write(&s, vec![literal(0, 2, 1)], at(0, 2));
    r.retired.push(VersionKey {
        slot: 0,
        generation: 1,
    });
    assert!(encode(b, &r).is_err());
    let mut r = write(&s, vec![], Reference::NULL);
    r.kind = Kind::Collect;
    r.sequence = s.selected().sequence;
    r.commit = s.commit();
    let d = decoded(b, &r);
    assert!(s.apply(&d).is_err());
}
#[test]
fn every_record_byte_is_digest_bound_and_rehashed_structural_damage_rejects() {
    let b = boot();
    let s = ReplayState::new(b).unwrap();
    let r = write(&s, vec![literal(0, 1, 0)], at(0, 1));
    let good = encode(b, &r).unwrap().bytes;
    for i in 0..good.len() {
        let mut bad = good.clone();
        bad[i] ^= 1;
        assert!(decode(b, &bad, DATA_START).is_err(), "byte{i}");
    }
    for (p, bytes) in [
        (11, vec![1]),
        (208, vec![1]),
        (184, u32::MAX.to_le_bytes().to_vec()),
        (256 + 6, vec![1]),
        (256 + 24, 4095u32.to_le_bytes().to_vec()),
        (256 + 32, 1u16.to_le_bytes().to_vec()),
    ] {
        let mut bad = good.clone();
        bad[p..p + bytes.len()].copy_from_slice(&bytes);
        rehash(&mut bad);
        assert!(decode(b, &bad, DATA_START).is_err(), "offset{p}");
    }
    assert!(decode(b, &good, DATA_START + 8).is_err());
    for n in 0..good.len() {
        assert!(decode(b, &good[..n], DATA_START).is_err());
    }
}
#[test]
fn tiny_ranges_padding_budgets_and_generations_are_strict() {
    let b = boot();
    let s = ReplayState::new(b).unwrap();
    let mut a = literal(3, 1, 0);
    a.ranges = vec![(0, 3), (5, 7), (100, 1)];
    let r = write(&s, vec![a], Reference::NULL);
    let e = encode(b, &r).unwrap();
    assert_eq!(decoded(b, &r).record, r);
    assert!(matches!(
        encode_limited(b, &r, e.bytes.len() - 1),
        Err(Error::LimitExceeded)
    ));
    assert!(matches!(
        decode_limited(b, &e.bytes, DATA_START, e.bytes.len() - 1),
        Err(Error::LimitExceeded)
    ));
    let padding = HEADER + 32 + 3 * 4 + PAGE;
    let mut bad = e.bytes;
    bad[padding] = 1;
    rehash(&mut bad);
    assert!(decode(b, &bad, DATA_START).is_err());
    for (generation, expected) in [(0, 0), (MAX_GENERATION + 1, 0), (1, 1), (2, 1)] {
        let mut x = r.clone();
        x.allocations[0].key.generation = generation;
        x.allocations[0].expected_previous_generation = expected;
        assert!(encode(b, &x).is_err());
    }
    for ranges in [
        vec![],
        vec![(0, 0)],
        vec![(0, 4095), (4094, 2)],
        vec![(4095, 2)],
        vec![(0, 1); 257],
    ] {
        let mut x = r.clone();
        x.allocations[0].ranges = ranges;
        assert!(encode(b, &x).is_err());
    }
}

#[test]
fn independent_python_wire_fixture_and_scanner_bounds() {
    // Generated independently with Python struct.pack + hashlib; no Rust codec
    // was used to calculate either expected digest (review/golden.json).
    let b = boot();
    let s = ReplayState::new(b).unwrap();
    let hex = |bytes: &[u8]| bytes.iter().map(|n| format!("{n:02x}")).collect::<String>();
    assert_eq!(
        hex(&genesis(&b).unwrap().digest),
        "20f78a2904c745be8296b01269bf57e28f1fe4b69c58bdb135aa2d4ad439d654"
    );
    let r = write(&s, vec![literal(0, 1, 0)], at(0, 1));
    let e = encode(b, &r).unwrap();
    assert_eq!(e.bytes.len(), 4424);
    assert_eq!(
        hex(&e.digest),
        "0aa426459802161b0a26c3f8e62d2559b1eafc3f628301089c83fdf8a1251065"
    );
    assert_eq!(
        hex(&Sha256::digest(&e.bytes)),
        "1d5434c544a184c4847c277a09ef0112a279037fef749ba2748a5eda0fb9c4c5"
    );
    let good = e.bytes[..HEADER].to_vec();
    assert_eq!(frame_len(&good).unwrap(), e.bytes.len());
    for len in [0, HEADER - 1, HEADER + 1] {
        assert!(frame_len(&vec![0; len]).is_err());
    }
    for (p, bytes) in [
        (16, u64::MAX.to_le_bytes().to_vec()),
        (16, ((MAX_RECORD_BYTES + 8) as u64).to_le_bytes().to_vec()),
        (16, 289u64.to_le_bytes().to_vec()),
        (16, 288u64.to_le_bytes().to_vec()), // counts cannot fit
        (184, u32::MAX.to_le_bytes().to_vec()),
        (188, u32::MAX.to_le_bytes().to_vec()),
        (192, 0u32.to_le_bytes().to_vec()),
        (192, u32::MAX.to_le_bytes().to_vec()),
        (196, (MAX_SLOTS as u32).to_le_bytes().to_vec()),
        (208, vec![1]),
    ] {
        let mut header = good.clone();
        header[p..p + bytes.len()].copy_from_slice(&bytes);
        assert!(frame_len(&header).is_err(), "offset{p}");
    }
    let mut wrong = e.bytes;
    wrong[200..208].copy_from_slice(&(DATA_START + 1).to_le_bytes());
    rehash(&mut wrong);
    assert!(decode(b, &wrong, DATA_START + 1).is_err());
}

#[test]
fn counter_exhaustion_and_replay_parent_fail_before_any_state_change() {
    let b = boot();
    let mut s = ReplayState::new(b).unwrap();
    let first = write(
        &s,
        vec![literal(0, MAX_GENERATION, 0)],
        at(0, MAX_GENERATION),
    );
    s.apply(&decoded(b, &first)).unwrap();
    let snapshot = (
        s.selected(),
        s.commit(),
        s.root(),
        s.next_database(),
        s.slots().to_vec(),
    );
    for case in 0..7 {
        let mut r = write(&s, vec![literal(1, 1, 0), literal(2, 2, 0)], at(1, 1));
        match case {
            0 => r.parent.digest[0] ^= 1,
            1 => r.next_database = 0,
            2 => r.parent_commit += 1,
            3 => r.lsn += 1,
            4 => r.sequence += 1,
            5 => r.retired.push(VersionKey {
                slot: 0,
                generation: MAX_GENERATION - 1,
            }),
            _ => r.allocations[1].expected_previous_generation = 1,
        }
        if let Ok(e) = encode(b, &r) {
            let d = decode(b, &e.bytes, r.parent.end().unwrap()).unwrap();
            assert!(s.apply(&d).is_err());
        }
        assert_eq!(
            (
                s.selected(),
                s.commit(),
                s.root(),
                s.next_database(),
                s.slots().to_vec()
            ),
            snapshot
        );
    }
    let mut overflow = b;
    overflow.commit = u128::MAX;
    let mut r = write(&s, vec![], s.root());
    r.parent = overflow.genesis().unwrap();
    r.lsn = 1;
    r.sequence = 1;
    r.parent_commit = u128::MAX;
    r.commit = 0;
    assert!(matches!(encode(overflow, &r), Err(Error::LimitExceeded)));
    for invalid in [
        Boot { kv_pages: 0, ..b },
        Boot {
            kv_pages: MAX_SLOTS as u32,
            tiny_pages: 1,
            ..b
        },
        Boot {
            kv_pages: u32::MAX,
            tiny_pages: 1,
            ..b
        },
        Boot { epoch: 0, ..b },
        Boot { uuid: [0; 16], ..b },
    ] {
        assert!(ReplayState::new(invalid).is_err());
    }
    assert_eq!(
        ReplayState::new(Boot {
            kv_pages: 4096,
            tiny_pages: 0,
            ..b
        })
        .unwrap()
        .slots()
        .len(),
        4096
    );
}

#[test]
fn empty_maintenance_is_distinct_from_business_sequence_and_never_retires_current_root() {
    let b = boot();
    let mut s = ReplayState::new(b).unwrap();
    s.apply(&decoded(b, &write(&s, vec![literal(0, 1, 0)], at(0, 1))))
        .unwrap();
    let mut collect = write(&s, vec![], s.root());
    collect.kind = Kind::Collect;
    collect.sequence = s.selected().sequence;
    collect.commit = s.commit();
    s.apply(&decoded(b, &collect)).unwrap();
    assert_eq!(
        (s.selected().lsn, s.selected().sequence, s.commit()),
        (2, 1, 101)
    );
    collect.parent = s.selected();
    collect.lsn += 1;
    collect.retired.push(VersionKey {
        slot: 0,
        generation: 1,
    });
    assert!(s.apply(&decoded(b, &collect)).is_err());
    assert_eq!(
        (s.selected().lsn, s.root(), s.slot(0)),
        (2, at(0, 1), Some(SlotState::Live(1)))
    );
}

#[test]
fn encode_owned_binds_same_receipt_without_cloning_or_decoding_literal_pages() {
    let b = boot();
    let mut s = ReplayState::new(b).unwrap();
    let r = write(&s, vec![literal(0, 1, 0)], at(0, 1));
    let original_allocation = r.allocations[0].bytes.as_ptr();
    let (encoded, owned) = encode_owned(b, r).unwrap();
    let from_wire = decode(b, &encoded.bytes, DATA_START).unwrap();
    assert_eq!(owned.record(), from_wire.record());
    assert_eq!(owned.receipt(), from_wire.receipt());
    assert_eq!(
        owned.record().allocations[0].bytes.as_ptr(),
        original_allocation
    );
    s.apply(&owned).unwrap();
    let (mut unbound, old_receipt) = owned.into_parts();
    unbound.allocations[0].bytes[0] ^= 1;
    assert_eq!(s.selected(), old_receipt);
    // No public constructor can turn the mutable consumed record back into the
    // old receipt's token. Encoding it anew creates a different receipt digest.
    let (_, rebound) = encode_owned(b, unbound).unwrap();
    assert_ne!(rebound.receipt().digest, old_receipt.digest);
}

#[test]
fn expanded_capacity_high_slots_replay_and_one_past_stays_below_generation() {
    assert_eq!(MAX_SLOTS, 65_534);
    assert_eq!(
        (MAX_SLOTS as u64 + 1) * PAGE as u64,
        (1u64 << 28) - PAGE as u64
    );
    assert!((MAX_SLOTS as u64 + 1) * PAGE as u64 <= PHYSICAL_MASK);
    assert!((MAX_SLOTS as u64 + 2) * PAGE as u64 > PHYSICAL_MASK);
    let b = Boot {
        kv_pages: MAX_SLOTS as u32,
        tiny_pages: 0,
        ..boot()
    };
    let mut state = ReplayState::new(b).unwrap();
    let high = MAX_SLOTS as u32 - 1;
    let r = write(
        &state,
        vec![literal(4096, MAX_GENERATION, 0), literal(high, 7, 0)],
        at(high, 7),
    );
    let token = decoded(b, &r);
    assert_eq!(
        frame_len(&encode(b, &r).unwrap().bytes[..HEADER]).unwrap(),
        token.receipt().length as usize
    );
    state.apply(&token).unwrap();
    assert_eq!(state.slot(4096), Some(SlotState::Live(MAX_GENERATION)));
    assert_eq!(state.slot(high), Some(SlotState::Live(7)));
    assert_eq!(state.slot(MAX_SLOTS as u32), None);
    assert_eq!(
        catalog_key(b, state.root()).unwrap(),
        Some(VersionKey {
            slot: high,
            generation: 7
        })
    );
    let one_past = Reference {
        segment: 1,
        offset: (7 << 28) | ((MAX_SLOTS as u64 + 1) * PAGE as u64),
    };
    assert!(catalog_key(b, one_past).is_err());
    assert_eq!(one_past.offset >> 28, 7);
}

#[test]
fn expanded_capacity_forged_boot_and_scanner_counts_reject_before_allocation() {
    let b = Boot {
        kv_pages: MAX_SLOTS as u32,
        tiny_pages: 0,
        ..boot()
    };
    for invalid in [
        Boot {
            kv_pages: MAX_SLOTS as u32 + 1,
            ..b
        },
        Boot { tiny_pages: 1, ..b },
        Boot {
            kv_pages: u32::MAX,
            tiny_pages: 1,
            ..b
        },
        Boot {
            kv_pages: 0,
            tiny_pages: MAX_SLOTS as u32,
            ..b
        },
    ] {
        assert!(invalid.slots().is_err());
        assert!(ReplayState::new(invalid).is_err());
    }
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
        (184, MAX_SLOTS as u32 + 1),
        (188, u32::MAX),
    ] {
        let mut forged = bytes.clone();
        forged[field..field + 4].copy_from_slice(&value.to_le_bytes());
        assert!(frame_len(&forged[..HEADER]).is_err());
        rehash(&mut forged);
        assert!(decode(b, &forged, DATA_START).is_err());
    }
}
