use ulmdb::{Environment, backup};
use ulmdb_ipdon::*;
#[test]
fn projection_reconciliation_pagination_and_export_survive_compaction_and_reclaim() {
    let root = std::env::temp_dir().join(format!(
        "ulmdb-ipdon-maintenance-{}",
        ulmdb_storage::new_identity().unwrap()
    ));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("database");
    let env = Environment::create(&path).unwrap();
    let mut write = env.begin_write().unwrap();
    Projection::initialize(&mut write).unwrap();
    let first = Observation {
        id: ObservationId::from_source(SourceId(1), b"first"),
        hostname: "Example.Test.".into(),
        address: Address::V4(0xc000_0201),
        scope: ScopeId(1),
        source: SourceId(1),
        role: Role::Direct,
        observed_at: 1,
        collected_at: 2,
        vantage: VantageId(3),
        resolver: ResolverId(4),
        ttl: 300,
        dnssec: Dnssec::Secure,
        evidence: EvidenceId::from_bytes(b"first"),
    };
    let mut second = first.clone();
    second.id = ObservationId::from_source(SourceId(2), b"second");
    second.source = SourceId(2);
    let mut third = first.clone();
    third.id = ObservationId::from_source(SourceId(1), b"third");
    third.address = Address::V6(1);
    for observation in [&first, &second, &third] {
        Projection::upsert(&mut write, observation).unwrap();
    }
    write.commit().unwrap();
    let old = env.begin_read();
    let view = Projection::view(&old).unwrap();
    let token = view
        .addresses(ScopeId(1), Role::Direct, "example.test", None)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .resume;
    let expected = view.reconcile().unwrap();
    env.retain_snapshot("projection", &old).unwrap();
    let old_generation = old.generation();
    let sequence = old.sequence();
    let commit = old.commit_identity();
    env.compact().unwrap();
    env.compact().unwrap();
    env.reclaim().unwrap();
    let read = env.begin_read();
    let current = Projection::view(&read).unwrap();
    assert_eq!(current.reconcile().unwrap(), expected);
    assert_eq!(read.sequence(), sequence);
    assert_eq!(read.commit_identity(), commit);
    assert_ne!(read.generation(), old_generation);
    assert!(
        current
            .addresses(ScopeId(1), Role::Direct, "example.test", Some(&token))
            .is_err()
    );
    assert_eq!(
        view.addresses(ScopeId(1), Role::Direct, "example.test", Some(&token))
            .unwrap()
            .count(),
        1
    );
    let export = root.join("export");
    backup::export_snapshot(&env, &old, &export).unwrap();
    drop(read);
    drop(old);
    drop(env);
    let env = Environment::open(&path).unwrap();
    let retained = env.open_retained("projection").unwrap();
    assert_eq!(
        Projection::view(&retained).unwrap().reconcile().unwrap(),
        expected
    );
    env.release_snapshot("projection").unwrap();
    env.reclaim().unwrap();
    drop(retained);
    env.reclaim().unwrap();
    let copy = Environment::open(export).unwrap();
    assert_eq!(
        Projection::view(&copy.begin_read())
            .unwrap()
            .reconcile()
            .unwrap(),
        expected
    );
    drop(copy);
    drop(env);
    std::fs::remove_dir_all(root).unwrap();
}
