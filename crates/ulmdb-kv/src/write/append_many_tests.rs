//! Compare the advertised append capability with the unchanged scratch path.
use super::*;
use std::cell::RefCell;
use ulmdb_core::storage::ReadStore;

struct Store {
    bytes: Vec<u8>,
    enabled: bool,
    reads: RefCell<Vec<(Reference, usize)>>,
    multi_patches: usize,
    rewrites: usize,
    prefix: usize,
    response_delta: isize,
}
impl Store {
    fn new(bytes: &[u8], enabled: bool) -> Self {
        Self {
            bytes: bytes.to_vec(),
            enabled,
            reads: RefCell::new(Vec::new()),
            multi_patches: 0,
            rewrites: 0,
            prefix: 0,
            response_delta: 0,
        }
    }
}
impl ReadStore for Store {
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        self.reads.borrow_mut().push((at, len));
        if at.segment != 1 {
            return Err(Error::Corrupt("append test segment"));
        }
        let start = at.offset as usize + self.prefix;
        let len = if len == PAGE_SIZE {
            len.checked_add_signed(self.response_delta)
                .ok_or(Error::LimitExceeded)?
        } else {
            len
        };
        self.bytes
            .get(start..start.checked_add(len).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("append test extent"))
    }
}
impl WriteStore for Store {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        let start = (self.bytes.len() - self.prefix).div_ceil(alignment) * alignment;
        self.bytes.resize(start + self.prefix, 0);
        self.bytes.extend_from_slice(bytes);
        Ok(Reference {
            segment: 1,
            offset: start as u64,
        })
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], _: usize) -> Result<Reference> {
        self.rewrites += 1;
        let start = old.offset as usize + self.prefix;
        self.bytes[start..start + bytes.len()].copy_from_slice(bytes);
        Ok(old)
    }
    fn supports_patch_many(&self, len: usize, alignment: usize) -> bool {
        self.enabled && len == PAGE_SIZE && alignment == PAGE_SIZE
    }
    fn try_patch_many(
        &mut self,
        old: Reference,
        len: usize,
        patches: &[(usize, &[u8])],
        alignment: usize,
    ) -> Result<Option<Reference>> {
        assert!(self.supports_patch_many(len, alignment));
        assert_eq!(patches.len(), 4);
        let mut end = 0;
        for &(offset, bytes) in patches {
            assert!(offset >= end);
            end = offset + bytes.len();
            assert!(end <= len);
        }
        let start = old.offset as usize + self.prefix;
        for &(offset, bytes) in patches {
            self.bytes[start + offset..start + offset + bytes.len()].copy_from_slice(bytes);
        }
        self.multi_patches += 1;
        Ok(Some(old))
    }
}

fn key(n: usize, width: usize) -> Vec<u8> {
    let mut key = vec![0; width];
    let take = width.min(8);
    key[width - take..].copy_from_slice(&(n as u64).to_be_bytes()[8 - take..]);
    key
}
fn leaf(count: usize, key_width: usize, value_width: usize) -> Vec<u8> {
    let mut bytes = vec![0; PAGE_SIZE];
    bytes[..2].copy_from_slice(&2u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&(count as u16).to_le_bytes());
    bytes[8..10].copy_from_slice(&(HEADER as u16).to_le_bytes());
    let stride = key_width + value_width;
    bytes[10..12].copy_from_slice(&((HEADER + count * stride) as u16).to_le_bytes());
    bytes[12..14].copy_from_slice(&(key_width as u16).to_le_bytes());
    bytes[14..16].copy_from_slice(&(value_width as u16).to_le_bytes());
    for n in 0..count {
        let start = HEADER + n * stride;
        bytes[start..start + key_width].copy_from_slice(&key(n, key_width));
        bytes[start + key_width..start + stride].fill(17);
    }
    bytes
}
fn compare(
    bytes: &[u8],
    key: &[u8],
    value: Option<&[u8]>,
    index: usize,
    exists: bool,
    right_edge: bool,
) -> (usize, usize) {
    compare_stores(
        Store::new(bytes, false),
        Store::new(bytes, true),
        key,
        value,
        index,
        exists,
        right_edge,
    )
}
fn compare_stores(
    mut original: Store,
    mut candidate: Store,
    key: &[u8],
    value: Option<&[u8]>,
    index: usize,
    exists: bool,
    right_edge: bool,
) -> (usize, usize) {
    let root = Reference {
        segment: 1,
        offset: 0,
    };
    let expected = change_leaf(&mut original, root, key, value, index, exists, right_edge);
    let actual = change_leaf(&mut candidate, root, key, value, index, exists, right_edge);
    match (expected, actual) {
        (Ok(expected), Ok(actual)) => {
            let expected = finish_change(&mut original, expected, 0).unwrap();
            let actual = finish_change(&mut candidate, actual, 0).unwrap();
            assert_eq!(expected, actual);
            assert_eq!(original.bytes, candidate.bytes);
        }
        (Err(expected), Err(actual)) => assert_eq!(expected.to_string(), actual.to_string()),
        (expected, actual) => panic!(
            "outcomes differ: expected ok={}, actual ok={}",
            expected.is_ok(),
            actual.is_ok()
        ),
    }
    assert_eq!(*original.reads.borrow(), *candidate.reads.borrow());
    (candidate.multi_patches, candidate.rewrites)
}

