use super::*;
use sha2::{Digest, Sha256};
fn fixture() -> (Global, Frame, [Block; 4]) {
    let global = Global::initial([7; 16]);
    let (frame, header) = encode_frame_header(
        global.file_id,
        crate::range::pack::DATA_START,
        0,
        0,
        None,
        Sha256::digest([]).into(),
    )
    .unwrap();
    let blocks = [
        encode_global(&global),
        encode_head(global.file_id, frame.receipt),
        header,
        encode_frame_footer(global.file_id, &frame),
    ];
    (global, frame, blocks)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
#[test]
fn independently_constructed_python_sha_vectors_fix_all_record_domains_and_offsets() {
    // Python stdlib hashlib/struct independently built each4096-byte record
    // from the published offsets and file_id=[7;16], then hashed domain+[:4064].
    let (global, frame, blocks) = fixture();
    for (block, expected) in blocks.iter().zip([
        "61b0943d6480c329a4f3ef3456661e415b5256eccde334d250332d3873a053f2",
        "e9042a39c0dd0edb7cde987b5cf35707bf480a3c69c6f87d8a8b1eba02cce128",
        "707640fb6b6e4dd7c0cb387b49fb35e3f3e284c27c09ae39bf3d8f71d028aefd",
        "c4fe0dd1e32a3edda3df120e356a9daee9f738c9cda6c9b364bb559e191c808c",
    ]) {
        assert_eq!(hex(&block[CHECKSUM..]), expected);
    }
    assert_eq!(decode_global(&blocks[0]).unwrap(), global);
    assert_eq!(
        decode_head(&blocks[1], global.file_id).unwrap(),
        frame.receipt
    );
    assert_eq!(
        decode_frame_header(&blocks[2], global.file_id).unwrap(),
        frame
    );
    decode_frame_footer(&blocks[3], global.file_id, &frame).unwrap();
}
#[test]
fn every_record_byte_is_covered_by_its_checksum() {
    let (global, frame, blocks) = fixture();
    for (kind, original) in blocks.iter().enumerate() {
        for offset in 0..BLOCK {
            let mut changed = *original;
            changed[offset] ^= 1 << (offset % 8);
            let bad = match kind {
                0 => decode_global(&changed).is_err(),
                1 => decode_head(&changed, global.file_id).is_err(),
                2 => decode_frame_header(&changed, global.file_id).is_err(),
                _ => decode_frame_footer(&changed, global.file_id, &frame).is_err(),
            };
            assert!(bad, "kind={kind} offset={offset}");
        }
    }
}
#[test]
fn correct_sha_does_not_bypass_reserved_fields_type_identity_or_geometry_checks() {
    let (global, frame, blocks) = fixture();
    for (kind, reserved, domain) in [
        (0, 72, b"ULMDB/range-env/global/v5\0".as_slice()),
        (1, 88, b"ULMDB/packed/head/v1\0"),
        (2, 152, b"ULMDB/packed/frame-header/v1\0"),
        (3, 184, b"ULMDB/packed/frame-footer/v1\0"),
    ] {
        for offset in [12, reserved] {
            let mut changed = blocks[kind];
            changed[offset] = 1;
            checksum::seal(domain, &mut changed);
            let bad = match kind {
                0 => decode_global(&changed).is_err(),
                1 => decode_head(&changed, global.file_id).is_err(),
                2 => decode_frame_header(&changed, global.file_id).is_err(),
                _ => decode_frame_footer(&changed, global.file_id, &frame).is_err(),
            };
            assert!(bad);
        }
    }
    for offset in [32, 40, 48, 56, 64] {
        let mut changed = blocks[0];
        changed[offset] ^= 1;
        checksum::seal(b"ULMDB/range-env/global/v5\0", &mut changed);
        assert!(decode_global(&changed).is_err());
    }
    let (next, header) = encode_frame_header(
        global.file_id,
        frame.receipt.frame_end().unwrap(),
        1,
        1,
        Some(frame.receipt),
        Sha256::digest(b"x").into(),
    )
    .unwrap();
    for field in [32, 40, 48, 56, 64, 72, 80] {
        let mut changed = header;
        changed[field..field + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        checksum::seal(b"ULMDB/packed/frame-header/v1\0", &mut changed);
        assert!(
            std::panic::catch_unwind(|| decode_frame_header(&changed, global.file_id))
                .unwrap()
                .is_err()
        );
    }
    let mut no_parent = header;
    no_parent[64..120].fill(0);
    checksum::seal(b"ULMDB/packed/frame-header/v1\0", &mut no_parent);
    assert!(decode_frame_header(&no_parent, global.file_id).is_err());
    let mut footer = encode_frame_footer(global.file_id, &next);
    footer[152] ^= 1;
    checksum::seal(b"ULMDB/packed/frame-footer/v1\0", &mut footer);
    assert!(decode_frame_footer(&footer, global.file_id, &next).is_err());
    assert!(decode_head(&blocks[1], [8; 16]).is_err());
    assert!(decode_frame_header(&header, [8; 16]).is_err());
}
#[test]
fn overflowed_aligned_head_extent_is_rejected_without_panic_and_chains_cannot_skip() {
    let (global, frame, blocks) = fixture();
    let mut head = blocks[1];
    head[32..40].copy_from_slice(&(!(crate::range::pack::ALIGNMENT - 1)).to_le_bytes());
    head[40..48].copy_from_slice(&1u64.to_le_bytes());
    checksum::seal(b"ULMDB/packed/head/v1\0", &mut head);
    assert!(
        std::panic::catch_unwind(|| decode_head(&head, global.file_id))
            .unwrap()
            .is_err()
    );
    assert!(
        encode_frame_header(
            global.file_id,
            frame.receipt.frame_end().unwrap() + crate::range::pack::ALIGNMENT,
            1,
            1,
            Some(frame.receipt),
            Sha256::digest(b"x").into()
        )
        .is_err()
    );
    assert!(
        encode_frame_header(
            global.file_id,
            frame.receipt.frame_end().unwrap(),
            2,
            1,
            Some(frame.receipt),
            Sha256::digest(b"x").into()
        )
        .is_err()
    );
}
