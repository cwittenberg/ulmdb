//! Differential execution against the frozen original patch body.
use super::*;
use crate::resident::{Config, Pool, View};
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
        #[cfg(feature = "resident-direct-page-patch")]
        if object_len == PAGE && alignment == PAGE {
            return arena.direct_page_patch(old, offset, replacement);
        }
        let mut bytes = bytes.to_vec();
        bytes[offset..end].copy_from_slice(replacement);
        arena.allocate_copy(old, &bytes, alignment)
    }
}

fn pool() -> Arc<Pool> {
    Pool::create(Config {
        kv_pages: 8,
        tiny_pages: 8,
        max_reserved_bytes: 1 << 24,
        max_metadata_bytes: 1 << 24,
        max_views: 32,
    })
    .unwrap()
}
struct PatchCase<'a> {
    object_len: usize,
    offset: usize,
    replacement: &'a [u8],
    alignment: usize,
}
fn compare(
    pool: &Arc<Pool>,
    base: &View,
    old: Option<Reference>,
    tiny: bool,
    patch: PatchCase<'_>,
) {
    let PatchCase {
        object_len,
        offset,
        replacement,
        alignment,
    } = patch;
    let mut expected = pool.begin(base.clone()).unwrap();
    let mut actual = pool.begin(base.clone()).unwrap();
    let (a, b) = if let Some(old) = old {
        (old, old)
    } else {
        if tiny {
            expected.allocate(&[8; 16], 16).unwrap();
            actual.allocate(&[8; 16], 16).unwrap();
        }
        let length = if tiny { 128 } else { PAGE };
        let alignment = if tiny { 16 } else { PAGE };
        (
            expected.allocate(&vec![17; length], alignment).unwrap(),
            actual.allocate(&vec![17; length], alignment).unwrap(),
        )
    };
    let x = original_patch(&mut expected, a, object_len, offset, replacement, alignment);
    let y = actual.patch(b, object_len, offset, replacement, alignment);
    assert_eq!(expected.revision(), actual.revision());
    match (x, y) {
        (Ok(x), Ok(y)) => {
            assert_eq!(x == a, y == b);
            assert_eq!(
                expected.read(x, object_len).unwrap(),
                actual.read(y, object_len).unwrap()
            );
        }
        (Err(x), Err(y)) => assert_eq!(x.to_string(), y.to_string()),
        (x, y) => panic!("original{x:?}; fused{y:?}"),
    }
    let x = expected.freeze().unwrap();
    let y = actual.freeze().unwrap();
    let shape = |p: &Prepared| {
        p.slots()
            .map(|s| (s.ranges.to_vec(), s.bytes.to_vec(), s.copy_source))
            .collect::<Vec<_>>()
    };
    assert_eq!(shape(&x), shape(&y));
}
#[test]
fn shared_private_tiny_noops_and_failures_match_original() {
    let pool = pool();
    let empty = pool.empty().unwrap();
    let mut a = pool.begin(empty.clone()).unwrap();
    let old = a.allocate(&[17; PAGE], PAGE).unwrap();
    let view = a.freeze().unwrap().activate();
    let held = view.read(old, PAGE).unwrap();
    for (reference, tiny, len) in [
        (Some(old), false, PAGE),
        (None, false, PAGE),
        (None, true, 128),
    ] {
        for alignment in [0, 1, 3, 16, PAGE, PAGE * 2] {
            for object_len in [len - 1, len, len + 1] {
                for offset in [0, 1, len / 2, len, len + 1, usize::MAX] {
                    for bytes in [&[][..], &[17; 8][..], &[73; 8][..]] {
                        compare(
                            &pool,
                            &view,
                            reference,
                            tiny,
                            PatchCase {
                                object_len,
                                offset,
                                replacement: bytes,
                                alignment,
                            },
                        );
                    }
                }
            }
        }
    }
    assert_eq!(held, [17; PAGE]);
}
#[test]
fn stale_generation_foreign_segment_quarantine_and_revision_overflow_match_original() {
    let pool = pool();
    let empty = pool.empty().unwrap();
    let mut a = pool.begin(empty).unwrap();
    let old = a.allocate(&[17; PAGE], PAGE).unwrap();
    let view = a.freeze().unwrap().activate();
    for reference in [
        Reference::NULL,
        Reference {
            segment: old.segment + 1,
            ..old
        },
        Reference {
            offset: old.offset + (1 << 28),
            ..old
        },
        Reference {
            offset: old.offset + 1,
            ..old
        },
    ] {
        compare(
            &pool,
            &view,
            Some(reference),
            false,
            PatchCase {
                object_len: PAGE,
                offset: 0,
                replacement: &[9],
                alignment: PAGE,
            },
        );
    }
    let mut a = pool.begin(view.clone()).unwrap();
    a.revision = u64::MAX;
    let mut b = pool.begin(view.clone()).unwrap();
    b.revision = u64::MAX;
    assert_eq!(
        original_patch(&mut a, old, PAGE, 0, &[9], PAGE)
            .unwrap_err()
            .to_string(),
        b.patch(old, PAGE, 0, &[9], PAGE).unwrap_err().to_string()
    );
    assert_eq!(a.revision(), b.revision());
    pool.quarantine();
    assert_eq!(
        original_patch(&mut a, old, PAGE, 0, &[9], PAGE)
            .unwrap_err()
            .to_string(),
        b.patch(old, PAGE, 0, &[9], PAGE).unwrap_err().to_string()
    );
}

