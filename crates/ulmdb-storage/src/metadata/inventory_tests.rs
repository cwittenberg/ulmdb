use super::*;
use crate::{Arena, ReadStore, SegmentTable, WriteStore};
use std::{fs, path::PathBuf};
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &std::path::Path) -> Self {
        let path = parent.join(format!(
            "ulmdb-inventory-{:032x}",
            crate::new_identity().unwrap()
        ));
        crate::create_layout(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn successor(base: &Manifest) -> Manifest {
    let mut next = base.clone_metadata();
    assert!(next.segments.is_empty());
    next.previous = base.generation;
    next.generation = crate::new_identity().unwrap();
    next.parent = base.commit;
    next.commit = next.generation;
    next.sequence += 1;
    next
}
#[test]
fn certified_actual_arena_appends_stay_lazy_and_match_generic_bytes_after_reopen() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        let directory = Directory::new(&parent);
        let mut base = Manifest::empty().unwrap();
        base.prepare_indexed(None).unwrap();
        let mut table = SegmentTable::default();
        base.certify_indexed_table(&table).unwrap();
        crate::publish(&directory.0, &base).unwrap();
        let mut old = Vec::new();
        for n in 0..96u64 {
            let mut arena = Arena::new(table.clone(), base.next_segment).unwrap();
            let payload = n.to_le_bytes();
            let value = arena.put_payload(&payload).unwrap();
            let mut next = successor(&base);
            let (new_table, publication) =
                arena.seal_indexed(&directory.0, &mut next, &base).unwrap();
            if n > 0 {
                assert!(
                    !next.segments.is_materialized(),
                    "append materialized whole inventory at {n}"
                );
            }
            if n > 0 {
                let ids = next.segments.iter().map(|entry| entry.id).sum::<u64>();
                assert_eq!(ids, (n + 1) * (n + 2) / 2);
                assert!(
                    !next.segments.is_materialized(),
                    "iteration must not collect a Vec"
                );
            }
            let encoded = next.encode().unwrap();
            if n > 0 {
                assert!(!next.segments.is_materialized());
            }
            assert_eq!(next.segments.len(), n as usize + 1);
            assert_eq!(next.segments.first().unwrap().id, 1);
            assert_eq!(next.segments.last().unwrap().id, n + 1);
            let entry = next.segments.last().unwrap().clone();
            let mut generic = successor(&base);
            generic.generation = next.generation;
            generic.commit = next.commit;
            generic.next_segment = next.next_segment;
            let mut entries = base.segments.to_vec();
            entries.push(entry);
            generic.segments = entries.into();
            generic.prepare_indexed(Some(&base)).unwrap();
            assert_eq!(
                generic.encode().unwrap(),
                encoded,
                "same0.4 descriptor bytes"
            );
            publication.publish().unwrap();
            old.push((new_table.clone(), next.clone(), value, payload));
            table = new_table;
            base = next;
        }
        assert!(!base.segments.is_materialized());
        assert_eq!(
            base.segments.iter().rev().map(|e| e.id).collect::<Vec<_>>(),
            (1..=96).rev().collect::<Vec<_>>()
        );
        let mut mixed = base.segments.iter();
        for n in 1..=48 {
            assert_eq!(mixed.next().unwrap().id, n);
            assert_eq!(mixed.next_back().unwrap().id, 97 - n);
            assert_eq!(mixed.len(), 96 - 2 * n as usize);
        }
        assert!(mixed.next().is_none() && mixed.next_back().is_none());
        assert!(!base.segments.is_materialized());
        let encoded = base.encode().unwrap();
        let mut lazy_mutation = base.clone();
        lazy_mutation.segments[0].hash[0] ^= 1;
        assert_eq!(lazy_mutation.segments.stamp(), 0);
        assert!(lazy_mutation.encode().is_err());
        assert_eq!(base.encode().unwrap(), encoded);
        assert!(
            !base.segments.is_materialized(),
            "mutating a clone must not fill the original cache"
        );
        for (old_table, old_manifest, at, expected) in &old {
            assert_eq!(old_table.payload(*at).unwrap(), expected);
            assert_eq!(old_table.entries().len(), old_manifest.segments.len());
        }
        // A newly mapped table is a different authority even for identical bytes.
        let mut reopened = crate::read_current(&directory.0).unwrap();
        let fresh_table = SegmentTable::load(&directory.0, &reopened.segments).unwrap();
        assert!(
            !reopened
                .index
                .as_ref()
                .unwrap()
                .append_eligible(&reopened, fresh_table.residency_identity())
        );
        reopened.certify_indexed_table(&fresh_table).unwrap();
        assert!(
            reopened
                .index
                .as_ref()
                .unwrap()
                .append_eligible(&reopened, fresh_table.residency_identity())
        );
        assert!(
            !reopened
                .index
                .as_ref()
                .unwrap()
                .append_eligible(&reopened, table.residency_identity())
        );
        for (_, _, at, expected) in old {
            assert_eq!(table.payload(at).unwrap(), expected);
        }
    }
}
#[test]
fn public_mutable_access_invalidates_permission_before_writing_and_preserves_clones() {
    let mut base = Manifest::empty().unwrap();
    base.segments = vec![SegmentEntry {
        id: 1,
        length: 4096,
        hash: [3; 32],
        cohort: 0,
    }]
    .into();
    base.next_segment = 2;
    base.prepare_indexed(None).unwrap();
    let encoded = base.encode().unwrap();
    let canonical_stamp = base.segments.stamp();
    for path in 0..5 {
        let mut changed = base.clone();
        match path {
            0 => changed.segments[0].hash[0] ^= 1,
            1 => changed.segments.get_mut(0).unwrap().length += 16,
            2 => changed.segments.iter_mut().next().unwrap().cohort = 7,
            3 => changed.segments.last_mut().unwrap().id = 2,
            _ => changed.segments.push(SegmentEntry {
                id: 2,
                length: 4096,
                hash: [4; 32],
                cohort: 0,
            }),
        }
        assert_eq!(changed.segments.stamp(), 0);
        assert!(changed.encode().is_err());
        assert_eq!(base.segments.stamp(), canonical_stamp);
        assert_eq!(base.encode().unwrap(), encoded);
    }
    let mut borrowed = base.clone();
    let _mutable_vec: &mut Vec<SegmentEntry> = &mut borrowed.segments;
    assert_eq!(
        borrowed.segments.stamp(),
        0,
        "even unmodified mutable access revokes permission"
    );
    assert_eq!(
        borrowed.encode().unwrap(),
        encoded,
        "generic full match remains permitted"
    );
}
#[test]
fn certified_append_rejects_wrong_table_generation_invalidated_stamp_and_preserves_generic_edits() {
    let directory = Directory::new(&std::env::temp_dir());
    let mut base = Manifest::empty().unwrap();
    base.prepare_indexed(None).unwrap();
    let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
    arena.put_payload(b"data").unwrap();
    let (table, prepared) = arena
        .seal_indexed(&directory.0, &mut base, &Manifest::empty().unwrap())
        .unwrap();
    prepared.publish().unwrap();
    let mut next = successor(&base);
    assert!(!prepare_append(&mut next, &base, table.residency_identity() + 1, None).unwrap());
    let mut mutated = base.clone();
    let _ = mutated.segments.get_mut(0).unwrap();
    assert!(!prepare_append(&mut next, &mutated, table.residency_identity(), None).unwrap());
    let mut rebound = base.clone();
    rebound.generation = crate::new_identity().unwrap();
    assert!(!prepare_append(&mut next, &rebound, table.residency_identity(), None).unwrap());
    let mut deleted = successor(&base);
    deleted.segments = base.segments.clone();
    deleted.segments.clear();
    deleted.prepare_indexed(Some(&base)).unwrap();
    assert!(deleted.segments.is_empty());
    assert!(deleted.encode().is_ok());
}

