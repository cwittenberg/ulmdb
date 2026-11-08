//! Frozen current-main by-value reader/lookup oracle. Copied scalar fixture
//! scaffolding now compares the selected OFF/ON API to that original, and
//! repeats ordinal failures as LimitExceeded to cover checked read budgets.
#[path = "lookup.before.rs"]
mod original_lookup;
#[path = "reader.before.rs"]
mod original_reader;
use crate::node::Node;
use std::cell::RefCell;
use ulmdb_core::{
    Address, Prefix,
    storage::{Error, ReadStore, Reference, Result, WriteStore},
};
#[derive(Clone)]
struct Store {
    data: Vec<u8>,
}
impl Default for Store {
    fn default() -> Self {
        Self { data: vec![0; 16] }
    }
}
impl ReadStore for Store {
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("oracle segment"));
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.data
            .get(start..start.checked_add(len).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("oracle bounds"))
    }
}
impl WriteStore for Store {
    fn allocate(&mut self, b: &[u8], align: usize) -> Result<Reference> {
        let start = self.data.len().div_ceil(align) * align;
        self.data.resize(start, 0);
        self.data.extend_from_slice(b);
        Ok(Reference {
            segment: 1,
            offset: start as u64,
        })
    }
    fn rewrite(&mut self, _: Reference, b: &[u8], align: usize) -> Result<Reference> {
        self.allocate(b, align)
    }
}
struct Record<'a> {
    store: &'a Store,
    reads: RefCell<Vec<(Reference, usize)>>,
    fault: Option<usize>,
    node_length: Option<usize>,
    as_limit: bool,
}
impl<'a> Record<'a> {
    fn new(store: &'a Store) -> Self {
        Self {
            store,
            reads: Default::default(),
            fault: None,
            node_length: None,
            as_limit: false,
        }
    }
}
impl ReadStore for Record<'_> {
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        let mut reads = self.reads.borrow_mut();
        reads.push((at, len));
        let nth = reads.len();
        drop(reads);
        if self.fault == Some(nth) {
            if self.as_limit {
                return Err(Error::LimitExceeded);
            }
            return Err(if nth.is_multiple_of(2) {
                Error::Io(std::io::Error::from_raw_os_error(5))
            } else {
                Error::Corrupt("injected ordinal read")
            });
        }
        self.store.read(
            at,
            if len == 96 {
                self.node_length.unwrap_or(len)
            } else {
                len
            },
        )
    }
}
#[derive(Clone, Copy)]
enum Query {
    Lpm(Address),
    Exact(Prefix),
    Cover(Prefix),
}
type Outcome = std::result::Result<Option<(Prefix, Vec<u8>, usize)>, String>;
fn outcome(r: Result<Option<(Prefix, &[u8])>>) -> Outcome {
    r.map(|x| x.map(|(p, b)| (p, b.to_vec(), b.as_ptr() as usize)))
        .map_err(|e| format!("{e:?}"))
}
fn compare(
    store: &Store,
    root: Reference,
    query: Query,
    fault: Option<usize>,
    node_length: Option<usize>,
) -> usize {
    let normal = compare_mode(store, root, query, fault, node_length, false);
    let budget = compare_mode(store, root, query, fault, node_length, true);
    assert_eq!(normal, budget);
    normal
}
fn compare_mode(
    store: &Store,
    root: Reference,
    query: Query,
    fault: Option<usize>,
    node_length: Option<usize>,
    as_limit: bool,
) -> usize {
    let mut old = Record::new(store);
    let mut new = Record::new(store);
    old.fault = fault;
    new.fault = fault;
    old.node_length = node_length;
    new.node_length = node_length;
    old.as_limit = as_limit;
    new.as_limit = as_limit;
    compare_free(store, root, query, fault, node_length, as_limit);
    let a = original_reader::Reader::new(&old, root);
    let b = crate::Reader::new(&new, root);
    let (a, b) = match (a, b) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a.root_prefix(), b.root_prefix());
            (a, b)
        }
        (Err(a), Err(b)) => {
            assert_eq!(a.to_string(), b.to_string());
            assert_eq!(*old.reads.borrow(), *new.reads.borrow());
            return old.reads.borrow().len();
        }
        _ => panic!("prepared constructor disagreement"),
    };
    let (a, b) = match query {
        Query::Lpm(q) => (a.longest_match(q), b.longest_match(q)),
        Query::Exact(q) => (a.exact(q), b.exact(q)),
        Query::Cover(q) => (a.covering(q), b.covering(q)),
    };
    assert_eq!(outcome(a), outcome(b));
    assert_eq!(*old.reads.borrow(), *new.reads.borrow());
    old.reads.borrow().len()
}
fn compare_free(
    store: &Store,
    root: Reference,
    query: Query,
    fault: Option<usize>,
    node_length: Option<usize>,
    as_limit: bool,
) {
    let mut old = Record::new(store);
    let mut new = Record::new(store);
    old.as_limit = as_limit;
    new.as_limit = as_limit;
    old.fault = fault;
    new.fault = fault;
    old.node_length = node_length;
    new.node_length = node_length;
    let (a, b) = match query {
        Query::Lpm(q) => (
            original_lookup::get(&old, root, q),
            crate::get(&new, root, q),
        ),
        Query::Exact(q) => (
            original_lookup::exact(&old, root, q),
            crate::exact(&new, root, q),
        ),
        Query::Cover(q) => (
            original_lookup::covering(&old, root, q),
            crate::covering(&new, root, q),
        ),
    };
    assert_eq!(outcome(a), outcome(b));
    assert_eq!(*old.reads.borrow(), *new.reads.borrow());
}

