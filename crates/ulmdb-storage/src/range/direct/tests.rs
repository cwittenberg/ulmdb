use ulmdb_platform::FileExt;
use super::*;
use crate::{ReadStore, Reference, WriteStore};
use std::{collections::BTreeSet, path::PathBuf};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "ulmdb-direct-unit-{}",
            crate::new_identity().unwrap()
        ));
        crate::create_layout(&p).unwrap();
        Self(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn next(base: &Manifest) -> Manifest {
    let mut m = base.clone_metadata();
    m.previous = m.generation;
    m.parent = m.commit;
    m.generation = crate::new_identity().unwrap();
    m.commit = m.generation;
    m.sequence += 1;
    m
}
#[test]
fn public_mutation_foreign_table_and_scalar_changes_reject_before_io() {
    let d = Dir::new();
    let c = ResidencyController::new(crate::ResidencyPolicy::Demand).unwrap();
    let (mut store, initial) = Store::create(&d.0, &c).unwrap();
    let mut seed = Arena::new(initial.segments.clone(), initial.manifest.next_segment).unwrap();
    seed.put_payload(b"seeded immutable mapping").unwrap();
    let state = store
        .commit_arena(seed, next(&initial.manifest), &initial.manifest, &c, None)
        .unwrap();

    let before = std::fs::metadata(d.0.join(FILE)).unwrap().len();
    for kind in 0..3 {
        let mut base = state.manifest.clone();
        let table = if kind == 2 {
            SegmentTable::default()
        } else {
            state.segments.clone()
        };
        if kind == 0 {
            base.segments = base.segments.iter().cloned().collect();
        }
        if kind == 1 {
            base.catalog = Reference {
                segment: 0,
                offset: 1,
            };
        }
        let arena = Arena::new(table, base.next_segment).unwrap();
        assert!(matches!(
            store.commit_arena(arena, next(&base), &base, &c, None),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(std::fs::metadata(d.0.join(FILE)).unwrap().len(), before);
        assert!(!store.is_poisoned());
    }
    let mut arena = Arena::new(state.segments.clone(), state.manifest.next_segment).unwrap();
    let value = arena.put_payload(b"typed raw data").unwrap();
    let result = store
        .commit_arena(arena, next(&state.manifest), &state.manifest, &c, None)
        .unwrap();
    assert_eq!(result.segments.payload(value).unwrap(), b"typed raw data");
}
#[test]
fn primitive_maximal_path_serializes65_nodes_and129_visits() {
    let file = [7; 16];
    let mut root = None;
    let mut ids = (0..64).map(|n| 1u64 << n).collect::<BTreeSet<_>>();
    ids.remove(&3);
    for (i, id) in ids.into_iter().chain([3]).enumerate() {
        let start = pack::DATA_START + (i as u64 + 2) * 65536;
        let entry = SegmentEntry {
            id,
            length: 4096,
            hash: [5; 32],
            cohort: 0,
        };
        let data = PhysicalRef {
            file,
            offset: start + 8192,
            length: 4096,
            hash: entry.hash,
        };
        root = Some(tree::insert(root, entry, data).unwrap());
        let mut encoded = tree::Encoded::default();
        root = Some(tree::serialize(root.unwrap(), file, start + 12288, &mut encoded).unwrap());
        assert!(encoded.written <= 65 && encoded.visited <= 129);
        if id == 3 {
            assert_eq!((encoded.written, encoded.visited), (65, 129));
            assert_eq!(encoded.bytes.len(), 10336);
        }
    }
}
#[test]
fn physical_refs_check_every_identity_extent_and_geometry_boundary() {
    let file = [7; 16];
    let r = PhysicalRef {
        file,
        offset: pack::DATA_START + 8192,
        length: 4096,
        hash: [8; 32],
    };
    assert!(r.checked(file, pack::DATA_START + 65536, false).is_ok());
    assert!(r.checked([6; 16], pack::DATA_START + 65536, false).is_err());
    assert!(r.checked(file, pack::DATA_START, false).is_err());
    for offset in [0, 65536, u64::MAX - 32] {
        let mut bad = r;
        bad.offset = offset;
        assert!(bad.checked(file, pack::MAX_FILE_BYTES, false).is_err());
    }
    let mut bad = r;
    bad.length = u64::MAX;
    assert!(bad.checked(file, pack::MAX_FILE_BYTES, false).is_err());
    assert_eq!(PhysicalRef::decode(&r.encode()).unwrap(), Some(r));
    assert_eq!(PhysicalRef::decode(&[0; 64]).unwrap(), None);
    let mut bad = r.encode();
    bad[..16].fill(0);
    assert!(PhysicalRef::decode(&bad).is_err());
}
fn malformed(kind: u8, heal: bool) {
    let d = Dir::new();
    let c = ResidencyController::new(crate::ResidencyPolicy::Demand).unwrap();
    let (mut store, initial) = Store::create(&d.0, &c).unwrap();
    let mut seed = Arena::new(initial.segments.clone(), initial.manifest.next_segment).unwrap();
    seed.put_payload(b"seeded immutable mapping").unwrap();
    let state = store
        .commit_arena(seed, next(&initial.manifest), &initial.manifest, &c, None)
        .unwrap();

    let file = store.writer.global.file_id;
    let start = store.writer.selected.frame_end().unwrap();
    let ordinal = store.writer.selected.ordinal + 1;
    let mut m = next(&state.manifest);
    m.generation =
        ulmdb_core::range_generation::physical_generation(u128::from_le_bytes(file), ordinal)
            .unwrap();
    let mut blob = Vec::new();
    let mut root = None;
    let mut local = 0;
    if kind == 0 {
        m.generation = if m.generation == 1 {
            2
        } else {
            m.generation ^ 1
        };
    } else {
        let fake_entry = SegmentEntry {
            id: 1,
            length: 4096,
            hash: [8; 32],
            cohort: 0,
        };
        m.segments = Inventory::range_from_entries(if kind == 1 {
            vec![fake_entry.clone()]
        } else {
            vec![
                fake_entry.clone(),
                SegmentEntry {
                    id: 2,
                    ..fake_entry.clone()
                },
            ]
        })
        .unwrap();
        m.next_segment = 3;
        let mut foreign = file;
        foreign[0] ^= 1;
        if foreign == [0; 16] {
            foreign[1] = 1;
        }
        let mut r = PhysicalRef {
            file: foreign,
            offset: pack::DATA_START + 8192,
            length: 96,
            hash: [9; 32],
        };
        if kind == 2 {
            let base = codec::blob_offset(start, 0).unwrap();
            let child = PhysicalRef {
                file,
                offset: base + 32,
                length: 96,
                hash: [4; 32],
            };
            blob = vec![0; 160];
            blob[..8].copy_from_slice(b"ULMDBL06");
            blob[8] = 1;
            blob[9] = 62;
            blob[24..32].copy_from_slice(&2u64.to_le_bytes());
            blob[32..96].copy_from_slice(&child.encode());
            blob[96..160].copy_from_slice(&child.encode());
            r = PhysicalRef {
                file,
                offset: base,
                length: 160,
                hash: tree::hash(&blob),
            };
            local = 1;
        }
        root = Some(Arc::new(Node {
            prefix: 0,
            depth: 0,
            count: m.segments.len(),
            location: Some(r),
            kind: tree::Kind::Leaf {
                entry: fake_entry,
                data: r,
            },
        }));
    }
    let payload = codec::encode(codec::EncodeInput {
        file,
        frame_start: start,
        manifest: &m,
        data: &[],
        root: root.as_ref(),
        blob: &blob,
        local,
        own_id: 0,
    })
    .unwrap();
    let frame = store.writer.prepare(&payload).unwrap();
    store.writer.publish_prepared(&frame).unwrap();
    let slot = store.writer.slot;
    drop(state);
    drop(store);
    let path = d.0.join(FILE);
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    if heal {
        f.write_all_at(&[0xff], pack::HEAD_OFFSETS[slot]).unwrap();
    } else {
        let len = f.metadata().unwrap().len();
        f.write_all_at(b"unselected tail", len).unwrap();
    }
    f.sync_all().unwrap();
    drop(f);
    let before = std::fs::read(&path).unwrap();
    assert!(matches!(
        Store::open_checked(&d.0, &c, |_, _| Ok(())),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(std::fs::read(path).unwrap(), before);
}
#[test]
fn rehashed_bad_generation_foreign_locator_and_forward_child_never_heal_or_trim() {
    for kind in 0..3 {
        for heal in [false, true] {
            malformed(kind, heal);
        }
    }
}
#[test]
fn all_header_bytes_are_bound_and_rehashed_unsupported_features_are_rejected() {
    let mut m = Manifest::empty().unwrap();
    m.format_minor = 6;
    m.segments = Inventory::range_from_entries(Vec::new()).unwrap();
    let file = [7; 16];
    let start = pack::DATA_START + 65536;
    let bytes = codec::encode(codec::EncodeInput {
        file,
        frame_start: start,
        manifest: &m,
        data: &[],
        root: None,
        blob: &[],
        local: 0,
        own_id: 0,
    })
    .unwrap();
    for index in 0..codec::HEADER {
        let mut changed = bytes.clone();
        changed[index] ^= 1;
        assert!(
            codec::decode(&changed, start).is_err(),
            "header byte {index}"
        );
    }
    for field in [8, 10, 12, 120, 144, 176, 184, 288, 336] {
        let mut changed = bytes.clone();
        changed[field] ^= 1;
        if matches!(field, 176 | 184) {
            changed[field..field + 8].fill(0);
        }
        let hash = Sha256::digest(&changed[..codec::HEADER - 32]);
        changed[codec::HEADER - 32..codec::HEADER].copy_from_slice(&hash);
        assert!(
            codec::decode(&changed, start).is_err(),
            "rehashed field {field}"
        );
    }
}

#[test]
fn independently_encoded_leaf_has_exact_node_domain_hash_and_physical_offsets() {
    let entry = SegmentEntry {
        id: 1,
        length: 4096,
        hash: [5; 32],
        cohort: 0,
    };
    let data = PhysicalRef {
        file: [7; 16],
        offset: 204800,
        length: 4096,
        hash: [5; 32],
    };
    let root = tree::insert(None, entry, data).unwrap();
    let mut encoded = tree::Encoded::default();
    let root = tree::serialize(root, [7; 16], 212992, &mut encoded).unwrap();
    assert_eq!(encoded.bytes.len(), 96);
    assert_eq!(
        root.location.unwrap().hash,
        [
            193, 33, 234, 162, 223, 147, 94, 192, 32, 208, 254, 96, 163, 116, 61, 188, 33, 113,
            245, 31, 187, 180, 144, 117, 105, 136, 96, 74, 28, 157, 9, 181
        ]
    );
    assert_eq!(&encoded.bytes[32..96], &data.encode());
}