#[test]
fn every_append_count_matches_scratch_bytes_and_read_trace_including_full_split() {
    for (key_width, value_width) in [(8, 8), (8, 0), (8, 9), (8, 512), (9, 8), (256, 512)] {
        let layout = crate::compact::Layout {
            key: key_width,
            value: value_width,
        };
        for count in 0..=layout.capacity(true) {
            let bytes = leaf(count, key_width, value_width);
            let (patches, _) = compare(
                &bytes,
                &key(count, key_width),
                Some(&vec![42; value_width]),
                count,
                false,
                true,
            );
            assert_eq!(patches, usize::from(count < layout.capacity(true)));
        }
    }
    assert_eq!(compare(&leaf(0, 0, 0), &[], Some(&[]), 0, false, true).0, 1);
}

#[test]
fn nonappend_and_width_change_fallbacks_reuse_the_same_read_image() {
    let bytes = leaf(8, 8, 8);
    for (index, exists, right_edge, key_value, value) in [
        (8, false, false, key(9, 8), Some(vec![3; 8])),
        (8, false, true, key(9, 9), Some(vec![3; 8])),
        (8, false, true, key(9, 8), Some(vec![3; 9])),
        (4, true, true, key(4, 8), Some(vec![3; 8])),
        (4, true, false, key(4, 8), None),
    ] {
        assert_eq!(
            compare(
                &bytes,
                &key_value,
                value.as_deref(),
                index,
                exists,
                right_edge
            )
            .0,
            0
        );
    }
}

#[test]
fn local_corruption_short_store_and_level_errors_match_original() {
    let base = leaf(3, 8, 8);
    let mut corruptions = Vec::new();
    let mut tail = base.clone();
    tail[PAGE_SIZE - 1] = 1;
    corruptions.push(tail);
    let mut unordered = base.clone();
    let first = unordered[HEADER..HEADER + 8].to_vec();
    unordered[HEADER + 16..HEADER + 24].copy_from_slice(&first);
    corruptions.push(unordered);
    let mut both = corruptions[1].clone();
    both[PAGE_SIZE - 1] = 1;
    corruptions.push(both);
    let mut bad_header = base.clone();
    bad_header[10..12].copy_from_slice(&33u16.to_le_bytes());
    corruptions.push(bad_header);
    corruptions.push(base[..PAGE_SIZE - 1].to_vec());
    let mut branch = base.clone();
    branch[..2].copy_from_slice(&3u16.to_le_bytes());
    branch[4..6].copy_from_slice(&1u16.to_le_bytes());
    branch[6..8].copy_from_slice(&0u16.to_le_bytes());
    branch[10..12].copy_from_slice(&(HEADER as u16).to_le_bytes());
    branch[14..16].fill(0);
    branch[16..32].copy_from_slice(
        &Reference {
            segment: 1,
            offset: 4096,
        }
        .encode(),
    );
    branch[HEADER..].fill(0);
    corruptions.push(branch);
    for bytes in corruptions {
        assert_eq!(
            compare(&bytes, &key(9, 8), Some(&[3; 8]), 3, false, true).0,
            0
        );
    }
}

