use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use ulmdb::{Environment, backup};
use ulmdb_ipdon::*;
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-ipdon-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn observation(n: u64, name: &str, address: Address, scope: u64, role: Role) -> Observation {
    Observation {
        id: ObservationId::from_source(SourceId(7), &n.to_be_bytes()),
        hostname: name.into(),
        address,
        scope: ScopeId(scope),
        source: SourceId(7),
        role,
        observed_at: 1_700_000_000 + n,
        collected_at: 1_700_000_100 + n,
        vantage: VantageId(4),
        resolver: ResolverId(9),
        ttl: 300,
        dnssec: Dnssec::Secure,
        evidence: EvidenceId::from_bytes(&n.to_be_bytes()),
    }
}
fn initialized(parent: &Path) -> (Directory, Environment) {
    let directory = Directory::new(parent);
    let env = Environment::create(&directory.0).unwrap();
    let mut w = env.begin_write().unwrap();
    Projection::initialize(&mut w).unwrap();
    w.commit().unwrap();
    (directory, env)
}
fn prefix(address: Address, length: u8) -> Prefix {
    Prefix::normalize(address, length).unwrap()
}
#[test]
fn strict_normalization_source_identity_and_evidence_provenance() {
    assert_eq!(
        normalize_hostname("WWW.Example.COM.").unwrap(),
        "www.example.com"
    );
    assert_eq!(
        normalize_hostname("XN--BCHER-KVA.Example").unwrap(),
        "xn--bcher-kva.example"
    );
    for name in [
        "",
        ".",
        "example..",
        ".example",
        "a..b",
        "-a.example",
        "a-.example",
        "a_b.example",
        "*.example",
        "bücher.example",
        "xn--.example",
    ] {
        assert!(normalize_hostname(name).is_err(), "{name}");
    }
    assert!(normalize_hostname(&format!("{}.example", "a".repeat(64))).is_err());
    assert!(
        normalize_hostname(&format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(62)
        ))
        .is_err()
    );
    assert_ne!(
        ObservationId::from_source(SourceId(1), b"record"),
        ObservationId::from_source(SourceId(2), b"record")
    );
    let (_directory, env) = initialized(&std::env::temp_dir());
    let o = observation(
        1,
        "WWW.Example.COM.",
        Address::V4(0xc000_0201),
        1,
        Role::Direct,
    );
    let mut w = env.begin_write().unwrap();
    assert!(Projection::upsert(&mut w, &o).unwrap());
    w.commit().unwrap();
    let read = env.begin_read();
    let view = Projection::view(&read).unwrap();
    assert_eq!(view.observation(o.id).unwrap().unwrap().to_owned(), o);
    let id = view.hostname_id("www.example.com").unwrap().unwrap();
    assert_eq!(view.hostname(id).unwrap(), Some("www.example.com"));
    let evidence = view
        .evidence(ScopeId(1), Role::Direct, "WWW.EXAMPLE.COM", o.address, None)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(evidence.to_owned(), o);
    assert_eq!(
        view.reconcile().unwrap(),
        Reconciliation {
            observations: 1,
            unique_edges: 1,
            assigned_hostnames: 1
        }
    );
}

