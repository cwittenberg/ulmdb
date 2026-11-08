use super::*;
use crate::range::pack::io::ReadAt;
use std::io;
fn stream() -> StreamId {
    StreamId::new([7; 16]).unwrap()
}
fn genesis() -> (Frame, Block) {
    encode_frame_header(stream(), 0, 0, 0, None, [3; 32], Sha256::digest([]).into()).unwrap()
}
#[test]
fn global_head_and_envelope_roundtrip_preserve_full_receipts() {
    let global = Global { stream: stream() };
    assert_eq!(decode_global(&encode_global(global)).unwrap(), global);
    let (first, header) = genesis();
    assert_eq!(first.receipt.start(), 0);
    assert_eq!(first.receipt.end(), 65536);
    assert_eq!(
        decode_frame_header_bound(&header, first.receipt.binding()).unwrap(),
        first
    );
    assert_eq!(
        decode_head(&encode_head(stream(), first.receipt), stream()).unwrap(),
        first.receipt
    );
    let footer = encode_frame_footer(stream(), &first);
    decode_frame_footer(&footer, stream(), &first).unwrap();
    let (next, bytes) = encode_frame_header(
        stream(),
        first.receipt.end(),
        1,
        17,
        Some(first.receipt),
        [3; 32],
        Sha256::digest([9; 17]).into(),
    )
    .unwrap();
    let decoded = decode_frame_header(&bytes, stream()).unwrap();
    assert_eq!(decoded.parent, Some(first.receipt));
    assert_eq!(decoded, next);
    assert_eq!(
        decoded.receipt.frame_descriptor(),
        decoded.receipt.descriptor()
    );
    assert_eq!(decoded.receipt.payload_length(), 17);
    assert_ne!(decoded.receipt.hash(), first.receipt.hash());
    assert!(decode_frame_footer(&footer, stream(), &next).is_err());
    let other = StreamId::new([8; 16]).unwrap();
    assert!(decode_head(&encode_head(stream(), first.receipt), other).is_err());
    assert!(decode_frame_header(&header, other).is_err());
    assert!(
        decode_frame_header_bound(
            &header,
            ChunkBinding {
                header_hash: [4; 32],
                ..first.receipt.binding()
            }
        )
        .is_err()
    );
    assert!(
        encode_frame_header(
            other,
            65536,
            1,
            0,
            Some(first.receipt),
            [3; 32],
            Sha256::digest([]).into()
        )
        .is_err()
    );
}
#[test]
fn every_block_byte_and_all_short_lengths_fail_closed() {
    let (frame, header) = genesis();
    let global = encode_global(Global { stream: stream() });
    let head = encode_head(stream(), frame.receipt);
    let footer = encode_frame_footer(stream(), &frame);
    for index in 0..BLOCK {
        let mut b = global;
        b[index] ^= 1;
        assert!(decode_global(&b).is_err());
        let mut b = head;
        b[index] ^= 1;
        assert!(decode_head(&b, stream()).is_err());
        let mut b = header;
        b[index] ^= 1;
        assert!(decode_frame_header(&b, stream()).is_err());
        let mut b = footer;
        b[index] ^= 1;
        assert!(decode_frame_footer(&b, stream(), &frame).is_err());
        assert!(decode_global(&global[..index]).is_err());
        assert!(decode_head(&head[..index], stream()).is_err());
        assert!(decode_frame_header(&header[..index], stream()).is_err());
        assert!(decode_frame_footer(&footer[..index], stream(), &frame).is_err());
    }
    let oversized = vec![0; BLOCK + 1];
    assert!(decode_global(&oversized).is_err());
    assert!(decode_frame_header(&oversized, stream()).is_err());
}
#[test]
fn rehashed_reserved_geometry_parent_and_footer_mutants_reject() {
    let (first, header) = genesis();
    let (next, child) = encode_frame_header(
        stream(),
        65536,
        1,
        1,
        Some(first.receipt),
        [3; 32],
        Sha256::digest([5]).into(),
    )
    .unwrap();
    for at in [0, 8, 10, 12, 32, 40, 48, 56, 64, 72, 80, 88, 96, 4000] {
        let mut b = encode_global(Global { stream: stream() });
        b[at] ^= 0x80;
        seal(b"ULMDB/chunks/control/v7\0", &mut b);
        assert!(decode_global(&b).is_err(), "global {at}");
    }
    for at in [0, 8, 10, 12, 16, 32, 40, 48, 56, 128, 4000] {
        let mut b = encode_head(stream(), first.receipt);
        b[at] ^= 0x80;
        seal(b"ULMDB/chunks/head/v7\0", &mut b);
        assert!(decode_head(&b, stream()).is_err(), "head {at}");
    }
    for at in [0, 8, 10, 12, 16, 32, 40, 48, 96, 104, 112, 120, 224, 4000] {
        let mut b = child;
        b[at] ^= 0x80;
        seal(b"ULMDB/chunks/frame-header/v7\0", &mut b);
        assert!(decode_frame_header(&b, stream()).is_err(), "frame {at}");
    }
    let mut b = child;
    b[160] ^= 1;
    seal(b"ULMDB/chunks/frame-header/v7\0", &mut b);
    assert!(
        decode_frame_header(&b, stream()).is_err(),
        "samechunk parent headerhash"
    );
    // Parent frame hash/payload hash are preserved metadata; only recovery can
    // compare them with actual parent/payload bytes. No codec authority claim.
    let mut b = child;
    b[128] ^= 1;
    seal(b"ULMDB/chunks/frame-header/v7\0", &mut b);
    let decoded = decode_frame_header(&b, stream()).unwrap();
    assert_ne!(decoded.parent, Some(first.receipt));
    let mut b = encode_frame_footer(stream(), &next);
    b[224] ^= 1;
    seal(b"ULMDB/chunks/frame-footer/v7\0", &mut b);
    assert!(decode_frame_footer(&b, stream(), &next).is_err());
    let mut old = header;
    old[..8].copy_from_slice(b"ULMDBPF1");
    seal(b"ULMDB/chunks/frame-header/v7\0", &mut old);
    assert!(decode_frame_header(&old, stream()).is_err());
}
#[test]
fn canonical_continuation_and_payload_extent_boundaries() {
    let (first, _) = genesis();
    let empty: [u8; 32] = Sha256::digest([]).into();
    assert!(
        encode_frame_header(stream(), 131072, 1, 0, Some(first.receipt), [3; 32], empty).is_err()
    );
    assert!(
        encode_frame_header(stream(), 65536, 2, 0, Some(first.receipt), [3; 32], empty).is_err()
    );
    assert!(
        encode_frame_header(stream(), 65536, 1, 0, Some(first.receipt), [4; 32], empty).is_err()
    );
    assert!(encode_frame_header(stream(), 0, 0, 1, None, [3; 32], empty).is_err());
    assert!(
        encode_frame_header(
            stream(),
            65536,
            1,
            geometry::MAX_PAYLOAD,
            Some(first.receipt),
            [3; 32],
            empty
        )
        .is_ok()
    );
    assert!(
        encode_frame_header(
            stream(),
            65536,
            1,
            geometry::MAX_PAYLOAD + 1,
            Some(first.receipt),
            [3; 32],
            empty
        )
        .is_err()
    );
    // Metadata-only rollover arithmetic fixture, not selected/source authority.
    let start = geometry::DATA_BYTES - geometry::ALIGN_BYTES;
    let parent = Receipt::checked(
        stream(),
        start,
        geometry::DATA_BYTES,
        start / geometry::ALIGN_BYTES,
        0,
        [5; 32],
        [3; 32],
    )
    .unwrap();
    let (rolled, bytes) = encode_frame_header(
        stream(),
        geometry::DATA_BYTES,
        parent.ordinal() + 1,
        0,
        Some(parent),
        [9; 32],
        empty,
    )
    .unwrap();
    assert_eq!(rolled.receipt.binding().index, 1);
    assert_eq!(decode_frame_header(&bytes, stream()).unwrap(), rolled);
    assert!(
        encode_frame_header(
            stream(),
            geometry::DATA_BYTES * 2,
            parent.ordinal() + 1,
            0,
            Some(parent),
            [9; 32],
            empty
        )
        .is_err()
    );
    assert!(
        Receipt::checked(stream(), u64::MAX - 65535, u64::MAX, 1, 0, [5; 32], [3; 32]).is_err()
    );
}
struct Partial<'a>(&'a [u8]);
impl ReadAt for Partial<'_> {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        let offset = usize::try_from(offset).map_err(|_| io::ErrorKind::InvalidInput)?;
        let source = self.0.get(offset..).ok_or(io::ErrorKind::UnexpectedEof)?;
        let count = buf.len().min(source.len()).min(19);
        buf[..count].copy_from_slice(&source[..count]);
        Ok(count)
    }
}
#[test]
fn streamed_payload_padding_short_reads_and_callback_errors() {
    let (first, _) = genesis();
    let payload = [0x71; 17];
    let (frame, _) = encode_frame_header(
        stream(),
        65536,
        1,
        17,
        Some(first.receipt),
        [3; 32],
        Sha256::digest(payload).into(),
    )
    .unwrap();
    let mut data = vec![0; frame.receipt.end() as usize];
    let start = frame.receipt.start() as usize + BLOCK;
    data[start..start + 17].copy_from_slice(&payload);
    validate_frame_payload(&Partial(&data), &frame).unwrap();
    data[start] ^= 1;
    assert!(validate_frame_payload(&Partial(&data), &frame).is_err());
    data[start] ^= 1;
    for at in [start + 17, frame.receipt.end() as usize - BLOCK - 1] {
        data[at] = 1;
        assert!(validate_frame_payload(&Partial(&data), &frame).is_err());
        data[at] = 0;
    }
    assert!(validate_frame_payload(&Partial(&data[..start + 8]), &frame).is_err());
    assert!(matches!(
        validate_payload_with(&frame, |_, _| Err(Error::Busy)),
        Err(Error::Busy)
    ));
}