fn p(a: Address, len: u8) -> Prefix {
    Prefix::normalize(a, len).unwrap()
}
fn fixture(v6: bool) -> (Store, Reference) {
    let mut store = Store::default();
    let mut root = Reference::NULL;
    let lens: &[u8] = if v6 {
        &[0, 1, 3, 32, 63, 64, 65, 96, 127, 128]
    } else {
        &[0, 1, 3, 8, 15, 16, 31, 32]
    };
    for (i, &len) in lens.iter().enumerate() {
        let a = if v6 {
            Address::V6(0x2001_0db8_abcd_1234_9876_5432_10ff_ffff)
        } else {
            Address::V4(0x0a80_12ff)
        };
        let prefix = p(a, len);
        let value = [i as u8; 7];
        root = crate::insert(&mut store, root, prefix, if i == 0 { b"" } else { &value }).unwrap();
    }
    for i in 0..48u8 {
        let a = if v6 {
            Address::V6((u128::from(i) << 120) | 0x1234_abcd)
        } else {
            Address::V4((u32::from(i) << 24) | 0x1234)
        };
        root = crate::insert(&mut store, root, p(a, if v6 { 64 } else { 24 }), &[i; 9]).unwrap();
    }
    store.data.extend_from_slice(&[0; 128]);
    (store, root)
}
#[test]
fn frozen_reader_random_boundaries_both_families_all_apis_and_fault_requests() {
    for v6 in [false, true] {
        let (s, r) = fixture(v6);
        let mut random = 0x8b21_c945_7241_a5f1u64;
        for i in 0..256 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let address = if v6 {
                Address::V6((u128::from(random) << 64) | u128::from(random.rotate_left(17)))
            } else {
                Address::V4(random as u32)
            };
            let len = (i % if v6 { 129 } else { 33 }) as u8;
            let cidr = p(address, len);
            for query in [Query::Lpm(address), Query::Exact(cidr), Query::Cover(cidr)] {
                let n = compare(&s, r, query, None, None);
                if i < 8 {
                    for ordinal in 1..=n + 1 {
                        compare(&s, r, query, Some(ordinal), None);
                    }
                }
            }
        }
        for len in 0..=if v6 { 128 } else { 32 } {
            let a = if v6 {
                Address::V6(u128::MAX)
            } else {
                Address::V4(u32::MAX)
            };
            for q in [
                Query::Lpm(a),
                Query::Exact(p(a, len)),
                Query::Cover(p(a, len)),
            ] {
                compare(&s, r, q, None, None);
            }
        }
        let other = if v6 { Address::V4(0) } else { Address::V6(0) };
        for q in [
            Query::Lpm(other),
            Query::Exact(p(other, 0)),
            Query::Cover(p(other, 0)),
        ] {
            assert_eq!(compare(&s, r, q, None, None), 1);
        }
        for q in [
            Query::Lpm(other),
            Query::Exact(p(other, 0)),
            Query::Cover(p(other, 0)),
        ] {
            assert_eq!(compare(&s, Reference::NULL, q, None, None), 0);
        }
    }
}
#[test]
fn all_node_bytes_malformed_roots_descendants_and_nonconforming_extents_match() {
    for v6 in [false, true] {
        let (s, r) = fixture(v6);
        let node = Node::read(&s, r).unwrap();
        let child = *node.children.iter().find(|at| !at.is_null()).unwrap();
        let addresses = if v6 {
            [
                Address::V6(0),
                Address::V6(u128::MAX),
                Address::V6(0x2001_0db8 << 96),
            ]
        } else {
            [
                Address::V4(0),
                Address::V4(u32::MAX),
                Address::V4(0x0a80_12ff),
            ]
        };
        for target in [r, child] {
            for byte in 0..96 {
                for mask in [1u8, 0x80, 0xff] {
                    let mut bad = s.clone();
                    bad.data[target.offset as usize + byte] ^= mask;
                    for a in addresses {
                        for query in [
                            Query::Lpm(a),
                            Query::Exact(p(a, if v6 { 128 } else { 32 })),
                            Query::Cover(p(a, 0)),
                        ] {
                            compare(&bad, r, query, None, None);
                        }
                    }
                }
            }
        }
        for len in [0, 1, 8, 95, 97, 112] {
            compare(&s, r, Query::Lpm(addresses[0]), None, Some(len));
        }
        // Full validation of wrong-family nodes precedes None/relationship errors.
        let mut bad = s.clone();
        let mut other = Node::read(&bad, child).unwrap();
        other.prefix = if v6 {
            p(Address::V4(0), 32)
        } else {
            p(Address::V6(0), 128)
        };
        other.children[1] = r;
        bad.data[child.offset as usize..child.offset as usize + 96]
            .copy_from_slice(&other.encode());
        for a in addresses {
            compare(&bad, r, Query::Lpm(a), None, None);
        }
        // Ref decoding precedes all alignment checks, including multiple faults.
        let mut bad = s.clone();
        let start = r.offset as usize;
        bad.data[start + 32..start + 48].copy_from_slice(
            &Reference {
                segment: 1,
                offset: 17,
            }
            .encode(),
        );
        bad.data[start + 48..start + 64].copy_from_slice(&[0; 16]);
        bad.data[start + 56] = 1;
        compare(&bad, r, Query::Lpm(addresses[0]), None, None);
    }
}
#[test]
fn mapped_prepared_snapshots_keep_actual_old_payload_borrows() {
    use ulmdb_storage::resident::{Config, Pool};
    let pool = Pool::create(Config {
        kv_pages: 8,
        tiny_pages: 128,
        max_reserved_bytes: 8 << 20,
        max_metadata_bytes: 32 << 20,
        max_views: 64,
    })
    .unwrap();
    let mut view = pool.empty().unwrap();
    let mut roots = [Reference::NULL; 2];
    let ps = [
        p(Address::V4(0x0a12_3456), 8),
        p(Address::V6(0x2001_0db8 << 96), 32),
    ];
    let mut a = pool.begin(view.clone()).unwrap();
    for i in 0..2 {
        roots[i] = crate::insert(&mut a, roots[i], ps[i], b"original").unwrap();
    }
    view = a.freeze().unwrap().activate();
    let original = view.clone();
    let old = crate::Reader::new(&original, roots[0]).unwrap();
    let bytes = old.longest_match(ps[0].network()).unwrap().unwrap().1;
    let pointer = bytes.as_ptr();
    for round in 0..16u8 {
        let mut a = pool.begin(view.clone()).unwrap();
        for i in 0..2 {
            roots[i] = crate::insert(&mut a, roots[i], ps[i], &[round; 8]).unwrap();
        }
        view = a.freeze().unwrap().activate();
        for i in 0..2 {
            let new = crate::Reader::new(&view, roots[i]).unwrap();
            let old = original_reader::Reader::new(&view, roots[i]).unwrap();
            assert_eq!(
                outcome(new.longest_match(ps[i].network())),
                outcome(old.longest_match(ps[i].network()))
            );
        }
        assert_eq!(bytes, b"original");
        assert_eq!(bytes.as_ptr(), pointer);
    }
    assert_eq!(bytes, b"original");
    assert_eq!(pool.stats().unwrap().maps_created, 136);
}