#[test]
fn public_put_splits_replacements_middle_insertions_and_deletes_match_read_trace() {
    let mut original = Store::new(&[], false);
    let mut candidate = Store::new(&[], true);
    let mut left = Reference::NULL;
    let mut right = Reference::NULL;
    for n in 0..600usize {
        original.reads.borrow_mut().clear();
        candidate.reads.borrow_mut().clear();
        left = put(
            &mut original,
            left,
            &key(n * 2, 8),
            &(n as u64).to_le_bytes(),
        )
        .unwrap();
        right = put(
            &mut candidate,
            right,
            &key(n * 2, 8),
            &(n as u64).to_le_bytes(),
        )
        .unwrap();
        assert_eq!(left, right);
        assert_eq!(original.bytes, candidate.bytes);
        assert_eq!(*original.reads.borrow(), *candidate.reads.borrow());
    }
    assert!(candidate.multi_patches > 500);
    for n in [1, 599, 201, 3] {
        original.reads.borrow_mut().clear();
        candidate.reads.borrow_mut().clear();
        left = put(&mut original, left, &key(n, 8), b"middle!!").unwrap();
        right = put(&mut candidate, right, &key(n, 8), b"middle!!").unwrap();
        assert_eq!(left, right);
        assert_eq!(original.bytes, candidate.bytes);
        assert_eq!(*original.reads.borrow(), *candidate.reads.borrow());
    }
    for n in [0, 2, 1198, 599] {
        original.reads.borrow_mut().clear();
        candidate.reads.borrow_mut().clear();
        left = delete(&mut original, left, &key(n, 8)).unwrap();
        right = delete(&mut candidate, right, &key(n, 8)).unwrap();
        assert_eq!(left, right);
        assert_eq!(original.bytes, candidate.bytes);
        assert_eq!(*original.reads.borrow(), *candidate.reads.borrow());
    }
    crate::audit_tree(&original, left, 100_000).unwrap();
    crate::audit_tree(&candidate, right, 100_000).unwrap();
}

#[test]
fn advertised_v1_fallback_and_malformed_cells_preserve_bytes_errors_and_reads() {
    let mut builder = Store::new(&[], false);
    let cells: Vec<_> = (0..3)
        .map(|n| leaf_cell(&mut builder, &key(n, 7), &[17; 8], None).unwrap())
        .collect();
    let slices: Vec<_> = cells.iter().map(CellBuffer::as_bytes).collect();
    let bytes = page::encode_v1(0, Reference::NULL, &slices).unwrap();
    assert_eq!(
        compare(&bytes, &key(9, 7), Some(&[3; 8]), 3, false, true).0,
        0
    );
    for variant in 0..3 {
        let mut bad = bytes;
        match variant {
            0 => bad[HEADER..HEADER + 2].copy_from_slice(&0u16.to_le_bytes()),
            1 => bad[12] = 1,
            _ => {
                let start = page::u16_at(&bad, HEADER).unwrap() as usize;
                bad[start + cells[0].as_bytes().len() - 1] = 1;
            }
        }
        assert_eq!(
            compare(&bad, &key(9, 7), Some(&[3; 8]), 3, false, true).0,
            0
        );
    }
}

#[test]
fn successful_short_oversized_reads_and_unaligned_source_images_match_fallback() {
    let mut bytes = leaf(3, 8, 8);
    bytes.push(0);
    for delta in [-1, 1] {
        let mut original = Store::new(&bytes, false);
        let mut candidate = Store::new(&bytes, true);
        original.response_delta = delta;
        candidate.response_delta = delta;
        assert_eq!(
            compare_stores(
                original,
                candidate,
                &key(9, 8),
                Some(&[3; 8]),
                3,
                false,
                true
            )
            .0,
            0
        );
    }
    bytes.pop();
    for prefix in 1..8 {
        let mut padded = vec![0x5a; prefix];
        padded.extend_from_slice(&bytes);
        let mut original = Store::new(&padded, false);
        let mut candidate = Store::new(&padded, true);
        original.prefix = prefix;
        candidate.prefix = prefix;
        assert_eq!(
            compare_stores(
                original,
                candidate,
                &key(9, 8),
                Some(&[3; 8]),
                3,
                false,
                true
            )
            .0,
            1
        );
    }
}
