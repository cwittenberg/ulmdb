//! Differential oracle captured from the frozen original Arena::patch.
use super::*;
use crate::resident::{Config, Pool};
use crate::{Error, WriteStore};
fn original_patch(
    arena: &mut Arena,
    old: Reference,
    object_len: usize,
    offset: usize,
    replacement: &[u8],
    alignment: usize,
) -> Result<Reference> {
    arena.pool.ensure_live()?;
    if !alignment.is_power_of_two()
        || alignment > PAGE
        || !old.offset.is_multiple_of(alignment as u64)
    {
        return Err(Error::InvalidInput("resident patch alignment"));
    }
    let end = offset
        .checked_add(replacement.len())
        .filter(|n| *n <= object_len)
        .ok_or(Error::InvalidInput("resident patch bounds"))?;
    if !arena.exact(old, object_len)? {
        return Err(Error::InvalidInput(
            "resident patch requires exact allocation",
        ));
    }
    let bytes = arena.read(old, object_len)?;
    if &bytes[offset..end] == replacement {
        return Ok(old);
    }
    let (slot, generation, start) = unpack(old)?;
    if arena
        .private
        .get(&slot)?
        .is_some_and(|p| p.generation == generation)
    {
        arena.tick()?;
        arena
            .private
            .get_mut(&slot)?
            .unwrap()
            .write(start + offset, replacement)?;
        Ok(old)
    } else {
        let mut bytes = bytes.to_vec();
        bytes[offset..end].copy_from_slice(replacement);
        arena.allocate_copy(old, &bytes, alignment)
    }
}

fn pool(pages: usize, tiny: usize) -> std::sync::Arc<Pool> {
    Pool::create(Config {
        kv_pages: pages,
        tiny_pages: tiny,
        max_reserved_bytes: 1 << 24,
        max_metadata_bytes: 1 << 24,
        max_views: 32,
    })
    .unwrap()
}

fn check_case(
    pool: &std::sync::Arc<Pool>,
    view: &crate::resident::View,
    old: Reference,
    object_len: usize,
    offset: usize,
    replacement: &[u8],
    alignment: usize,
) {
    let source = view.read(old, PAGE).unwrap().to_vec();
    let mut expected = pool.begin(view.clone()).unwrap();
    let mut actual = pool.begin(view.clone()).unwrap();
    let a = original_patch(
        &mut expected,
        old,
        object_len,
        offset,
        replacement,
        alignment,
    );
    let b = actual.patch(old, object_len, offset, replacement, alignment);
    assert_eq!(expected.revision(), actual.revision());
    match (a, b) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a == old, b == old);
            assert_eq!(
                expected.read(a, object_len).unwrap(),
                actual.read(b, object_len).unwrap()
            );
            let expected = expected.freeze().unwrap();
            let actual = actual.freeze().unwrap();
            let shape = |prepared: &super::super::Prepared| {
                prepared
                    .slots()
                    .map(|s| (s.ranges.to_vec(), s.bytes.to_vec(), s.copy_source))
                    .collect::<Vec<_>>()
            };
            assert_eq!(shape(&expected), shape(&actual));
        }
        (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
        (a, b) => panic!("original {a:?}; candidate {b:?}"),
    }
    assert_eq!(view.read(old, PAGE).unwrap(), source);
}

#[test]
fn every_page_offset_and_seeded_replacement_matches_frozen_original() {
    assert!(Pool::direct_page_patch_enabled());
    let pool = pool(6, 2);
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let source: Vec<_> = (0..PAGE).map(|n| (n.wrapping_mul(73) >> 3) as u8).collect();
    let old = arena.allocate(&source, PAGE).unwrap();
    let view = arena.freeze().unwrap().activate();
    let held = view.read(old, PAGE).unwrap();
    for offset in 0..=PAGE {
        check_case(
            &pool,
            &view,
            old,
            PAGE,
            offset,
            &[!source[offset % PAGE]],
            PAGE,
        );
    }
    let mut seed = 73u64;
    for _ in 0..512 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let offset = seed as usize % (PAGE + 2);
        let length = (seed >> 16) as usize % 65;
        let replacement = vec![(seed >> 32) as u8; length];
        check_case(&pool, &view, old, PAGE, offset, &replacement, PAGE);
    }
    for offset in [0, 1, PAGE, usize::MAX] {
        for replacement in [&[][..], &[0xff; PAGE][..]] {
            check_case(&pool, &view, old, PAGE, offset, replacement, PAGE);
        }
    }
    assert_eq!(held, source);
}

