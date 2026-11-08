use super::*;
use crate::{cohort, manifest};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "ulmdb-metadata-{:032x}",
            manifest::new_identity().unwrap()
        ));
        manifest::create_layout(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn entry(id: u64, value: u8) -> SegmentEntry {
    SegmentEntry {
        id,
        length: 4096,
        hash: [value; 32],
        cohort: 0,
    }
}
fn initialized(entries: Vec<SegmentEntry>) -> Manifest {
    let mut m = Manifest::empty().unwrap();
    m.next_segment = entries.last().map_or(1, |e| e.id + 1);
    m.segments = (entries).into();
    m.prepare_indexed(None).unwrap();
    m
}
fn successor(base: &Manifest, entries: Vec<SegmentEntry>) -> Manifest {
    let mut m = base.clone_metadata();
    m.generation = manifest::new_identity().unwrap();
    m.previous = base.generation;
    m.parent = base.commit;
    m.commit = m.generation;
    m.sequence += 1;
    m.next_segment = m.next_segment.max(entries.last().map_or(1, |e| e.id + 1));
    m.segments = (entries).into();
    m.prepare_indexed(Some(base)).unwrap();
    m
}
fn equal_entries(actual: &[SegmentEntry], expected: &[SegmentEntry]) {
    assert_eq!(actual.len(), expected.len());
    assert!(actual.iter().zip(expected).all(|(a, b)| tree::equal(a, b)));
}
fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}
#[test]
fn randomized_persistent_edits_match_independent_map_across_reopen_and_exact_encoding() {
    let directory = Directory::new();
    let mut model = BTreeMap::new();
    let mut old = initialized(Vec::new());
    cohort::write(&directory.0, &old, None).unwrap();
    let mut seed = 0x829a6d103e5bc107;
    for _ in 0..240 {
        let id = 1 + next(&mut seed) % 127;
        if next(&mut seed).is_multiple_of(3) {
            model.remove(&id);
        } else {
            model.insert(id, entry(id, next(&mut seed) as u8));
        }
        let entries = model.values().cloned().collect::<Vec<_>>();
        let new = successor(&old, entries.clone());
        assert!(
            new.index.as_ref().unwrap().origins.get().is_none(),
            "ordinary preparation must not eagerly trace origins"
        );
        let encoded = new.encode().unwrap();
        assert!(encoded.len() <= 256 + 65 * 160);
        let hash = cohort::write(&directory.0, &new, None).unwrap();
        let decoded = cohort::decode(&directory.0, new.generation, hash).unwrap();
        equal_entries(&decoded.segments, &entries);
        assert_eq!(decoded.encode().unwrap(), encoded);
        assert_eq!(decoded.metadata_origins(), new.metadata_origins());
        old = decoded;
    }
}
#[test]
fn full_tree_size_incremental_path_bound_and_mutated_context_rejection() {
    let entries = (1..=1024).map(|id| entry(id, id as u8)).collect::<Vec<_>>();
    let base = initialized(entries.clone());
    assert_eq!(base.encode().unwrap().len(), 256 * 1024 + 96);
    let mut next_entries = entries;
    next_entries.push(entry(1025, 7));
    let incremental = successor(&base, next_entries);
    assert!(incremental.encode().unwrap().len() <= 256 + 65 * 160);
    assert!(incremental.index.as_ref().unwrap().local_nodes < 20);
    let mut bad = incremental.clone();
    bad.segments[0].hash[0] ^= 1;
    assert!(bad.encode().is_err());
    let mut bad_base = base.clone();
    bad_base.segments[0].hash[0] ^= 1;
    let mut target = base.clone_metadata();
    target.generation = manifest::new_identity().unwrap();
    assert!(target.prepare_indexed(Some(&bad_base)).is_err());
    let mut unprepared = base.clone_metadata();
    unprepared.segments = base.segments.clone();
    assert!(unprepared.encode().is_err());
}
fn root_offset(bytes: &[u8]) -> usize {
    u64::from_le_bytes(bytes[208..216].try_into().unwrap()) as usize
}
fn rehash_root(bytes: &mut [u8]) {
    let offset = root_offset(bytes);
    let len = u32::from_le_bytes(bytes[216..220].try_into().unwrap()) as usize;
    let hash = Sha256::digest(&bytes[offset..offset + len]);
    bytes[224..256].copy_from_slice(&hash);
}
#[test]
fn checked_decoder_rejects_hash_reserved_geometry_alias_cycles_and_unused_local_nodes() {
    let m = initialized(vec![entry(7, 1), entry(8, 2)]);
    let original = m.encode().unwrap();
    let root = root_offset(&original);
    for offset in [
        116,
        120,
        192,
        208,
        216,
        220,
        224,
        root + 2,
        root + 4,
        root + 16,
        root + 17,
        root + 8,
    ] {
        let mut b = original.clone();
        b[offset] ^= 1;
        if offset >= root {
            rehash_root(&mut b);
        }
        assert!(Manifest::decode(&b).is_err(), "offset {offset}");
    }
    // A repeated child reference is a DAG alias, not a valid two-leaf tree.
    let mut alias = original.clone();
    let left = alias[root + 32..root + 96].to_vec();
    alias[root + 96..root + 160].copy_from_slice(&left);
    rehash_root(&mut alias);
    assert!(Manifest::decode(&alias).is_err());
    // Self-reference is rejected before traversal can exhaust memory or recurse.
    let mut cycle = original.clone();
    let selfref = cycle[192..256].to_vec();
    cycle[root + 32..root + 96].copy_from_slice(&selfref);
    rehash_root(&mut cycle);
    assert!(Manifest::decode(&cycle).is_err());
    // Validly formed but unreachable leaf bytes cannot hide in the local blob.
    let mut junk = original.clone();
    let extra = junk[256..352].to_vec();
    junk.extend_from_slice(&extra);
    junk[116..120].copy_from_slice(&4u32.to_le_bytes());
    assert!(Manifest::decode(&junk).is_err());
    let mut extra = original.clone();
    extra.extend_from_slice(&[0; 32]);
    assert!(Manifest::decode(&extra).is_err());
}
#[test]
fn exact_postorder_is_required_even_when_all_node_hashes_and_geometry_are_valid() {
    let m = initialized(vec![entry(7, 1), entry(8, 2)]);
    let mut b = m.encode().unwrap();
    let root = root_offset(&b);
    let left = b[256..352].to_vec();
    let right = b[352..448].to_vec();
    b[256..352].copy_from_slice(&right);
    b[352..448].copy_from_slice(&left);
    // Rebind each leaf reference to its correct swapped location. Traversal is
    // still geometrically valid, but right-before-left is not canonical order.
    b[root + 48..root + 56].copy_from_slice(&352u64.to_le_bytes());
    b[root + 112..root + 120].copy_from_slice(&256u64.to_le_bytes());
    rehash_root(&mut b);
    assert!(Manifest::decode(&b).is_err());
}
#[test]
fn extreme_ids_exercise_a_sixty_five_node_path_in_the_tree_primitive() {
    // Directory admission has a narrower live-ID window; the primitive itself
    // handles the whole64-bit domain, including the excluded boundary sentinels.
    let entries = (0..64)
        .rev()
        .map(|bit| entry(u64::MAX ^ (1u64 << bit), 1))
        .collect::<Vec<_>>();
    let root = tree::build(&entries);
    let added = tree::insert(root, entry(u64::MAX, 2)).unwrap();
    let mut blob = Vec::new();
    let mut count = 0;
    let prepared = super::codec::serialize(added, 1, true, &mut blob, &mut count).unwrap();
    assert_eq!(prepared.count, 65);
    assert_eq!(count, 129);
    // Updating the deepest leaf path copies64 branches+one leaf, never more.
    let changed = tree::insert(Some(prepared), entry(u64::MAX, 3)).unwrap();
    let mut blob = Vec::new();
    let mut count = 0;
    super::codec::serialize(changed, 2, true, &mut blob, &mut count).unwrap();
    assert_eq!(count, 65);
}
#[test]
fn resolver_handle_cache_is_bounded_and_missing_primary_uses_only_valid_fallback() {
    let source = Directory::new();
    let staging = Directory::new();
    let mut refs = Vec::new();
    for id in 1..=80 {
        let m = initialized(vec![entry(id, id as u8)]);
        cohort::write(&source.0, &m, None).unwrap();
        refs.push(
            m.index
                .as_ref()
                .unwrap()
                .root
                .as_ref()
                .unwrap()
                .location
                .unwrap(),
        );
    }
    let mut resolver = Resolver::new(&staging.0, Some(&source.0));
    for r in refs.iter().chain(refs.iter().rev()) {
        resolver.read(*r, &mut [0; 160]).unwrap();
        assert!(resolver.open_handles() <= 64);
    }
    let reference = refs[0];
    let path = cohort::path(&staging.0, reference.origin);
    fs::write(&path, b"bad primary").unwrap();
    let mut fresh = Resolver::new(&staging.0, Some(&source.0));
    assert!(
        fresh.read(reference, &mut [0; 160]).is_err(),
        "corrupt primary never falls back"
    );
}
#[test]
fn selected_root_never_uses_fallback_but_shared_old_nodes_may() {
    let source = Directory::new();
    let stage = Directory::new();
    let base = initialized((1..=6).map(|id| entry(id, id as u8)).collect());
    cohort::write(&source.0, &base, None).unwrap();
    let mut entries = base.segments.clone();
    entries.push(entry(7, 7));
    let new = successor(&base, entries.to_vec());
    let hash = cohort::write(&source.0, &new, None).unwrap();
    assert!(cohort::decode_with_fallback(&stage.0, Some(&source.0), new.generation, hash).is_err());
    fs::copy(
        cohort::path(&source.0, new.generation),
        cohort::path(&stage.0, new.generation),
    )
    .unwrap();
    let decoded =
        cohort::decode_with_fallback(&stage.0, Some(&source.0), new.generation, hash).unwrap();
    equal_entries(&decoded.segments, &new.segments);
    assert!(cohort::decode(&stage.0, new.generation, hash).is_err());
}
#[test]
fn indexed_high_ids_round_trip_with_the_same_directory_window_contract() {
    for first in [1, 1u64 << 40, u64::MAX - 128] {
        let m = initialized((first..first + 63).map(|id| entry(id, id as u8)).collect());
        let bytes = m.encode().unwrap();
        equal_entries(&Manifest::decode(&bytes).unwrap().segments, &m.segments);
    }
}