fn journey(parent: &Path) {
    let (directory, env) = initialized(parent);
    let address = Address::V4(0xc000_0201);
    let v6 = Address::V6(0x20010db8u128 << 96 | 1);
    let a = observation(1, "WWW.Example.", address, 1, Role::Direct);
    let mut b = observation(2, "www.example", address, 1, Role::Direct);
    b.source = SourceId(8);
    b.vantage = VantageId(99);
    let c = observation(3, "www.example", address, 2, Role::Direct);
    let d = observation(4, "www.example", address, 1, Role::MailTarget);
    let e = observation(5, "www.example", v6, 1, Role::Direct);
    let mut w = env.begin_write().unwrap();
    for o in [&a, &b, &c, &d, &e] {
        Projection::upsert(&mut w, o).unwrap();
    }
    Projection::put_profile(&mut w, ScopeId(1), prefix(address, 24), b"profile-four").unwrap();
    Projection::put_profile(&mut w, ScopeId(1), prefix(v6, 32), b"profile-six").unwrap();
    Projection::put_profile(&mut w, ScopeId(2), prefix(address, 24), b"different-scope").unwrap();
    w.commit().unwrap();
    let read = env.begin_read();
    let view = Projection::view(&read).unwrap();
    let id = view.hostname_id("www.example").unwrap().unwrap();
    assert_eq!(
        view.host_edge_count_id(ScopeId(1), Role::Direct, id)
            .unwrap(),
        2
    );
    assert_eq!(
        view.ip_edge_count(ScopeId(1), Role::Direct, address)
            .unwrap(),
        1
    );
    let edges = view
        .addresses_id(ScopeId(1), Role::Direct, id, None)
        .unwrap()
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(edges.len(), 2);
    assert_eq!(edges[0].supports, 2);
    assert_eq!(edges[1].address, v6);
    assert_eq!(
        view.hostnames(ScopeId(1), Role::Direct, address, None)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .hostname,
        "www.example"
    );
    assert_eq!(
        view.evidence(ScopeId(1), Role::Direct, "www.example", address, None)
            .unwrap()
            .count(),
        2
    );
    let profiles = view.profiles(ScopeId(1)).unwrap();
    assert_eq!(
        profiles.longest_match(address).unwrap().unwrap().1,
        b"profile-four"
    );
    assert_eq!(
        profiles.longest_match(v6).unwrap().unwrap().1,
        b"profile-six"
    );
    assert_eq!(
        view.profiles(ScopeId(2))
            .unwrap()
            .longest_match(address)
            .unwrap()
            .unwrap()
            .1,
        b"different-scope"
    );
    assert!(
        view.profiles(ScopeId(3))
            .unwrap()
            .longest_match(address)
            .unwrap()
            .is_none()
    );
    view.reconcile().unwrap();
    let previous_sequence = read.sequence();
    let mut replay = env.begin_write().unwrap();
    assert!(!Projection::upsert(&mut replay, &a).unwrap());
    assert_eq!(replay.commit().unwrap(), previous_sequence);
    let mut w = env.begin_write().unwrap();
    Projection::delete(&mut w, a.id).unwrap();
    w.commit().unwrap();
    let now = env.begin_read();
    let now = Projection::view(&now).unwrap();
    assert_eq!(
        now.hostnames(ScopeId(1), Role::Direct, address, None)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .supports,
        1
    );
    assert_eq!(
        now.ip_edge_count(ScopeId(1), Role::Direct, address)
            .unwrap(),
        1
    );
    let mut w = env.begin_write().unwrap();
    Projection::delete(&mut w, b.id).unwrap();
    Projection::delete(&mut w, e.id).unwrap();
    w.commit().unwrap();
    let now = env.begin_read();
    let now = Projection::view(&now).unwrap();
    assert_eq!(
        now.host_edge_count(ScopeId(1), Role::Direct, "www.example")
            .unwrap(),
        0
    );
    assert_eq!(
        now.ip_edge_count(ScopeId(1), Role::Direct, address)
            .unwrap(),
        0
    );
    assert_eq!(
        now.ip_edge_count(ScopeId(2), Role::Direct, address)
            .unwrap(),
        1
    );
    assert_eq!(
        now.ip_edge_count(ScopeId(1), Role::MailTarget, address)
            .unwrap(),
        1
    );
    assert_eq!(now.hostname_id("www.example").unwrap(), Some(id));
    now.reconcile().unwrap();
    // Original borrowed readers still see both supporting records and both IPs.
    assert_eq!(
        view.addresses(ScopeId(1), Role::Direct, "www.example", None)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .supports,
        2
    );
    let mut w = env.begin_write().unwrap();
    Projection::upsert(&mut w, &a).unwrap();
    w.commit().unwrap();
    let exported = Directory::new(parent);
    let restored = Directory::new(parent);
    backup::export(&env, &exported.0).unwrap();
    backup::restore(&exported.0, &restored.0).unwrap();
    let restored_env = Environment::open(&restored.0).unwrap();
    let r = restored_env.begin_read();
    let r = Projection::view(&r).unwrap();
    assert_eq!(r.hostname_id("www.example").unwrap(), Some(id));
    assert_eq!(r.reconcile().unwrap().observations, 3);
    assert_eq!(
        r.profiles(ScopeId(1))
            .unwrap()
            .longest_match(v6)
            .unwrap()
            .unwrap()
            .1,
        b"profile-six"
    );
    drop(env);
    let reopened = Environment::open(&directory.0).unwrap();
    let r = reopened.begin_read();
    Projection::view(&r).unwrap().reconcile().unwrap();
}
#[test]
fn hostname_ip_profiles_support_transitions_reopen_and_restore_disk() {
    journey(&std::env::temp_dir());
}
#[test]
fn hostname_ip_profiles_support_transitions_reopen_and_restore_tmpfs() {
    if Path::new("/dev/shm").is_dir() {
        journey(Path::new("/dev/shm"));
    }
}

