use std::path::PathBuf;
use ulmdb::Environment;
use ulmdb_ipdon::{rrsets::*, *};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ipdon-rrsets-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn env() -> (Directory, Environment) {
    let d = Directory::new();
    let e = Environment::create(&d.0).unwrap();
    let mut w = e.begin_write().unwrap();
    Rrsets::initialize(&mut w).unwrap();
    w.commit().unwrap();
    (d, e)
}
fn set(origin: u64, revision: u64, addresses: &[u32]) -> Rrset {
    Rrset {
        key: Key {
            origin,
            source: SourceId(7),
            scope: ScopeId(1),
            resolver: ResolverId(2),
            vantage: VantageId(3),
            role: Role::Direct,
            hostname: "Example.TEST.".into(),
            kind: RecordType::A,
        },
        revision,
        addresses: addresses.iter().copied().map(Address::V4).collect(),
        dnssec: Dnssec::Secure,
    }
}
fn fresh(t: u64) -> Freshness {
    Freshness {
        observed_at: t,
        recorded_at: t + 1,
        expires_at: t + 100,
        evidence: EvidenceId::from_bytes(&t.to_be_bytes()),
    }
}
fn query() -> Query {
    Query::Hostname {
        scope: ScopeId(1),
        role: Role::Direct,
        hostname: "EXAMPLE.test".into(),
    }
}
fn members(e: &Environment, at: u64) -> Vec<Relation> {
    let r = e.begin_read();
    Rrsets::view(&r)
        .unwrap()
        .query(&query(), at, None, Limits::default())
        .unwrap()
        .relations
}

#[test]
fn rrset_replacement_merges_only_changed_members_and_roundtrips_metadata() {
    let (_d, e) = env();
    let mut w = e.begin_write().unwrap();
    let initial = set(1, 1, &[3, 1, 2, 2]);
    assert_eq!(
        Rrsets::replace(&mut w, &initial, fresh(10)).unwrap(),
        Delta {
            added: 3,
            removed: 0,
            retained: 0,
            freshness_changed: true
        }
    );
    w.commit().unwrap();
    let mut w = e.begin_write().unwrap();
    assert_eq!(
        Rrsets::replace(&mut w, &set(1, 2, &[2, 3, 4]), fresh(20)).unwrap(),
        Delta {
            added: 1,
            removed: 1,
            retained: 2,
            freshness_changed: true
        }
    );
    w.commit().unwrap();
    let rows = members(&e, 30);
    assert_eq!(
        rows.iter().map(|r| r.address).collect::<Vec<_>>(),
        vec![Address::V4(2), Address::V4(3), Address::V4(4)]
    );
    assert!(
        rows.iter()
            .all(|r| r.hostname == "example.test" && r.revision == 2 && r.freshness == fresh(20))
    );
    let r = e.begin_read();
    let view = Rrsets::view(&r).unwrap();
    let inverse = Query::Ip {
        scope: ScopeId(1),
        role: Role::Direct,
        address: Address::V4(1),
    };
    assert!(
        view.query(&inverse, 30, None, Limits::default())
            .unwrap()
            .relations
            .is_empty()
    );
    let prefix = Prefix::new(Address::V4(0), 30).unwrap();
    let cidr = Query::Cidr {
        scope: ScopeId(1),
        role: Role::Direct,
        prefix,
    };
    assert_eq!(
        view.query(&cidr, 30, None, Limits::default())
            .unwrap()
            .relations
            .len(),
        2
    );
}

