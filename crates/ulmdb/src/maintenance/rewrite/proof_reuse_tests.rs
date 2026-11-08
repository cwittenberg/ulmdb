use super::*;
use std::path::PathBuf;
use ulmdb_storage::SegmentTable;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-rewrite-proof-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        ulmdb_storage::create_layout(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn budget(visits: usize) -> Budget {
    Budget {
        allowance: 16 << 20,
        bytes: 16 << 20,
        visits,
        copied: 0,
        inspected: 0,
    }
}

#[test]
fn new_cow_parent_reuses_unchanged_child_proofs_with_the_same_visit_budget() {
    let dir = Directory::new();
    let mut seed = Arena::new(SegmentTable::default(), 1).unwrap();
    let items: Vec<_> = (0..12_000u64).map(|n| (n.to_be_bytes(), [7; 64])).collect();
    let root = ulmdb_kv::build_sorted(
        &mut seed,
        items.iter().map(|(k, v)| Ok((k.as_slice(), v.as_slice()))),
    )
    .unwrap()
    .root;
    assert!(ulmdb_kv::height(&seed, root).unwrap() >= 3);
    let (base, _) = seed.seal(&dir.0).unwrap();
    let mut progress = Progress {
        cutoff: 2,
        ..Default::default()
    };
    let mut rewrite = Arena::new(base, 2).unwrap();
    let mut first = budget(MAX_VISITS);
    let (root, done) = progress
        .walk(&mut rewrite, root, Kind::Kv, &mut first, 0, 0)
        .unwrap();
    assert!(done);
    assert!(first.inspected > 16);
    let (base, _) = rewrite.seal(&dir.0).unwrap();
    let held = base.clone();
    let mut business = Arena::new(base, 3).unwrap();
    let changed = ulmdb_kv::put(&mut business, root, &6000u64.to_be_bytes(), &[9; 64]).unwrap();
    assert_ne!(changed, root);
    let mut next = budget(16);
    let (updated, done) = progress
        .walk(&mut business, changed, Kind::Kv, &mut next, 0, 0)
        .unwrap();
    assert!(
        done,
        "unchanged subtrees lost their completed proof; inspected {} nodes",
        next.inspected
    );
    assert_eq!(updated, changed);
    assert!(
        next.inspected <= 4,
        "unmodified completed children were reread"
    );
    assert_eq!(next.copied, 0);
    assert_eq!(
        ulmdb_kv::get(&business, updated, &6000u64.to_be_bytes()).unwrap(),
        Some([9; 64].as_slice())
    );
    assert_eq!(
        ulmdb_kv::get(&held, root, &6000u64.to_be_bytes()).unwrap(),
        Some([7; 64].as_slice())
    );
    assert_eq!(
        ulmdb_kv::audit_tree(&business, updated, 10_000)
            .unwrap()
            .leaf_records,
        12_000
    );
}