#[test]
fn unselected_refs_actual_wrong_family_and_first_error_order_are_eager() {
    for v6 in [false, true] {
        let mut s = Store::default();
        let actual = if v6 { Address::V6(0) } else { Address::V4(0) };
        let opposite = if v6 { Address::V4(0) } else { Address::V6(0) };
        let node = Node::leaf(p(actual, if v6 { 64 } else { 8 }), Reference::NULL);
        let r = s.allocate(&node.encode(), 16).unwrap();
        s.data.extend_from_slice(&[0; 128]);
        for field in [32, 48, 64] {
            let mut bad = s.clone();
            let at = r.offset as usize + field;
            bad.data[at..at + 16].copy_from_slice(
                &Reference {
                    segment: 1,
                    offset: 17,
                }
                .encode(),
            );
            assert!(matches!(
                original_reader::Reader::new(&bad, r),
                Err(Error::Corrupt("unaligned radix reference"))
            ));
            // No child/value can be selected by an opposite-family query.
            assert_eq!(compare(&bad, r, Query::Lpm(opposite), None, None), 1);
        }
        let mut full = node;
        full.prefix = p(opposite, if v6 { 32 } else { 128 });
        full.children[1] = r;
        let mut bad = s.clone();
        bad.data[r.offset as usize..r.offset as usize + 96].copy_from_slice(&full.encode());
        assert!(matches!(
            original_reader::Reader::new(&bad, r),
            Err(Error::Corrupt("full-length radix node has children"))
        ));
        compare(&bad, r, Query::Lpm(actual), None, None);
        let mut bad = s.clone();
        let start = r.offset as usize;
        bad.data[start + 32..start + 48].copy_from_slice(
            &Reference {
                segment: 1,
                offset: 17,
            }
            .encode(),
        );
        bad.data[start + 64..start + 80].copy_from_slice(&[0; 16]);
        bad.data[start + 72] = 1;
        assert!(matches!(
            original_reader::Reader::new(&bad, r),
            Err(Error::Corrupt("malformed null reference"))
        ));
        compare(&bad, r, Query::Lpm(opposite), None, None);
        for len in [0, 8, 95, 97, 112] {
            let mut rec = Record::new(&s);
            rec.node_length = Some(len);
            assert!(matches!(
                crate::Reader::new(&rec, r),
                Err(Error::Corrupt("short radix node"))
            ));
            compare(&s, r, Query::Lpm(opposite), None, Some(len));
        }
        for root in [
            Reference {
                segment: 0,
                offset: 16,
            },
            Reference {
                segment: 1,
                offset: 17,
            },
            Reference {
                segment: 99,
                offset: 16,
            },
        ] {
            compare(&s, root, Query::Lpm(opposite), None, None);
        }
    }
}
