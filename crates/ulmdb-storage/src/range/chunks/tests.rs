//! Actual core arithmetic, real chunk files and bounded-source controls.
use super::{geometry::*, header::*, source::*};
use crate::{Error, Result};
use std::{
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let p =
            std::env::temp_dir().join(format!("ulmdb-chunk7-{}", crate::new_identity().unwrap()));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn stream() -> StreamId {
    StreamId::new([7; 16]).unwrap()
}
fn file(dir: &Path, header: &ChunkHeader, used: u64, marker: u8) {
    let mut f = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(dir.join(header.filename()))
        .unwrap();
    f.write_all(&header.encode()).unwrap();
    f.set_len(HEADER_BYTES + used).unwrap();
    f.seek(SeekFrom::Start(HEADER_BYTES)).unwrap();
    f.write_all(&[marker; 16]).unwrap();
    f.sync_all().unwrap();
}
fn rehash(b: &mut [u8]) {
    use sha2::{Digest, Sha256};
    let n = b.len() - 32;
    let mut h = Sha256::new();
    h.update(b"ULMDB/chunk-envelope/v7\0");
    h.update(&b[..n]);
    b[n..].copy_from_slice(&h.finalize());
}
#[test]
fn real_envelope_and_full_parent_continuation_checks() {
    let old = ChunkHeader::new(stream(), 0, None).unwrap();
    let parent = FrameDescriptor::new(
        DATA_BYTES - 2 * ALIGN_BYTES,
        DATA_BYTES - ALIGN_BYTES,
        1,
        [2; 32],
    )
    .unwrap();
    let plan = AppendPlan::after(parent, ALIGN_BYTES).unwrap();
    assert!(plan.rollover);
    assert_eq!(plan.start, DATA_BYTES);
    assert_eq!(plan.unallocated_tail, Some((parent.end(), DATA_BYTES)));
    let next = ChunkHeader::new(
        stream(),
        1,
        Some(Continuation {
            prior_header_hash: old.binding().header_hash,
            parent,
        }),
    )
    .unwrap();
    assert_eq!(ChunkHeader::decode(&next.encode()).unwrap(), next);
    validate_continuation(
        &old,
        parent,
        HEADER_BYTES + parent.end(),
        &next,
        plan.start,
        plan.end - plan.start,
    )
    .unwrap();
    for wrong_eof in [
        HEADER_BYTES + parent.end() - 1,
        HEADER_BYTES + parent.end() + 1,
    ] {
        assert!(
            validate_continuation(
                &old,
                parent,
                wrong_eof,
                &next,
                plan.start,
                plan.end - plan.start
            )
            .is_err()
        );
    }
    assert!(
        validate_continuation(
            &old,
            parent,
            HEADER_BYTES + parent.end(),
            &next,
            plan.start,
            ALIGN_BYTES
        )
        .is_err(),
        "unnecessary rollover must reject"
    );
    let wrong_parent =
        FrameDescriptor::new(parent.start(), parent.end(), parent.ordinal(), [9; 32]).unwrap();
    assert!(
        validate_continuation(
            &old,
            wrong_parent,
            HEADER_BYTES + parent.end(),
            &next,
            plan.start,
            plan.end - plan.start
        )
        .is_err()
    );
    assert!(
        ChunkHeader::new(
            stream(),
            2,
            Some(Continuation {
                prior_header_hash: old.binding().header_hash,
                parent
            })
        )
        .is_err()
    );
    let wrong_header = ChunkHeader::new(
        stream(),
        1,
        Some(Continuation {
            prior_header_hash: [8; 32],
            parent,
        }),
    )
    .unwrap();
    assert!(
        validate_continuation(
            &old,
            parent,
            HEADER_BYTES + parent.end(),
            &wrong_header,
            plan.start,
            plan.end - plan.start
        )
        .is_err()
    );
    let original = next.encode();
    for at in [
        0, 8, 10, 12, 16, 32, 40, 48, 56, 64, 80, 88, 120, 152, 65535,
    ] {
        let mut b = original;
        b[at] ^= 1;
        assert!(ChunkHeader::decode(&b).is_err(), "byte {at}");
    }
    let mut malformed = original;
    malformed[40..48].copy_from_slice(&(DATA_BYTES + 1).to_le_bytes());
    rehash(&mut malformed);
    assert!(ChunkHeader::decode(&malformed).is_err());
}
#[test]
fn exact_eof_append_bound_empty_endpoints_and_old_fd_owner() {
    let dir = Dir::new();
    let h = ChunkHeader::new(stream(), 0, None).unwrap();
    file(&dir.0, &h, ALIGN_BYTES, 3);
    let lease =
        ChunkLease::open(&dir.0, h.binding(), ALIGN_BYTES, ExtentMode::SealedExact).unwrap();
    assert_eq!(lease.physical_span(0, 16).unwrap(), (HEADER_BYTES, 16));
    let mut bytes = [0; 16];
    assert_eq!(lease.read_at(&mut bytes, 0).unwrap(), 16);
    assert_eq!(bytes, [3; 16]);
    assert_eq!(lease.read_at(&mut bytes, ALIGN_BYTES).unwrap(), 0);
    assert!(lease.physical_span(ALIGN_BYTES, 1).is_err());
    let f = OpenOptions::new()
        .write(true)
        .open(dir.0.join(h.filename()))
        .unwrap();
    f.set_len(HEADER_BYTES + 2 * ALIGN_BYTES).unwrap();
    assert!(ChunkLease::open(&dir.0, h.binding(), ALIGN_BYTES, ExtentMode::SealedExact).is_err());
    let old =
        ChunkLease::open(&dir.0, h.binding(), ALIGN_BYTES, ExtentMode::AppendSnapshot).unwrap();
    assert_eq!(old.read_at(&mut bytes, ALIGN_BYTES).unwrap(), 0);
    let full = ChunkHeader::new(
        stream(),
        1,
        Some(Continuation {
            prior_header_hash: h.binding().header_hash,
            parent: FrameDescriptor::new(0, ALIGN_BYTES, 0, [1; 32]).unwrap(),
        }),
    )
    .unwrap();
    file(&dir.0, &full, DATA_BYTES, 5);
    let full_lease = ChunkLease::open(
        &dir.0,
        full.binding(),
        2 * DATA_BYTES,
        ExtentMode::SealedExact,
    )
    .unwrap();
    assert_eq!(
        full_lease.physical_span(2 * DATA_BYTES, 0).unwrap(),
        (HEADER_BYTES + DATA_BYTES, 0)
    );
    assert!(full_lease.physical_span(2 * DATA_BYTES, 1).is_err());
    let mut wrong = h.binding();
    wrong.header_hash[0] ^= 1;
    assert!(ChunkLease::open(&dir.0, wrong, ALIGN_BYTES, ExtentMode::AppendSnapshot).is_err());
    fs::remove_file(dir.0.join(h.filename())).unwrap();
    assert_eq!(old.read_at(&mut bytes, 0).unwrap(), 16);
    assert_eq!(bytes, [3; 16]);
}
struct Pair([ChunkLease; 2]);
impl ChunkResolve for Pair {
    fn lease(&self, index: u64) -> Result<ChunkLease> {
        self.0
            .iter()
            .find(|lease| lease.header().index() == index)
            .cloned()
            .ok_or(Error::Corrupt("test missing chunk"))
    }
}
#[test]
fn existing_readat_consumer_rejects_rollover_gaps_and_foreign_streams() {
    let dir = Dir::new();
    let first = ChunkHeader::new(stream(), 0, None).unwrap();
    file(&dir.0, &first, ALIGN_BYTES, 11);
    let parent = FrameDescriptor::new(0, ALIGN_BYTES, 0, [1; 32]).unwrap();
    let second = ChunkHeader::new(
        stream(),
        1,
        Some(Continuation {
            prior_header_hash: first.binding().header_hash,
            parent,
        }),
    )
    .unwrap();
    file(&dir.0, &second, ALIGN_BYTES, 22);
    let a = ChunkLease::open(
        &dir.0,
        first.binding(),
        ALIGN_BYTES,
        ExtentMode::SealedExact,
    )
    .unwrap();
    let b = ChunkLease::open(
        &dir.0,
        second.binding(),
        DATA_BYTES + ALIGN_BYTES,
        ExtentMode::SealedExact,
    )
    .unwrap();
    let source = PackRead::new(Pair([a.clone(), b]), stream(), DATA_BYTES + ALIGN_BYTES).unwrap();
    let mut bytes = [0; 16];
    super::super::pack::io::read_exact_at(&source, &mut bytes, 0).unwrap();
    assert_eq!(bytes, [11; 16]);
    super::super::pack::io::read_exact_at(&source, &mut bytes, DATA_BYTES).unwrap();
    assert_eq!(bytes, [22; 16]);
    assert!(source.read_at(&mut bytes, ALIGN_BYTES).is_err());
    assert_eq!(
        source.read_at(&mut bytes, source.selected_end()).unwrap(),
        0
    );
    let foreign = PackRead::new(a, StreamId::new([8; 16]).unwrap(), ALIGN_BYTES).unwrap();
    assert!(foreign.read_at(&mut bytes, 0).is_err());
}
#[test]
fn logical_positions_above_one_tib_use_only_small_physical_file_offsets() {
    let dir = Dir::new();
    let index = 2048;
    let base = index * DATA_BYTES;
    let parent = FrameDescriptor::new(
        base - DATA_BYTES,
        base - DATA_BYTES + ALIGN_BYTES,
        1,
        [2; 32],
    )
    .unwrap();
    let h = ChunkHeader::new(
        stream(),
        index,
        Some(Continuation {
            prior_header_hash: [3; 32],
            parent,
        }),
    )
    .unwrap();
    file(&dir.0, &h, ALIGN_BYTES, 33);
    let lease = ChunkLease::open(
        &dir.0,
        h.binding(),
        base + ALIGN_BYTES,
        ExtentMode::SealedExact,
    )
    .unwrap();
    let source = PackRead::new(lease, stream(), base + ALIGN_BYTES).unwrap();
    let mut bytes = [0; 16];
    super::super::pack::io::read_exact_at(&source, &mut bytes, base).unwrap();
    assert_eq!(bytes, [33; 16]);
    assert!(base > 1 << 40);
    assert_eq!(locate(base).1, HEADER_BYTES);
    assert!(HEADER_BYTES + DATA_BYTES < i32::MAX as u64);
    assert!(FrameDescriptor::new(u64::MAX - ALIGN_BYTES + 1, u64::MAX, 1, [0; 32]).is_err());
    assert!(AppendPlan::after(parent, MAX_PAYLOAD + 1).is_err());
}
