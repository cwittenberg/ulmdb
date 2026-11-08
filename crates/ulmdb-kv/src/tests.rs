use crate::{Cursor, PAGE_SIZE, delete, get, height, put, validate_page};
use std::{cell::Cell, collections::BTreeMap, ops::Bound};
use ulmdb_core::storage::{Error, ReadStore, Reference, Result, WriteStore};

#[test]
fn ordered_integer_cow_split_range_delete_and_full_audit() {
    use crate::ordering::{IntegerEndian, IntegerWidth, UnsignedInteger};
    use crate::{audit_tree_structure, audit_tree_with, delete_with, get_with, put_with};
    let order = UnsignedInteger {
        width: IntegerWidth::U32,
        endian: IntegerEndian::Little,
    };
    let mut store = Arena::default();
    let mut root = Reference::NULL;
    for n in (0..512u32).rev() {
        root = put_with(&mut store, root, &n.to_le_bytes(), &n.to_be_bytes(), &order).unwrap();
    }
    for n in 0..512u32 {
        assert_eq!(
            get_with(&store, root, &n.to_le_bytes(), &order).unwrap(),
            Some(n.to_be_bytes().as_slice())
        );
    }
    let records = Cursor::with_order(
        &store,
        root,
        Bound::Included(100u32.to_le_bytes().as_slice()),
        Bound::Included(104u32.to_le_bytes().as_slice()),
        false,
        order,
    )
    .unwrap()
    .map(|p| u32::from_le_bytes(p.unwrap().0.try_into().unwrap()))
    .collect::<Vec<_>>();
    assert_eq!(records, vec![100, 101, 102, 103, 104]);
    assert_eq!(
        audit_tree_with(&store, root, 4096, &order)
            .unwrap()
            .leaf_records,
        512
    );
    assert!(crate::audit_tree(&store, root, 4096).is_err());
    assert_eq!(
        audit_tree_structure(&store, root, 4096)
            .unwrap()
            .leaf_records,
        512
    );
    let old = root;
    root = put_with(&mut store, root, &256u32.to_le_bytes(), b"changed", &order).unwrap();
    assert_eq!(
        get_with(&store, old, &256u32.to_le_bytes(), &order).unwrap(),
        Some(256u32.to_be_bytes().as_slice())
    );
    root = delete_with(&mut store, root, &256u32.to_le_bytes(), &order).unwrap();
    assert_eq!(
        get_with(&store, root, &256u32.to_le_bytes(), &order).unwrap(),
        None
    );
    assert_eq!(
        audit_tree_with(&store, root, 4096, &order)
            .unwrap()
            .leaf_records,
        511
    );
}

#[test]
fn ordered_reverse_bytes_are_not_descending_lexical_order() {
    use crate::ordering::ReverseBytes;
    let mut store = Arena::default();
    let mut root = Reference::NULL;
    for key in [b"az".as_slice(), b"ba", b"a", b"b", b"ab", b"aa"] {
        root = crate::put_with(&mut store, root, key, key, &ReverseBytes).unwrap();
    }
    let keys = Cursor::with_order(
        &store,
        root,
        Bound::Unbounded,
        Bound::Unbounded,
        false,
        ReverseBytes,
    )
    .unwrap()
    .map(|p| p.unwrap().0.to_vec())
    .collect::<Vec<_>>();
    assert_eq!(
        keys,
        vec![
            b"a".to_vec(),
            b"aa".to_vec(),
            b"ba".to_vec(),
            b"b".to_vec(),
            b"ab".to_vec(),
            b"az".to_vec()
        ]
    );
    crate::audit_tree_with(&store, root, 64, &ReverseBytes).unwrap();
}

