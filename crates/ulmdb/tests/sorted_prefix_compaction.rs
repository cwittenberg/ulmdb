use std::{collections::BTreeMap, path::PathBuf};
use ulmdb::{Address, DatabaseKind, Environment, Prefix, PublicationMode, ReadTxn};
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn p(a: Address, n: u8) -> Prefix {
    Prefix::normalize(a, n).unwrap()
}
fn rows(ipv6: bool) -> Vec<(Prefix, Vec<u8>)> {
    let address = |v| {
        if ipv6 {
            Address::V6(v)
        } else {
            Address::V4(v as u32)
        }
    };
    let width = if ipv6 { 128 } else { 32 };
    let mut rows = BTreeMap::new();
    for length in 0..=width {
        for value in [0, u128::MAX, 0x20010db8123456789abcdef0u128] {
            let prefix = p(address(value), length);
            rows.insert((prefix.network(), length), vec![length]);
        }
    }
    for n in 0..1000u128 {
        let prefix = p(address(n.wrapping_mul(0x9e3779b97f4a7c15)), width);
        rows.insert((prefix.network(), width), n.to_le_bytes().to_vec());
    }
    rows.insert((address(0), 0), Vec::new());
    rows.into_iter().map(|((a, n), v)| (p(a, n), v)).collect()
}
fn check(read: &ReadTxn, name: &[u8], rows: &[(Prefix, Vec<u8>)]) {
    let whole = p(rows[0].0.network(), 0);
    let actual: Vec<_> = read
        .subnets(name, whole)
        .unwrap()
        .map(|r| {
            let (p, v) = r.unwrap();
            (p, v.to_vec())
        })
        .collect();
    assert_eq!(actual, rows);
    for (prefix, value) in rows {
        assert_eq!(read.exact_prefix(name, *prefix).unwrap().unwrap().1, value);
        assert_eq!(
            read.longest_match(name, prefix.network())
                .unwrap()
                .unwrap()
                .1,
            rows.iter()
                .filter(|(p, _)| p.contains_address(prefix.network()))
                .max_by_key(|(p, _)| p.length())
                .unwrap()
                .1
        );
        assert_eq!(
            read.covering_prefix(name, *prefix).unwrap().unwrap().1,
            value
        );
    }
}
#[test]
fn direct_prefix_compaction_preserves_all_lengths_old_borrows_and_future_updates() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for mode in [
            PublicationMode::Legacy,
            PublicationMode::Cohort,
            PublicationMode::Indexed,
        ] {
            let dir = Directory(parent.join(format!(
                "ulmdb-prefix-compact-{:032x}",
                ulmdb_storage::new_identity().unwrap()
            )));
            let env = Environment::create_with_publication(&dir.0, mode).unwrap();
            let mut write = env.begin_write().unwrap();
            let mut families = Vec::new();
            for (name, kind, ipv6) in [
                (b"v4".as_slice(), DatabaseKind::Ipv4, false),
                (b"v6".as_slice(), DatabaseKind::Ipv6, true),
            ] {
                write.create_database(name, kind).unwrap();
                let mut entries = rows(ipv6);
                for (prefix, value) in &entries {
                    write.insert_prefix(name, *prefix, value).unwrap();
                }
                // Fragment the original tree and verify the direct builder handles deletion gaps.
                let removed = entries.remove(entries.len() / 2);
                write.delete_prefix(name, removed.0).unwrap();
                families.push((name, entries));
            }
            write
                .create_database(b"empty4", DatabaseKind::Ipv4)
                .unwrap();
            write
                .create_database(b"empty6", DatabaseKind::Ipv6)
                .unwrap();
            write.commit().unwrap();
            let old = env.begin_read();
            let state = old.stats();
            let borrowed = old
                .exact_prefix(b"v4", families[0].1[2].0)
                .unwrap()
                .unwrap()
                .1;
            let expected_borrow = borrowed.to_vec();
            let report = env.compact().unwrap();
            assert_eq!(
                report.prefix_values,
                families.iter().map(|(_, r)| r.len() as u64).sum::<u64>()
            );
            assert_eq!(env.stats().sequence, state.sequence);
            assert_eq!(env.stats().commit, state.commit);
            assert_ne!(env.stats().generation, state.generation);
            let current = env.begin_read();
            for (name, rows) in &families {
                check(&old, name, rows);
                check(&current, name, rows);
            }
            assert_eq!(
                current
                    .subnets(b"empty4", p(Address::V4(0), 0))
                    .unwrap()
                    .count(),
                0
            );
            assert_eq!(
                current
                    .subnets(b"empty6", p(Address::V6(0), 0))
                    .unwrap()
                    .count(),
                0
            );
            let mut write = env.begin_write().unwrap();
            let full = p(Address::V6(u128::MAX), 128);
            write.delete_prefix(b"v6", full).unwrap();
            write.commit().unwrap();
            let mut write = env.begin_write().unwrap();
            write.insert_prefix(b"v6", full, b"reinserted").unwrap();
            write.abort();
            assert!(
                env.begin_read()
                    .exact_prefix(b"v6", full)
                    .unwrap()
                    .is_none()
            );
            drop(current);
            drop(env);
            assert_eq!(borrowed, expected_borrow);
            let reopened = Environment::open(&dir.0).unwrap();
            let read = reopened.begin_read();
            check(&read, b"v4", &families[0].1);
            let remaining: Vec<_> = families[1]
                .1
                .iter()
                .filter(|(p, _)| *p != full)
                .cloned()
                .collect();
            check(&read, b"v6", &remaining);
            assert_eq!(borrowed, expected_borrow);
        }
    }
}
