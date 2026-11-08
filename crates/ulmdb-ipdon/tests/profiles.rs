use std::{ops::Bound, path::PathBuf};
use ulmdb::{Environment, Result};
use ulmdb_ipdon::{Address, Prefix, profiles::*};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ulmdb-fields-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn setup() -> (Directory, Environment) {
    let dir = Directory::new();
    let env = Environment::create(&dir.0).unwrap();
    let mut txn = env.begin_write().unwrap();
    Profiles::initialize(&mut txn, 7).unwrap();
    txn.commit().unwrap();
    (dir, env)
}
fn prefix(address: Address, length: u8) -> Prefix {
    Prefix::normalize(address, length).unwrap()
}
fn claim(n: u64, field: u64, prefix: Prefix, priority: u32, value: &[u8]) -> Claim {
    Claim {
        id: ClaimId::from_source(n, b"record"),
        generation: 1,
        field: FieldId(field),
        prefix,
        source: n,
        priority,
        value: value.to_vec(),
        observed_at: 10,
        recorded_at: 11,
        expires_at: 100,
        evidence: [n as u8; 32],
    }
}
fn store(env: &Environment, claims: &[Claim]) -> Vec<Revision> {
    let mut txn = env.begin_write().unwrap();
    let revisions = claims
        .iter()
        .map(|claim| Profiles::upsert(&mut txn, claim).unwrap())
        .collect();
    txn.commit().unwrap();
    revisions
}

#[test]
fn regional_claim_identity_retains_independent_origin_evidence() {
    let first = ClaimId::from_origin_source(1, 7, b"shared-record");
    let second = ClaimId::from_origin_source(2, 7, b"shared-record");
    assert_ne!(first, second);
    assert_eq!(first, ClaimId::from_origin_source(1, 7, b"shared-record"));
    assert_ne!(first, ClaimId::from_source(7, b"shared-record"));
    assert_ne!(first, ClaimId::from_origin_source(1, 8, b"shared-record"));
    assert_ne!(first, ClaimId::from_origin_source(1, 7, b"another-record"));
}
fn table(read: &ulmdb::ReadTxn, name: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    read.range(name, Bound::Unbounded, Bound::Unbounded, false)
        .unwrap()
        .map(|item| {
            let (k, v) = item.unwrap();
            (k.to_vec(), v.to_vec())
        })
        .collect()
}

#[test]
fn trust_precedes_specificity_and_expiry_falls_back_without_cleanup() {
    let (_dir, env) = setup();
    let address = Address::V4(0x0a010203);
    let mut covering = claim(1, 1, prefix(address, 8), 100, b"trusted");
    covering.expires_at = 50;
    let specific = claim(2, 1, prefix(address, 32), 10, b"weak");
    store(&env, &[covering.clone(), specific.clone()]);
    let read = env.begin_read();
    let view = Profiles::view(&read).unwrap();
    assert_eq!(
        view.lookup(address, FieldId(1), 40)
            .unwrap()
            .unwrap()
            .claim_id,
        covering.id
    );
    assert_eq!(
        view.lookup(address, FieldId(1), 50)
            .unwrap()
            .unwrap()
            .claim_id,
        specific.id
    );
    assert!(view.lookup(address, FieldId(1), 100).unwrap().is_none());
    assert_eq!(view.claim(covering.id).unwrap().unwrap().value, b"trusted");
    assert!(view.lookup(address, FieldId(1), 9).unwrap().is_none());
}

#[test]
fn same_prefix_competitors_and_arrival_order_resolve_identically() {
    let (_dir1, env1) = setup();
    let (_dir2, env2) = setup();
    let address = Address::V4(0xc0000201);
    let mut first = claim(1, 2, prefix(address, 24), 10, b"first");
    first.expires_at = 30;
    let second = claim(2, 2, prefix(address, 24), 10, b"second");
    store(&env1, &[first.clone(), second.clone()]);
    store(&env2, &[second.clone(), first.clone()]);
    for now in [20, 30, 40, 100] {
        let read1 = env1.begin_read();
        let read2 = env2.begin_read();
        let v1 = Profiles::view(&read1).unwrap();
        let v2 = Profiles::view(&read2).unwrap();
        assert_eq!(
            v1.lookup(address, FieldId(2), now).unwrap(),
            v2.lookup(address, FieldId(2), now).unwrap()
        );
    }
    let read = env1.begin_read();
    let view = Profiles::view(&read).unwrap();
    assert_eq!(
        view.lookup(address, FieldId(2), 20)
            .unwrap()
            .unwrap()
            .source,
        1
    );
    assert_eq!(
        view.lookup(address, FieldId(2), 30)
            .unwrap()
            .unwrap()
            .source,
        2
    );
}

