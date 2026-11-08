use std::path::PathBuf;
use ulmdb::Environment;
use ulmdb_ipdon::{Address, Prefix, profiles::*};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ipdon-candidates-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn setup(policy: u64) -> (Directory, Environment) {
    let directory = Directory::new();
    let environment = Environment::create(&directory.0).unwrap();
    let mut write = environment.begin_write().unwrap();
    Profiles::initialize(&mut write, policy).unwrap();
    write.commit().unwrap();
    (directory, environment)
}
fn prefix(address: Address, length: u8) -> Prefix {
    Prefix::normalize(address, length).unwrap()
}
fn claim(id: u64, field: u64, prefix: Prefix, priority: u32, value: &[u8]) -> Claim {
    Claim {
        id: ClaimId::from_origin_source(id, 7, b"observation"),
        generation: 1,
        field: FieldId(field),
        prefix,
        source: id,
        priority,
        value: value.to_vec(),
        observed_at: 10,
        recorded_at: 11,
        expires_at: 100,
        evidence: [id as u8; 32],
    }
}
fn store(environment: &Environment, claims: &[Claim]) {
    let mut write = environment.begin_write().unwrap();
    for claim in claims {
        Profiles::upsert(&mut write, claim).unwrap();
    }
    write.commit().unwrap();
}

#[test]
fn contradictory_equal_rank_claims_and_every_covering_prefix_are_returned() {
    let (_directory, environment) = setup(7);
    let address = Address::V4(0xc0000201);
    let broad = claim(1, 1, prefix(address, 16), 100, b"NL");
    let first = claim(2, 1, prefix(address, 24), 50, b"NL");
    let second = claim(3, 1, prefix(address, 24), 50, b"DE");
    let host = claim(4, 1, prefix(address, 32), 10, b"BE");
    let unrelated = claim(5, 1, prefix(Address::V4(0xcb007101), 24), 100, b"US");
    store(
        &environment,
        &[
            second.clone(),
            broad.clone(),
            host.clone(),
            first.clone(),
            unrelated,
        ],
    );
    let read = environment.begin_read();
    let view = Profiles::view(&read).unwrap();
    let page = view
        .covering_candidates(address, FieldId(1), 20, None, CandidateLimits::default())
        .unwrap();
    assert!(page.complete);
    assert!(page.resume.is_none());
    assert_eq!(page.scanned, 4);
    assert_eq!(
        page.candidates
            .iter()
            .map(|v| v.claim_id)
            .collect::<Vec<_>>(),
        vec![host.id, first.id, second.id, broad.id]
    );
    for (selected, expected) in page.candidates.iter().zip([&host, &first, &second, &broad]) {
        assert_eq!(selected.revision, expected.revision().unwrap());
        assert_eq!(selected.evidence, expected.evidence);
        assert_eq!(selected.value, expected.value);
        assert_eq!(selected.policy_version, 7);
    }
    // Existing selected-answer semantics remain trust-first, not enumeration order.
    assert_eq!(
        view.lookup(address, FieldId(1), 20)
            .unwrap()
            .unwrap()
            .claim_id,
        broad.id
    );
    assert_eq!(
        view.batch(&[address], &[FieldId(1)], 20, BatchLimits::default())
            .unwrap()[0]
            .fields[0]
            .unwrap()
            .claim_id,
        broad.id
    );
}

