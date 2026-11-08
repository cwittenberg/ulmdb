//! Actual on-disk v7 admission/repair controls. These do not simulate power loss.
use super::{
    geometry,
    header::{self, ChunkHeader, StreamId},
    protocol,
    recovery::recover_checked,
};
use crate::{
    Error,
    range::pack::io::{read_exact_at, write_all_at},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    dir: PathBuf,
    control: File,
    chunk: File,
    header: ChunkHeader,
    current: protocol::Receipt,
    slot: usize,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "ulmdb-v7-recovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let stream = StreamId::new([0x51; 16]).unwrap();
        let header = ChunkHeader::new(stream, 0, None).unwrap();
        let chunk = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(dir.join(header.filename()))
            .unwrap();
        write_all_at(&chunk, &header.encode(), 0).unwrap();
        let (frame, bytes) = protocol::encode_frame_header(
            stream,
            0,
            0,
            0,
            None,
            header.binding().header_hash,
            Sha256::digest([]).into(),
        )
        .unwrap();
        Self::write_frame(&chunk, &frame, &bytes, &[]);
        let control = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(dir.join(protocol::CONTROL_FILE))
            .unwrap();
        control.set_len(protocol::CONTROL_LEN).unwrap();
        write_all_at(
            &control,
            &protocol::encode_global(protocol::Global { stream }),
            0,
        )
        .unwrap();
        for at in protocol::HEAD_OFFSETS {
            write_all_at(&control, &protocol::encode_head(stream, frame.receipt), at).unwrap();
        }
        Self {
            dir,
            control,
            chunk,
            header,
            current: frame.receipt,
            slot: 0,
        }
    }
    fn write_frame(file: &File, frame: &protocol::Frame, bytes: &[u8], payload: &[u8]) {
        let physical = geometry::locate(frame.receipt.start()).1;
        write_all_at(file, bytes, physical).unwrap();
        write_all_at(file, payload, physical + protocol::BLOCK as u64).unwrap();
        let end = geometry::HEADER_BYTES + frame.receipt.end()
            - frame.receipt.binding().index * geometry::DATA_BYTES;
        file.set_len(end).unwrap();
        write_all_at(
            file,
            &protocol::encode_frame_footer(frame.receipt.binding().stream, frame),
            end - protocol::BLOCK as u64,
        )
        .unwrap();
    }
    fn append(&mut self, payload: &[u8], publish: bool) -> protocol::Receipt {
        let plan =
            geometry::AppendPlan::after(self.current.descriptor(), payload.len() as u64).unwrap();
        assert!(!plan.rollover);
        let (frame, bytes) = protocol::encode_frame_header(
            self.header.stream(),
            plan.start,
            plan.ordinal,
            payload.len() as u64,
            Some(self.current),
            self.header.binding().header_hash,
            Sha256::digest(payload).into(),
        )
        .unwrap();
        Self::write_frame(&self.chunk, &frame, &bytes, payload);
        if publish {
            self.slot = 1 - self.slot;
            write_all_at(
                &self.control,
                &protocol::encode_head(self.header.stream(), frame.receipt),
                protocol::HEAD_OFFSETS[self.slot],
            )
            .unwrap();
            self.current = frame.receipt;
        }
        frame.receipt
    }
    fn bytes(&self) -> (Vec<u8>, Vec<u8>) {
        (
            fs::read(self.dir.join(protocol::CONTROL_FILE)).unwrap(),
            fs::read(self.dir.join(self.header.filename())).unwrap(),
        )
    }
}
#[test]
fn both_anchors_and_selected_graph_precede_tail_trim_and_reader_pins() {
    let mut f = Fixture::new();
    f.append(b"old", true);
    f.append(b"current", true);
    let selected = f.current;
    let old = recover_checked(&f.control, &f.dir, |_| Ok(())).unwrap();
    f.append(b"unselected complete bytes", false);
    let before = f.bytes();
    assert!(matches!(
        recover_checked(&f.control, &f.dir, |_| Err(Error::Corrupt("graph refused"))),
        Err(Error::Corrupt("graph refused"))
    ));
    assert_eq!(before, f.bytes());
    let result = recover_checked(&f.control, &f.dir, |ready| {
        assert_eq!(ready.selected, selected);
        assert_eq!(ready.chunks.chunk_count(), 1);
        Ok(())
    })
    .unwrap();
    assert_eq!(result.selected, selected);
    assert_eq!(
        f.chunk.metadata().unwrap().len(),
        geometry::locate(selected.end() - 1).1 + 1
    );
    let mut payload = [0; 7];
    read_exact_at(
        &old.chunks.reader().unwrap(),
        &mut payload,
        selected.start() + protocol::BLOCK as u64,
    )
    .unwrap();
    assert_eq!(&payload, b"current");
}
#[test]
fn single_bad_head_requires_complete_exact_successor_and_heals_only_after_callback() {
    let mut f = Fixture::new();
    f.append(b"anchor", true);
    let successor = f.append(b"durable successor", false);
    let damaged = 1 - f.slot;
    write_all_at(&f.control, b"bad", protocol::HEAD_OFFSETS[damaged]).unwrap();
    let before = f.bytes();
    assert!(recover_checked(&f.control, &f.dir, |_| Err(Error::LimitExceeded)).is_err());
    assert_eq!(before, f.bytes());
    let recovered = recover_checked(&f.control, &f.dir, |r| {
        assert_eq!(r.selected, successor);
        Ok(())
    })
    .unwrap();
    assert_eq!(recovered.slot, damaged);
    let mut head = [0; protocol::BLOCK];
    read_exact_at(&f.control, &mut head, protocol::HEAD_OFFSETS[damaged]).unwrap();
    assert_eq!(
        protocol::decode_head(&head, f.header.stream()).unwrap(),
        successor
    );
    // No successor can be invented by accepting only the remaining anchor.
    write_all_at(&f.control, b"bad", protocol::HEAD_OFFSETS[f.slot]).unwrap();
    let before = f.bytes();
    assert!(recover_checked(&f.control, &f.dir, |_| Ok(())).is_err());
    assert_eq!(before, f.bytes());
}
#[test]
fn corrupt_older_payload_reserved_control_and_foreign_orphan_preserve_all_bytes() {
    let mut f = Fixture::new();
    let older = f.append(b"older", true);
    f.append(b"latest", true);
    f.append(b"tail", false);
    write_all_at(
        &f.chunk,
        b"X",
        geometry::locate(older.start()).1 + protocol::BLOCK as u64,
    )
    .unwrap();
    let before = f.bytes();
    assert!(
        recover_checked(&f.control, &f.dir, |_| panic!(
            "corruption reached graph callback"
        ))
        .is_err()
    );
    assert_eq!(before, f.bytes());
    write_all_at(
        &f.chunk,
        b"o",
        geometry::locate(older.start()).1 + protocol::BLOCK as u64,
    )
    .unwrap();
    write_all_at(&f.control, b"X", protocol::BLOCK as u64).unwrap();
    let before = f.bytes();
    assert!(
        recover_checked(&f.control, &f.dir, |_| panic!(
            "reserved corruption reached callback"
        ))
        .is_err()
    );
    assert_eq!(before, f.bytes());
    write_all_at(&f.control, &[0], protocol::BLOCK as u64).unwrap();
    let orphan = f.dir.join(header::filename(f.header.stream(), 1));
    fs::write(&orphan, b"foreign truncated chunk").unwrap();
    let before = f.bytes();
    let foreign = fs::read(&orphan).unwrap();
    assert!(
        recover_checked(&f.control, &f.dir, |_| panic!(
            "foreign orphan reached callback"
        ))
        .is_err()
    );
    assert_eq!(before, f.bytes());
    assert_eq!(foreign, fs::read(orphan).unwrap());
}
