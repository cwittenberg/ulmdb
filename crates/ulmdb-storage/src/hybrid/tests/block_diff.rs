//! Scalar equivalence controls, including exact minimal contiguous boundaries.
use super::{MAX_OBJECT_BYTES, MAX_PATCHES, Patch, copy_bytes, final_patches};
use crate::{Error, Result};
fn scalar(before: &[u8], after: &[u8]) -> Result<Vec<Patch>> {
    if before.len() != after.len() || after.len() > MAX_OBJECT_BYTES as usize {
        return Err(Error::InvalidInput("hybrid diff widths"));
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < after.len() {
        if before[i] == after[i] {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < after.len() && before[i] != after[i] {
            i += 1;
        }
        if out.len() == MAX_PATCHES {
            return Err(Error::LimitExceeded);
        }
        out.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
        out.push(Patch {
            offset: start as u32,
            bytes: copy_bytes(&after[start..i])?,
        });
    }
    Ok(out)
}
fn check(before: &[u8], after: &[u8]) {
    let expected = scalar(before, after);
    let actual = final_patches(before, after);
    match (expected, actual) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a, b);
            let mut replay = before.to_vec();
            for (i, p) in b.iter().enumerate() {
                let start = p.offset as usize;
                let end = start + p.bytes.len();
                assert!(!p.bytes.is_empty());
                assert!(
                    before[start..end]
                        .iter()
                        .zip(&after[start..end])
                        .all(|(a, b)| a != b)
                );
                assert!(start == 0 || before[start - 1] == after[start - 1]);
                assert!(end == after.len() || before[end] == after[end]);
                if i > 0 {
                    let prior = &b[i - 1];
                    assert!((prior.offset as usize + prior.bytes.len()) < start);
                }
                replay[start..end].copy_from_slice(&p.bytes);
            }
            assert_eq!(replay, after);
        }
        (Err(Error::LimitExceeded), Err(Error::LimitExceeded)) => {}
        (Err(Error::InvalidInput(a)), Err(Error::InvalidInput(b))) => assert_eq!(a, b),
        other => panic!("different result classes: {other:?}"),
    }
}
#[test]
fn exhaustive_small_binary_pairs() {
    for n in 0..=8 {
        for mask in 0..(1usize << (2 * n)) {
            let mut before = [0; 8];
            let mut after = [0; 8];
            for i in 0..n {
                before[i] = ((mask >> i) & 1) as u8;
                after[i] = ((mask >> (n + i)) & 1) as u8;
            }
            check(&before[..n], &after[..n]);
        }
    }
}
#[test]
fn exhaustive_three_byte_alphabet() {
    let alphabet = [0, 1, 255];
    for n in 0..=5u32 {
        let bound = 3usize.pow(n);
        for mut mask in 0..bound * bound {
            let mut before = [0; 5];
            let mut after = [0; 5];
            for i in 0..n as usize {
                before[i] = alphabet[mask % 3];
                mask /= 3;
                after[i] = alphabet[mask % 3];
                mask /= 3;
            }
            check(&before[..n as usize], &after[..n as usize]);
        }
    }
}
fn next(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}
#[test]
fn randomized_sparse_dense_and_unaligned() {
    let mut seed = 0x639a59b402387e15;
    for case in 0..12000 {
        let n = (next(&mut seed) % 8193) as usize;
        let bpad = (next(&mut seed) % 32) as usize;
        let apad = (next(&mut seed) % 32) as usize;
        let mut before = vec![0; bpad + n];
        let mut after = vec![0; apad + n];
        for i in 0..n {
            let v = next(&mut seed) as u8;
            before[bpad + i] = v;
            after[apad + i] = v;
            if next(&mut seed).is_multiple_of(match case % 5 {
                0 => 4096,
                1 => 64,
                2 => 2,
                3 => 1,
                _ => 17,
            }) {
                after[apad + i] ^= (next(&mut seed) as u8) | 1;
            }
        }
        check(&before[bpad..], &after[apad..]);
    }
}
#[test]
fn cacheline_group_and_patch_edges() {
    for n in [
        0, 1, 2, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 129, 255, 256, 257, 4095, 4096, 4097,
    ] {
        let before = vec![0x17; n];
        check(&before, &before);
        for start in [
            0,
            1,
            15,
            16,
            31,
            32,
            63,
            64,
            65,
            127,
            128,
            n.saturating_sub(1),
        ] {
            for length in [1, 2, 7, 8, 15, 16, 31, 32, 33, 63, 64, 65, 127, 128] {
                if start + length > n {
                    continue;
                }
                let mut after = before.clone();
                after[start..start + length].fill(0x91);
                check(&before, &after);
                if start + length + 1 < n {
                    after[start + length + 1] ^= 1;
                    check(&before, &after);
                }
            }
        }
    }
}
#[test]
fn scalar_error_and_patch_limit_boundaries() {
    check(&[0], &[0, 0]);
    let n = MAX_PATCHES * 2;
    let before = vec![0; n + 1];
    let mut after = before.clone();
    for i in (0..n).step_by(2) {
        after[i] = 1;
    }
    check(&before[..n], &after[..n]);
    after[n] = 1;
    check(&before, &after);
    assert!(matches!(
        final_patches(&before, &after),
        Err(Error::LimitExceeded)
    ));
}
#[test]
fn maximum_legal_payload_object_is_accepted() {
    let before = vec![0; MAX_OBJECT_BYTES as usize];
    assert!(final_patches(&before, &before).unwrap().is_empty());
    let too_long = vec![0; MAX_OBJECT_BYTES as usize + 1];
    assert!(matches!(
        final_patches(&too_long, &too_long),
        Err(Error::InvalidInput("hybrid diff widths"))
    ));
}
