use super::*;
use std::collections::BTreeMap;
#[path = "control.rs"]
mod control;
thread_local! {static BASELINE_ALLOCATIONS:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
fn baseline_allocation() {
    BASELINE_ALLOCATIONS.with(|n| n.set(n.get() + 1));
}
fn baseline_count() -> usize {
    BASELINE_ALLOCATIONS.with(std::cell::Cell::get)
}
fn check<K: Ord + Clone + std::fmt::Debug, V: Clone>(
    m: &Map<K, V>,
    low: Option<&K>,
    high: Option<&K>,
) -> (u16, usize) {
    let Some(n) = &m.0 else { return (0, 0) };
    assert!(low.is_none_or(|k| k < &n.key));
    assert!(high.is_none_or(|k| k > &n.key));
    let (lh, ls) = check(&n.left, low, Some(&n.key));
    let (rh, rs) = check(&n.right, Some(&n.key), high);
    assert!(
        lh.abs_diff(rh) <= 1,
        "unbalanced key {:?}: {lh}/{rh}",
        n.key
    );
    assert_eq!(n.height, 1 + lh.max(rh));
    assert_eq!(n.size, 1 + ls + rs);
    (n.height, n.size)
}
#[test]
fn randomized_correspondence_checks_balance_and_every_retained_root() {
    let mut m = Map::default();
    let mut oracle = BTreeMap::new();
    let mut old = Vec::new();
    let mut seed = 73u64;
    for i in 0..12000u32 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let k = ((seed >> 25) % 1021) as u32;
        if i % 5 == 0 {
            m.remove(&k);
            oracle.remove(&k);
        } else {
            m.insert(k, i);
            oracle.insert(k, i);
        }
        check(&m, None, None);
        assert_eq!(m.len(), oracle.len());
        assert_eq!(
            m.entries(),
            oracle.iter().map(|(a, b)| (*a, *b)).collect::<Vec<_>>()
        );
        if i % 251 == 0 {
            old.push((m.clone(), m.entries()));
        }
        if i % 127 == 0 {
            for (snapshot, rows) in &old {
                check(snapshot, None, None);
                assert_eq!(snapshot.entries(), *rows);
            }
        }
    }
    for (snapshot, rows) in old {
        check(&snapshot, None, None);
        assert_eq!(snapshot.entries(), rows);
    }
}
#[test]
fn rotations_successor_removal_empty_and_root_deletions_preserve_shared_snapshots() {
    for keys in [
        vec![3, 2, 1],
        vec![1, 2, 3],
        vec![3, 1, 2],
        vec![1, 3, 2],
        (0..255).collect(),
        (0..255).rev().collect(),
    ] {
        let mut m = Map::default();
        for k in &keys {
            m.insert(*k, *k + 9);
            check(&m, None, None);
        }
        let saved = m.clone();
        let old = saved.entries();
        for k in &keys {
            m.remove(k);
            check(&m, None, None);
            assert_eq!(saved.entries(), old);
        }
        assert_eq!(m.len(), 0);
        m.remove(&999);
        assert_eq!(m.first(), None);
        m.insert(7, 1);
        m.insert(7, 2);
        assert_eq!(m.entries(), vec![(7, 2)]);
        m.remove(&7);
        assert_eq!(m.len(), 0);
    }
}
#[test]
fn owned_pending_updates_reuse_nodes_and_shared_detach_happens_once() {
    NODE_ALLOCATIONS.with(|n| n.set(0));
    BASELINE_ALLOCATIONS.with(|n| n.set(0));
    let mut m = Map::default();
    let mut baseline = control::Map::default();
    for key in 0..4095u32 {
        m.insert(key, 0u32);
        baseline.insert(key, 0u32);
    }
    let build_cow = allocation_count();
    let build_full = baseline_count();
    assert_eq!(build_cow, 4095);
    let saved = m.clone();
    let saved_baseline = baseline.clone();
    let saved_rows = saved.entries();
    let before = allocation_count();
    m.insert(4041, 1);
    let detach = allocation_count() - before;
    assert!(detach > 0 && detach <= saved.height() as usize);
    let before = allocation_count();
    let old_before = baseline_count();
    for n in 2..8194u32 {
        m.insert(4041, n);
        baseline.insert(4041, n);
    }
    let repeated = allocation_count() - before;
    let repeated_full = baseline_count() - old_before;
    assert_eq!(repeated, 0);
    assert!(repeated_full > 8192);
    assert_eq!(saved.entries(), saved_rows);
    assert_eq!(saved_baseline.get(&4041), Some(&0));
    assert_eq!(m.entries(), baseline.entries());
    check(&m, None, None);
    drop(saved);
    drop(saved_baseline);
    let before = allocation_count();
    for n in 0..4095u32 {
        m.remove(&n);
    }
    let deletion = allocation_count() - before;
    assert_eq!(deletion, 0);
    assert_eq!(m.len(), 0);
    println!(
        "MAP_ALLOCATION_MECHANISM {{\"build_nodes_cow\":{build_cow},\"build_nodes_frozen_control\":{build_full},\"first_shared_detach_nodes\":{detach},\"repeated_updates\":8192,\"repeated_nodes_cow\":{repeated},\"repeated_nodes_frozen_control\":{repeated_full},\"unique_delete_nodes_cow\":{deletion}}}"
    );
}
