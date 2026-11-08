use crate::{PAGE_SIZE, Reader, get_pair, put};
use std::{cell::Cell, collections::BTreeMap};
use ulmdb_core::storage::{Error, ReadStore, Reference, Result, WriteStore};

#[derive(Default)]
struct Store {
    bytes: Vec<u8>,
    page_reads: Cell<usize>,
}
impl ReadStore for Store {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("reader test segment"));
        }
        if length == PAGE_SIZE {
            self.page_reads.set(self.page_reads.get() + 1);
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes
            .get(start..start.checked_add(length).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("reader test extent"))
    }
}
impl WriteStore for Store {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        let offset = self.bytes.len().div_ceil(alignment) * alignment;
        self.bytes.resize(offset, 0);
        self.bytes.extend_from_slice(bytes);
        Ok(Reference {
            segment: 1,
            offset: offset as u64,
        })
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], _alignment: usize) -> Result<Reference> {
        let start = old.offset as usize;
        self.bytes[start..start + bytes.len()].copy_from_slice(bytes);
        Ok(old)
    }
}
#[test]
fn empty_and_leaf_binding_avoid_page_reads_and_borrow_independently_of_reader() {
    let mut store = Store::default();
    let empty = Reader::new(&store, Reference::NULL).unwrap();
    assert_eq!(empty.height(), 0);
    assert_eq!(empty.get(b"missing").unwrap(), None);
    assert_eq!(store.page_reads.get(), 0);
    let mut root = Reference::NULL;
    for (key, value) in [
        (b"a".as_slice(), b"one".as_slice()),
        (b"b", b"two"),
        (b"c", b""),
    ] {
        root = put(&mut store, root, key, value).unwrap();
    }
    store.page_reads.set(0);
    let (key, value) = {
        let reader = Reader::new(&store, root).unwrap();
        assert_eq!(reader.height(), 1);
        for _ in 0..100 {
            assert_eq!(reader.get(b"a").unwrap(), Some(b"one".as_slice()));
            assert_eq!(reader.get(b"c").unwrap(), Some(b"".as_slice()));
            assert_eq!(reader.get(b"after").unwrap(), None);
        }
        assert_eq!(
            store.page_reads.get(),
            1,
            "only binding resolves a leaf root"
        );
        {
            let needle = vec![b'b'];
            reader.get_pair(&needle).unwrap().unwrap()
        }
    };
    assert_eq!((key, value), (b"b".as_slice(), b"two".as_slice()));
}

#[test]
fn multilevel_hits_and_misses_match_an_independent_map_and_save_one_load_per_query() {
    let mut store = Store::default();
    let mut root = Reference::NULL;
    let mut model = BTreeMap::new();
    for n in 0..3000u64 {
        let key = (n * 2).to_be_bytes();
        let value = (n * 17).to_le_bytes();
        root = put(&mut store, root, &key, &value).unwrap();
        model.insert(key, value);
    }
    store.page_reads.set(0);
    for n in 0..6200u64 {
        let key = n.to_be_bytes();
        let actual = get_pair(&store, root, &key).unwrap();
        let expected = model
            .get_key_value(&key)
            .map(|(k, v)| (k.as_slice(), v.as_slice()));
        assert_eq!(actual, expected);
    }
    let baseline_reads = store.page_reads.get();
    store.page_reads.set(0);
    let reader = Reader::new(&store, root).unwrap();
    assert!(reader.height() > 1);
    for n in 0..6200u64 {
        let key = n.to_be_bytes();
        let expected = model
            .get_key_value(&key)
            .map(|(k, v)| (k.as_slice(), v.as_slice()));
        assert_eq!(reader.get_pair(&key).unwrap(), expected);
    }
    assert_eq!(
        baseline_reads - store.page_reads.get(),
        6199,
        "one root load per query, plus one shared bind"
    );
}

#[test]
fn overflow_keys_and_values_keep_checked_borrowed_extents() {
    let mut store = Store::default();
    let key = vec![0x31; 8192];
    let value = vec![0xa7; 65536];
    let root = put(&mut store, Reference::NULL, &key, &value).unwrap();
    let reader = Reader::new(&store, root).unwrap();
    let (stored_key, stored_value) = reader.get_pair(&key).unwrap().unwrap();
    assert_eq!(stored_key, key);
    assert_eq!(stored_value, value);
    let mut miss = key.clone();
    miss[4096] ^= 1;
    assert!(reader.get(&miss).unwrap().is_none());
    let mut corrupt = store;
    let start = root.offset as usize;
    // Header remains valid, but the leaf's overflow value reference is invalid.
    let slot =
        u16::from_le_bytes(corrupt.bytes[start + 32..start + 34].try_into().unwrap()) as usize;
    corrupt.bytes[start + slot + 24..start + slot + 40].copy_from_slice(
        &Reference {
            segment: 99,
            offset: 0,
        }
        .encode(),
    );
    let reader = Reader::new(&corrupt, root).unwrap();
    assert!(reader.get(&key).is_err());
}

#[test]
fn eager_root_checks_and_descendant_level_checks_reject_corruption() {
    let mut store = Store::default();
    let mut root = Reference::NULL;
    for n in 0..500u64 {
        root = put(&mut store, root, &n.to_be_bytes(), &n.to_be_bytes()).unwrap();
    }
    let page = crate::page::Page::load(&store, root).unwrap();
    assert!(page.level > 0);
    let child = page.first;
    // A valid cached root cannot bypass corruption detected in a descendant.
    store.bytes[child.offset as usize + 4..child.offset as usize + 6]
        .copy_from_slice(&1u16.to_le_bytes());
    let reader = Reader::new(&store, root).unwrap();
    assert!(reader.get(&0u64.to_be_bytes()).is_err());
    store.bytes[root.offset as usize + 2] = 255;
    assert!(Reader::new(&store, root).is_err());
    assert!(
        Reader::new(
            &store,
            Reference {
                segment: 1,
                offset: store.bytes.len() as u64
            }
        )
        .is_err()
    );
}