#[test]
fn ordered_custom_equality_preserves_stored_representative_and_overflow_borrows() {
    let fold = |a: &[u8], b: &[u8]| {
        a.iter()
            .map(u8::to_ascii_lowercase)
            .cmp(b.iter().map(u8::to_ascii_lowercase))
    };
    let mut store = Arena::default();
    let mut root = Reference::NULL;
    root = crate::put_with(&mut store, root, b"Alpha", b"old", &fold).unwrap();
    let old = root;
    let large = vec![7; 8192];
    root = crate::put_with(&mut store, root, b"ALPHA", &large, &fold).unwrap();
    let pair = crate::get_pair_with(&store, root, b"aLpHa", &fold)
        .unwrap()
        .unwrap();
    assert_eq!(pair.0, b"Alpha");
    assert_eq!(pair.1, large.as_slice());
    assert_eq!(
        crate::get_with(&store, old, b"alpha", &fold).unwrap(),
        Some(b"old".as_slice())
    );
    let key = vec![b'A'; 600];
    let lower = vec![b'a'; 600];
    root = crate::put_with(&mut store, root, &key, b"first", &fold).unwrap();
    root = crate::put_with(&mut store, root, &lower, b"longer replacement", &fold).unwrap();
    let pair = crate::get_pair_with(&store, root, &lower, &fold)
        .unwrap()
        .unwrap();
    assert_eq!(pair.0, key.as_slice());
    assert_eq!(pair.1, b"longer replacement");
    crate::audit_tree_with(&store, root, 64, &fold).unwrap();
    root = crate::delete_with(&mut store, root, b"ALphA", &fold).unwrap();
    assert_eq!(
        crate::get_with(&store, root, b"alpha", &fold).unwrap(),
        None
    );
}

#[test]
fn ordered_bulk_poisoning_and_integer_width_errors_are_safe() {
    use crate::ordering::{IntegerEndian, IntegerWidth, UnsignedInteger};
    let order = UnsignedInteger {
        width: IntegerWidth::U64,
        endian: IntegerEndian::Little,
    };
    let mut store = Arena::default();
    let built = crate::build_sorted_with(
        &mut store,
        (0..300u64).map(|n| Ok((n.to_le_bytes(), n.to_be_bytes()))),
        order,
    )
    .unwrap();
    assert_eq!(
        crate::audit_tree_with(&store, built.root, 1000, &order)
            .unwrap()
            .leaf_records,
        300
    );
    assert!(matches!(
        crate::get_with(&store, built.root, b"bad", &order),
        Err(Error::InvalidInput(_))
    ));
    let malformed = put(&mut store, Reference::NULL, b"x", b"value").unwrap();
    assert!(matches!(
        crate::get_with(&store, malformed, &1u64.to_le_bytes(), &order),
        Err(Error::Corrupt(_))
    ));
    let fold = |a: &[u8], b: &[u8]| {
        a.iter()
            .map(u8::to_ascii_lowercase)
            .cmp(b.iter().map(u8::to_ascii_lowercase))
    };
    let mut builder = crate::SortedBuilder::with_order(fold);
    builder.push(&mut store, b"Alpha", b"one").unwrap();
    assert!(builder.push(&mut store, b"ALPHA", b"two").is_err());
    assert!(builder.finish(&mut store).is_err());
    let at = built.root.offset as usize;
    store.bytes[at + 4..at + 6].copy_from_slice(&63u16.to_le_bytes());
    assert!(crate::audit_tree_structure(&store, built.root, 1000).is_err());
}

#[derive(Default)]
struct Arena {
    bytes: Vec<u8>,
    reuse: bool,
    payload_reads: Cell<usize>,
    rewrites: usize,
    allocations: usize,
    payload_replacements: usize,
    payload_reuses: usize,
}

