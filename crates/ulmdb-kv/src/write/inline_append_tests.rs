use super::*;
use crate::{Cursor, audit_tree, get, height};
use std::ops::Bound;
use ulmdb_core::storage::ReadStore;

struct Store {
    bytes: Vec<u8>,
    size: usize,
    private: bool,
    advertised: bool,
    fail_patch: bool,
    multi_patches: Vec<Vec<(usize, usize)>>,
    allocations: usize,
    rewrites: usize,
    dense_admitted: bool,
    admitted_queries: std::cell::Cell<usize>,
    reads: std::cell::Cell<usize>,
}

impl Store {
    fn new(size: usize, private: bool, advertised: bool) -> Self {
        Self {
            bytes: Vec::new(),
            size,
            private,
            advertised,
            fail_patch: false,
            multi_patches: Vec::new(),
            allocations: 0,
            rewrites: 0,
            dense_admitted: false,
            admitted_queries: std::cell::Cell::new(0),
            reads: std::cell::Cell::new(0),
        }
    }
}

impl ReadStore for Store {
    fn page_size(&self) -> usize {
        self.size
    }

    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        self.reads.set(self.reads.get()+1);
        if at.segment != 1 {
            return Err(Error::Corrupt("inline append test segment"));
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes
            .get(start..start.checked_add(length).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("inline append test extent"))
    }
}

impl WriteStore for Store {
    fn admitted_dense_leaf(&self,_:Reference,bytes:&[u8])->bool {
        if self.dense_admitted && bytes.len()==4096 && bytes.get(2..4)==Some(&1u16.to_le_bytes()[..]) {
            self.admitted_queries.set(self.admitted_queries.get()+1);true
        }else{false}
    }
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        let start = self.bytes.len().div_ceil(alignment) * alignment;
        self.bytes.resize(start, 0);
        self.bytes.extend_from_slice(bytes);
        self.allocations += 1;
        Ok(Reference {
            segment: 1,
            offset: start as u64,
        })
    }

    fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.rewrites += 1;
        if self.private {
            let start = old.offset as usize;
            self.bytes[start..start + bytes.len()].copy_from_slice(bytes);
            Ok(old)
        } else {
            self.allocate(bytes, alignment)
        }
    }

    fn supports_patch_many(&self, length: usize, alignment: usize) -> bool {
        self.advertised && length == self.size && alignment == self.size
    }

    fn try_patch_many(
        &mut self,
        old: Reference,
        length: usize,
        patches: &[(usize, &[u8])],
        alignment: usize,
    ) -> Result<Option<Reference>> {
        if !self.supports_patch_many(length, alignment) {
            return Ok(None);
        }
        if self.fail_patch {
            return Err(Error::Io(std::io::Error::other("injected atomic patch")));
        }
        let mut bytes = self.read(old, length)?.to_vec();
        let mut end = 0;
        for (offset, patch) in patches {
            if *offset < end {
                return Err(Error::InvalidInput("overlapping test patches"));
            }
            end = offset
                .checked_add(patch.len())
                .ok_or(Error::LimitExceeded)?;
            bytes
                .get_mut(*offset..end)
                .ok_or(Error::InvalidInput("test patch extent"))?
                .copy_from_slice(patch);
        }
        self.multi_patches.push(
            patches
                .iter()
                .map(|(offset, bytes)| (*offset, bytes.len()))
                .collect(),
        );
        self.rewrite(old, &bytes, alignment).map(Some)
    }
}