#[test]
fn expired_and_future_candidates_consume_scan_budget_without_hidden_rescans() {
    let (_directory, environment) = setup(7);
    let address = Address::V4(1);
    let mut claims = (1..=7)
        .map(|id| claim(id, 1, prefix(address, 32), 100 - id as u32, b"value"))
        .collect::<Vec<_>>();
    for c in &mut claims[..3] {
        c.expires_at = 20;
    }
    claims[3].observed_at = 50;
    store(&environment, &claims);
    let read = environment.begin_read();
    let view = Profiles::view(&read).unwrap();
    let limits = CandidateLimits {
        max_results: 10,
        max_scan: 2,
    };
    let first = view
        .covering_candidates(address, FieldId(1), 20, None, limits)
        .unwrap();
    assert_eq!(first.scanned, 2);
    assert!(first.candidates.is_empty());
    assert_eq!(first.candidates.capacity(), 0);
    assert!(!first.complete);
    let second = view
        .covering_candidates(address, FieldId(1), 20, first.resume.as_ref(), limits)
        .unwrap();
    assert_eq!(second.scanned, 2);
    assert!(second.candidates.is_empty());
    assert_eq!(second.candidates.capacity(), 0);
    assert!(!second.complete);
    let third = view
        .covering_candidates(address, FieldId(1), 20, second.resume.as_ref(), limits)
        .unwrap();
    assert_eq!(third.scanned, 2);
    assert_eq!(third.candidates.len(), 2);
    assert!(!third.complete);
    let fourth = view
        .covering_candidates(address, FieldId(1), 20, third.resume.as_ref(), limits)
        .unwrap();
    assert_eq!(fourth.scanned, 1);
    assert_eq!(fourth.candidates[0].claim_id, claims[6].id);
    assert!(fourth.complete);
    assert!(fourth.resume.is_none());
    assert_eq!(
        first.scanned + second.scanned + third.scanned + fourth.scanned,
        7
    );
}

#[test]
fn tokens_reject_different_snapshot_address_field_policy_and_freshness_cutoff() {
    let (_directory, environment) = setup(7);
    let address = Address::V4(1);
    store(
        &environment,
        &[
            claim(1, 1, prefix(address, 24), 10, b"first"),
            claim(2, 1, prefix(address, 24), 10, b"second"),
        ],
    );
    let old = environment.begin_read();
    let view = Profiles::view(&old).unwrap();
    let limits = CandidateLimits {
        max_results: 1,
        max_scan: 1,
    };
    let first = view
        .covering_candidates(address, FieldId(1), 20, None, limits)
        .unwrap();
    let token = first.resume.as_ref().unwrap();
    assert!(
        view.covering_candidates(address, FieldId(1), 21, Some(token), limits)
            .is_err()
    );
    assert!(
        view.covering_candidates(Address::V4(2), FieldId(1), 20, Some(token), limits)
            .is_err()
    );
    assert!(
        view.covering_candidates(address, FieldId(2), 20, Some(token), limits)
            .is_err()
    );
    store(
        &environment,
        &[claim(3, 1, prefix(address, 24), 10, b"third")],
    );
    let current = environment.begin_read();
    assert!(
        Profiles::view(&current)
            .unwrap()
            .covering_candidates(address, FieldId(1), 20, Some(token), limits)
            .is_err()
    );
    let (_other_directory, other) = setup(8);
    store(&other, &[claim(1, 1, prefix(address, 24), 10, b"first")]);
    let other_read = other.begin_read();
    assert!(
        Profiles::view(&other_read)
            .unwrap()
            .covering_candidates(address, FieldId(1), 20, Some(token), limits)
            .is_err()
    );
    // The pinned older snapshot remains a valid continuation after publication.
    let rest = view
        .covering_candidates(address, FieldId(1), 20, Some(token), limits)
        .unwrap();
    assert_eq!(rest.candidates.len(), 1);
    assert!(rest.complete);
}