#[test]
fn refresh_writes_only_freshness_and_preserves_old_snapshot() {
    let (_dir, env) = setup();
    let address = Address::V4(0xcb007109);
    let claim = claim(1, 3, prefix(address, 24), 20, &vec![b'a'; 8000]);
    let rev = store(&env, std::slice::from_ref(&claim))[0];
    let old = env.begin_read();
    let before: Vec<_> = [
        b"ipdon.fields.v1.claims".as_slice(),
        b"ipdon.fields.v1.buckets",
        b"ipdon.fields.v1.catalog",
        b"ipdon.fields.v1.withdrawn",
    ]
    .iter()
    .map(|name| table(&old, name))
    .collect();
    let mut txn = env.begin_write().unwrap();
    assert!(
        Profiles::refresh(
            &mut txn,
            claim.id,
            rev,
            Freshness {
                observed_at: 90,
                recorded_at: 91,
                expires_at: 190
            }
        )
        .unwrap()
    );
    assert!(
        !Profiles::refresh(
            &mut txn,
            claim.id,
            rev,
            Freshness {
                observed_at: 90,
                recorded_at: 91,
                expires_at: 190
            }
        )
        .unwrap()
    );
    txn.commit().unwrap();
    let current = env.begin_read();
    for (i, name) in [
        b"ipdon.fields.v1.claims".as_slice(),
        b"ipdon.fields.v1.buckets",
        b"ipdon.fields.v1.catalog",
        b"ipdon.fields.v1.withdrawn",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(before[i], table(&current, name));
    }
    assert_ne!(
        table(&old, b"ipdon.fields.v1.freshness"),
        table(&current, b"ipdon.fields.v1.freshness")
    );
    let old_view = Profiles::view(&old).unwrap();
    let view = Profiles::view(&current).unwrap();
    assert!(old_view.lookup(address, FieldId(3), 120).unwrap().is_none());
    let selected = view.lookup(address, FieldId(3), 120).unwrap().unwrap();
    assert_eq!(selected.revision, rev);
    assert_eq!(selected.expires_at, 190);
    assert_eq!(selected.policy_version, 7);
    assert_eq!(selected.evidence, claim.evidence);
}

#[test]
fn refresh_dirties_less_storage_than_replacing_large_claim_content() {
    let (_dir, env) = setup();
    let mut c = claim(1, 1, prefix(Address::V4(1), 32), 10, &vec![b'x'; 8000]);
    let revision = store(&env, std::slice::from_ref(&c))[0];
    let mut replay = env.begin_write().unwrap();
    assert_eq!(Profiles::upsert(&mut replay, &c).unwrap(), revision);
    assert_eq!(
        replay.dirty_bytes(),
        0,
        "identical claim replay must not dirty pages"
    );
    replay.abort();
    let mut refresh = env.begin_write().unwrap();
    Profiles::refresh(
        &mut refresh,
        c.id,
        revision,
        Freshness {
            observed_at: 20,
            recorded_at: 21,
            expires_at: 200,
        },
    )
    .unwrap();
    let refresh_bytes = refresh.dirty_bytes();
    refresh.abort();
    let mut replace = env.begin_write().unwrap();
    c.value[0] = b'y';
    c.generation = 2;
    c.recorded_at = 21;
    Profiles::upsert(&mut replace, &c).unwrap();
    let replace_bytes = replace.dirty_bytes();
    replace.abort();
    assert!(refresh_bytes > 0);
    assert!(
        refresh_bytes < replace_bytes,
        "refresh={refresh_bytes}, replacement={replace_bytes}"
    );
}