#[test]
fn checked_dense_classifier_distinguishes_permutation_fragmentation_and_malformed_cells() {
    use crate::classify_dense_v1_leaf as classify;
    let mut store=Store::new(4096,true,true);
    let root=v1_leaf(&mut store,&[0,2,4],&[7;64]);let original=store.read(root,4096).unwrap().to_vec();
    assert_eq!(classify(&original).unwrap(),Some(true));
    let mut permuted=original.clone();let a=permuted[32..34].to_vec();let b=permuted[34..36].to_vec();
    permuted[32..34].copy_from_slice(&b);permuted[34..36].copy_from_slice(&a);
    assert_eq!(classify(&permuted).unwrap(),Some(true),"physical density is not key-order authority");
    let mut fragment=original.clone();let upper=page::u16_at(&fragment,10).unwrap();
    fragment[10..12].copy_from_slice(&(upper-2).to_le_bytes());assert_eq!(classify(&fragment).unwrap(),Some(false));
    let mut overlap=original.clone();overlap[34..36].copy_from_slice(&original[32..34]);assert!(classify(&overlap).is_err());
    let mut malformed=original.clone();let cell=page::u16_at(&malformed,32).unwrap()as usize;
    malformed[cell+2]=0x80;assert!(classify(&malformed).is_err());
    assert_eq!(classify(&original[..2048]).unwrap(),None);
    let mut unsupported=original.clone();unsupported[2..4].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(classify(&unsupported).unwrap(),None);
    // Valid raw overflow representation is classified WITHOUT a payload store.
    let mut raw=[0u8;26];raw[..2].copy_from_slice(&1u16.to_le_bytes());raw[2..4].copy_from_slice(&2u16.to_le_bytes());
    raw[4..8].copy_from_slice(&70000u32.to_le_bytes());raw[8]=b'a';
    raw[9..25].copy_from_slice(&Reference{segment:99,offset:4096}.encode());
    let payload_leaf=page::encode_v1(0,Reference::NULL,&[&raw]).unwrap();assert_eq!(classify(&payload_leaf).unwrap(),Some(true));
    raw[9..25].fill(0);let invalid=page::encode_v1(0,Reference::NULL,&[&raw]).unwrap();assert!(classify(&invalid).is_err());
}

#[cfg(not(feature="compact-pages"))]
#[test]
fn admitted_middle_and_permuted_append_match_strict_bytes_without_extra_store_reads() {
    for private in [true,false] {
        let mut admitted=Store::new(4096,private,true);admitted.dense_admitted=true;
        let mut strict=Store::new(4096,private,true);let(mut root,mut control)=(Reference::NULL,Reference::NULL);
        // Fixture starts empty and uses ONLY the checked V1 builder, so every
        // live/base/private leaf stays dense by induction, including CoW roots.
        for id in [0u64,4,2,3,1,5,8,6,7,9] {
            let(previous,old_control)=(root,control);let before=(admitted.reads.get(),strict.reads.get());
            root=put(&mut admitted,root,&id.to_be_bytes(),&[id as u8;64]).unwrap();
            control=put(&mut strict,control,&id.to_be_bytes(),&[id as u8;64]).unwrap();
            assert_eq!(admitted.reads.get()-before.0,strict.reads.get()-before.1,"no certification/getter rereads");
            let left=admitted.read(root,4096).unwrap();let right=strict.read(control,4096).unwrap();assert_eq!(left,right);
            assert_eq!(crate::classify_dense_v1_leaf(left).unwrap(),Some(true));
            if !private&&!previous.is_null(){assert_ne!(previous,root);
                assert_eq!(get(&admitted,previous,&id.to_be_bytes()).unwrap(),None);
                assert_eq!(get(&strict,old_control,&id.to_be_bytes()).unwrap(),None);}
        }
        assert!(admitted.admitted_queries.get()>=8);assert_eq!(strict.admitted_queries.get(),0);
        audit_tree(&admitted,root,100).unwrap();audit_tree(&strict,control,100).unwrap();
    }
}

#[cfg(not(feature="compact-pages"))]
#[test]
fn unadmitted_fragmented_and_malformed_sources_keep_strict_fallback() {
    let mut store=Store::new(4096,true,true);let mut root=v1_leaf(&mut store,&[0,1,2,3,4,5,6,7],&[9;64]);
    let upper=page::u16_at(store.read(root,4096).unwrap(),10).unwrap();
    store.patch(root,4096,10,&(upper-2).to_le_bytes(),4096).unwrap();
    assert_eq!(crate::classify_dense_v1_leaf(store.read(root,4096).unwrap()).unwrap(),Some(false));
    let before=store.multi_patches.len();root=put(&mut store,root,&8u64.to_be_bytes(),&[8;64]).unwrap();
    assert_eq!(store.multi_patches.len(),before,"fragmentation requires checked reconstruction");
    audit_tree(&store,root,100).unwrap();assert_eq!(store.admitted_queries.get(),0);
    let first=page::u16_at(store.read(root,4096).unwrap(),32).unwrap()as usize;
    store.patch(root,4096,first+2,&[0x80],4096).unwrap();let bytes=store.read(root,4096).unwrap().to_vec();
    assert!(put(&mut store,root,&9u64.to_be_bytes(),&[9;64]).is_err());
    assert_eq!(store.read(root,4096).unwrap(),bytes);
}