#[test]
fn batch_preserves_ipv4_ipv6_and_duplicate_address_field_order_and_unknowns() {
    let (_directory, environment) = setup(7);
    let v4 = Address::V4(0xc0000201);
    let v6 = Address::V6(0x20010db8000000000000000000000001);
    let first = claim(1, 1, prefix(v4, 24), 10, b"false");
    let second = claim(2, 1, prefix(v6, 32), 10, b"true");
    store(&environment, &[first.clone(), second.clone()]);
    let read = environment.begin_read();
    let view = Profiles::view(&read).unwrap();
    let batch = view
        .batch_candidates(
            &[v6, v4, v6],
            &[FieldId(2), FieldId(1), FieldId(1)],
            20,
            CandidateBatchLimits::default(),
        )
        .unwrap();
    assert!(batch.complete);
    assert_eq!(batch.scanned, 2);
    assert_eq!(batch.rows.len(), 3);
    assert_eq!(batch.rows[0].address, v6);
    assert_eq!(batch.rows[1].address, v4);
    assert_eq!(batch.rows[0], batch.rows[2]);
    for row in &batch.rows {
        assert!(row.fields[0].complete);
        assert!(row.fields[0].candidates.is_empty());
        assert_eq!(row.fields[1], row.fields[2]);
    }
    assert_eq!(batch.rows[1].fields[1].candidates[0].value, b"false");
    assert_eq!(batch.rows[0].fields[1].candidates[0].value, b"true");
}

#[test]
fn aggregate_batch_limits_count_duplicate_outputs_and_leave_explicit_continuations() {
    let (_directory, environment) = setup(7);
    let a = Address::V4(1);
    let b = Address::V4(257);
    store(
        &environment,
        &[
            claim(1, 1, prefix(a, 24), 10, b"a1"),
            claim(2, 1, prefix(a, 24), 10, b"a2"),
            claim(3, 1, prefix(b, 24), 10, b"b"),
        ],
    );
    let read = environment.begin_read();
    let view = Profiles::view(&read).unwrap();
    let limits = CandidateBatchLimits {
        per_query: CandidateLimits {
            max_results: 10,
            max_scan: 10,
        },
        max_queries: 10,
        max_results: 2,
        max_scan: 10,
    };
    let batch = view
        .batch_candidates(&[a, a, b], &[FieldId(1)], 20, limits)
        .unwrap();
    assert!(!batch.complete);
    assert_eq!(batch.scanned, 1);
    assert_eq!(batch.rows[0], batch.rows[1]);
    assert_eq!(
        batch
            .rows
            .iter()
            .flat_map(|row| &row.fields)
            .map(|page| page.candidates.len())
            .sum::<usize>(),
        2
    );
    assert!(
        batch
            .rows
            .iter()
            .all(|row| !row.fields[0].complete && row.fields[0].resume.is_some())
    );
    assert!(batch.rows[2].fields[0].candidates.is_empty());
    let remaining = view
        .covering_candidates(
            b,
            FieldId(1),
            20,
            batch.rows[2].fields[0].resume.as_ref(),
            CandidateLimits::default(),
        )
        .unwrap();
    assert_eq!(remaining.candidates[0].value, b"b");
    assert!(remaining.complete);
}

#[test]
fn aggregate_scan_exhaustion_is_explicit_even_when_no_live_candidate_was_returned() {
    let (_directory, environment) = setup(7);
    let a = Address::V4(1);
    let b = Address::V4(257);
    let mut expired = claim(1, 1, prefix(a, 24), 10, b"old");
    expired.expires_at = 20;
    store(
        &environment,
        &[
            expired,
            claim(2, 1, prefix(a, 24), 5, b"fresh"),
            claim(3, 1, prefix(b, 24), 5, b"other"),
        ],
    );
    let read = environment.begin_read();
    let view = Profiles::view(&read).unwrap();
    let batch = view
        .batch_candidates(
            &[a, b],
            &[FieldId(1)],
            20,
            CandidateBatchLimits {
                max_scan: 1,
                ..CandidateBatchLimits::default()
            },
        )
        .unwrap();
    assert_eq!(batch.scanned, 1);
    assert!(!batch.complete);
    for row in &batch.rows {
        assert!(row.fields[0].candidates.is_empty());
        assert!(!row.fields[0].complete);
        assert!(row.fields[0].resume.is_some());
    }
}

