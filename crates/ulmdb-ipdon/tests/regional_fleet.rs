//! Seventeen independent native databases and all272 direct replication paths.
//! This is an in-process protocol journey, not deployed WAN/network evidence.
use std::path::PathBuf;
use ulmdb::Environment;
use ulmdb_ipdon::{
    journal,
    regional::{Command, Regional},
    rrsets, *,
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ipdon-fleet-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
const REALM: u128 = 0x464c454554;
fn command(origin: u64, revision: u64, time: u64) -> Command {
    Command::ReplaceRrset {
        set: rrsets::Rrset {
            key: rrsets::Key {
                origin,
                source: SourceId(origin),
                scope: ScopeId(1),
                resolver: ResolverId(1),
                vantage: VantageId(origin),
                role: Role::Direct,
                hostname: "regional.test".into(),
                kind: rrsets::RecordType::A,
            },
            revision,
            addresses: vec![Address::V4(
                0xc0000200 + origin as u32 + (revision as u32 - 1) * 256,
            )],
            dnssec: Dnssec::Secure,
        },
        freshness: rrsets::Freshness {
            observed_at: time,
            recorded_at: time + 1,
            expires_at: 10000,
            evidence: EvidenceId::from_bytes(&[origin as u8, revision as u8]),
        },
    }
}
fn replicate(source: &Environment, target: &Environment, origin: u64) {
    let read = source.begin_read();
    let after = journal::cursor(&target.begin_read(), origin)
        .unwrap()
        .applied;
    let page = journal::page(&read, origin, after, journal::Limits::default()).unwrap();
    let mut write = target.begin_write().unwrap();
    Regional::apply_page(&mut write, &page.events, journal::Limits::default()).unwrap();
    write.commit().unwrap();
}
fn rows(env: &Environment) -> Vec<(u64, Address, u64)> {
    let read = env.begin_read();
    let view = Regional::view(&read).unwrap();
    let page = view
        .rrsets
        .query(
            &rrsets::Query::Hostname {
                scope: ScopeId(1),
                role: Role::Direct,
                hostname: "regional.test".into(),
            },
            100,
            None,
            rrsets::Limits::default(),
        )
        .unwrap();
    let mut values = page
        .relations
        .into_iter()
        .map(|r| (r.origin, r.address, r.revision))
        .collect::<Vec<_>>();
    values.sort();
    values
}

#[test]
fn seventeen_replicas_converge_partition_and_recover_without_losing_local_observations() {
    let root = Directory::new();
    std::fs::create_dir(&root.0).unwrap();
    let mut fleet = Vec::new();
    for origin in 1..=17u64 {
        let env = Environment::create(root.0.join(format!("node-{origin}"))).unwrap();
        let mut write = env.begin_write().unwrap();
        Regional::initialize_in_realm(&mut write, 1, REALM).unwrap();
        Regional::apply_page(
            &mut write,
            &[command(origin, 1, 10).event(origin, 1, 11).unwrap()],
            journal::Limits::default(),
        )
        .unwrap();
        write.commit().unwrap();
        fleet.push(env);
    }
    let mut paths = 0;
    for (i, source) in fleet.iter().enumerate() {
        for (j, target) in fleet.iter().enumerate() {
            if i != j {
                replicate(source, target, i as u64 + 1);
                paths += 1;
            }
        }
    }
    assert_eq!(paths, 272);
    let expected = rows(&fleet[0]);
    assert_eq!(expected.len(), 17);
    for env in &fleet {
        assert_eq!(rows(env), expected);
        assert!(
            journal::cursors(&env.begin_read())
                .unwrap()
                .values()
                .all(|c| c.applied == 1 && c.serving == 1)
        );
    }
    // Node17 is isolated. Both it and origin1 continue local acquisition.
    for index in [0usize, 16] {
        let origin = index as u64 + 1;
        let mut write = fleet[index].begin_write().unwrap();
        Regional::apply_page(
            &mut write,
            &[command(origin, 2, 20).event(origin, 2, 21).unwrap()],
            journal::Limits::default(),
        )
        .unwrap();
        write.commit().unwrap();
    }
    for target in &fleet[1..16] {
        replicate(&fleet[0], target, 1);
    }
    assert_eq!(
        journal::cursor(&fleet[16].begin_read(), 1).unwrap().applied,
        1
    );
    assert_eq!(
        journal::cursor(&fleet[0].begin_read(), 17).unwrap().applied,
        1
    );
    // Healthy checkpoint contains origin1's update, but predates17's local tail.
    let bundle = root.0.join("checkpoint");
    journal::export_snapshot(&fleet[0], &fleet[0].begin_read(), &bundle).unwrap();
    let recovered = journal::rebase_all_snapshot(
        &bundle,
        &fleet[16],
        root.0.join("recovered17"),
        journal::Limits {
            max_events: 8,
            ..journal::Limits::default()
        },
        Regional::apply_event,
    )
    .unwrap();
    let guard = journal::validate_full_handover(&fleet[16], &recovered).unwrap();
    assert_eq!(
        journal::cursor(&recovered.begin_read(), 1).unwrap().applied,
        2
    );
    assert_eq!(
        journal::cursor(&recovered.begin_read(), 17)
            .unwrap()
            .applied,
        2
    );
    drop(guard);
    for target in &fleet[..16] {
        replicate(&recovered, target, 17);
    }
    let expected = rows(&recovered);
    for env in &fleet[..16] {
        assert_eq!(rows(env), expected);
    }
    let mut write = recovered.begin_write().unwrap();
    Regional::apply_page(
        &mut write,
        &[command(17, 3, 30).event(17, 3, 31).unwrap()],
        journal::Limits::default(),
    )
    .unwrap();
    write.commit().unwrap();
    assert_eq!(
        journal::cursor(&recovered.begin_read(), 17)
            .unwrap()
            .applied,
        3
    );
}