impl ReadStore for Arena {
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
        self.payload_reads.set(self.payload_reads.get() + 1);
        let header = self.read(at, 16)?;
        if header[..8] != [1, 0, 0, 0, 16, 0, 0, 0] {
            return Err(Error::Corrupt("test payload header"));
        }
        let length = u64::from_le_bytes(header[8..16].try_into().unwrap()) as usize;
        self.read(
            Reference {
                segment: at.segment,
                offset: at.offset + 16,
            },
            length,
        )
    }
}
impl WriteStore for Arena {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.allocations += 1;
        let at = self.bytes.len().div_ceil(alignment) * alignment;
        self.bytes.resize(at, 0);
        self.bytes.extend_from_slice(bytes);
        Ok(Reference {
            segment: 1,
            offset: at as u64,
        })
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.rewrites += 1;
        if self.reuse {
            let start = old.offset as usize;
            self.bytes[start..start + bytes.len()].copy_from_slice(bytes);
            Ok(old)
        } else {
            self.allocate(bytes, alignment)
        }
    }
    fn replace_payload(&mut self, old: Reference, value: &[u8]) -> Result<Reference> {
        self.payload_replacements += 1;
        if self.reuse && self.payload(old)?.len() == value.len() {
            let start = old.offset as usize + 16;
            self.bytes[start..start + value.len()].copy_from_slice(value);
            self.payload_reuses += 1;
            Ok(old)
        } else {
            self.put_payload(value)
        }
    }
}

#[test]
fn repeated_overflow_replacement_reuses_private_payloads_and_preserves_base() {
    for reuse in [false, true] {
        let mut arena = Arena {
            reuse,
            ..Arena::default()
        };
        let key = vec![0x44; 8192];
        let mut value = vec![0x55; 64 * 1024];
        let mut root = put(&mut arena, Reference::NULL, &key, &value).unwrap();
        let snapshot = root;
        let original_size = arena.bytes.len();
        let original_allocations = arena.allocations;
        let old_key_reference = Reference::decode(
            &crate::page::Page::load(&arena, root)
                .unwrap()
                .raw_cell(0)
                .unwrap()[8..24],
        )
        .unwrap();
        let repetitions = if reuse { 1000 } else { 4 };
        for n in 0..repetitions {
            value.fill(n as u8);
            root = put(&mut arena, root, &key, &value).unwrap();
        }
        let key_reference = Reference::decode(
            &crate::page::Page::load(&arena, root)
                .unwrap()
                .raw_cell(0)
                .unwrap()[8..24],
        )
        .unwrap();
        assert_eq!(key_reference, old_key_reference);
        assert_eq!(arena.payload_replacements, repetitions);
        assert_eq!(get(&arena, root, &key).unwrap(), Some(value.as_slice()));
        if reuse {
            assert_eq!(arena.bytes.len(), original_size);
            assert_eq!(arena.allocations, original_allocations);
            assert_eq!(arena.payload_reuses, repetitions);
            assert_eq!(root, snapshot);
        } else {
            assert_eq!(
                arena.allocations - original_allocations,
                repetitions * 2,
                "only new value payload and CoW page are allocated"
            );
            assert_eq!(
                get(&arena, snapshot, &key).unwrap(),
                Some(vec![0x55; value.len()].as_slice())
            );
        }
        // Size changes allocate a new payload rather than overwriting its extent.
        let previous_allocations = arena.allocations;
        value.push(0x33);
        root = put(&mut arena, root, &key, &value).unwrap();
        assert_eq!(
            arena.allocations - previous_allocations,
            if reuse { 1 } else { 2 }
        );
        assert_eq!(get(&arena, root, &key).unwrap(), Some(value.as_slice()));
    }
}