fn v1_leaf(store: &mut Store, ids: &[u64], value: &[u8]) -> Reference {
    let cells: Vec<_> = ids
        .iter()
        .map(|id| leaf_cell(store, &id.to_be_bytes(), value, None).unwrap())
        .collect();
    let cells: Vec<_> = cells.iter().map(CellBuffer::as_bytes).collect();
    store
        .allocate(&page::encode_v1(0, Reference::NULL, &cells).unwrap(), 4096)
        .unwrap()
}

#[cfg(not(feature = "compact-pages"))]
#[test]
fn right_edge_inline_append_matches_encoder_bytes_for_private_and_cow_stores() {
    for private in [false, true] {
        let mut patched = Store::new(4096, private, true);
        let mut fallback = Store::new(4096, private, false);
        let (mut root, mut baseline) = (Reference::NULL, Reference::NULL);
        for n in 0..60u64 {
            let previous = root;
            let value = vec![n as u8; 1 + n as usize % 23];
            root = put(&mut patched, root, &n.to_be_bytes(), &value).unwrap();
            baseline = put(&mut fallback, baseline, &n.to_be_bytes(), &value).unwrap();
            assert_eq!(
                patched.read(root, 4096).unwrap(),
                fallback.read(baseline, 4096).unwrap(),
                "same persisted image after append {n}"
            );
            if n != 0 && !private {
                assert_ne!(root, previous);
                assert!(get(&patched, previous, &n.to_be_bytes()).unwrap().is_none());
                assert!(
                    get(&patched, previous, &(n - 1).to_be_bytes())
                        .unwrap()
                        .is_some()
                );
            }
        }
        assert_eq!(patched.multi_patches.len(), 59);
        assert!(
            patched
                .multi_patches
                .iter()
                .all(|patches| { patches.len() == 3 && patches[0] == (6, 6) && patches[1].1 == 2 })
        );
        if private {
            assert_eq!(patched.allocations, 1);
        }
        audit_tree(&patched, root, 100).unwrap();
    }
}

#[test]
fn splits_and_variable_page_geometry_keep_the_existing_tree_invariants() {
    for size in [256, 4096, 65536] {
        let mut store = Store::new(size, true, true);
        let mut root = Reference::NULL;
        let width = 64.min(size / 8);
        for n in 0..900u64 {
            root = put(&mut store, root, &n.to_be_bytes(), &vec![n as u8; width]).unwrap();
        }
        assert!(height(&store, root).unwrap() > 1);
        assert!(!store.multi_patches.is_empty());
        for reverse in [false, true] {
            let ids = Cursor::new(&store, root, Bound::Unbounded, Bound::Unbounded, reverse)
                .unwrap()
                .map(|pair| {
                    let (key, value) = pair.unwrap();
                    let id = u64::from_be_bytes(key.try_into().unwrap());
                    assert_eq!(value, vec![id as u8; width]);
                    id
                })
                .collect::<Vec<_>>();
            let want: Vec<_> = if reverse {
                (0..900).rev().collect()
            } else {
                (0..900).collect()
            };
            assert_eq!(ids, want);
        }
        assert_eq!(audit_tree(&store, root, 4096).unwrap().leaf_records, 900);
    }
}