#[test]
fn exhausted_pool_and_private_revision_overflow_match_original() {
    let pool = Pool::create(Config {
        kv_pages: 1,
        tiny_pages: 0,
        max_reserved_bytes: 1 << 20,
        max_metadata_bytes: 1 << 24,
        max_views: 32,
    })
    .unwrap();
    let mut init = pool.begin(pool.empty().unwrap()).unwrap();
    let old = init.allocate(&[17; PAGE], PAGE).unwrap();
    let view = init.freeze().unwrap().activate();
    let mut a = pool.begin(view.clone()).unwrap();
    let mut b = pool.begin(view.clone()).unwrap();
    assert_eq!(
        original_patch(&mut a, old, PAGE, 1, &[9], PAGE)
            .unwrap_err()
            .to_string(),
        b.patch(old, PAGE, 1, &[9], PAGE).unwrap_err().to_string()
    );
    assert_eq!(a.revision(), b.revision());
    assert_eq!(view.read(old, PAGE).unwrap(), [17; PAGE]);
    drop(a);
    drop(b);
    drop(view);
    let mut a = pool.begin(pool.empty().unwrap()).unwrap();
    let r = a.allocate(&[17; PAGE], PAGE).unwrap();
    a.revision = u64::MAX;
    assert_eq!(a.patch(r, PAGE, 0, &[17], PAGE).unwrap(), r);
    assert!(a.patch(r, PAGE, 0, &[9], PAGE).is_err());
    assert_eq!(a.read(r, PAGE).unwrap(), [17; PAGE]);
    assert_eq!(a.revision(), u64::MAX);
}

#[test]
fn private_stale_and_zero_allocation_spans_reject_like_original() {
    let pool = pool();
    let base = pool.empty().unwrap();
    for (delta, length) in [(0, 0), (128, 128), (1 << 28, 128)] {
        let mut expected = pool.begin(base.clone()).unwrap();
        let mut actual = pool.begin(base.clone()).unwrap();
        let a = expected.allocate(&[17; 128], 16).unwrap();
        let b = actual.allocate(&[17; 128], 16).unwrap();
        let bad_a = Reference {
            offset: a.offset + delta,
            ..a
        };
        let bad_b = Reference {
            offset: b.offset + delta,
            ..b
        };
        let x = original_patch(&mut expected, bad_a, length, 0, &[], 16);
        let y = actual.patch(bad_b, length, 0, &[], 16);
        assert_eq!(x.unwrap_err().to_string(), y.unwrap_err().to_string());
        assert_eq!(expected.revision(), actual.revision());
        assert_eq!(actual.read(b, 128).unwrap(), [17; 128]);
    }
}