#[test]
fn exact_page_boundary_is_complete_and_results_budget_never_hides_truncation() {
    let (_directory, environment) = setup(7);
    let address = Address::V4(1);
    store(
        &environment,
        &[
            claim(1, 1, prefix(address, 32), 10, b"host"),
            claim(2, 1, prefix(address, 0), 100, b"global"),
        ],
    );
    let read = environment.begin_read();
    let view = Profiles::view(&read).unwrap();
    let first = view
        .covering_candidates(
            address,
            FieldId(1),
            20,
            None,
            CandidateLimits {
                max_results: 1,
                max_scan: 100,
            },
        )
        .unwrap();
    assert!(!first.complete);
    assert_eq!(first.scanned, 1);
    assert_eq!(first.candidates[0].value, b"host");
    let second = view
        .covering_candidates(
            address,
            FieldId(1),
            20,
            first.resume.as_ref(),
            CandidateLimits {
                max_results: 1,
                max_scan: 100,
            },
        )
        .unwrap();
    assert!(second.complete);
    assert!(second.resume.is_none());
    assert_eq!(second.candidates[0].value, b"global");
    let unknown = view
        .covering_candidates(address, FieldId(2), 20, None, CandidateLimits::default())
        .unwrap();
    assert!(unknown.complete);
    assert_eq!(unknown.scanned, 0);
    assert!(unknown.candidates.is_empty());
}

#[test]
fn input_limits_and_field_validation_bound_allocations() {
    let (_directory, environment) = setup(7);
    let read = environment.begin_read();
    let view = Profiles::view(&read).unwrap();
    let address = Address::V4(1);
    for limits in [
        CandidateLimits {
            max_results: 0,
            max_scan: 1,
        },
        CandidateLimits {
            max_results: 1,
            max_scan: 0,
        },
        CandidateLimits {
            max_results: 65_537,
            max_scan: 1,
        },
        CandidateLimits {
            max_results: 1,
            max_scan: 65_537,
        },
    ] {
        assert!(
            view.covering_candidates(address, FieldId(1), 20, None, limits)
                .is_err()
        );
    }
    assert!(
        view.covering_candidates(address, FieldId(0), 20, None, CandidateLimits::default())
            .is_err()
    );
    assert!(
        view.batch_candidates(
            &[address; 2],
            &[FieldId(1); 2],
            20,
            CandidateBatchLimits {
                max_queries: 3,
                ..CandidateBatchLimits::default()
            }
        )
        .is_err()
    );
    assert!(
        view.batch_candidates(
            &[address; 2],
            &[],
            20,
            CandidateBatchLimits {
                max_queries: 1,
                ..CandidateBatchLimits::default()
            }
        )
        .is_err()
    );
    assert!(
        view.batch_candidates(
            &[address],
            &[FieldId(0)],
            20,
            CandidateBatchLimits::default()
        )
        .is_err()
    );
    assert!(
        view.batch_candidates(&[], &[], 20, CandidateBatchLimits::default())
            .unwrap()
            .complete
    );
}

#[test]
fn maximum_bucket_resumes_one_record_at_a_time_and_persists_on_reopen() {
    let (directory, environment) = setup(7);
    let address = Address::V6(1);
    let claims = (1..=MAX_CLAIMS_PER_PREFIX as u64)
        .map(|id| claim(id, 1, prefix(address, 128), 100, b"evidence"))
        .collect::<Vec<_>>();
    store(&environment, &claims);
    drop(environment);
    let environment = Environment::open(&directory.0).unwrap();
    let read = environment.begin_read();
    let view = Profiles::view(&read).unwrap();
    let mut after = None;
    let mut seen = Vec::new();
    let mut scanned = 0;
    loop {
        let page = view
            .covering_candidates(
                address,
                FieldId(1),
                20,
                after.as_ref(),
                CandidateLimits {
                    max_results: 1,
                    max_scan: 1,
                },
            )
            .unwrap();
        scanned += page.scanned;
        seen.extend(page.candidates.iter().map(|c| c.claim_id));
        after = page.resume;
        if page.complete {
            break;
        }
    }
    assert_eq!(scanned, MAX_CLAIMS_PER_PREFIX);
    assert_eq!(seen, claims.iter().map(|c| c.id).collect::<Vec<_>>());
    assert!(after.is_none());
}
