use super::*;
use crate::{Cursor, Reader, audit_tree, delete, get, get_pair, put, validate_page};
use std::{collections::BTreeMap, ops::Bound};
use ulmdb_core::storage::{ReadStore, WriteStore};

#[test]
fn checked_layout_preserves_every_admissible_wire_width() {
    let mut bytes = [0u8; PAGE_SIZE];
    bytes[..2].copy_from_slice(&2u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
    bytes[8..10].copy_from_slice(&(HEADER as u16).to_le_bytes());
    bytes[10..12].copy_from_slice(&(HEADER as u16).to_le_bytes());
    for key in 0..=INLINE_KEY as u16 {
        for value in 0..=INLINE_VALUE as u16 {
            bytes[12..14].copy_from_slice(&key.to_le_bytes());
            bytes[14..16].copy_from_slice(&value.to_le_bytes());
            let page = Page::decode(&bytes).unwrap();
            let decoded = page.compact().unwrap();
            assert_eq!(decoded.key, usize::from(key));
            assert_eq!(decoded.value, usize::from(value));
        }
    }
    let legacy = crate::page::encode_v1(0, Reference::NULL, &[]).unwrap();
    assert!(Page::decode(&legacy).unwrap().compact().is_none());
}

#[derive(Default)]
struct Store {
    bytes: Vec<u8>,
    patches: Vec<(usize, usize)>,
}
impl ReadStore for Store {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("test segment"));
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes
            .get(start..start.checked_add(length).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("test extent"))
    }
    fn payload(&self, at: Reference) -> Result<&[u8]> {
        let h = self.read(at, 16)?;
        if h[..8] != [1, 0, 0, 0, 16, 0, 0, 0] {
            return Err(Error::Corrupt("test payload"));
        }
        let n = usize::try_from(u64::from_le_bytes(h[8..].try_into().unwrap()))
            .map_err(|_| Error::LimitExceeded)?;
        self.read(
            Reference {
                offset: at.offset + 16,
                ..at
            },
            n,
        )
    }
}
impl WriteStore for Store {
    fn allocate(&mut self, bytes: &[u8], align: usize) -> Result<Reference> {
        let start = self.bytes.len().div_ceil(align) * align;
        self.bytes.resize(start, 0);
        self.bytes.extend_from_slice(bytes);
        Ok(Reference {
            segment: 1,
            offset: start as u64,
        })
    }
    fn rewrite(&mut self, _: Reference, bytes: &[u8], align: usize) -> Result<Reference> {
        self.allocate(bytes, align)
    }
    fn patch(
        &mut self,
        old: Reference,
        length: usize,
        offset: usize,
        patch: &[u8],
        align: usize,
    ) -> Result<Reference> {
        let mut bytes = self.read(old, length)?.to_vec();
        bytes
            .get_mut(
                offset
                    ..offset
                        .checked_add(patch.len())
                        .ok_or(Error::LimitExceeded)?,
            )
            .ok_or(Error::Corrupt("test patch"))?
            .copy_from_slice(patch);
        self.patches.push((offset, patch.len()));
        self.allocate(&bytes, align)
    }
}
fn legacy(key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut raw = vec![0; (8 + key.len() + value.len()).next_multiple_of(2)];
    raw[..2].copy_from_slice(&(key.len() as u16).to_le_bytes());
    raw[4..8].copy_from_slice(&(value.len() as u32).to_le_bytes());
    raw[8..8 + key.len()].copy_from_slice(key);
    raw[8 + key.len()..8 + key.len() + value.len()].copy_from_slice(value);
    raw
}
fn packed(
    store: &mut Store,
    key_width: usize,
    value_width: usize,
    rows: &[(Vec<u8>, Vec<u8>)],
) -> Reference {
    let cells: Vec<_> = rows.iter().map(|(k, v)| legacy(k, v)).collect();
    let refs: Vec<_> = cells.iter().map(Vec::as_slice).collect();
    let bytes = encode(
        0,
        Reference::NULL,
        &refs,
        Layout {
            key: key_width,
            value: value_width,
        },
    )
    .unwrap();
    store.allocate(&bytes, PAGE_SIZE).unwrap()
}
fn check(store: &Store, root: Reference, expected: &BTreeMap<Vec<u8>, Vec<u8>>) {
    let reader = Reader::new(store, root).unwrap();
    for (k, v) in expected {
        assert_eq!(reader.get(k).unwrap(), Some(v.as_slice()));
        let pair = get_pair(store, root, k).unwrap().unwrap();
        assert_eq!(pair, (k.as_slice(), v.as_slice()));
        assert_eq!(reader.get_pair(k).unwrap(), Some(pair));
    }
    for reverse in [false, true] {
        let got = Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, reverse)
            .unwrap()
            .map(|pair| {
                let (k, v) = pair.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect::<Vec<_>>();
        let mut want: Vec<_> = expected
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if reverse {
            want.reverse();
        }
        assert_eq!(got, want);
    }
    assert_eq!(
        audit_tree(store, root, 10000).unwrap().leaf_records,
        expected.len() as u64
    );
}
#[test]
fn all_width_classes_read_v2_even_when_emission_is_disabled() {
    for (k, v) in [
        (0, 0),
        (0, 512),
        (1, 0),
        (3, 5),
        (8, 8),
        (16, 0),
        (256, 512),
    ] {
        let mut store = Store::default();
        let count = if k == 0 {
            1
        } else {
            Layout { key: k, value: v }.capacity(true).min(15)
        };
        let rows: Vec<_> = (0..count)
            .map(|n| {
                let mut key = vec![0; k];
                if k > 0 {
                    key[k - 1] = n as u8;
                }
                (key, vec![n as u8; v])
            })
            .collect();
        let root = packed(&mut store, k, v, &rows);
        validate_page(&store, root).unwrap();
        check(&store, root, &rows.into_iter().collect());
        assert_eq!(get(&store, root, &vec![255; k + 1]).unwrap(), None);
    }
}
#[test]
fn dense_import_normalization_can_create_many_siblings_and_preserves_old_roots() {
    let mut store = Store::default();
    let rows: Vec<_> = (0..2032u16)
        .map(|n| (n.to_be_bytes().to_vec(), vec![]))
        .collect();
    let root = packed(&mut store, 2, 0, &rows);
    let mut expected: BTreeMap<_, _> = rows.into_iter().collect();
    let changed = put(&mut store, root, &1000u16.to_be_bytes(), b"mixed").unwrap();
    expected.insert(1000u16.to_be_bytes().to_vec(), b"mixed".to_vec());
    check(&store, changed, &expected);
    assert!(audit_tree(&store, changed, 10000).unwrap().pages > 3);
    assert_eq!(
        get(&store, root, &1000u16.to_be_bytes()).unwrap(),
        Some([].as_slice())
    );
    let deleted = delete(&mut store, root, &500u16.to_be_bytes()).unwrap();
    expected.insert(1000u16.to_be_bytes().to_vec(), vec![]);
    expected.remove(500u16.to_be_bytes().as_slice());
    check(&store, deleted, &expected);
    let next = put(&mut store, changed, b"longer-key", &vec![7; 4097]).unwrap();
    expected.insert(500u16.to_be_bytes().to_vec(), vec![]);
    expected.insert(1000u16.to_be_bytes().to_vec(), b"mixed".to_vec());
    expected.insert(b"longer-key".to_vec(), vec![7; 4097]);
    check(&store, next, &expected);
}
#[test]
fn zero_length_endpoint_and_layout_aware_object_events_are_exact() {
    let mut store = Store::default();
    let rows: Vec<_> = (0..254u16)
        .map(|n| {
            let mut k = vec![0; 16];
            k[14..].copy_from_slice(&n.to_be_bytes());
            (k, vec![])
        })
        .collect();
    let root = packed(&mut store, 16, 0, &rows);
    let mut seen = 0;
    crate::object_graph::visit_tree(&store, root, 100, |event| {
        if let crate::object_graph::Event::Leaf {
            index,
            key,
            value,
            value_reference,
            ..
        } = event
        {
            assert_eq!(key, rows[index as usize].0);
            assert!(value.is_empty());
            assert_eq!(
                value_reference.offset,
                root.offset + 32 + (index as u64 + 1) * 16
            );
            assert_eq!(store.read(value_reference, 0)?, value);
            seen += 1;
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(seen, 254);
    assert_eq!(
        Page::load(&store, root)
            .unwrap()
            .cell(&store, 253)
            .unwrap()
            .start
            + 16,
        PAGE_SIZE
    );
}
#[test]
fn packed_header_and_tail_corruption_is_rejected_without_panics() {
    let mut store = Store::default();
    let root = packed(&mut store, 3, 5, &[(b"key".to_vec(), b"value".to_vec())]);
    let original = store.read(root, PAGE_SIZE).unwrap().to_vec();
    for (offset, word) in [
        (0, 3),
        (2, 3),
        (4, 64),
        (6, 65535),
        (8, 34),
        (10, 4097),
        (12, 257),
        (14, 513),
    ] {
        let mut bytes = original.clone();
        bytes[offset..offset + 2].copy_from_slice(&(word as u16).to_le_bytes());
        assert!(Page::decode(&bytes).is_err(), "field {offset}");
    }
    let mut bytes = original.clone();
    bytes[16] = 1;
    assert!(Page::decode(&bytes).is_err());
    let empty = packed(&mut store, 0, 0, &[(vec![], vec![])]);
    let mut bytes = store.read(empty, PAGE_SIZE).unwrap().to_vec();
    bytes[6..8].copy_from_slice(&2u16.to_le_bytes());
    assert!(Page::decode(&bytes).is_err());
    let mut bytes = original;
    bytes[40] = 1;
    let bad = store.allocate(&bytes, PAGE_SIZE).unwrap();
    assert!(validate_page(&store, bad).is_err());
    let duplicate = packed(
        &mut store,
        3,
        5,
        &[
            (b"key".to_vec(), b"value".to_vec()),
            (b"key".to_vec(), b"other".to_vec()),
        ],
    );
    assert!(validate_page(&store, duplicate).is_err());
}

#[test]
fn compact_tail_validation_rejects_every_corrupt_position_and_remainder() {
    // Single rows cover all vector remainders and starting alignments. Nearly
    // full pages cover short/empty tails; 16-byte rows cover normal KV occupancy.
    let mut geometries: Vec<_> = (0..=32).map(|value| (3, value, 1)).collect();
    geometries.extend((2..=34).map(|width| (2, width - 2, (PAGE_SIZE - HEADER) / width)));
    geometries
        .extend([0, 1, 2, 3, 4, 126, 127, 128, 129, 252, 253, 254].map(|count| (8, 8, count)));
    let mut seen_remainders = [false; 32];
    let mut empty_tail_seen = false;
    for (key, value, count) in geometries {
        let cells: Vec<_> = (0..count)
            .map(|i| {
                let mut key_bytes = vec![0; key];
                key_bytes[key - 2..].copy_from_slice(&(i as u16).to_be_bytes());
                legacy(&key_bytes, &vec![0; value])
            })
            .collect();
        let refs: Vec<_> = cells.iter().map(Vec::as_slice).collect();
        let mut bytes = encode(0, Reference::NULL, &refs, Layout { key, value }).unwrap();
        let upper = HEADER + count * (key + value);
        seen_remainders[(PAGE_SIZE - upper) % 32] = true;
        empty_tail_seen |= upper == PAGE_SIZE;
        Page::decode(&bytes).unwrap().validate_compact().unwrap();
        for position in upper..PAGE_SIZE {
            for corruption in [1, 128, 255] {
                bytes[position] = corruption;
                assert!(
                    matches!(
                        Page::decode(&bytes).unwrap().validate_compact(),
                        Err(Error::Corrupt("nonzero compact page tail"))
                    ),
                    "key={key} value={value} count={count} position={position} byte={corruption}"
                );
                bytes[position] = 0;
            }
        }
        Page::decode(&bytes).unwrap().validate_compact().unwrap();
    }
    assert!(seen_remainders.into_iter().all(|seen| seen));
    assert!(empty_tail_seen);
}

#[test]
fn value_patch_and_branch_child_offsets_preserve_cow_and_noop() {
    let mut store = Store::default();
    let mut root = Reference::NULL;
    for n in 0..800u64 {
        root = put(&mut store, root, &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
    }
    let old = root;
    store.patches.clear();
    let next = put(&mut store, root, &400u64.to_be_bytes(), b"replaced").unwrap();
    assert_eq!(
        get(&store, old, &400u64.to_be_bytes()).unwrap(),
        Some(400u64.to_le_bytes().as_slice())
    );
    assert_eq!(
        get(&store, next, &400u64.to_be_bytes()).unwrap(),
        Some(b"replaced".as_slice())
    );
    if cfg!(feature = "compact-pages") {
        assert_eq!(store.patches.len(), 2);
        assert_eq!(store.patches[0].1, 8);
        assert_eq!(store.patches[1].1, 16);
    }
    let size = store.bytes.len();
    assert_eq!(
        put(&mut store, next, &400u64.to_be_bytes(), b"replaced").unwrap(),
        next
    );
    assert_eq!(store.bytes.len(), size);
}

#[test]
fn dense_child_multi_split_propagates_through_an_existing_mixed_parent() {
    let mut store = Store::default();
    let rows: Vec<_> = (0..2032u16)
        .map(|n| (n.to_be_bytes().to_vec(), vec![]))
        .collect();
    let left = packed(&mut store, 2, 0, &rows);
    let right = packed(
        &mut store,
        2,
        0,
        &[(4000u16.to_be_bytes().to_vec(), vec![])],
    );
    let mut branch = vec![0; 22];
    branch[..2].copy_from_slice(&2u16.to_le_bytes());
    branch[4..20].copy_from_slice(&right.encode());
    branch[20..].copy_from_slice(&4000u16.to_be_bytes());
    // V1 ancestor with V2 children is valid independently of emission mode.
    let bytes = crate::page::encode_v1(1, left, &[&branch]).unwrap();
    let root = store.allocate(&bytes, PAGE_SIZE).unwrap();
    let next = put(&mut store, root, &1000u16.to_be_bytes(), b"changed").unwrap();
    let mut expected: BTreeMap<_, _> = rows.into_iter().collect();
    expected.insert(4000u16.to_be_bytes().to_vec(), vec![]);
    expected.insert(1000u16.to_be_bytes().to_vec(), b"changed".to_vec());
    check(&store, next, &expected);
    assert_eq!(
        get(&store, root, &1000u16.to_be_bytes()).unwrap(),
        Some([].as_slice())
    );
}
#[test]
fn default_encoder_is_exact_legacy_wire_and_compact_rows_save_header_and_slot_bytes() {
    let cells = [
        legacy(b"12345678", b"abcdefgh"),
        legacy(b"87654321", b"hgfedcba"),
    ];
    let refs: Vec<_> = cells.iter().map(Vec::as_slice).collect();
    let bytes = crate::page::encode(0, Reference::NULL, &refs).unwrap();
    let v1 = crate::page::encode_v1(0, Reference::NULL, &refs).unwrap();
    if !cfg!(feature = "compact-pages") {
        assert_eq!(bytes, v1);
    } else {
        assert_eq!(&bytes[2..4], &2u16.to_le_bytes());
        assert_eq!(u16::from_le_bytes(bytes[10..12].try_into().unwrap()), 64);
    }
    assert_eq!(HEADER + 254 * 16, PAGE_SIZE);
    const {
        assert!(HEADER + 254 * (2 + 8 + 16) > PAGE_SIZE);
    }
}

// Differential oracle: retain the original slice-ordering validator, including
// per-row child validation and the original first-error order.
fn compact_validation_matches_slice_reference(bytes: &[u8]) -> Option<&'static str> {
    let reference = (|| -> Result<()> {
        let page = Page::decode(bytes)?;
        let layout = page
            .compact()
            .ok_or(Error::Corrupt("expected compact validation"))?;
        let mut previous: Option<&[u8]> = None;
        for index in 0..page.count {
            let cell = layout.cell(bytes, page.level == 0, page.count, index)?;
            if previous.is_some_and(|key| key >= cell.key) {
                return Err(Error::Corrupt("unordered tree keys"));
            }
            previous = Some(cell.key);
        }
        let upper = crate::page::u16_at(bytes, 10)? as usize;
        if bytes[upper..].iter().any(|b| *b != 0) {
            return Err(Error::Corrupt("nonzero compact page tail"));
        }
        Ok(())
    })();
    let actual = Page::decode(bytes).and_then(|page| page.validate_compact());
    let error = |result| match result {
        Ok(()) => None,
        Err(Error::Corrupt(reason)) => Some(reason),
        Err(other) => panic!("unexpected validation error: {other:?}"),
    };
    let expected = error(reference);
    let actual = error(actual);
    assert_eq!(actual, expected);
    actual
}

#[test]
fn compact_word_order_matches_bytes_at_boundaries_and_for_generic_widths() {
    let mut words = vec![
        0u64,
        1,
        0x7fff_ffff_ffff_ffff,
        0x8000_0000_0000_0000,
        u64::MAX - 1,
        u64::MAX,
    ];
    for bits in (8..64).step_by(8) {
        words.extend([(1u64 << bits) - 1, 1u64 << bits]);
    }
    words.sort_unstable();
    words.dedup();
    let cells: Vec<_> = words
        .iter()
        .map(|n| legacy(&n.to_be_bytes(), &[0; 8]))
        .collect();
    let refs: Vec<_> = cells.iter().map(Vec::as_slice).collect();
    let valid = encode(0, Reference::NULL, &refs, Layout { key: 8, value: 8 }).unwrap();
    assert_eq!(compact_validation_matches_slice_reference(&valid), None);
    for index in 1..words.len() {
        for (left, right) in [
            (words[index - 1], words[index - 1]),
            (words[index], words[index - 1]),
        ] {
            let mut bytes = valid;
            let a = HEADER + (index - 1) * 16;
            let b = HEADER + index * 16;
            bytes[a..a + 8].copy_from_slice(&left.to_be_bytes());
            bytes[b..b + 8].copy_from_slice(&right.to_be_bytes());
            bytes[PAGE_SIZE - 1] = 128; // Key-order error must precede tail error.
            assert_eq!(
                compact_validation_matches_slice_reference(&bytes),
                Some("unordered tree keys")
            );
        }
    }
    // Width nine deliberately differs only in its final byte: truncating to an
    // eight-byte word would incorrectly classify the valid keys as duplicates.
    for width in [1, 3, 7, 8, 9, 16, 256] {
        let cells: Vec<_> = (0..3)
            .map(|n| {
                let mut key = vec![0x80; width];
                key[width - 1] = n;
                legacy(&key, &[0; 5])
            })
            .collect();
        let refs: Vec<_> = cells.iter().map(Vec::as_slice).collect();
        let mut bytes = encode(
            0,
            Reference::NULL,
            &refs,
            Layout {
                key: width,
                value: 5,
            },
        )
        .unwrap();
        assert_eq!(compact_validation_matches_slice_reference(&bytes), None);
        bytes[HEADER + 2 * (width + 5) + width - 1] = 0;
        assert_eq!(
            compact_validation_matches_slice_reference(&bytes),
            Some("unordered tree keys")
        );
    }
    for count in [0, 1] {
        let cells: Vec<_> = (0..count).map(|_| legacy(&[], &[])).collect();
        let refs: Vec<_> = cells.iter().map(Vec::as_slice).collect();
        let bytes = encode(0, Reference::NULL, &refs, Layout { key: 0, value: 0 }).unwrap();
        assert_eq!(compact_validation_matches_slice_reference(&bytes), None);
    }
}

#[test]
fn compact_word_branch_validation_preserves_child_and_first_error_order() {
    let cells: Vec<_> = (1..=3u64)
        .map(|key| {
            let mut raw = vec![0; 28];
            raw[..2].copy_from_slice(&8u16.to_le_bytes());
            raw[4..20].copy_from_slice(
                &Reference {
                    segment: 1,
                    offset: (key + 1) * 4096,
                }
                .encode(),
            );
            raw[20..].copy_from_slice(&key.to_be_bytes());
            raw
        })
        .collect();
    let refs: Vec<_> = cells.iter().map(Vec::as_slice).collect();
    let valid = encode(
        1,
        Reference {
            segment: 1,
            offset: 4096,
        },
        &refs,
        Layout { key: 8, value: 0 },
    )
    .unwrap();
    assert_eq!(compact_validation_matches_slice_reference(&valid), None);
    // The current row's invalid child precedes its duplicate key and tail error.
    for (child, expected) in [
        (Reference::NULL, "null branch child"),
        (
            Reference {
                segment: 0,
                offset: 1,
            },
            "malformed null reference",
        ),
    ] {
        let mut bytes = valid;
        let second = HEADER + 24;
        bytes[second..second + 8].copy_from_slice(&1u64.to_be_bytes());
        bytes[second + 8..second + 24].copy_from_slice(&child.encode());
        bytes[PAGE_SIZE - 1] = 255;
        assert_eq!(
            compact_validation_matches_slice_reference(&bytes),
            Some(expected)
        );
    }
    // An earlier ordering error must still precede a later row's invalid child.
    let mut bytes = valid;
    bytes[HEADER + 24..HEADER + 32].copy_from_slice(&1u64.to_be_bytes());
    bytes[HEADER + 2 * 24 + 8..HEADER + 3 * 24].fill(0);
    assert_eq!(
        compact_validation_matches_slice_reference(&bytes),
        Some("unordered tree keys")
    );
}

fn frozen_compact_search(
    layout: Layout,
    page: &Page<'_>,
    needle: &[u8],
    upper: bool,
) -> Result<usize> {
    let (mut lo, mut hi) = (0, page.count);
    let leaf = page.level == 0;
    #[cfg(feature = "word-search")]
    if layout.key == 8
        && let Ok(n) = <&[u8; 8]>::try_from(needle)
    {
        let n = u64::from_be_bytes(*n);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let (row, _) = layout.record(page.bytes, leaf, page.count, mid)?;
            let key = u64::from_be_bytes(row[..8].try_into().unwrap());
            if key < n || upper && key == n {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        return Ok(lo);
    }
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let (row, _) = layout.record(page.bytes, leaf, page.count, mid)?;
        let key = &row[..layout.key];
        if key < needle || upper && key == needle {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    Ok(lo)
}
fn compact_search_fixture(layout: Layout, count: usize, branch: bool) -> [u8; PAGE_SIZE] {
    let cells: Vec<_> = (0..count)
        .map(|index| {
            let word = (2 * index as u64).to_be_bytes();
            let mut key = vec![0x80; layout.key];
            if layout.key <= 8 {
                key.copy_from_slice(&word[8 - layout.key..]);
            } else {
                key[layout.key - 8..].copy_from_slice(&word);
            }
            if branch {
                let mut cell = vec![0; (20 + key.len()).next_multiple_of(2)];
                cell[..2].copy_from_slice(&(key.len() as u16).to_le_bytes());
                cell[4..20].copy_from_slice(
                    &Reference {
                        segment: 1,
                        offset: (index as u64 + 2) * 4096,
                    }
                    .encode(),
                );
                cell[20..20 + key.len()].copy_from_slice(&key);
                cell
            } else {
                legacy(&key, &vec![0xa5; layout.value])
            }
        })
        .collect();
    let refs: Vec<_> = cells.iter().map(Vec::as_slice).collect();
    encode(
        if branch { 1 } else { 0 },
        if branch {
            Reference {
                segment: 1,
                offset: 4096,
            }
        } else {
            Reference::NULL
        },
        &refs,
        layout,
    )
    .unwrap()
}
fn compare_compact_search(bytes: &[u8], needle: &[u8], upper: bool) {
    let page = Page::decode(bytes);
    if let Ok(page) = page {
        if let Some(layout) = page.compact() {
            let actual = layout
                .search(&page, needle, upper)
                .map_err(|e| e.to_string());
            let expected =
                frozen_compact_search(layout, &page, needle, upper).map_err(|e| e.to_string());
            assert_eq!(actual, expected);
        }
    }
}
#[test]
fn fixed_row_search_matches_frozen_loop_for_all_counts_and_word_bounds() {
    for branch in [false, true] {
        let layout = Layout {
            key: 8,
            value: if branch { 0 } else { 8 },
        };
        for count in 0..=layout.capacity(!branch) {
            let bytes = compact_search_fixture(layout, count, branch);
            let page = Page::decode(&bytes).unwrap();
            let words: Vec<_> = (0..count).map(|index| 2 * index as u64).collect();
            for query in (0..=2 * count + 1).map(|n| n as u64).chain([
                u64::MAX,
                1 << 63,
                0xffff_ffff,
                0x1_0000_0000,
            ]) {
                for upper in [false, true] {
                    let needle = query.to_be_bytes();
                    compare_compact_search(&bytes, &needle, upper);
                    assert_eq!(
                        layout.search(&page, &needle, upper).unwrap(),
                        words.partition_point(|key| *key < query || upper && *key == query)
                    );
                }
            }
        }
        for bits in (8..64).step_by(8) {
            let boundary = 1u64 << bits;
            let mut bytes = compact_search_fixture(layout, 3, branch);
            for (index, key) in [boundary - 1, boundary, boundary + 1]
                .into_iter()
                .enumerate()
            {
                let at = HEADER + index * layout.stride(!branch);
                bytes[at..at + 8].copy_from_slice(&key.to_be_bytes());
            }
            for query in [
                0,
                boundary - 2,
                boundary - 1,
                boundary,
                boundary + 1,
                boundary + 2,
                u64::MAX,
            ] {
                for upper in [false, true] {
                    for offset in [0, 1, 7, 15] {
                        let mut padded = vec![0; offset];
                        padded.extend_from_slice(&bytes);
                        compare_compact_search(&padded[offset..], &query.to_be_bytes(), upper);
                    }
                }
            }
        }
    }
}
#[test]
fn fixed_row_search_retains_generic_fallback_and_malformed_geometry_guard() {
    for (key, value, branch) in [
        (0, 0, false),
        (1, 8, false),
        (7, 8, false),
        (9, 8, false),
        (16, 0, false),
        (8, 0, false),
        (8, 8, false),
        (8, 0, true),
        (8, 5, false),
        (8, 512, false),
        (7, 0, true),
        (9, 0, true),
    ] {
        let layout = Layout { key, value };
        for count in [0, 1, 2, layout.capacity(!branch).min(16)] {
            if count > layout.capacity(!branch) {
                continue;
            }
            let bytes = compact_search_fixture(layout, count, branch);
            for needle in [
                vec![],
                vec![0; 7],
                vec![0; 8],
                vec![0; 9],
                vec![0x80; key],
                vec![0xff; key],
            ] {
                for upper in [false, true] {
                    compare_compact_search(&bytes, &needle, upper);
                }
            }
        }
    }
    for branch in [false, true] {
        let layout = Layout {
            key: 8,
            value: if branch { 0 } else { 8 },
        };
        let bytes = compact_search_fixture(layout, 32, branch);
        for offset in 0..PAGE_SIZE {
            for xor in [1, 128, 255] {
                let mut bad = bytes;
                bad[offset] ^= xor;
                // Malformed header geometry never reaches either compact search.
                // Accepted, possibly unordered key bytes preserve the exact old
                // midpoint selection rather than switching search algorithms.
                for query in [0u64, 31, 63, u64::MAX] {
                    for upper in [false, true] {
                        compare_compact_search(&bad, &query.to_be_bytes(), upper);
                    }
                }
            }
        }
        for raw_count in 0..=u16::MAX {
            let mut bad = bytes;
            bad[6..8].copy_from_slice(&raw_count.to_le_bytes());
            assert_eq!(Page::decode(&bad).is_ok(), raw_count == 32);
        }
        for len in [0, 1, 31, 32, PAGE_SIZE - 1] {
            assert!(Page::decode(&bytes[..len]).is_err());
        }
        let mut bad = bytes.to_vec();
        bad.push(0);
        assert!(Page::decode(&bad).is_err());
    }
}

struct CompactSearchReads<'a> {
    store: &'a Store,
    calls: std::cell::RefCell<Vec<(Reference, usize)>>,
    short: Option<Reference>,
}
impl ReadStore for CompactSearchReads<'_> {
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        self.calls.borrow_mut().push((at, len));
        let bytes = self.store.read(at, len)?;
        if self.short == Some(at) && len == PAGE_SIZE {
            Ok(&bytes[..PAGE_SIZE - 1])
        } else {
            Ok(bytes)
        }
    }
}
fn frozen_compact_get<'a, S: ReadStore>(
    store: &'a S,
    root: Reference,
    key: &[u8],
) -> Result<Option<&'a [u8]>> {
    let mut page = Page::load(store, root)?;
    loop {
        let layout = page.compact().expect("compact fixture");
        let index = frozen_compact_search(layout, &page, key, page.level != 0)?;
        if page.level == 0 {
            if index == page.count {
                return Ok(None);
            }
            let cell = page.cell(store, index)?;
            return Ok((cell.key == key).then_some(cell.value));
        }
        let child = page.child(store, index)?;
        let expected = page.level - 1;
        page = Page::load(store, child)?;
        if page.level != expected {
            return Err(Error::Corrupt("tree level mismatch"));
        }
    }
}
#[test]
fn fixed_row_search_preserves_selected_child_errors_and_actual_read_trace() {
    for (bad_child, expected) in [
        (Reference::NULL, "null branch child"),
        (
            Reference {
                segment: 0,
                offset: 1,
            },
            "malformed null reference",
        ),
        (
            Reference {
                segment: 99,
                offset: 4096,
            },
            "test segment",
        ),
    ] {
        let mut store = Store::default();
        let left_bytes = compact_search_fixture(Layout { key: 8, value: 8 }, 1, false);
        let left = store.allocate(&left_bytes, PAGE_SIZE).unwrap();
        let mut branch = compact_search_fixture(Layout { key: 8, value: 0 }, 1, true);
        branch[16..32].copy_from_slice(&left.encode());
        branch[HEADER..HEADER + 8].copy_from_slice(&2u64.to_be_bytes());
        branch[HEADER + 8..HEADER + 24].copy_from_slice(&bad_child.encode());
        let root = store.allocate(&branch, PAGE_SIZE).unwrap();
        for query in [0u64, 2] {
            let actual = CompactSearchReads {
                store: &store,
                calls: Default::default(),
                short: None,
            };
            let reference = CompactSearchReads {
                store: &store,
                calls: Default::default(),
                short: None,
            };
            let a = get(&actual, root, &query.to_be_bytes()).map_err(|e| e.to_string());
            let b = frozen_compact_get(&reference, root, &query.to_be_bytes())
                .map_err(|e| e.to_string());
            assert_eq!(a, b);
            assert_eq!(*actual.calls.borrow(), *reference.calls.borrow());
            if query == 0 {
                assert_eq!(a.unwrap(), Some([0xa5; 8].as_slice()));
            } else {
                assert_eq!(a.unwrap_err(), Error::Corrupt(expected).to_string());
            }
        }
        // An invalid first-child header still fails before an invalid selected
        // record child. Search must not add a complete-branch validation pass.
        let start = root.offset as usize;
        store.bytes[start + 16..start + 32].fill(0);
        assert!(matches!(
            get(&store, root, &2u64.to_be_bytes()),
            Err(Error::Corrupt("invalid tree page header"))
        ));
    }
    let mut store = Store::default();
    let leaf = store
        .allocate(
            &compact_search_fixture(Layout { key: 8, value: 8 }, 1, false),
            PAGE_SIZE,
        )
        .unwrap();
    for short in [Some(leaf), None] {
        let actual = CompactSearchReads {
            store: &store,
            calls: Default::default(),
            short,
        };
        let reference = CompactSearchReads {
            store: &store,
            calls: Default::default(),
            short,
        };
        assert_eq!(
            get(&actual, leaf, &0u64.to_be_bytes()).map_err(|e| e.to_string()),
            frozen_compact_get(&reference, leaf, &0u64.to_be_bytes()).map_err(|e| e.to_string())
        );
        assert_eq!(*actual.calls.borrow(), *reference.calls.borrow());
        if short.is_some() {
            assert!(matches!(
                Reader::new(&actual, leaf),
                Err(Error::Corrupt("short tree page"))
            ));
        }
    }
}