#[test]
fn tiny_full_page_source_stays_literal_and_fallback_guards_match() {
    let pool = pool(6, 4);
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    // A full object allocated with weak alignment is a Tiny map, not a KV page.
    let old = arena.allocate(&[17; PAGE], 16).unwrap();
    let view = arena.freeze().unwrap().activate();
    for alignment in [0, 1, 16, 3, PAGE, PAGE * 2] {
        for object_len in [PAGE - 1, PAGE, PAGE + 1] {
            check_case(&pool, &view, old, object_len, 7, &[9; 8], alignment);
        }
    }
    let mut arena = pool.begin(view).unwrap();
    let copied = arena.patch(old, PAGE, 8, &[3; 8], PAGE).unwrap();
    assert_ne!(copied, old);
    let prepared = arena.freeze().unwrap();
    assert!(prepared.slots().all(|s| s.copy_source.is_none()));
}

#[test]
fn private_repatch_abort_and_generation_gaps_keep_original_source_and_old_borrow() {
    let pool = pool(4, 1);
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let old = arena.allocate(&[1; PAGE], PAGE).unwrap();
    let view = arena.freeze().unwrap().activate();
    let held = view.read(old, PAGE).unwrap();
    let mut arena = pool.begin(view.clone()).unwrap();
    let abandoned = arena.patch(old, PAGE, 3, &[2; 8], PAGE).unwrap();
    drop(arena);
    let mut arena = pool.begin(view.clone()).unwrap();
    assert!(arena.read(abandoned, PAGE).is_err());
    let copied = arena.patch(old, PAGE, 3, &[2; 8], PAGE).unwrap();
    assert_ne!(copied, abandoned);
    let revision = arena.revision();
    assert_eq!(arena.patch(copied, PAGE, 3, &[2; 8], PAGE).unwrap(), copied);
    assert_eq!(arena.revision(), revision);
    assert_eq!(
        arena.patch(copied, PAGE, PAGE - 8, &[5; 8], PAGE).unwrap(),
        copied
    );
    let prepared = arena.freeze().unwrap();
    let (slot, generation, _) = unpack(old).unwrap();
    let versions: Vec<_> = prepared.slots().collect();
    assert_eq!(versions.len(), 1);
    assert_eq!(
        versions[0].copy_source,
        Some(VersionKey { slot, generation })
    );
    assert_eq!(&versions[0].bytes[3..11], &[2; 8]);
    assert_eq!(&versions[0].bytes[PAGE - 8..], &[5; 8]);
    assert_eq!(held, &[1; PAGE]);
    assert!(view.read(abandoned, PAGE).is_err());
}

#[test]
fn exhaustion_and_revision_overflow_preserve_original_failure_order() {
    let pool = pool(1, 1);
    let mut arena = pool.begin(pool.empty().unwrap()).unwrap();
    let old = arena.allocate(&[1; PAGE], PAGE).unwrap();
    let view = arena.freeze().unwrap().activate();
    check_case(&pool, &view, old, PAGE, 0, &[2], PAGE);
    let mut expected = pool.begin(view.clone()).unwrap();
    let mut actual = pool.begin(view.clone()).unwrap();
    expected.revision = u64::MAX;
    actual.revision = u64::MAX;
    let a = original_patch(&mut expected, old, PAGE, 0, &[2], PAGE).unwrap_err();
    let b = actual.patch(old, PAGE, 0, &[2], PAGE).unwrap_err();
    assert_eq!(a.to_string(), b.to_string());
    assert_eq!(actual.revision(), u64::MAX);
    assert_eq!(view.read(old, PAGE).unwrap(), &[1; PAGE]);
}
