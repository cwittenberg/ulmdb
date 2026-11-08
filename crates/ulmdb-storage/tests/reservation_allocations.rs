use ulmdb_core::storage::{ReadStore, Reference, Result, WriteStore};
use ulmdb_storage::{Arena, SegmentTable};
struct Count {
    inner: Arena,
    zeroed: usize,
    copied_large: usize,
}
impl ReadStore for Count {
    fn read(&self, r: Reference, n: usize) -> Result<&[u8]> {
        self.inner.read(r, n)
    }
}
impl WriteStore for Count {
    fn allocate(&mut self, b: &[u8], a: usize) -> Result<Reference> {
        if b.len() >= 1024 * 1024 {
            self.copied_large += 1;
        }
        self.inner.allocate(b, a)
    }
    fn allocate_zeroed(&mut self, n: usize, a: usize) -> Result<Reference> {
        self.zeroed += 1;
        self.inner.allocate_zeroed(n, a)
    }
    fn rewrite(&mut self, r: Reference, b: &[u8], a: usize) -> Result<Reference> {
        self.inner.rewrite(r, b, a)
    }
    fn patch(&mut self, r: Reference, n: usize, o: usize, b: &[u8], a: usize) -> Result<Reference> {
        self.inner.patch(r, n, o, b, a)
    }
}
#[test]
fn one_initialized_arena_allocation_and_no_value_copy_into_kv() {
    let mut s = Count {
        inner: Arena::new(SegmentTable::default(), 1).unwrap(),
        zeroed: 0,
        copied_large: 0,
    };
    let value = s.reserve_payload(2 * 1024 * 1024).unwrap();
    let root = ulmdb_kv::put_reserved(&mut s, Reference::NULL, b"key", &value).unwrap();
    assert_eq!(s.zeroed, 1);
    assert_eq!(s.copied_large, 0);
    assert_eq!(
        ulmdb_kv::get(&s, root, b"key").unwrap().unwrap().len(),
        2 * 1024 * 1024
    );
}