#[test]
fn replacement_mid_operation_failure_and_outer_abort_are_atomic() {
    let (_directory, env) = initialized(&std::env::temp_dir());
    let old = observation(1, "old.example", Address::V4(1), 1, Role::Direct);
    let mut w = env.begin_write().unwrap();
    Projection::upsert(&mut w, &old).unwrap();
    w.commit().unwrap();
    let mut replacement = old.clone();
    replacement.hostname = "new.example".into();
    replacement.address = Address::V6(2);
    replacement.scope = ScopeId(2);
    let mut w = env.begin_write().unwrap();
    let next = w
        .get(b"ipdon.meta", b"next-hostname-id")
        .unwrap()
        .unwrap()
        .to_vec();
    let canonical = w
        .get(b"ipdon.observations", &old.id.0)
        .unwrap()
        .unwrap()
        .to_vec();
    w.put(b"ipdon.meta", b"next-hostname-id", &u64::MAX.to_be_bytes())
        .unwrap();
    // Removal of old support and counts precedes the overflowing new-ID assignment.
    assert!(matches!(
        Projection::upsert(&mut w, &replacement),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(
        w.get(b"ipdon.observations", &old.id.0).unwrap(),
        Some(canonical.as_slice())
    );
    w.put(b"ipdon.meta", b"next-hostname-id", &next).unwrap();
    w.commit().unwrap();
    let r = env.begin_read();
    let v = Projection::view(&r).unwrap();
    v.reconcile().unwrap();
    assert_eq!(v.observation(old.id).unwrap().unwrap().to_owned(), old);
    let mut w = env.begin_write().unwrap();
    Projection::upsert(&mut w, &replacement).unwrap();
    w.abort();
    let r = env.begin_read();
    assert_eq!(
        Projection::view(&r)
            .unwrap()
            .observation(old.id)
            .unwrap()
            .unwrap()
            .to_owned(),
        old
    );
    let mut w = env.begin_write().unwrap();
    Projection::upsert(&mut w, &replacement).unwrap();
    w.commit().unwrap();
    let r = env.begin_read();
    let v = Projection::view(&r).unwrap();
    assert_eq!(
        v.observation(old.id).unwrap().unwrap().to_owned(),
        replacement
    );
    assert_eq!(
        v.host_edge_count(ScopeId(1), Role::Direct, "old.example")
            .unwrap(),
        0
    );
    assert_eq!(
        v.host_edge_count(ScopeId(2), Role::Direct, "new.example")
            .unwrap(),
        1
    );
    v.reconcile().unwrap();
}

#[test]
fn snapshot_bound_pagination_and_concurrent_updates() {
    let (_directory, env) = initialized(&std::env::temp_dir());
    let env = Arc::new(env);
    let ip = Address::V4(0xc000_0201);
    let mut w = env.begin_write().unwrap();
    for n in 0..24 {
        Projection::upsert(
            &mut w,
            &observation(n, &format!("host{n}.example"), ip, 1, Role::Direct),
        )
        .unwrap();
    }
    Projection::upsert(
        &mut w,
        &observation(
            500,
            "outside.example",
            Address::V4(0xc633_6401),
            1,
            Role::Direct,
        ),
    )
    .unwrap();
    Projection::upsert(
        &mut w,
        &observation(501, "scoped.example", ip, 2, Role::Direct),
    )
    .unwrap();
    w.commit().unwrap();
    let old = env.begin_read();
    let view = Projection::view(&old).unwrap();
    let first = view
        .hostnames(ScopeId(1), Role::Direct, ip, None)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let writer_env = env.clone();
    std::thread::spawn(move || {
        let mut w = writer_env.begin_write().unwrap();
        Projection::delete(&mut w, observation(5, "x", ip, 1, Role::Direct).id).unwrap();
        Projection::upsert(
            &mut w,
            &observation(100, "new.example", ip, 1, Role::Direct),
        )
        .unwrap();
        w.commit().unwrap();
    })
    .join()
    .unwrap();
    let mut token = None;
    let mut collected = Vec::new();
    loop {
        let page = view
            .hostnames(ScopeId(1), Role::Direct, ip, token.as_ref())
            .unwrap()
            .take(5)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        if page.is_empty() {
            break;
        }
        token = Some(page.last().unwrap().resume);
        collected.extend(page.into_iter().map(|e| e.hostname.to_owned()));
    }
    assert_eq!(collected.len(), 24);
    assert!(collected.iter().any(|n| n == "host5.example"));
    assert!(!collected.iter().any(|n| n == "new.example"));
    let new = env.begin_read();
    let new = Projection::view(&new).unwrap();
    assert!(
        new.hostnames(ScopeId(1), Role::Direct, ip, Some(&first.resume))
            .is_err()
    );
    assert!(
        view.hostnames(ScopeId(2), Role::Direct, ip, Some(&first.resume))
            .is_err()
    );
    assert!(
        view.hostnames(ScopeId(1), Role::MailTarget, ip, Some(&first.resume))
            .is_err()
    );
    assert!(
        view.cidr(
            ScopeId(1),
            Role::Direct,
            prefix(ip, 24),
            Some(&first.resume)
        )
        .is_err()
    );
    assert_eq!(
        view.cidr(ScopeId(1), Role::Direct, prefix(ip, 24), None)
            .unwrap()
            .count(),
        24
    );
    assert_eq!(
        new.cidr(ScopeId(1), Role::Direct, prefix(ip, 24), None)
            .unwrap()
            .count(),
        24
    );
    new.reconcile().unwrap();
}

fn random(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}
#[test]
fn differential_reconstruction_after_mixed_replacements_and_deletes() {
    let (_directory, env) = initialized(&std::env::temp_dir());
    let mut seed = 0xa334_f123_5b09_8743;
    let mut model = BTreeMap::<ObservationId, Observation>::new();
    for _batch in 0..18 {
        let mut w = env.begin_write().unwrap();
        for _ in 0..15 {
            let id = random(&mut seed) % 40;
            if random(&mut seed).is_multiple_of(4) {
                let id = ObservationId::from_source(SourceId(7), &id.to_be_bytes());
                assert_eq!(
                    Projection::delete(&mut w, id).unwrap(),
                    model.remove(&id).is_some()
                );
            } else {
                let hostname = format!("host{}.example", random(&mut seed) % 6);
                let n = random(&mut seed) % 8;
                let address = if random(&mut seed).is_multiple_of(2) {
                    Address::V4(0xc000_0200 + n as u32)
                } else {
                    Address::V6(0x20010db8u128 << 96 | u128::from(n))
                };
                let role = if random(&mut seed).is_multiple_of(2) {
                    Role::Direct
                } else {
                    Role::NameserverAddress
                };
                let o = observation(id, &hostname, address, random(&mut seed) % 3, role);
                let changed = model.get(&o.id) != Some(&o);
                assert_eq!(Projection::upsert(&mut w, &o).unwrap(), changed);
                model.insert(o.id, o);
            }
        }
        w.commit().unwrap();
        // Independent tuple-based oracle: no production key codecs or update helpers.
        let mut edges = BTreeMap::new();
        for o in model.values() {
            *edges
                .entry((o.scope, o.role, o.hostname.to_ascii_lowercase(), o.address))
                .or_insert(0u64) += 1;
        }
        let r = env.begin_read();
        let view = Projection::view(&r).unwrap();
        let report = view.reconcile().unwrap();
        assert_eq!(report.observations, model.len() as u64);
        assert_eq!(report.unique_edges, edges.len() as u64);
        for scope in 0..3 {
            for role in [Role::Direct, Role::NameserverAddress] {
                for hostname in 0..6 {
                    let name = format!("host{hostname}.example");
                    let expected: Vec<_> = edges
                        .iter()
                        .filter(|((s, r, h, _), _)| {
                            *s == ScopeId(scope) && *r == role && h == &name
                        })
                        .map(|((_, _, _, a), count)| (*a, *count))
                        .collect();
                    let actual = view
                        .addresses(ScopeId(scope), role, &name, None)
                        .unwrap()
                        .map(|e| {
                            let e = e.unwrap();
                            (e.address, e.supports)
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(actual, expected);
                    assert_eq!(
                        view.host_edge_count(ScopeId(scope), role, &name).unwrap(),
                        expected.len() as u64
                    );
                }
            }
        }
        for o in model.values() {
            assert_eq!(view.observation(o.id).unwrap().unwrap().to_owned(), *o);
            let expected = edges
                .keys()
                .filter(|(s, r, _, a)| *s == o.scope && *r == o.role && *a == o.address)
                .count();
            assert_eq!(
                view.ip_edge_count(o.scope, o.role, o.address).unwrap(),
                expected as u64
            );
            assert_eq!(
                view.hostnames(o.scope, o.role, o.address, None)
                    .unwrap()
                    .count(),
                expected
            );
        }
    }
}

#[test]
fn cidr_family_boundaries_and_evidence_pagination_preserve_filters() {
    let (_directory, env) = initialized(&std::env::temp_dir());
    let addresses = [
        Address::V4(0),
        Address::V4(u32::MAX),
        Address::V6(0),
        Address::V6(u128::MAX),
    ];
    let scope = ScopeId(u64::MAX);
    let role = Role::Ptr;
    let mut w = env.begin_write().unwrap();
    for (n, address) in addresses.into_iter().enumerate() {
        Projection::upsert(
            &mut w,
            &observation(n as u64, "host.example", address, scope.0, role),
        )
        .unwrap();
    }
    for n in 10..15 {
        let mut o = observation(n, "other.example", Address::V6(u128::MAX), scope.0, role);
        // Same raw evidence may contain several distinct canonical records;
        // record identity controls support, while edge identity controls counts.
        o.evidence = EvidenceId::from_bytes(b"same upstream batch");
        Projection::upsert(&mut w, &o).unwrap();
    }
    w.commit().unwrap();
    let r = env.begin_read();
    let view = Projection::view(&r).unwrap();
    assert_eq!(
        view.cidr(scope, role, prefix(Address::V4(0), 0), None)
            .unwrap()
            .count(),
        2
    );
    assert_eq!(
        view.cidr(scope, role, prefix(Address::V6(0), 0), None)
            .unwrap()
            .count(),
        3
    );
    assert_eq!(
        view.cidr(scope, role, prefix(Address::V4(u32::MAX), 32), None)
            .unwrap()
            .count(),
        1
    );
    assert_eq!(
        view.cidr(scope, role, prefix(Address::V6(u128::MAX), 128), None)
            .unwrap()
            .count(),
        2
    );
    assert_eq!(
        view.host_edge_count(scope, role, "host.example").unwrap(),
        4
    );
    let mut cursor = view
        .evidence(scope, role, "other.example", Address::V6(u128::MAX), None)
        .unwrap();
    let first = cursor.next().unwrap().unwrap();
    let token = cursor.resume_after(first.id);
    let rest = view
        .evidence(
            scope,
            role,
            "other.example",
            Address::V6(u128::MAX),
            Some(&token),
        )
        .unwrap()
        .collect::<Result<Vec<_>>>()
        .unwrap();
    assert_eq!(rest.len(), 4);
    assert!(rest.iter().all(|o| o.id != first.id));
    assert!(
        view.evidence(
            ScopeId(0),
            role,
            "other.example",
            Address::V6(u128::MAX),
            Some(&token)
        )
        .is_err()
    );
    let mut w = env.begin_write().unwrap();
    Projection::delete(&mut w, first.id).unwrap();
    w.commit().unwrap();
    let newer = env.begin_read();
    let newer = Projection::view(&newer).unwrap();
    assert!(
        newer
            .evidence(
                scope,
                role,
                "other.example",
                Address::V6(u128::MAX),
                Some(&token)
            )
            .is_err()
    );
    assert_eq!(
        view.evidence(
            scope,
            role,
            "other.example",
            Address::V6(u128::MAX),
            Some(&token)
        )
        .unwrap()
        .count(),
        4
    );
    newer.reconcile().unwrap();
}
