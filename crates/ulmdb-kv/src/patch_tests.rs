use crate::{
    PAGE_SIZE, get,
    page::{self, Page},
    put,
};
use ulmdb_core::storage::{Error, ReadStore, Reference, Result, WriteStore};
#[derive(Default)]
struct DefaultStore {
    bytes: Vec<u8>,
    reuse: bool,
}
impl ReadStore for DefaultStore {
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("patch test segment"));
        }
        let start = at.offset as usize;
        self.bytes
            .get(start..start.checked_add(len).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("patch test bounds"))
    }
}
impl WriteStore for DefaultStore {
    fn allocate(&mut self, bytes: &[u8], align: usize) -> Result<Reference> {
        let start = self.bytes.len().div_ceil(align) * align;
        self.bytes.resize(start, 0);
        self.bytes.extend_from_slice(bytes);
        Ok(Reference {
            segment: 1,
            offset: start as u64,
        })
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], align: usize) -> Result<Reference> {
        if self.reuse {
            let start = old.offset as usize;
            self.bytes[start..start + bytes.len()].copy_from_slice(bytes);
            Ok(old)
        } else {
            self.allocate(bytes, align)
        }
    }
}
#[derive(Default)]
struct Store {
    inner: DefaultStore,
    patches: Vec<(Reference, usize, usize)>,
}
impl ReadStore for Store {
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        self.inner.read(at, len)
    }
}
impl WriteStore for Store {
    fn allocate(&mut self, b: &[u8], a: usize) -> Result<Reference> {
        self.inner.allocate(b, a)
    }
    fn rewrite(&mut self, r: Reference, b: &[u8], a: usize) -> Result<Reference> {
        self.inner.rewrite(r, b, a)
    }
    fn patch(
        &mut self,
        r: Reference,
        len: usize,
        offset: usize,
        b: &[u8],
        a: usize,
    ) -> Result<Reference> {
        self.patches.push((r, offset, b.len()));
        self.inner.patch(r, len, offset, b, a)
    }
}
#[test]
fn private_inline_updates_patch_eight_bytes_and_noops_do_nothing() {
    let mut s = Store::default();
    s.inner.reuse = true;
    let root = put(&mut s, Reference::NULL, b"key", &0u64.to_le_bytes()).unwrap();
    let initial = s.inner.bytes.len();
    for n in 1..1000u64 {
        assert_eq!(put(&mut s, root, b"key", &n.to_le_bytes()).unwrap(), root);
    }
    assert_eq!(s.inner.bytes.len(), initial);
    assert_eq!(s.patches.len(), 999);
    assert!(s.patches.iter().all(|(_, _, length)| *length == 8));
    let count = s.patches.len();
    put(&mut s, root, b"key", &999u64.to_le_bytes()).unwrap();
    assert_eq!(s.patches.len(), count);
    assert_eq!(
        get(&s, root, b"key").unwrap(),
        Some(&999u64.to_le_bytes()[..])
    );
}
#[test]
fn first_and_nonfirst_branch_pointer_patches_preserve_immutable_roots() {
    let mut s = Store::default();
    let mut root = Reference::NULL;
    for n in 0..1000u64 {
        root = put(&mut s, root, &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
    }
    assert!(Page::load(&s, root).unwrap().level > 0);
    let old = root;
    s.patches.clear();
    root = put(&mut s, root, &0u64.to_be_bytes(), b"changed!").unwrap();
    assert!(
        s.patches
            .iter()
            .any(|(at, offset, length)| *at == old && *offset == 16 && *length == 16)
    );
    assert_eq!(
        get(&s, old, &0u64.to_be_bytes()).unwrap(),
        Some(&0u64.to_le_bytes()[..])
    );
    s.patches.clear();
    root = put(&mut s, root, &999u64.to_be_bytes(), b"updated!").unwrap();
    assert!(
        s.patches
            .iter()
            .any(|(_, offset, length)| *offset >= 36 && *length == 16)
    );
    assert_eq!(
        get(&s, old, &999u64.to_be_bytes()).unwrap(),
        Some(&999u64.to_le_bytes()[..])
    );
    assert_eq!(
        get(&s, root, &999u64.to_be_bytes()).unwrap(),
        Some(&b"updated!"[..])
    );
    for n in 1000..1200u64 {
        root = put(&mut s, root, &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
    }
    for n in 1000..1200u64 {
        assert_eq!(
            get(&s, root, &n.to_be_bytes()).unwrap(),
            Some(&n.to_le_bytes()[..])
        );
    }
    crate::audit_tree(&s, root, 1000).unwrap();
}
#[test]
fn eight_byte_semantic_overflow_value_is_not_an_inline_patch() {
    let mut s = Store::default();
    let payload = s.put_payload(b"old8byte").unwrap();
    let mut raw = vec![3, 0, 2, 0];
    raw.extend_from_slice(&8u32.to_le_bytes());
    raw.extend_from_slice(b"key");
    raw.extend_from_slice(&payload.encode());
    raw.push(0);
    let root = s
        .allocate(
            &page::encode(0, Reference::NULL, &[&raw]).unwrap(),
            PAGE_SIZE,
        )
        .unwrap();
    let next = put(&mut s, root, b"key", b"new8byte").unwrap();
    assert!(
        s.patches.is_empty(),
        "overflow representation must take layout-change fallback"
    );
    assert_eq!(get(&s, root, b"key").unwrap(), Some(&b"old8byte"[..]));
    assert_eq!(get(&s, next, b"key").unwrap(), Some(&b"new8byte"[..]));
    let page = Page::load(&s, next).unwrap();
    assert_eq!(page.encoding(&page.cell(&s, 0).unwrap()).unwrap().0 & 2, 0);
}
#[test]
fn valid_external_oversized_inline_key_and_value_keep_their_layout() {
    let mut s = Store::default();
    let key = vec![0x31; 1000];
    let old_value = vec![0x41; 600];
    let new_value = vec![0x42; 600];
    let mut raw = Vec::new();
    raw.extend_from_slice(&1000u16.to_le_bytes());
    raw.extend_from_slice(&0u16.to_le_bytes());
    raw.extend_from_slice(&600u32.to_le_bytes());
    raw.extend_from_slice(&key);
    raw.extend_from_slice(&old_value);
    let root = s
        .allocate(
            &page::encode(0, Reference::NULL, &[&raw]).unwrap(),
            PAGE_SIZE,
        )
        .unwrap();
    let new = put(&mut s, root, &key, &new_value).unwrap();
    assert_eq!(s.patches.len(), 1);
    assert_eq!(s.patches[0].2, 600);
    assert_eq!(get(&s, root, &key).unwrap(), Some(old_value.as_slice()));
    assert_eq!(get(&s, new, &key).unwrap(), Some(new_value.as_slice()));
    let page = Page::load(&s, new).unwrap();
    let raw = page.raw_cell(0).unwrap();
    assert_eq!(page::u16_at(raw, 0).unwrap(), 1000);
    assert_eq!(page::u16_at(raw, 2).unwrap(), 0);
}