fn append_payload(
    directory: &std::path::Path,
    table: &SegmentTable,
    base: &Manifest,
    id: u64,
    bytes: &[u8],
) -> (SegmentTable, Manifest, crate::Reference) {
    let mut arena = Arena::new(table.clone(), id).unwrap();
    let at = arena.put_payload(bytes).unwrap();
    let mut next = successor(base);
    let (table, prepared) = arena.seal_indexed(directory, &mut next, base).unwrap();
    prepared.publish().unwrap();
    (table, next, at)
}
fn publication_state(directory: &std::path::Path) -> (Vec<u8>, Vec<std::ffi::OsString>) {
    let mut files: Vec<_> = fs::read_dir(directory.join("cohorts"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    files.sort();
    (fs::read(directory.join("CURRENT")).unwrap(), files)
}

#[test]
fn certified_high_ids_cross_chunks_reopen_and_reject_u64_exhaustion_without_reuse() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        for (first, count) in [((1u64 << 40) + 253, 8), (u64::MAX - 5, 5)] {
            let directory = Directory::new(&parent);
            let mut base = Manifest::empty().unwrap();
            base.next_segment = first;
            base.prepare_indexed(None).unwrap();
            crate::publish(&directory.0, &base).unwrap();
            let mut table = SegmentTable::default();
            let mut old = Vec::new();
            for n in 0..count {
                let id = first + n;
                let bytes = id.to_be_bytes();
                let (updated, next, at) = append_payload(&directory.0, &table, &base, id, &bytes);
                if n > 0 {
                    assert!(
                        !next.segments.is_materialized(),
                        "high-ID append fell off fast path"
                    );
                }
                assert_eq!(next.segments.len() as u64, n + 1);
                assert_eq!(next.segments.first().unwrap().id, first);
                assert_eq!(next.segments.last().unwrap().id, id);
                assert_eq!(
                    next.segments
                        .iter()
                        .map(|entry| entry.id)
                        .collect::<Vec<_>>(),
                    (first..=id).collect::<Vec<_>>()
                );
                assert_eq!(updated.payload(at).unwrap(), bytes);
                old.push((updated.clone(), at, bytes));
                table = updated;
                base = next;
            }
            for (held, at, bytes) in &old {
                assert_eq!(held.payload(*at).unwrap(), bytes);
            }
            let mut reopened = crate::read_current(&directory.0).unwrap();
            let fresh = SegmentTable::load(&directory.0, &reopened.segments).unwrap();
            reopened.certify_indexed_table(&fresh).unwrap();
            assert!(
                reopened
                    .index
                    .as_ref()
                    .unwrap()
                    .append_eligible(&reopened, fresh.residency_identity())
            );
            for (_, at, bytes) in &old {
                assert_eq!(fresh.payload(*at).unwrap(), bytes);
            }
            if reopened.next_segment == u64::MAX {
                let before = publication_state(&directory.0);
                assert!(matches!(
                    Arena::new(fresh.clone(), reopened.next_segment),
                    Err(Error::LimitExceeded)
                ));
                assert_eq!(publication_state(&directory.0), before);
                assert_eq!(reopened.segments.last().unwrap().id, u64::MAX - 1);
                assert_eq!(reopened.next_segment, u64::MAX);
            } else {
                let id = reopened.next_segment;
                let (updated, next, at) =
                    append_payload(&directory.0, &fresh, &reopened, id, b"reopened fast append");
                assert!(!next.segments.is_materialized());
                assert_eq!(updated.payload(at).unwrap(), b"reopened fast append");
            }
        }
    }
}

