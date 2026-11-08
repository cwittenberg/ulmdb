use super::*;
#[derive(Default)]
struct Store {
    bytes: Vec<u8>,
    rewrites: usize,
}
impl ReadStore for Store {
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("missing test object"));
        }
        let start = at.offset as usize;
        self.bytes
            .get(start..start.checked_add(len).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("test object bounds"))
    }
}
impl WriteStore for Store {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        let start = self.bytes.len().div_ceil(alignment) * alignment;
        self.bytes.resize(start, 0);
        self.bytes.extend_from_slice(bytes);
        Ok(Reference {
            segment: 1,
            offset: start as u64,
        })
    }
    fn rewrite(&mut self, _old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.rewrites += 1;
        self.allocate(bytes, alignment)
    }
}
#[test]
fn default_patch_preserves_immutable_object_and_noop_avoids_rewrite() {
    let mut s = Store::default();
    let old = s.allocate(&[0; 16], 16).unwrap();
    let new = s.patch(old, 16, 4, b"XY", 16).unwrap();
    assert_ne!(old, new);
    assert_eq!(s.read(old, 16).unwrap(), [0; 16]);
    assert_eq!(&s.read(new, 16).unwrap()[4..6], b"XY");
    assert_eq!(s.patch(new, 16, 4, b"XY", 16).unwrap(), new);
    assert_eq!(s.rewrites, 1);
}
#[test]
fn invalid_patch_inputs_fail_before_rewrite() {
    let mut s = Store::default();
    let old = s.allocate(&[0; 16], 16).unwrap();
    for (length, offset, alignment) in [
        (16, 16, 16),
        (15, 15, 16),
        (16, usize::MAX, 16),
        (16, 0, 0),
        (16, 0, 3),
        (16, 0, 32768),
    ] {
        assert!(s.patch(old, length, offset, b"XY", alignment).is_err());
    }
    assert!(s.patch(Reference::NULL, 16, 0, b"XY", 16).is_err());
    assert_eq!(s.bytes, [0; 16]);
    assert_eq!(s.rewrites, 0);
    assert_eq!(s.patch(old, 16, 16, b"", 16).unwrap(), old);
}

#[test]
fn optional_multi_patch_is_object_safe_and_default_declines_without_work() {
    struct NoWork;
    impl ReadStore for NoWork {
        fn read(&self, _: Reference, _: usize) -> Result<&[u8]> {
            panic!("declined capability must not read")
        }
    }
    impl WriteStore for NoWork {
        fn allocate(&mut self, _: &[u8], _: usize) -> Result<Reference> {
            panic!("declined capability must not allocate")
        }
        fn rewrite(&mut self, _: Reference, _: &[u8], _: usize) -> Result<Reference> {
            panic!("declined capability must not rewrite")
        }
    }
    let mut implementation = NoWork;
    let store: &mut dyn WriteStore = &mut implementation;
    assert!(!store.supports_patch_many(4096, 4096));
    assert_eq!(
        store
            .try_patch_many(Reference::NULL, 0, &[(usize::MAX, b"bad")], 0)
            .unwrap(),
        None
    );
}
