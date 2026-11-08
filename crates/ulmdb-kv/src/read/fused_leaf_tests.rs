use super::*;
use crate::page::{HEADER, PAGE_SIZE};
use std::cell::RefCell;

struct Store {
    image: Vec<u8>,
    prefix: usize,
    budget: usize,
    delta: isize,
    delta_at: Option<Reference>,
    calls: RefCell<Vec<(Reference, usize)>>,
}
impl Store {
    fn new(bytes: &[u8], prefix: usize) -> Self {
        let mut image = vec![0x5a; prefix];
        image.extend_from_slice(bytes);
        Self {
            image,
            prefix,
            budget: usize::MAX,
            delta: 0,
            delta_at: None,
            calls: RefCell::new(Vec::new()),
        }
    }
}
impl ReadStore for Store {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        let mut calls = self.calls.borrow_mut();
        calls.push((at, length));
        if calls.len() > self.budget {
            return Err(Error::LimitExceeded);
        }
        if at.segment != 1 {
            return Err(Error::Corrupt("fused test segment"));
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)? + self.prefix;
        let length = if length == PAGE_SIZE && self.delta_at.is_none_or(|selected| selected == at) {
            length
                .checked_add_signed(self.delta)
                .ok_or(Error::LimitExceeded)?
        } else {
            length
        };
        self.image
            .get(start..start.checked_add(length).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("fused test extent"))
    }
}

// Frozen append6 get_pair traversal. Its Page/search/cell implementations are
// shared with the candidate; only the new fused leaf dispatch differs.
fn original_from_page<'a, S: ReadStore>(
    store: &'a S,
    root: &Page<'a>,
    key: &[u8],
) -> Result<Option<(&'a [u8], &'a [u8])>> {
    let mut loaded;
    let mut page = root;
    loop {
        if page.level == 0 {
            let index = page.search(store, key, false)?;
            return if index == page.count {
                Ok(None)
            } else {
                let cell = page.cell(store, index)?;
                Ok((cell.key == key).then_some((cell.key, cell.value)))
            };
        }
        let child = page.child(store, page.search(store, key, true)?)?;
        let expected = page.level - 1;
        loaded = Page::load(store, child)?;
        if loaded.level != expected {
            return Err(Error::Corrupt("tree level mismatch"));
        }
        page = &loaded;
    }
}
fn original<'a>(
    store: &'a Store,
    root: Reference,
    key: &[u8],
) -> Result<Option<(&'a [u8], &'a [u8])>> {
    if root.is_null() {
        return Ok(None);
    }
    original_from_page(store, &Page::load(store, root)?, key)
}
fn equal<'a>(
    actual: Result<Option<(&'a [u8], &'a [u8])>>,
    expected: Result<Option<(&'a [u8], &'a [u8])>>,
) {
    match (actual, expected) {
        (Ok(actual), Ok(expected)) => {
            assert_eq!(actual, expected);
            if let (Some((ak, av)), Some((ek, ev))) = (actual, expected) {
                assert_eq!(ak.as_ptr(), ek.as_ptr());
                assert_eq!(av.as_ptr(), ev.as_ptr());
            }
        }
        (Err(actual), Err(expected)) => assert_eq!(actual.to_string(), expected.to_string()),
        (actual, expected) => panic!("fused mismatch: {actual:?} / {expected:?}"),
    }
}
fn compare(store: &Store, root: Reference, needle: &[u8]) {
    store.calls.borrow_mut().clear();
    let actual = get_pair(store, root, needle);
    let trace = store.calls.borrow().clone();
    store.calls.borrow_mut().clear();
    let expected = original(store, root, needle);
    equal(actual, expected);
    assert_eq!(trace, *store.calls.borrow());
}
fn key(n: u64, width: usize) -> Vec<u8> {
    let mut key = vec![0x80; width];
    let take = width.min(8);
    key[width - take..].copy_from_slice(&n.to_be_bytes()[8 - take..]);
    key
}
fn leaf(count: usize, key_width: usize, value_width: usize) -> Vec<u8> {
    let mut bytes = vec![0; PAGE_SIZE];
    bytes[..2].copy_from_slice(&2u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&(count as u16).to_le_bytes());
    bytes[8..10].copy_from_slice(&(HEADER as u16).to_le_bytes());
    bytes[10..12]
        .copy_from_slice(&((HEADER + count * (key_width + value_width)) as u16).to_le_bytes());
    bytes[12..14].copy_from_slice(&(key_width as u16).to_le_bytes());
    bytes[14..16].copy_from_slice(&(value_width as u16).to_le_bytes());
    for index in 0..count {
        let at = HEADER + index * (key_width + value_width);
        bytes[at..at + key_width].copy_from_slice(&key(2 * index as u64, key_width));
        bytes[at + key_width..at + key_width + value_width].fill(index as u8);
    }
    bytes
}
const ROOT: Reference = Reference {
    segment: 1,
    offset: 0,
};