#[test]
fn certified_bounds_and_mutated_base_fail_before_publication_and_lower_holes_use_generic_path() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        let directory = Directory::new(&parent);
        let first = (1u64 << 40) + 21;
        let mut base = Manifest::empty().unwrap();
        base.next_segment = first;
        base.prepare_indexed(None).unwrap();
        crate::publish(&directory.0, &base).unwrap();
        let (table, base, old_at) =
            append_payload(&directory.0, &SegmentTable::default(), &base, first, b"old");
        let before = publication_state(&directory.0);
        for id in [
            0,
            u64::MAX,
            ((first >> 8) + crate::table::MAX_DIRECTORY_CHUNKS) * 256,
        ] {
            assert!(matches!(
                Arena::new(table.clone(), id),
                Err(Error::LimitExceeded)
            ));
            assert_eq!(publication_state(&directory.0), before);
        }
        let mut next = successor(&base);
        let outside = SegmentEntry {
            id: ((first >> 8) + crate::table::MAX_DIRECTORY_CHUNKS) * 256,
            length: 4096,
            hash: [7; 32],
            cohort: next.generation,
        };
        assert!(matches!(
            prepare_append(&mut next, &base, table.residency_identity(), Some(&outside)),
            Err(Error::LimitExceeded)
        ));
        assert!(next.segments.is_empty() && next.index.is_none());
        assert_eq!(publication_state(&directory.0), before);

        let mut tampered = base.clone();
        tampered.segments[0].hash[0] ^= 1;
        assert_eq!(tampered.segments.stamp(), 0);
        assert!(
            !tampered
                .index
                .as_ref()
                .unwrap()
                .append_eligible(&tampered, table.residency_identity())
        );
        assert!(tampered.certify_indexed_table(&table).is_err());
        let mut arena = Arena::new(table.clone(), base.next_segment).unwrap();
        arena.put_payload(b"must not publish").unwrap();
        let mut rejected = successor(&base);
        assert!(
            arena
                .seal_indexed(&directory.0, &mut rejected, &tampered)
                .is_err()
        );
        assert_eq!(publication_state(&directory.0), before);
        assert_eq!(table.payload(old_at).unwrap(), b"old");

        // A legal unused lower ID is not an append. The public general-purpose
        // builder must still preserve it, rather than accepting a false delta.
        let (lower, generic, lower_at) =
            append_payload(&directory.0, &table, &base, first - 1, b"lower gap");
        assert!(
            generic.segments.is_materialized(),
            "nonappend should use validated generic preparation"
        );
        assert_eq!(
            generic
                .segments
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            vec![first - 1, first]
        );
        assert_eq!(lower.payload(lower_at).unwrap(), b"lower gap");
        assert_eq!(table.payload(old_at).unwrap(), b"old");
        let id = generic.next_segment;
        let (updated, fast, at) =
            append_payload(&directory.0, &lower, &generic, id, b"after generic");
        assert!(!fast.segments.is_materialized());
        assert_eq!(updated.payload(at).unwrap(), b"after generic");
    }
}
