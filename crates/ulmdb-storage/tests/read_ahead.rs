#![cfg(target_os = "linux")]
#[path = "support/read_ahead.rs"]
mod support;
use ulmdb_storage::{Arena, ReadAhead, ReadStore, SegmentTable, WriteStore};
struct Dir(std::path::PathBuf);
impl Dir {
    fn new() -> Self {
        let d = Self(std::env::temp_dir().join(format!(
            "read-ahead-storage-{}",
            ulmdb_storage::new_identity().unwrap()
        )));
        ulmdb_storage::create_layout(&d.0).unwrap();
        d
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn real_mapping_advice_survives_clone_and_new_committed_segments() {
    for advice in [ReadAhead::Normal, ReadAhead::Random] {
        let d = Dir::new();
        let mut arena =
            Arena::new(SegmentTable::empty_with_read_ahead(advice).unwrap(), 1).unwrap();
        let at = arena.put_payload(&vec![7; 8192]).unwrap();
        let (table, _) = arena.seal(&d.0).unwrap();
        let old = table.clone();
        let borrowed = old.payload(at).unwrap();
        support::assert_random(borrowed, advice == ReadAhead::Random);
        let mut next = Arena::new(table, 2).unwrap();
        let new = next.put_payload(&vec![8; 8192]).unwrap();
        let (table, _) = next.seal(&d.0).unwrap();
        support::assert_random(table.payload(new).unwrap(), advice == ReadAhead::Random);
        assert_eq!(borrowed, vec![7; 8192]);
        support::assert_random(borrowed, advice == ReadAhead::Random);
    }
}
#[test]
fn different_policy_requires_an_independent_vma_and_keeps_old_borrow_unchanged() {
    let d = Dir::new();
    let mut arena = Arena::new(
        SegmentTable::empty_with_read_ahead(ReadAhead::Random).unwrap(),
        1,
    )
    .unwrap();
    let at = arena.put_payload(&vec![9; 8192]).unwrap();
    let (random, entry) = arena.seal(&d.0).unwrap();
    let entry = entry.unwrap();
    let old = random.payload(at).unwrap();
    let mut normal = SegmentTable::default();
    assert!(!normal.reuse_from(&random, &entry).unwrap());
    normal.add(&d.0, entry.clone()).unwrap();
    let normal_bytes = normal.payload(at).unwrap();
    assert_ne!(normal_bytes.as_ptr(), old.as_ptr());
    support::assert_random(normal_bytes, false);
    support::assert_random(old, true);
    assert_eq!(normal_bytes, old);
    let mut same = SegmentTable::empty_with_read_ahead(ReadAhead::Random).unwrap();
    assert!(same.reuse_from(&random, &entry).unwrap());
    assert_eq!(same.payload(at).unwrap().as_ptr(), old.as_ptr());
    let mut malformed = entry;
    malformed.hash[0] ^= 1;
    assert!(normal.reuse_from(&random, &malformed).is_err());
}