#[test]
fn replacement_converts_external_large_inline_key_to_overflow() {
    let mut arena = Arena {
        reuse: true,
        ..Arena::default()
    };
    let key = vec![0x71; 1000];
    let mut raw = Vec::new();
    raw.extend_from_slice(&(key.len() as u16).to_le_bytes());
    raw.extend_from_slice(&0u16.to_le_bytes());
    raw.extend_from_slice(&4u32.to_le_bytes());
    raw.extend_from_slice(&key);
    raw.extend_from_slice(b"old!");
    let page = crate::page::encode(0, Reference::NULL, &[&raw]).unwrap();
    let root = arena.allocate(&page, PAGE_SIZE).unwrap();
    assert_eq!(get(&arena, root, &key).unwrap(), Some(b"old!".as_slice()));
    let root = put(&mut arena, root, &key, &vec![0x81; 64 * 1024]).unwrap();
    assert_eq!(
        get(&arena, root, &key).unwrap(),
        Some(vec![0x81; 64 * 1024].as_slice())
    );
    validate_page(&arena, root).unwrap();
}

#[test]
fn single_update_dirty_path_and_unchanged_overflow_values() {
    let mut arena = Arena {
        reuse: true,
        ..Arena::default()
    };
    let mut root = Reference::NULL;
    let large = vec![0x88; 64 * 1024];
    for n in 0..1000u64 {
        root = put(&mut arena, root, &n.to_be_bytes(), &large).unwrap();
    }
    assert!(height(&arena, root).unwrap() >= 2);
    arena.payload_reads.set(0);
    arena.rewrites = 0;
    arena.allocations = 0;
    let next = put(&mut arena, root, &500u64.to_be_bytes(), b"changed").unwrap();
    eprintln!(
        "single update: payload_reads={} rewrites={} storage_allocations={}",
        arena.payload_reads.get(),
        arena.rewrites,
        arena.allocations
    );
    assert_eq!(next, root);
    assert_eq!(
        arena.payload_reads.get(),
        0,
        "unchanged overflow values must stay encoded"
    );
    assert_eq!(arena.rewrites, 1);
    assert_eq!(arena.allocations, 0);
    assert_eq!(
        get(&arena, root, &499u64.to_be_bytes()).unwrap(),
        Some(large.as_slice())
    );
    assert_eq!(
        get(&arena, root, &500u64.to_be_bytes()).unwrap(),
        Some(b"changed".as_slice())
    );
}

#[test]
fn short_inline_update_dirties_only_changed_snapshot_path() {
    for reuse in [false, true] {
        let mut arena = Arena {
            reuse,
            ..Arena::default()
        };
        let mut root = Reference::NULL;
        for n in 0..1000u64 {
            root = put(&mut arena, root, &n.to_be_bytes(), &n.to_le_bytes()).unwrap();
        }
        let old_root = root;
        let levels = usize::from(height(&arena, root).unwrap());
        arena.rewrites = 0;
        arena.allocations = 0;
        arena.payload_reads.set(0);
        root = put(&mut arena, root, &500u64.to_be_bytes(), b"changed!").unwrap();
        assert_eq!(arena.payload_reads.get(), 0);
        assert_eq!(arena.rewrites, if reuse { 1 } else { levels });
        assert_eq!(arena.allocations, if reuse { 0 } else { levels });
        if !reuse {
            assert_eq!(
                get(&arena, old_root, &500u64.to_be_bytes()).unwrap(),
                Some(500u64.to_le_bytes().as_slice())
            );
        }
        assert_eq!(
            get(&arena, root, &500u64.to_be_bytes()).unwrap(),
            Some(b"changed!".as_slice())
        );
    }
}

type Pairs = Vec<(Vec<u8>, Vec<u8>)>;
fn scan(arena: &Arena, root: Reference, reverse: bool) -> Pairs {
    Cursor::new(arena, root, Bound::Unbounded, Bound::Unbounded, reverse)
        .unwrap()
        .map(|item| {
            let (k, v) = item.unwrap();
            (k.to_vec(), v.to_vec())
        })
        .collect()
}
fn assert_map(arena: &Arena, root: Reference, model: &BTreeMap<Vec<u8>, Vec<u8>>) {
    let expected: Pairs = model.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    assert_eq!(scan(arena, root, false), expected);
    assert_eq!(
        scan(arena, root, true),
        expected.iter().rev().cloned().collect::<Pairs>()
    );
    for (k, v) in model {
        assert_eq!(get(arena, root, k).unwrap(), Some(v.as_slice()));
    }
}