#[test]
fn fused_leaf_all_counts_queries_and_exact_borrow_pointers_match_original() {
    for count in 0..=254 {
        let bytes = leaf(count, 8, 8);
        for prefix in [0, 1, 7, 15] {
            let store = Store::new(&bytes, prefix);
            for query in
                (0..=2 * count as u64 + 1).chain([u64::MAX, 1 << 63, 0xffff_ffff, 0x1_0000_0000])
            {
                compare(&store, ROOT, &query.to_be_bytes());
            }
        }
    }
    for bits in (8..64).step_by(8) {
        let boundary = 1u64 << bits;
        let mut bytes = leaf(4, 8, 8);
        for (index, word) in [boundary - 2, boundary - 1, boundary, boundary + 1]
            .into_iter()
            .enumerate()
        {
            bytes[HEADER + 16 * index..HEADER + 16 * index + 8]
                .copy_from_slice(&word.to_be_bytes());
        }
        let store = Store::new(&bytes, 3);
        for query in [
            0,
            boundary - 3,
            boundary - 2,
            boundary - 1,
            boundary,
            boundary + 1,
            boundary + 2,
            u64::MAX,
        ] {
            compare(&store, ROOT, &query.to_be_bytes());
        }
    }
    let store = Store::new(&leaf(4, 8, 8), 0);
    let pair = {
        let needle = 2u64.to_be_bytes();
        get_pair(&store, ROOT, &needle).unwrap().unwrap()
    };
    assert_eq!(pair.0, 2u64.to_be_bytes());
    assert_eq!(pair.0.as_ptr(), store.image[HEADER + 16..].as_ptr());
    assert_eq!(pair.1.as_ptr(), store.image[HEADER + 24..].as_ptr());
}

#[test]
fn fused_leaf_mixed_width_and_v1_fallbacks_preserve_results_and_reads() {
    for (kw, vw) in [
        (0, 0),
        (1, 8),
        (7, 8),
        (8, 0),
        (8, 7),
        (8, 8),
        (8, 9),
        (8, 512),
        (9, 8),
        (256, 512),
    ] {
        for count in [0, 1, 2, 4] {
            if (kw + vw == 0 && count > 1) || count * (kw + vw) > PAGE_SIZE - HEADER {
                continue;
            }
            let store = Store::new(&leaf(count, kw, vw), 1);
            for needle in [
                vec![],
                key(0, kw),
                key(3, kw),
                vec![0; 7],
                vec![0; 8],
                vec![0; 9],
                vec![0xff; kw],
            ] {
                compare(&store, ROOT, &needle);
            }
            // Common eight-byte prefixes with shorter/longer search keys must
            // retain lexicographic fallback rather than word equality.
            for word in [0u64, 2, 8, u64::MAX] {
                let mut longer = word.to_be_bytes().to_vec();
                longer.push(1);
                compare(&store, ROOT, &longer);
                compare(&store, ROOT, &longer[..7]);
            }
        }
    }
    let mut cells = Vec::new();
    for index in 0..3u64 {
        let mut raw = vec![0; 24];
        raw[..2].copy_from_slice(&8u16.to_le_bytes());
        raw[4..8].copy_from_slice(&8u32.to_le_bytes());
        raw[8..16].copy_from_slice(&(2 * index).to_be_bytes());
        raw[16..24].copy_from_slice(&index.to_le_bytes());
        cells.push(raw);
    }
    let refs: Vec<_> = cells.iter().map(Vec::as_slice).collect();
    let bytes = crate::page::encode_v1(0, Reference::NULL, &refs).unwrap();
    let store = Store::new(&bytes, 0);
    for query in 0..8u64 {
        compare(&store, ROOT, &query.to_be_bytes());
    }
}