#[test]
fn compact_refresh_changes_no_content_or_relationship_bytes_and_replay_dirties_nothing() {
    let (_d, e) = env();
    let s = set(1, 1, &[1, 2, 3]);
    let id = s.key.id().unwrap();
    let mut w = e.begin_write().unwrap();
    Rrsets::replace(&mut w, &s, fresh(10)).unwrap();
    w.commit().unwrap();
    let before = e.begin_read();
    let tables: [&[u8]; 3] = [
        b"ipdon.rrsets.content",
        b"ipdon.rrsets.forward",
        b"ipdon.rrsets.inverse",
    ];
    let records = |r: &ulmdb::ReadTxn, name: &[u8]| {
        r.range(
            name,
            std::ops::Bound::Unbounded,
            std::ops::Bound::Unbounded,
            false,
        )
        .unwrap()
        .map(|v| {
            let (k, v) = v.unwrap();
            (k.to_vec(), v.to_vec())
        })
        .collect::<Vec<_>>()
    };
    let expected = tables
        .iter()
        .map(|name| records(&before, name))
        .collect::<Vec<_>>();
    let mut w = e.begin_write().unwrap();
    assert!(Rrsets::refresh(&mut w, id, 1, fresh(20)).unwrap());
    w.commit().unwrap();
    let after = e.begin_read();
    for (name, expected) in tables.into_iter().zip(expected) {
        assert_eq!(records(&after, name), expected);
    }
    assert_eq!(
        Rrsets::view(&before)
            .unwrap()
            .rrset(id)
            .unwrap()
            .unwrap()
            .1
            .freshness,
        fresh(10)
    );
    assert_eq!(
        Rrsets::view(&after)
            .unwrap()
            .rrset(id)
            .unwrap()
            .unwrap()
            .1
            .freshness,
        fresh(20)
    );
    let mut w = e.begin_write().unwrap();
    let dirty = w.dirty_bytes();
    assert!(!Rrsets::refresh(&mut w, id, 1, fresh(20)).unwrap());
    assert_eq!(dirty, w.dirty_bytes());
}

#[test]
fn exact_revision_and_monotonic_freshness_prevent_replaced_or_withdrawn_resurrection() {
    let (_d, e) = env();
    let s = set(1, 1, &[1]);
    let id = s.key.id().unwrap();
    let mut w = e.begin_write().unwrap();
    Rrsets::replace(&mut w, &s, fresh(10)).unwrap();
    Rrsets::replace(&mut w, &set(1, 2, &[2]), fresh(20)).unwrap();
    assert!(Rrsets::refresh(&mut w, id, 1, fresh(30)).is_err());
    assert!(Rrsets::refresh(&mut w, id, 2, fresh(5)).is_err());
    Rrsets::withdraw(&mut w, id, 2, 31).unwrap();
    assert!(Rrsets::refresh(&mut w, id, 2, fresh(40)).is_err());
    assert!(Rrsets::replace(&mut w, &set(1, 2, &[2]), fresh(40)).is_err());
    Rrsets::replace(&mut w, &set(1, 3, &[1]), fresh(40)).unwrap();
    assert!(Rrsets::refresh(&mut w, id, 1, fresh(50)).is_err());
    w.commit().unwrap();
    assert_eq!(members(&e, 60)[0].address, Address::V4(1));
}

#[test]
fn expiry_reads_are_correct_before_cleanup_stale_plans_cannot_remove_a_refresh() {
    let (_d, e) = env();
    let s = set(1, 1, &[1, 2]);
    let id = s.key.id().unwrap();
    let mut w = e.begin_write().unwrap();
    Rrsets::replace(&mut w, &s, fresh(10)).unwrap();
    w.commit().unwrap();
    assert!(members(&e, 9).is_empty());
    assert_eq!(members(&e, 109).len(), 2);
    assert!(members(&e, 110).is_empty());
    let r = e.begin_read();
    let view = Rrsets::view(&r).unwrap();
    let plan = view.plan_expiry(110, 10).unwrap();
    assert_eq!(plan.len(), 1);
    assert!(view.plan_expiry_with_budget(110, 10, 1).is_err());
    let mut w = e.begin_write().unwrap();
    Rrsets::refresh(&mut w, id, 1, fresh(100)).unwrap();
    assert_eq!(Rrsets::expire(&mut w, &plan).unwrap().removed, 0);
    w.commit().unwrap();
    assert_eq!(members(&e, 150).len(), 2);
    let r = e.begin_read();
    let plan = Rrsets::view(&r).unwrap().plan_expiry(200, 10).unwrap();
    let mut w = e.begin_write().unwrap();
    assert_eq!(Rrsets::expire(&mut w, &plan).unwrap().removed, 2);
    w.commit().unwrap();
    let r = e.begin_read();
    assert_eq!(
        Rrsets::view(&r)
            .unwrap()
            .rrset(id)
            .unwrap()
            .unwrap()
            .1
            .status,
        Status::Expired
    );
    let mut w = e.begin_write().unwrap();
    assert!(!Rrsets::refresh(&mut w, id, 1, fresh(100)).unwrap());
    Rrsets::refresh(&mut w, id, 1, fresh(201)).unwrap();
    w.commit().unwrap();
    assert_eq!(members(&e, 220).len(), 2);
}