#[test]
fn empty_keys_values_overflow_and_snapshot_roots() {
    let mut arena = Arena::default();
    let mut root = Reference::NULL;
    assert_eq!(get(&arena, root, b"").unwrap(), None);
    assert_eq!(height(&arena, root).unwrap(), 0);
    assert!(scan(&arena, root, false).is_empty());
    let large_key = vec![0xff; 70_000];
    let large_value = vec![0xab; 120_000];
    root = put(&mut arena, root, b"", b"").unwrap();
    let empty_snapshot = root;
    root = put(&mut arena, root, &large_key, &large_value).unwrap();
    let large_snapshot = root;
    root = put(&mut arena, root, &large_key, b"updated").unwrap();
    assert_eq!(
        get(&arena, root, &large_key).unwrap(),
        Some(b"updated".as_slice())
    );
    assert_eq!(
        get(&arena, large_snapshot, &large_key).unwrap(),
        Some(large_value.as_slice())
    );
    assert_eq!(get(&arena, empty_snapshot, &large_key).unwrap(), None);
    assert_eq!(
        get(&arena, empty_snapshot, b"").unwrap(),
        Some(b"".as_slice())
    );
    assert_eq!(delete(&mut arena, root, b"absent").unwrap(), root);
    root = delete(&mut arena, root, &large_key).unwrap();
    root = delete(&mut arena, root, b"").unwrap();
    assert_eq!(root, Reference::NULL);
    assert_eq!(
        get(&arena, large_snapshot, &large_key).unwrap(),
        Some(large_value.as_slice())
    );
}

fn long_key(number: u64) -> Vec<u8> {
    let mut key = number.to_be_bytes().to_vec();
    key.resize(200, (number % 251) as u8);
    key
}

#[test]
fn overflow_separators_survive_branch_splits_and_replacements() {
    let mut arena = Arena {
        reuse: true,
        ..Arena::default()
    };
    let mut root = Reference::NULL;
    let mut model = BTreeMap::new();
    for n in (0..1700u64).rev() {
        let mut key = n.to_be_bytes().to_vec();
        key.resize(600, (n % 251) as u8);
        let value = vec![n as u8; 512];
        root = put(&mut arena, root, &key, &value).unwrap();
        model.insert(key, value);
    }
    assert!(height(&arena, root).unwrap() >= 3);
    assert_map(&arena, root, &model);
    for (index, (key, value)) in model.iter_mut().enumerate() {
        if index.is_multiple_of(7) {
            *value = vec![0x77; 10_000];
            root = put(&mut arena, root, key, value).unwrap();
        }
    }
    assert_map(&arena, root, &model);
}

#[test]
fn multilevel_splits_empty_leaves_reinsertion_and_old_roots() {
    let mut arena = Arena::default();
    let mut root = Reference::NULL;
    let mut model = BTreeMap::new();
    let mut snapshots = Vec::new();
    for n in 0..1400u64 {
        // Permutation forces both middle-page and edge splits.
        let number = (n * 997) % 1400;
        let key = long_key(number);
        let value = number.to_le_bytes().to_vec();
        root = put(&mut arena, root, &key, &value).unwrap();
        model.insert(key, value);
        if n % 350 == 349 {
            snapshots.push((root, model.clone()));
        }
    }
    assert!(height(&arena, root).unwrap() >= 3);
    assert_map(&arena, root, &model);
    for (at, expected) in &snapshots {
        assert_map(&arena, *at, expected);
    }
    for number in 0..1400 {
        root = delete(&mut arena, root, &long_key(number)).unwrap();
    }
    assert!(scan(&arena, root, false).is_empty());
    assert!(scan(&arena, root, true).is_empty());
    for number in (0..1400u64).step_by(3) {
        root = put(&mut arena, root, &long_key(number), b"again").unwrap();
    }
    let expected: BTreeMap<_, _> = (0..1400u64)
        .step_by(3)
        .map(|n| (long_key(n), b"again".to_vec()))
        .collect();
    assert_map(&arena, root, &expected);
    for (at, expected) in snapshots {
        assert_map(&arena, at, &expected);
    }
}