#[test]
fn revision_and_time_fences_prevent_stale_refresh_and_resurrection() {
    let (_dir, env) = setup();
    let address = Address::V4(1);
    let original = claim(1, 1, prefix(address, 32), 1, b"old");
    let old_rev = store(&env, std::slice::from_ref(&original))[0];
    let mut next = original.clone();
    next.value = b"new".to_vec();
    next.generation = 2;
    next.recorded_at = 20;
    let next_rev = store(&env, std::slice::from_ref(&next))[0];
    assert_ne!(old_rev, next_rev);
    let mut txn = env.begin_write().unwrap();
    assert!(
        Profiles::refresh(
            &mut txn,
            next.id,
            old_rev,
            Freshness {
                observed_at: 30,
                recorded_at: 31,
                expires_at: 130
            }
        )
        .is_err()
    );
    assert!(Profiles::withdraw_at(&mut txn, next.id, next_rev, 40).unwrap());
    assert!(
        Profiles::refresh(
            &mut txn,
            next.id,
            next_rev,
            Freshness {
                observed_at: 50,
                recorded_at: 51,
                expires_at: 150
            }
        )
        .is_err()
    );
    next.recorded_at = 30;
    assert!(Profiles::upsert(&mut txn, &next).is_err());
    next.recorded_at = 50;
    next.generation = 3;
    let reintroduced = Profiles::upsert(&mut txn, &next).unwrap();
    assert_ne!(reintroduced, next_rev);
    assert!(
        Profiles::refresh(
            &mut txn,
            next.id,
            next_rev,
            Freshness {
                observed_at: 60,
                recorded_at: 61,
                expires_at: 160
            }
        )
        .is_err()
    );
    assert!(
        Profiles::refresh(
            &mut txn,
            next.id,
            reintroduced,
            Freshness {
                observed_at: 9,
                recorded_at: 61,
                expires_at: 160
            }
        )
        .is_err()
    );
    txn.commit().unwrap();
    let read = env.begin_read();
    assert_eq!(
        Profiles::view(&read)
            .unwrap()
            .claim(next.id)
            .unwrap()
            .unwrap()
            .recorded_at,
        50
    );
}

#[test]
fn repeated_withdrawal_advances_floor_and_retains_exact_revision() {
    let (_dir, env) = setup();
    let c = claim(1, 1, prefix(Address::V4(1), 32), 1, b"old");
    let revision = store(&env, std::slice::from_ref(&c))[0];
    let mut txn = env.begin_write().unwrap();
    assert!(Profiles::withdraw_at(&mut txn, c.id, revision, 20).unwrap());
    assert!(!Profiles::withdraw_at(&mut txn, c.id, revision, 20).unwrap());
    assert!(Profiles::withdraw_at(&mut txn, c.id, revision, 40).unwrap());
    assert!(Profiles::withdraw_at(&mut txn, c.id, revision, 30).is_err());
    assert!(Profiles::withdraw_at(&mut txn, c.id, Revision([1; 32]), 50).is_err());
    let mut delayed = c;
    delayed.generation = 2;
    delayed.recorded_at = 30;
    assert!(Profiles::upsert(&mut txn, &delayed).is_err());
    delayed.recorded_at = 41;
    Profiles::upsert(&mut txn, &delayed).unwrap();
    txn.commit().unwrap();
    let read = env.begin_read();
    assert_eq!(
        Profiles::view(&read)
            .unwrap()
            .claim(delayed.id)
            .unwrap()
            .unwrap()
            .recorded_at,
        41
    );
}

#[test]
fn generations_allow_same_second_replacement_and_fence_retired_content() {
    let (_dir, env) = setup();
    let original = claim(1, 1, prefix(Address::V4(1), 32), 1, b"old");
    let first = store(&env, std::slice::from_ref(&original))[0];
    let mut next = original.clone();
    next.generation = 2;
    next.value = b"corrected".to_vec();
    next.observed_at = 9; // New knowledge may correct an earlier effective time.
    let mut txn = env.begin_write().unwrap();
    let second = Profiles::upsert(&mut txn, &next).unwrap();
    assert_ne!(first, second);
    assert_eq!(next.recorded_at, original.recorded_at);
    let mut conflicting = next.clone();
    conflicting.value = b"same-generation-conflict".to_vec();
    conflicting.recorded_at += 1;
    assert!(Profiles::upsert(&mut txn, &conflicting).is_err());
    assert!(Profiles::withdraw_at(&mut txn, next.id, second, next.recorded_at).unwrap());
    let mut stale = original;
    stale.recorded_at = 50; // A newer wall clock cannot revive an old generation.
    assert!(Profiles::upsert(&mut txn, &stale).is_err());
    assert!(Profiles::upsert(&mut txn, &next).is_err());
    next.generation = 3;
    let third = Profiles::upsert(&mut txn, &next).unwrap();
    assert_ne!(third, second);
    assert!(
        Profiles::refresh(
            &mut txn,
            next.id,
            second,
            Freshness {
                observed_at: 20,
                recorded_at: 21,
                expires_at: 100
            }
        )
        .is_err()
    );
    txn.commit().unwrap();
    let read = env.begin_read();
    let selected = Profiles::view(&read)
        .unwrap()
        .lookup(Address::V4(1), FieldId(1), 20)
        .unwrap()
        .unwrap();
    assert_eq!(selected.generation, 3);
    assert_eq!(selected.value, b"corrected");
}