#[test]
fn inline_middle_insert_and_overflow_fallback_remain_separate() {
    let mut store = Store::new(4096, true, false);
    let mut root = Reference::NULL;
    for n in [0u64, 4] {
        root = put(&mut store, root, &n.to_be_bytes(), b"value").unwrap();
    }
    store.advertised = true;
    root = put(&mut store, root, &2u64.to_be_bytes(), b"middle").unwrap();
    let expected = usize::from(!cfg!(feature = "compact-pages"));
    assert_eq!(store.multi_patches.len(), expected);
    let large = vec![7; 1024];
    root = put(&mut store, root, &6u64.to_be_bytes(), &large).unwrap();
    assert_eq!(store.multi_patches.len(), expected);
    assert_eq!(
        get(&store, root, &2u64.to_be_bytes()).unwrap(),
        Some(&b"middle"[..])
    );
    assert_eq!(
        get(&store, root, &6u64.to_be_bytes()).unwrap(),
        Some(large.as_slice())
    );
    audit_tree(&store, root, 100).unwrap();
}

#[test]
fn valid_permuted_dense_leaf_accepts_later_insert_without_repacking() {
    let mut store = Store::new(4096, true, false);
    let mut root = v1_leaf(&mut store, &[0, 2], b"value");
    let page = Page::load(&store, root).unwrap();
    let first = page.raw_cell(0).unwrap().to_vec();
    let second = page.raw_cell(1).unwrap().to_vec();
    assert_eq!(first.len(), second.len());
    let first_at = usize::from(page::u16_at(page.bytes, HEADER).unwrap());
    let second_at = usize::from(page::u16_at(page.bytes, HEADER + 2).unwrap());
    let start = root.offset as usize;
    store.bytes[start + first_at..start + first_at + second.len()].copy_from_slice(&second);
    store.bytes[start + second_at..start + second_at + first.len()].copy_from_slice(&first);
    store.bytes[start + HEADER..start + HEADER + 2]
        .copy_from_slice(&(second_at as u16).to_le_bytes());
    store.bytes[start + HEADER + 2..start + HEADER + 4]
        .copy_from_slice(&(first_at as u16).to_le_bytes());
    crate::validate_page(&store, root).unwrap();
    store.advertised = true;
    root = put(&mut store, root, &4u64.to_be_bytes(), b"next").unwrap();
    assert_eq!(
        store.multi_patches.len(),
        usize::from(!cfg!(feature = "compact-pages"))
    );
    for (n, value) in [(0u64, b"value".as_slice()), (2, b"value"), (4, b"next")] {
        assert_eq!(get(&store, root, &n.to_be_bytes()).unwrap(), Some(value));
    }
    audit_tree(&store, root, 100).unwrap();
}

#[test]
fn fragmented_external_leaf_keeps_reconstruction_fallback() {
    let mut store = Store::new(4096, true, false);
    let root = v1_leaf(&mut store, &[0, 4], b"value");
    let page = Page::load(&store, root).unwrap();
    let slot = HEADER + 2;
    let at = usize::from(page::u16_at(page.bytes, slot).unwrap());
    let len = page.raw_cell(1).unwrap().len();
    let start = root.offset as usize;
    store
        .bytes
        .copy_within(start + at..start + at + len, start + at - 2);
    store.bytes[start + slot..start + slot + 2].copy_from_slice(&((at - 2) as u16).to_le_bytes());
    store.bytes[start + 10..start + 12].copy_from_slice(&((at - 2) as u16).to_le_bytes());
    crate::validate_page(&store, root).unwrap();
    store.advertised = true;
    let root = put(&mut store, root, &2u64.to_be_bytes(), b"middle").unwrap();
    assert!(store.multi_patches.is_empty());
    assert_eq!(
        get(&store, root, &2u64.to_be_bytes()).unwrap(),
        Some(b"middle".as_slice())
    );
    audit_tree(&store, root, 100).unwrap();
}