#[test]
fn deleting_to_one_or_zero_and_reinserting_preserves_old_snapshot_and_local_root() {
    let base = initialized(vec![entry(1, 1), entry(2, 2)]);
    let old = base.encode().unwrap();
    let one = successor(&base, vec![entry(2, 2)]);
    assert_eq!(one.index.as_ref().unwrap().local_nodes, 1);
    assert_eq!(one.metadata_origins(), BTreeSet::from([one.generation]));
    let decoded = Manifest::decode(&one.encode().unwrap()).unwrap();
    equal_entries(&decoded.segments, &one.segments);
    let empty = successor(&one, vec![]);
    let bytes = empty.encode().unwrap();
    assert_eq!(bytes.len(), 256);
    assert_eq!(&bytes[192..256], &[0; 64]);
    assert!(empty.metadata_origins().is_empty());
    let decoded = Manifest::decode(&bytes).unwrap();
    assert!(decoded.segments.is_empty());
    let restored = successor(&decoded, vec![entry(3, 3)]);
    assert_eq!(
        restored.metadata_origins(),
        BTreeSet::from([restored.generation])
    );
    equal_entries(
        &Manifest::decode(&restored.encode().unwrap())
            .unwrap()
            .segments,
        &restored.segments,
    );
    assert_eq!(base.encode().unwrap(), old);
}