#[test]
fn deterministic_differential_mutations_and_private_rewrites() {
    for reuse in [false, true] {
        let mut arena = Arena {
            reuse,
            ..Arena::default()
        };
        let mut root = Reference::NULL;
        let mut model = BTreeMap::new();
        let mut seed = 0x1234_5678_9abc_def0u64;
        for step in 0..6000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let number = seed % 900;
            let mut key = number.to_be_bytes().to_vec();
            if number.is_multiple_of(11) {
                key.resize(300 + number as usize, 0x55);
            }
            if seed.is_multiple_of(4) {
                root = delete(&mut arena, root, &key).unwrap();
                model.remove(&key);
            } else {
                let size = if number.is_multiple_of(13) {
                    1500
                } else {
                    (seed % 73) as usize
                };
                let value = vec![step as u8; size];
                root = put(&mut arena, root, &key, &value).unwrap();
                model.insert(key, value);
            }
            if step % 200 == 0 {
                assert_map(&arena, root, &model);
            }
        }
        assert_map(&arena, root, &model);
    }
}

#[test]
fn all_bound_combinations_in_both_directions() {
    let mut arena = Arena {
        reuse: true,
        ..Arena::default()
    };
    let mut root = Reference::NULL;
    // Enough entries to cross many leaf boundaries, including empty ones.
    for n in 0..500u64 {
        root = put(&mut arena, root, &long_key(n * 2), b"v").unwrap();
    }
    for n in 100..300 {
        root = delete(&mut arena, root, &long_key(n * 2)).unwrap();
    }
    let all = scan(&arena, root, false);
    for low in [0, 1, 199, 200, 599, 600, 999, 1000] {
        for high in [0, 1, 199, 200, 599, 600, 999, 1000] {
            let low = long_key(low);
            let high = long_key(high);
            for include_low in [true, false] {
                for include_high in [true, false] {
                    let lower = if include_low {
                        Bound::Included(low.as_slice())
                    } else {
                        Bound::Excluded(low.as_slice())
                    };
                    let upper = if include_high {
                        Bound::Included(high.as_slice())
                    } else {
                        Bound::Excluded(high.as_slice())
                    };
                    let expected: Pairs = all
                        .iter()
                        .filter(|(k, _)| {
                            (if include_low { k >= &low } else { k > &low })
                                && (if include_high { k <= &high } else { k < &high })
                        })
                        .cloned()
                        .collect();
                    for reverse in [false, true] {
                        let actual: Pairs = Cursor::new(&arena, root, lower, upper, reverse)
                            .unwrap()
                            .map(|item| {
                                let (k, v) = item.unwrap();
                                (k.to_vec(), v.to_vec())
                            })
                            .collect();
                        let expected = if reverse {
                            expected.iter().rev().cloned().collect()
                        } else {
                            expected.clone()
                        };
                        assert_eq!(actual, expected);
                    }
                }
            }
        }
    }
}