#[test]
fn refresh_accepts_tied_recorded_time_but_rejects_ambiguous_freshness() {
    let (_dir, env) = setup();
    let mut c = claim(1, 1, prefix(Address::V4(1), 32), 1, b"unchanged");
    c.recorded_at = 20;
    let revision = store(&env, std::slice::from_ref(&c))[0];
    let mut txn = env.begin_write().unwrap();
    let fresh = Freshness {
        observed_at: 11,
        recorded_at: 20,
        expires_at: 100,
    };
    assert!(Profiles::refresh(&mut txn, c.id, revision, fresh).unwrap());
    assert!(!Profiles::refresh(&mut txn, c.id, revision, fresh).unwrap());
    assert!(
        Profiles::refresh(
            &mut txn,
            c.id,
            revision,
            Freshness {
                expires_at: 200,
                ..fresh
            }
        )
        .is_err()
    );
    assert!(
        Profiles::refresh(
            &mut txn,
            c.id,
            revision,
            Freshness {
                observed_at: 10,
                recorded_at: 21,
                expires_at: 100
            }
        )
        .is_err()
    );
    txn.commit().unwrap();
    let read = env.begin_read();
    assert_eq!(
        Profiles::view(&read)
            .unwrap()
            .claim(c.id)
            .unwrap()
            .unwrap()
            .observed_at,
        11
    );
}

#[test]
fn moving_claim_updates_only_old_and_new_prefixes_and_fields() {
    let (_dir, env) = setup();
    let a = Address::V4(0x0a000001);
    let b = Address::V4(0xc0000201);
    let mut moving = claim(1, 1, prefix(a, 24), 1, b"move");
    let unaffected = claim(2, 3, prefix(b, 24), 1, b"stable");
    store(&env, &[moving.clone(), unaffected.clone()]);
    moving.field = FieldId(2);
    moving.generation = 2;
    moving.prefix = prefix(b, 32);
    moving.recorded_at = 20;
    store(&env, std::slice::from_ref(&moving));
    let read = env.begin_read();
    let view = Profiles::view(&read).unwrap();
    assert!(view.lookup(a, FieldId(1), 30).unwrap().is_none());
    assert!(view.lookup(b, FieldId(1), 30).unwrap().is_none());
    assert_eq!(
        view.lookup(b, FieldId(2), 30).unwrap().unwrap().claim_id,
        moving.id
    );
    assert_eq!(
        view.lookup(b, FieldId(3), 30).unwrap().unwrap().claim_id,
        unaffected.id
    );
}

#[test]
fn ipv6_batch_retains_input_order_unknowns_and_explicit_false() {
    let (_dir, env) = setup();
    let a = Address::V6(0x20010db8000000000000000000000001);
    let b = Address::V6(0x20010db9000000000000000000000001);
    store(
        &env,
        &[
            claim(1, 1, prefix(a, 32), 10, b"false"),
            claim(2, 2, prefix(b, 32), 10, b"NL"),
        ],
    );
    let read = env.begin_read();
    let view = Profiles::view(&read).unwrap();
    let results = view
        .batch(
            &[a, b, a],
            &[FieldId(2), FieldId(1), FieldId(3)],
            20,
            BatchLimits::default(),
        )
        .unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0], results[2]);
    assert_eq!(results[1].address, b);
    assert!(results[0].fields[0].is_none());
    assert_eq!(results[0].fields[1].unwrap().value, b"false");
    assert!(results[0].fields[2].is_none());
    assert_eq!(results[1].fields[0].unwrap().value, b"NL");
    assert!(
        view.batch(
            &[a, b],
            &[FieldId(1), FieldId(2)],
            20,
            BatchLimits { max_results: 3 }
        )
        .is_err()
    );
    assert!(
        view.batch(&[a], &[FieldId(0)], 20, BatchLimits::default())
            .is_err()
    );
}