fn write_unchecked_cohort(directory: &Path, generation: u128, descriptor: &[u8]) {
    let mut bytes = descriptor.to_vec();
    let mut trailer = [0; 32];
    trailer[..8].copy_from_slice(b"ULMDBCH4");
    trailer[16..24].copy_from_slice(&(descriptor.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&trailer);
    fs::write(cohort::path(directory, generation), bytes).unwrap();
}
#[test]
fn external_node_children_must_also_be_backward_even_without_decoding_old_inventory() {
    let directory = Directory::new();
    let base = initialized(vec![entry(2, 2), entry(3, 3)]);
    let original = base.encode().unwrap();
    // Move the old branch before its two leaves, with valid node hashes and
    // geometry. Reading only this branch must still reject forward child refs.
    let mut bad = original[..256].to_vec();
    bad.extend_from_slice(&original[448..608]);
    bad.extend_from_slice(&original[256..448]);
    bad[256 + 48..256 + 56].copy_from_slice(&416u64.to_le_bytes());
    bad[256 + 112..256 + 120].copy_from_slice(&512u64.to_le_bytes());
    let old_hash: [u8; 32] = Sha256::digest(&bad[256..416]).into();
    write_unchecked_cohort(&directory.0, base.generation, &bad);
    let new = successor(&base, vec![entry(1, 1), entry(2, 2), entry(3, 3)]);
    let mut bytes = new.encode().unwrap();
    let root = root_offset(&bytes);
    bytes[root + 112..root + 120].copy_from_slice(&256u64.to_le_bytes());
    bytes[root + 128..root + 160].copy_from_slice(&old_hash);
    rehash_root(&mut bytes);
    let hash = Sha256::digest(&bytes).into();
    write_unchecked_cohort(&directory.0, new.generation, &bytes);
    assert!(cohort::decode(&directory.0, new.generation, hash).is_err());
}

#[test]
fn hostile_aligned_root_and_child_extents_return_errors_without_overflow_panics() {
    let manifest = initialized(vec![entry(7, 1), entry(8, 2)]);
    let original = manifest.encode().unwrap();
    let root = root_offset(&original);
    for offset in [
        u64::MAX - 31,
        u64::MAX - 63,
        MAX_DESCRIPTOR_BYTES - 32,
        MAX_DESCRIPTOR_BYTES,
    ] {
        let mut bad_root = original.clone();
        bad_root[208..216].copy_from_slice(&offset.to_le_bytes());
        assert!(
            std::panic::catch_unwind(|| Manifest::decode(&bad_root))
                .expect("root extent must not panic")
                .is_err()
        );
        for field in [root + 48, root + 112] {
            let mut bad_child = original.clone();
            bad_child[field..field + 8].copy_from_slice(&offset.to_le_bytes());
            rehash_root(&mut bad_child);
            assert!(
                std::panic::catch_unwind(|| Manifest::decode(&bad_child))
                    .expect("hashed child extent must not panic")
                    .is_err()
            );
        }
    }
}