#[test]
fn checked_decoder_rejects_overlap_flags_and_cycles() {
    let mut arena = Arena::default();
    let mut root = Reference::NULL;
    for n in 0..400 {
        root = put(&mut arena, root, &long_key(n), b"v").unwrap();
    }
    // Every allocated page is individually valid (payloads not present here).
    for offset in (0..arena.bytes.len()).step_by(PAGE_SIZE) {
        validate_page(
            &arena,
            Reference {
                segment: 1,
                offset: offset as u64,
            },
        )
        .unwrap();
    }
    let offset = root.offset as usize;
    let original = arena.bytes[offset..offset + PAGE_SIZE].to_vec();
    arena.bytes[offset + 16..offset + 32].copy_from_slice(&root.encode());
    assert!(get(&arena, root, b"").is_err());
    let mut cursor = Cursor::new(&arena, root, Bound::Unbounded, Bound::Unbounded, false);
    assert!(cursor.is_err());
    arena.bytes[offset..offset + PAGE_SIZE].copy_from_slice(&original);
    // Fill policy may leave a root with one separator. Corrupt two actual
    // slots in a populated page, rather than bytes outside its slot directory.
    // Slot-overlap corruption explicitly targets the unchanged V1 wire format.
    let a = crate::write::leaf_cell(&mut arena, b"a", b"v", None).unwrap();
    let b = crate::write::leaf_cell(&mut arena, b"b", b"v", None).unwrap();
    let legacy = crate::page::encode_v1(0, Reference::NULL, &[a.as_bytes(), b.as_bytes()]).unwrap();
    let overlap = arena.allocate(&legacy, PAGE_SIZE).unwrap().offset as usize;
    let overlap_original = arena.bytes[overlap..overlap + PAGE_SIZE].to_vec();
    let slot0 = arena.bytes[overlap + 32..overlap + 34].to_vec();
    arena.bytes[overlap + 34..overlap + 36].copy_from_slice(&slot0);
    assert!(
        validate_page(
            &arena,
            Reference {
                segment: 1,
                offset: overlap as u64
            }
        )
        .is_err()
    );
    arena.bytes[overlap..overlap + PAGE_SIZE].copy_from_slice(&overlap_original);
    arena.bytes[offset + 2] = 99;
    assert!(get(&arena, root, b"").is_err());
    cursor = Cursor::new(
        &arena,
        Reference::NULL,
        Bound::Unbounded,
        Bound::Unbounded,
        false,
    );
    assert!(cursor.unwrap().next().is_none());
}

#[test]
fn legal_external_large_inline_cells_split_at_a_feasible_boundary() {
    let mut arena = Arena::default();
    let keys = [vec![b'a'; 1492], vec![b'c'; 2392], vec![b'd'; 92]];
    let cells: Vec<Vec<u8>> = keys
        .iter()
        .map(|key| {
            let mut raw = vec![0; 8 + key.len()];
            raw[..2].copy_from_slice(&(key.len() as u16).to_le_bytes());
            raw[8..].copy_from_slice(key);
            raw
        })
        .collect();
    assert_eq!(
        cells.iter().map(Vec::len).collect::<Vec<_>>(),
        [1500, 2400, 100]
    );
    let refs: Vec<&[u8]> = cells.iter().map(Vec::as_slice).collect();
    let encoded = crate::page::encode(0, Reference::NULL, &refs).unwrap();
    let original = arena.allocate(&encoded, PAGE_SIZE).unwrap();
    validate_page(&arena, original).unwrap();
    let before = arena.bytes.clone();
    let value = vec![0x77; 247];
    let root =
        put(&mut arena, original, b"b", &value).expect("a feasible split exists: 1792/2536 bytes");
    assert_eq!(get(&arena, root, b"b").unwrap(), Some(value.as_slice()));
    for key in &keys {
        assert_eq!(get(&arena, root, key).unwrap(), Some(&[][..]));
        assert_eq!(get(&arena, original, key).unwrap(), Some(&[][..]));
    }
    assert_eq!(&arena.bytes[..before.len()], before.as_slice());
    crate::audit_tree(&arena, root, 16).unwrap();
    let observed: Vec<Vec<u8>> =
        Cursor::new(&arena, root, Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .map(|entry| entry.unwrap().0.to_vec())
            .collect();
    assert_eq!(
        observed,
        vec![
            keys[0].clone(),
            b"b".to_vec(),
            keys[1].clone(),
            keys[2].clone()
        ]
    );
}
