use std::path::PathBuf;
use ulmdb::Environment;
use ulmdb_ipdon::{
    journal, profiles,
    regional::{Command, Regional},
    rrsets, *,
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ipdon-regional-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn init() -> (Directory, Environment) {
    let d = Directory::new();
    let e = Environment::create(&d.0).unwrap();
    let mut w = e.begin_write().unwrap();
    Regional::initialize(&mut w, 7).unwrap();
    w.commit().unwrap();
    (d, e)
}
fn rrset() -> rrsets::Rrset {
    rrsets::Rrset {
        key: rrsets::Key {
            origin: 1,
            source: SourceId(2),
            scope: ScopeId(1),
            resolver: ResolverId(3),
            vantage: VantageId(4),
            role: Role::Direct,
            hostname: "example.test".into(),
            kind: rrsets::RecordType::A,
        },
        revision: 1,
        addresses: vec![Address::V4(0xc0000201)],
        dnssec: Dnssec::Secure,
    }
}
fn freshness(t: u64) -> rrsets::Freshness {
    rrsets::Freshness {
        observed_at: t,
        recorded_at: t + 1,
        expires_at: t + 100,
        evidence: EvidenceId::from_bytes(&t.to_be_bytes()),
    }
}
fn claim() -> profiles::Claim {
    profiles::Claim {
        id: profiles::ClaimId::from_source(2, b"country"),
        generation: 1,
        field: profiles::FieldId(1),
        prefix: Prefix::new(Address::V4(0xc0000200), 24).unwrap(),
        source: 2,
        value: b"NL".to_vec(),
        priority: 100,
        observed_at: 10,
        recorded_at: 11,
        expires_at: 200,
        evidence: [7; 32],
    }
}

#[test]
fn typed_page_updates_rrsets_profiles_and_both_watermarks_atomically() {
    let (_d, e) = init();
    let set = rrset();
    let id = set.key.id().unwrap();
    let c = claim();
    let events = vec![
        Command::ReplaceRrset {
            set,
            freshness: freshness(10),
        }
        .event(1, 1, 11)
        .unwrap(),
        Command::PutClaim(c.clone()).event(2, 1, 11).unwrap(),
    ];
    let mut w = e.begin_write().unwrap();
    let report = Regional::apply_page(&mut w, &events, journal::Limits::default()).unwrap();
    assert_eq!(report.work.added_members, 1);
    assert_eq!(report.work.claim_changes, 1);
    w.commit().unwrap();
    let r = e.begin_read();
    let view = Regional::view(&r).unwrap();
    assert_eq!(view.watermarks[&1].applied, 1);
    assert_eq!(view.watermarks[&1].serving, 1);
    assert_eq!(
        view.profiles
            .lookup(Address::V4(0xc0000201), profiles::FieldId(1), 20)
            .unwrap()
            .unwrap()
            .value,
        b"NL"
    );
    let refresh = Command::RefreshRrset {
        id,
        revision: 1,
        freshness: freshness(20),
    }
    .event(1, 2, 21)
    .unwrap();
    let mut w = e.begin_write().unwrap();
    let report = Regional::apply_page(
        &mut w,
        std::slice::from_ref(&refresh),
        journal::Limits::default(),
    )
    .unwrap();
    assert_eq!(report.work.refreshes, 1);
    assert_eq!(report.work.added_members, 0);
    assert_eq!(report.work.removed_members, 0);
    w.commit().unwrap();
    let mut w = e.begin_write().unwrap();
    let report = Regional::apply_page(&mut w, &[refresh], journal::Limits::default()).unwrap();
    assert_eq!(report.journal.replayed, 1);
    assert_eq!(report.work.refreshes, 0);
    assert_eq!(w.dirty_bytes(), 0);
}

#[test]
fn foreign_origin_or_malformed_final_event_cannot_publish_partial_page() {
    let (_d, e) = init();
    let good = Command::ReplaceRrset {
        set: rrset(),
        freshness: freshness(10),
    }
    .event(1, 1, 11)
    .unwrap();
    let mut bad = Command::PutClaim(claim()).event(2, 1, 11).unwrap();
    bad.payload.push(0);
    let mut w = e.begin_write().unwrap();
    assert!(
        Regional::apply_page(&mut w, &[good.clone(), bad], journal::Limits::default()).is_err()
    );
    w.commit().unwrap();
    let r = e.begin_read();
    assert!(Regional::view(&r).unwrap().watermarks.is_empty());
    let mut w = e.begin_write().unwrap();
    Regional::apply_page(&mut w, &[good], journal::Limits::default()).unwrap();
    w.commit().unwrap();
    let foreign = Command::RefreshRrset {
        id: rrset().key.id().unwrap(),
        revision: 1,
        freshness: freshness(20),
    }
    .event(3, 1, 21)
    .unwrap();
    let mut w = e.begin_write().unwrap();
    assert!(Regional::apply_page(&mut w, &[foreign], journal::Limits::default()).is_err());
    w.commit().unwrap();
    assert_eq!(journal::cursor(&e.begin_read(), 3).unwrap().applied, 0);
}

#[test]
fn claim_ownership_survives_withdrawal_and_wire_is_strict() {
    let (_d, e) = init();
    let c = claim();
    let rev = c.revision().unwrap();
    let put = Command::PutClaim(c.clone()).event(1, 1, 11).unwrap();
    let withdraw = Command::WithdrawClaim {
        id: c.id,
        revision: rev,
    }
    .event(1, 2, 30)
    .unwrap();
    let mut w = e.begin_write().unwrap();
    Regional::apply_page(&mut w, &[put, withdraw], journal::Limits::default()).unwrap();
    w.commit().unwrap();
    let mut updated = c.clone();
    updated.recorded_at = 40;
    updated.generation = 2;
    let mut w = e.begin_write().unwrap();
    assert!(
        Regional::apply_page(
            &mut w,
            &[Command::PutClaim(updated.clone()).event(2, 1, 40).unwrap()],
            journal::Limits::default()
        )
        .is_err()
    );
    Regional::apply_page(
        &mut w,
        &[Command::PutClaim(updated).event(1, 3, 40).unwrap()],
        journal::Limits::default(),
    )
    .unwrap();
    w.commit().unwrap();
    for command in [
        Command::PutClaim(c.clone()),
        Command::RefreshClaim {
            id: c.id,
            revision: rev,
            freshness: profiles::Freshness {
                observed_at: 20,
                recorded_at: 21,
                expires_at: 300,
            },
        },
        Command::WithdrawClaim {
            id: c.id,
            revision: rev,
        },
        Command::ReplaceRrset {
            set: rrset(),
            freshness: freshness(10),
        },
        Command::RefreshRrset {
            id: rrset().key.id().unwrap(),
            revision: 1,
            freshness: freshness(20),
        },
        Command::WithdrawRrset {
            id: rrset().key.id().unwrap(),
            revision: 1,
        },
    ] {
        let bytes = command.encode().unwrap();
        assert_eq!(Command::decode(&bytes).unwrap(), command);
        for n in 0..bytes.len() {
            assert!(Command::decode(&bytes[..n]).is_err(), "accepted prefix {n}");
        }
        let mut extra = bytes;
        extra.push(0);
        assert!(Command::decode(&extra).is_err());
    }
}

#[test]
fn native_snapshot_rebase_keeps_local_tail_and_typed_query_results() {
    let (d, local) = init();
    let mut w = local.begin_write().unwrap();
    let first = Command::ReplaceRrset {
        set: rrset(),
        freshness: freshness(10),
    }
    .event(1, 1, 11)
    .unwrap();
    Regional::apply_page(&mut w, &[first], journal::Limits::default()).unwrap();
    w.commit().unwrap();
    let bundle = d.0.with_extension("checkpoint");
    let candidate = d.0.with_extension("rebased");
    let _cleanup1 = Directory(bundle.clone());
    let _cleanup2 = Directory(candidate.clone());
    journal::export_snapshot(&local, &local.begin_read(), &bundle).unwrap();
    let mut changed = rrset();
    changed.revision = 2;
    changed.addresses = vec![Address::V4(0xc0000202)];
    let mut w = local.begin_write().unwrap();
    Regional::apply_page(
        &mut w,
        &[Command::ReplaceRrset {
            set: changed,
            freshness: freshness(20),
        }
        .event(1, 2, 21)
        .unwrap()],
        journal::Limits::default(),
    )
    .unwrap();
    w.commit().unwrap();
    let restored = journal::rebase_snapshot(
        &bundle,
        &local,
        1,
        &candidate,
        journal::Limits::default(),
        Regional::apply_event,
    )
    .unwrap();
    let r = restored.begin_read();
    let v = Regional::view(&r).unwrap();
    assert_eq!(v.watermarks[&1].serving, 2);
    let query = rrsets::Query::Hostname {
        scope: ScopeId(1),
        role: Role::Direct,
        hostname: "example.test".into(),
    };
    let rows = v
        .rrsets
        .query(&query, 30, None, rrsets::Limits::default())
        .unwrap();
    assert_eq!(rows.relations.len(), 1);
    assert_eq!(rows.relations[0].address, Address::V4(0xc0000202));
}

#[test]
fn expired_refresh_revival_is_charged_to_the_page_member_budget() {
    let (_d, e) = init();
    let mut set = rrset();
    set.addresses.push(Address::V4(0xc0000202));
    let id = set.key.id().unwrap();
    let mut w = e.begin_write().unwrap();
    Regional::apply_page(
        &mut w,
        &[Command::ReplaceRrset {
            set,
            freshness: freshness(10),
        }
        .event(1, 1, 11)
        .unwrap()],
        journal::Limits::default(),
    )
    .unwrap();
    w.commit().unwrap();
    let r = e.begin_read();
    let plan = Regional::view(&r)
        .unwrap()
        .rrsets
        .plan_expiry(110, 1)
        .unwrap();
    let mut w = e.begin_write().unwrap();
    rrsets::Rrsets::expire(&mut w, &plan).unwrap();
    w.commit().unwrap();
    let refresh = Command::RefreshRrset {
        id,
        revision: 1,
        freshness: freshness(120),
    }
    .event(1, 2, 121)
    .unwrap();
    let mut w = e.begin_write().unwrap();
    assert!(
        Regional::apply_page_with_budget(
            &mut w,
            std::slice::from_ref(&refresh),
            journal::Limits::default(),
            1
        )
        .is_err()
    );
    w.commit().unwrap();
    let r = e.begin_read();
    assert_eq!(journal::cursor(&r, 1).unwrap().applied, 1);
    assert_eq!(
        Regional::view(&r)
            .unwrap()
            .rrsets
            .rrset(id)
            .unwrap()
            .unwrap()
            .1
            .status,
        rrsets::Status::Expired
    );
    let mut w = e.begin_write().unwrap();
    let report =
        Regional::apply_page_with_budget(&mut w, &[refresh], journal::Limits::default(), 2)
            .unwrap();
    assert_eq!(report.work.added_members, 2);
    assert_eq!(report.work.refreshes, 1);
    w.commit().unwrap();
}