#[test]
fn fused_leaf_malformed_bytes_short_oversized_and_budget_errors_match_original() {
    let valid = leaf(16, 8, 8);
    for offset in 0..PAGE_SIZE {
        for xor in [1, 128, 255] {
            let mut bytes = valid.clone();
            bytes[offset] ^= xor;
            let store = Store::new(&bytes, 1);
            for query in [0u64, 15, 31, u64::MAX] {
                compare(&store, ROOT, &query.to_be_bytes());
            }
        }
    }
    for raw_count in 0..=u16::MAX {
        let mut bytes = valid.clone();
        bytes[6..8].copy_from_slice(&raw_count.to_le_bytes());
        let store = Store::new(&bytes, 0);
        compare(&store, ROOT, &2u64.to_be_bytes());
    }
    let mut padded = valid;
    padded.push(0);
    for delta in [-4096, -4065, -1, 1] {
        let mut store = Store::new(&padded, 0);
        store.delta = delta;
        compare(&store, ROOT, &2u64.to_be_bytes());
    }
    for budget in [0, 1] {
        let mut store = Store::new(&padded, 0);
        store.budget = budget;
        compare(&store, ROOT, &2u64.to_be_bytes());
        compare(&store, Reference::NULL, &2u64.to_be_bytes());
    }
}

#[test]
fn fused_leaf_branch_selection_child_errors_and_read_budget_order_are_unchanged() {
    let left = Reference {
        segment: 1,
        offset: PAGE_SIZE as u64,
    };
    let right = Reference {
        segment: 1,
        offset: (2 * PAGE_SIZE) as u64,
    };
    for child in [
        right,
        Reference::NULL,
        Reference {
            segment: 0,
            offset: 1,
        },
        Reference {
            segment: 99,
            offset: 4096,
        },
    ] {
        let mut branch = leaf(0, 8, 0);
        branch[..2].copy_from_slice(&3u16.to_le_bytes());
        branch[4..6].copy_from_slice(&1u16.to_le_bytes());
        branch[6..8].copy_from_slice(&1u16.to_le_bytes());
        branch[10..12].copy_from_slice(&56u16.to_le_bytes());
        branch[16..32].copy_from_slice(&left.encode());
        branch[32..40].copy_from_slice(&2u64.to_be_bytes());
        branch[40..56].copy_from_slice(&child.encode());
        let mut bytes = branch;
        bytes.extend_from_slice(&leaf(1, 8, 8));
        let mut last = leaf(1, 8, 8);
        last[HEADER..HEADER + 8].copy_from_slice(&2u64.to_be_bytes());
        bytes.extend_from_slice(&last);
        bytes.push(0);
        for budget in [0, 1, 2, usize::MAX] {
            let mut store = Store::new(&bytes, 3);
            store.budget = budget;
            for query in [0u64, 2, 3] {
                compare(&store, ROOT, &query.to_be_bytes());
            }
        }
        for delta in [-1, 1] {
            let mut store = Store::new(&bytes, 0);
            store.delta = delta;
            store.delta_at = Some(right);
            compare(&store, ROOT, &2u64.to_be_bytes());
        }
        bytes[16..32].fill(0);
        compare(&Store::new(&bytes, 0), ROOT, &2u64.to_be_bytes());
    }
}

#[test]
fn fused_leaf_cached_root_needs_no_store_read_and_keeps_checked_extent_errors() {
    for count in [0, 1, 16, 254] {
        let bytes = leaf(count, 8, 8);
        let store = Store::new(&bytes, 1);
        let reader = crate::Reader::new(&store, ROOT).unwrap();
        store.calls.borrow_mut().clear();
        let page = Page::decode(&store.image[1..]).unwrap();
        for query in [0u64, 1, 2, 507, u64::MAX] {
            let needle = query.to_be_bytes();
            equal(
                reader.get_pair(&needle),
                original_from_page(&store, &page, &needle),
            );
            assert!(store.calls.borrow().is_empty());
        }
    }
    #[cfg(feature = "word-search")]
    {
        let mut bytes = leaf(16, 8, 8);
        bytes.push(0);
        let store = Store::new(&bytes, 0);
        for length in [0, 1, 31, 32, 47, PAGE_SIZE - 1, PAGE_SIZE, PAGE_SIZE + 1] {
            for count in [0, 1, 16, 254, 255, usize::MAX] {
                let mut page = Page::decode(&bytes[..PAGE_SIZE]).unwrap();
                page.bytes = &bytes[..length];
                page.count = count;
                for query in [0u64, 31, u64::MAX] {
                    let needle = query.to_be_bytes();
                    equal(
                        get_pair_from_page(&store, &page, &needle),
                        original_from_page(&store, &page, &needle),
                    );
                }
            }
        }
    }
}