#[test]
fn batch_failure_rolls_back_successful_items_and_keeps_parent_transaction_usable() {
    let (_d, e) = env();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"other", ulmdb::DatabaseKind::Kv)
        .unwrap();
    w.put(b"other", b"keep", b"parent").unwrap();
    let mut invalid = set(2, 1, &[1]);
    invalid.addresses = vec![Address::V6(1)];
    assert!(
        Rrsets::replace_batch(
            &mut w,
            &[(set(1, 1, &[1]), fresh(10)), (invalid, fresh(20))]
        )
        .is_err()
    );
    Rrsets::replace(&mut w, &set(3, 1, &[3]), fresh(30)).unwrap();
    w.commit().unwrap();
    assert_eq!(members(&e, 40).len(), 1);
    assert_eq!(members(&e, 40)[0].origin, 3);
    assert_eq!(
        e.begin_read().get(b"other", b"keep").unwrap(),
        Some(&b"parent"[..])
    );
}

#[test]
fn regional_vantages_are_independent_and_pagination_binds_snapshot_query_and_time() {
    let (_d, e) = env();
    let mut w = e.begin_write().unwrap();
    Rrsets::replace(&mut w, &set(1, 1, &[1, 2]), fresh(10)).unwrap();
    Rrsets::replace(&mut w, &set(2, 1, &[1, 3]), fresh(10)).unwrap();
    w.commit().unwrap();
    let read = e.begin_read();
    let view = Rrsets::view(&read).unwrap();
    let limits = Limits {
        max_results: 1,
        max_scan: 1,
    };
    let mut after = None;
    let mut rows = Vec::new();
    loop {
        let page = view.query(&query(), 20, after.as_ref(), limits).unwrap();
        rows.extend(page.relations);
        after = page.resume;
        if after.is_none() {
            break;
        }
    }
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows.iter().filter(|r| r.address == Address::V4(1)).count(),
        2
    );
    let token = view
        .query(&query(), 20, None, limits)
        .unwrap()
        .resume
        .unwrap();
    assert!(view.query(&query(), 21, Some(&token), limits).is_err());
    let other = Query::Ip {
        scope: ScopeId(1),
        role: Role::Direct,
        address: Address::V4(1),
    };
    assert!(view.query(&other, 20, Some(&token), limits).is_err());
    let mut w = e.begin_write().unwrap();
    Rrsets::refresh(&mut w, set(1, 1, &[]).key.id().unwrap(), 1, fresh(20)).unwrap();
    w.commit().unwrap();
    let new = e.begin_read();
    assert!(
        Rrsets::view(&new)
            .unwrap()
            .query(&query(), 20, Some(&token), limits)
            .is_err()
    );
}

#[test]
fn stale_scan_work_is_bounded_and_ipv6_is_native() {
    let (_d, e) = env();
    let mut w = e.begin_write().unwrap();
    let mut s = set(1, 1, &[]);
    s.key.kind = RecordType::Aaaa;
    s.addresses = vec![Address::V6(1), Address::V6(2)];
    Rrsets::replace(&mut w, &s, fresh(10)).unwrap();
    w.commit().unwrap();
    let r = e.begin_read();
    let v = Rrsets::view(&r).unwrap();
    let page = v
        .query(
            &query(),
            200,
            None,
            Limits {
                max_results: 1,
                max_scan: 1,
            },
        )
        .unwrap();
    assert_eq!(page.scanned, 1);
    assert!(page.relations.is_empty());
    assert!(page.resume.is_some());
    let page = v
        .query(
            &Query::Cidr {
                scope: ScopeId(1),
                role: Role::Direct,
                prefix: Prefix::new(Address::V6(0), 126).unwrap(),
            },
            20,
            None,
            Limits::default(),
        )
        .unwrap();
    assert_eq!(page.relations.len(), 2);
}

#[test]
fn close_reopen_preserves_revisions_withdrawals_and_freshness() {
    let (d, e) = env();
    let s = set(1, 1, &[1]);
    let id = s.key.id().unwrap();
    let mut w = e.begin_write().unwrap();
    Rrsets::replace(&mut w, &s, fresh(10)).unwrap();
    Rrsets::withdraw(&mut w, id, 1, 30).unwrap();
    w.commit().unwrap();
    drop(e);
    let e = Environment::open(&d.0).unwrap();
    let mut w = e.begin_write().unwrap();
    assert!(Rrsets::refresh(&mut w, id, 1, fresh(40)).is_err());
    Rrsets::replace(&mut w, &set(1, 2, &[2]), fresh(40)).unwrap();
    w.commit().unwrap();
    assert_eq!(members(&e, 50)[0].address, Address::V4(2));
}
