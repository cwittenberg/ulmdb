use super::*;
#[test]
fn repeatable_unique_sized_inputs_and_expected_results() {
    let config = Config {
        seed: 73,
        prefixes: 200,
        kv: 300,
        queries: 90,
        prefix_profile: PrefixProfile::Gapped,
    };
    let first = Workload::generate(config).unwrap();
    let second = Workload::generate(config).unwrap();
    assert_eq!(first.prefixes.0.len(), 200);
    assert_eq!(first.kv.len(), 300);
    assert_eq!(first.queries.len(), 90);
    assert_eq!(first.encoded(), second.encoded());
    let other = Workload::generate(Config { seed: 74, ..config }).unwrap();
    assert_ne!(first.encoded(), other.encoded());
    assert!(first.prefixes.0.keys().all(|p| p.length() != 0));
    for (i, &query) in first.queries.iter().enumerate() {
        assert_eq!(
            first.prefixes.longest_match(query).is_some(),
            (i / 2).is_multiple_of(2)
        );
    }
    let defaults = Workload::generate(Config {
        prefix_profile: PrefixProfile::WithDefaults,
        ..config
    })
    .unwrap();
    assert!(defaults.prefixes.0.keys().any(|p| p.length() == 0));
    assert!(
        defaults
            .queries
            .iter()
            .all(|q| defaults.prefixes.longest_match(*q).is_some())
    );
}
#[test]
fn empty_and_oversized_workloads_are_defined() {
    let w = Workload::generate(Config {
        seed: 0,
        prefixes: 0,
        kv: 0,
        queries: 3,
        prefix_profile: PrefixProfile::Gapped,
    })
    .unwrap();
    assert_eq!(w.encoded()[3].1.len(), 19);
    assert!(
        Workload::generate(Config {
            seed: 0,
            prefixes: 2_000_001,
            kv: 0,
            queries: 0,
            prefix_profile: PrefixProfile::Gapped,
        })
        .is_err()
    );
    assert!(
        Workload::generate(Config {
            seed: 0,
            prefixes: 10_000,
            kv: 0,
            queries: 10_000,
            prefix_profile: PrefixProfile::Gapped
        })
        .is_err()
    );
}
