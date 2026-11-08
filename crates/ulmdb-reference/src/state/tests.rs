use super::*;

#[test]
fn snapshot_commit_abort_duplicates_and_ranges() {
    let mut env = Environment::default();
    let old = env.snapshot();
    let mut w = env.begin_write();
    w.private.kv.insert(vec![], vec![]);
    w.private.kv.insert(vec![0], vec![1]);
    w.private.duplicates.insert((vec![], vec![]));
    assert!(!w.private.duplicates.insert((vec![], vec![])));
    assert_eq!(w.commit(), 1);
    assert!(old.1.kv.is_empty());
    let mut w = env.begin_write();
    w.private.kv.clear();
    w.abort();
    let (_, s) = env.snapshot();
    assert_eq!(
        s.range(Bound::Unbounded, Bound::Unbounded, true),
        vec![(&[0][..], &[1][..]), (&[][..], &[][..])]
    );
    assert!(
        s.range(Bound::Included(&[1]), Bound::Included(&[0]), false)
            .is_empty()
    );
    assert_eq!(s.duplicate_values(&[]), vec![&[][..]]);
}