#[test]
fn policy_and_input_errors_leave_parent_transaction_usable() {
    let (_dir, env) = setup();
    let c = claim(1, 1, prefix(Address::V4(1), 32), 1, b"okay");
    let mut txn = env.begin_write().unwrap();
    Profiles::initialize(&mut txn, 7).unwrap();
    assert!(Profiles::initialize(&mut txn, 8).is_err());
    let mut invalid = c.clone();
    invalid.value = vec![0; MAX_VALUE_BYTES + 1];
    assert!(Profiles::upsert(&mut txn, &invalid).is_err());
    invalid = c.clone();
    invalid.expires_at = 9;
    assert!(Profiles::upsert(&mut txn, &invalid).is_err());
    invalid = c.clone();
    invalid.source = 0;
    assert!(Profiles::upsert(&mut txn, &invalid).is_err());
    invalid = c.clone();
    invalid.generation = 0;
    assert!(Profiles::upsert(&mut txn, &invalid).is_err());
    Profiles::upsert(&mut txn, &c).unwrap();
    txn.commit().unwrap();
    let read = env.begin_read();
    assert_eq!(
        Profiles::view(&read)
            .unwrap()
            .claim(c.id)
            .unwrap()
            .unwrap()
            .value,
        b"okay"
    );
}

#[test]
fn bucket_capacity_failure_rolls_back_replacement_and_can_commit_other_work() {
    let (_dir, env) = setup();
    let target = prefix(Address::V4(0), 0);
    let mut txn = env.begin_write().unwrap();
    for n in 1..=MAX_CLAIMS_PER_PREFIX as u64 {
        Profiles::upsert(&mut txn, &claim(n, 1, target, 1, b"full")).unwrap();
    }
    let mut moving = claim(1000, 2, prefix(Address::V4(1), 32), 1, b"original");
    Profiles::upsert(&mut txn, &moving).unwrap();
    moving.field = FieldId(1);
    moving.generation = 2;
    moving.prefix = target;
    moving.recorded_at = 20;
    assert!(Profiles::upsert(&mut txn, &moving).is_err());
    txn.commit().unwrap();
    let read = env.begin_read();
    let view = Profiles::view(&read).unwrap();
    let current = view.claim(moving.id).unwrap().unwrap();
    assert_eq!(current.field, FieldId(2));
    assert_eq!(current.recorded_at, 11);
    assert_eq!(
        view.lookup(Address::V4(1), FieldId(1), 20)
            .unwrap()
            .unwrap()
            .source,
        1
    );
}

#[test]
fn reopen_preserves_claims_refresh_and_withdrawal_floor() {
    let (dir, env) = setup();
    let c = claim(1, 1, prefix(Address::V4(7), 32), 1, b"reopen");
    let revision = store(&env, std::slice::from_ref(&c))[0];
    let mut txn = env.begin_write().unwrap();
    Profiles::withdraw_at(&mut txn, c.id, revision, 200).unwrap();
    txn.commit().unwrap();
    drop(env);
    let reopened = Environment::open(&dir.0).unwrap();
    let mut txn = reopened.begin_write().unwrap();
    let mut delayed = c;
    delayed.generation = 2;
    delayed.recorded_at = 150;
    assert!(Profiles::upsert(&mut txn, &delayed).is_err());
    delayed.recorded_at = 201;
    Profiles::upsert(&mut txn, &delayed).unwrap();
    txn.commit().unwrap();
    let read = reopened.begin_read();
    assert_eq!(
        Profiles::view(&read)
            .unwrap()
            .claim(delayed.id)
            .unwrap()
            .unwrap()
            .recorded_at,
        201
    );
}

#[test]
fn malformed_current_claim_values_fail_without_panicking() -> Result<()> {
    let (_dir, env) = setup();
    let c = claim(1, 1, prefix(Address::V4(1), 32), 1, b"okay");
    store(&env, std::slice::from_ref(&c));
    for bytes in [vec![], vec![1; 30], vec![1; 99], vec![1; 100], vec![0; 200]] {
        let mut txn = env.begin_write()?;
        txn.put(b"ipdon.fields.v1.claims", &c.id.0, &bytes)?;
        txn.commit()?;
        let read = env.begin_read();
        let view = Profiles::view(&read)?;
        assert!(view.claim(c.id).is_err());
    }
    Ok(())
}