#[cfg(not(feature = "compact-pages"))]
#[test]
fn repeated_shuffled_inserts_match_model_and_keep_cow_roots() {
    use std::collections::BTreeMap;
    for size in [256, 4096, 65536] {
        for private in [false, true] {
            let mut store = Store::new(size, private, true);
            let mut root = Reference::NULL;
            let mut model = BTreeMap::new();
            let mut keys: Vec<_> = (0..420u64).collect();
            let mut rng = 0x9a78_6574_3210_1234u64;
            for at in (1..keys.len()).rev() {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                keys.swap(at, rng as usize % (at + 1));
            }
            let mut held = Vec::new();
            for (at, id) in keys.into_iter().enumerate() {
                let key = id.to_be_bytes();
                let value = vec![id as u8; id as usize % 29];
                root = put(&mut store, root, &key, &value).unwrap();
                model.insert(key.to_vec(), value);
                if at % 53 == 0 {
                    assert_eq!(
                        audit_tree(&store, root, 4096).unwrap().leaf_records as usize,
                        model.len()
                    );
                    for reverse in [false, true] {
                        let got =
                            Cursor::new(&store, root, Bound::Unbounded, Bound::Unbounded, reverse)
                                .unwrap()
                                .map(|row| {
                                    let (k, v) = row.unwrap();
                                    (k.to_vec(), v.to_vec())
                                })
                                .collect::<Vec<_>>();
                        let mut want = model
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect::<Vec<_>>();
                        if reverse {
                            want.reverse();
                        }
                        assert_eq!(got, want);
                    }
                    if !private {
                        held.push((root, model.clone()));
                    }
                }
            }
            assert!(!store.multi_patches.is_empty());
            for (root, old) in held {
                let got = Cursor::new(&store, root, Bound::Unbounded, Bound::Unbounded, false)
                    .unwrap()
                    .map(|row| {
                        let (k, v) = row.unwrap();
                        (k.to_vec(), v.to_vec())
                    })
                    .collect::<BTreeMap<_, _>>();
                assert_eq!(got, old);
            }
        }
    }
}

#[cfg(not(feature = "compact-pages"))]
#[test]
fn middle_insert_rejects_unchanged_cell_overlap_and_patch_failure() {
    let mut store = Store::new(4096, true, false);
    let root = v1_leaf(&mut store, &[0, 4, 8], b"x");
    let start = root.offset as usize;
    let first = store.bytes[start + HEADER..start + HEADER + 2].to_vec();
    store.bytes[start + HEADER + 2..start + HEADER + 4].copy_from_slice(&first);
    store.advertised = true;
    let original = store.bytes.clone();
    assert!(matches!(
        put(&mut store, root, &6u64.to_be_bytes(), b"insert"),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(store.bytes, original);
    let root = v1_leaf(&mut store, &[0, 4, 8], b"x");
    store.fail_patch = true;
    let original = store.bytes.clone();
    assert!(matches!(
        put(&mut store, root, &6u64.to_be_bytes(), b"insert"),
        Err(Error::Io(_))
    ));
    assert_eq!(store.bytes, original);
    assert!(get(&store, root, &6u64.to_be_bytes()).unwrap().is_none());
}

#[cfg(not(feature = "compact-pages"))]
#[test]
fn malformed_unchanged_cell_and_atomic_patch_failure_preserve_source_bytes() {
    let mut store = Store::new(4096, true, false);
    let root = v1_leaf(&mut store, &[0, 2, 4], b"x");
    // Searching for 6 visits slots 1 and 2, so the append's full raw-cell
    // validation must still reject the nonzero padding in untouched slot 0.
    let page = Page::load(&store, root).unwrap();
    let first = page.cell(&store, 0).unwrap();
    let padding = root.offset as usize + first.start + first.raw.len() - 1;
    store.bytes[padding] = 1;
    store.advertised = true;
    let corrupt = store.bytes.clone();
    assert!(matches!(
        put(&mut store, root, &6u64.to_be_bytes(), b"x"),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(store.bytes, corrupt);
    assert!(store.multi_patches.is_empty());
    store.bytes[padding] = 0;
    store.fail_patch = true;
    let original = store.bytes.clone();
    assert!(matches!(
        put(&mut store, root, &6u64.to_be_bytes(), b"x"),
        Err(Error::Io(_))
    ));
    assert_eq!(store.bytes, original);
    assert!(get(&store, root, &6u64.to_be_bytes()).unwrap().is_none());
    audit_tree(&store, root, 100).unwrap();
}
