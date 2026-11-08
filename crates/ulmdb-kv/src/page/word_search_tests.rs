use super::*;
use crate::write::{Separator, branch_cell, leaf_cell};
use std::{cell::Cell, collections::BTreeSet};
use ulmdb_core::storage::WriteStore;

#[derive(Default)]
struct Store {
    objects: Vec<Vec<u8>>,
    reads: Cell<usize>,
}
impl ReadStore for Store {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        self.reads.set(self.reads.get() + 1);
        let index = usize::try_from(at.segment)
            .ok()
            .and_then(|n| n.checked_sub(1))
            .ok_or(Error::Corrupt("test object"))?;
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.objects
            .get(index)
            .and_then(|b| b.get(start..start.checked_add(length)?))
            .ok_or(Error::Corrupt("test object extent"))
    }
}
impl WriteStore for Store {
    fn allocate(&mut self, bytes: &[u8], _: usize) -> Result<Reference> {
        self.objects.push(bytes.to_vec());
        Ok(Reference {
            segment: self.objects.len() as u64,
            offset: 0,
        })
    }
    fn rewrite(&mut self, _: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.allocate(bytes, alignment)
    }
}

fn original_search(page: Page<'_>, store: &Store, needle: &[u8], upper: bool) -> Result<usize> {
    let (mut lo, mut hi) = (0, page.count);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let key = page.key_at(store, mid)?;
        if key < needle || (upper && key == needle) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    Ok(lo)
}

#[test]
fn word_order_matches_byte_order_at_every_first_mismatch_position() {
    let mut checks = 0;
    for index in 0..8 {
        for left in 0..=u8::MAX {
            for right in 0..=u8::MAX {
                let mut a = [0x81; 8];
                let mut b = a;
                a[index] = left;
                b[index] = right;
                a[index + 1..].fill(0xff);
                b[index + 1..].fill(0);
                assert_eq!(cmp_validation_keys(&a, &b), a.cmp(&b));
                for upper in [false, true] {
                    assert_eq!(
                        precedes_word(&a, &b, u64::from_be_bytes(b), upper),
                        a < b || (upper && a == b)
                    );
                    checks += 1;
                }
            }
        }
    }
    assert_eq!(checks, 1_048_576);
}

fn keys() -> Vec<Vec<u8>> {
    let mut keys = BTreeSet::new();
    for length in [0, 1, 7, 8, 9, 16, 255, 256, 257, 1024] {
        for byte in [0, 0x31, 0x80, 0xff] {
            keys.insert(vec![byte; length]);
        }
    }
    keys.into_iter().collect()
}

#[test]
fn validation_comparison_matches_slice_order_for_mixed_lengths() {
    let keys = keys();
    for left in &keys {
        for right in &keys {
            assert_eq!(cmp_validation_keys(left, right), left.cmp(right));
        }
    }
    // Identical first eight bytes are insufficient when only one key is eight
    // bytes long. The ninth byte and the shorter-prefix rule must be preserved.
    for suffix in [0, 0x7f, 0x80, 0xff] {
        let a = [0x80; 8];
        let mut b = a.to_vec();
        b.push(suffix);
        assert!(cmp_validation_keys(&a, &b).is_lt());
        assert!(cmp_validation_keys(&b, &a).is_gt());
    }
}

fn fixture(store: &mut Store, keys: &[Vec<u8>], level: u16) -> [u8; PAGE_SIZE] {
    let child = Reference {
        segment: 500,
        offset: 0,
    };
    let mut cells = Vec::new();
    for key in keys {
        let leaf = leaf_cell(store, key, &[7; 600], None).unwrap();
        let bytes = if level == 0 {
            leaf.as_bytes().to_vec()
        } else {
            let separator = Separator::from_cell(leaf.as_bytes(), 0).unwrap();
            branch_cell(store, &separator, child)
                .unwrap()
                .as_bytes()
                .to_vec()
        };
        cells.push(bytes);
    }
    let refs: Vec<_> = cells.iter().map(Vec::as_slice).collect();
    encode(
        level,
        if level == 0 { Reference::NULL } else { child },
        &refs,
    )
    .unwrap()
}

#[test]
fn mixed_lengths_overflow_keys_and_both_bounds_keep_exact_search_and_io() {
    let keys = keys();
    let mut queries = keys.clone();
    for length in [0, 1, 7, 8, 9, 16, 255, 257, 1024] {
        queries.push(vec![0x7f; length]);
    }
    for level in [0, 1] {
        let mut store = Store::default();
        let bytes = fixture(&mut store, &keys, level);
        let page = Page::decode(&bytes).unwrap();
        for needle in &queries {
            for upper in [false, true] {
                store.reads.set(0);
                let actual = page.search(&store, needle, upper).unwrap();
                let reads = store.reads.get();
                store.reads.set(0);
                let old = original_search(page, &store, needle, upper).unwrap();
                assert_eq!(actual, old);
                assert_eq!(reads, store.reads.get());
                assert_eq!(
                    actual,
                    keys.partition_point(|k| k < needle || (upper && k == needle))
                );
            }
        }
    }
}

#[test]
fn malformed_selected_cells_keep_frozen_search_errors_and_accesses() {
    let keys = keys();
    let mut store = Store::default();
    let original = fixture(&mut store, &keys, 0);
    for offset in HEADER..PAGE_SIZE {
        let mut bytes = original;
        bytes[offset] ^= 0xff;
        let page = Page::decode(&bytes).unwrap();
        for needle in [vec![0x80; 8], vec![0xff; 7], vec![0x31; 257]] {
            for upper in [false, true] {
                store.reads.set(0);
                let actual = page
                    .search(&store, &needle, upper)
                    .map_err(|e| e.to_string());
                let reads = store.reads.get();
                store.reads.set(0);
                let expected =
                    original_search(page, &store, &needle, upper).map_err(|e| e.to_string());
                assert_eq!(actual, expected, "byte={offset}");
                assert_eq!(reads, store.reads.get(), "byte={offset}");
            }
        }
    }
}
