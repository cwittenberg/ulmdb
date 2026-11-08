//! Cold v7 admission. All anchors, frames, continuations and the selected graph
//! are admitted before any repair or removal. Caller holds the writer fence.
use super::{
    geometry::{self, HEADER_BYTES},
    header::{self, ChunkHeader, StreamId},
    protocol::{self, Frame, Receipt},
    resolver::VerifiedChunks,
    source::{ChunkLease, ExtentMode},
};
use crate::range::pack::io::{read_exact_at, write_all_at};
use crate::{Error, Result};
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

pub(crate) struct Recovered {
    pub(crate) global: protocol::Global,
    pub(crate) selected: Receipt,
    pub(crate) slot: usize,
    pub(crate) chunks: VerifiedChunks,
}
fn physical_end(receipt: Receipt) -> u64 {
    HEADER_BYTES + receipt.end() - receipt.binding().index * geometry::DATA_BYTES
}
fn open_lease(directory: &Path, receipt: Receipt, mode: ExtentMode) -> Result<ChunkLease> {
    ChunkLease::open(directory, receipt.binding(), receipt.end(), mode)
}
fn exact_read(lease: &ChunkLease, offset: u64, bytes: &mut [u8]) -> Result<()> {
    // Keep the existing checked/partial positional-I/O loop as the consumer.
    struct Reader<'a>(&'a ChunkLease);
    impl crate::range::pack::io::ReadAt for Reader<'_> {
        fn read_at(&self, b: &mut [u8], off: u64) -> std::io::Result<usize> {
            self.0.read_at(b, off)
        }
    }
    read_exact_at(&Reader(lease), bytes, offset)?;
    Ok(())
}
fn verify_frame(lease: &ChunkLease, receipt: Receipt) -> Result<Frame> {
    if lease.header().binding() != receipt.binding() {
        return Err(Error::Corrupt("v7 frame/chunk binding"));
    }
    let mut block = [0; protocol::BLOCK];
    exact_read(lease, receipt.start(), &mut block)?;
    let frame = protocol::decode_frame_header_bound(&block, receipt.binding())?;
    if frame.receipt != receipt {
        return Err(Error::Corrupt("v7 selected frame identity"));
    }
    exact_read(lease, receipt.end() - protocol::BLOCK as u64, &mut block)?;
    protocol::decode_frame_footer(&block, receipt.binding().stream, &frame)?;
    protocol::validate_payload_with(&frame, |offset, bytes| exact_read(lease, offset, bytes))?;
    Ok(frame)
}
/// Cold-only work is proportional to the admitted history. Ordinary publication
/// does not call this walk. Every parent payload is checked, not just envelopes.
fn verify_chain(directory: &Path, selected: Receipt) -> Result<(VerifiedChunks, Frame)> {
    let mut current = selected;
    let mut lease = open_lease(directory, current, ExtentMode::AppendSnapshot)?;
    let mut reverse = Vec::new();
    reverse.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
    reverse.push(lease.clone());
    let first = verify_frame(&lease, current)?;
    let mut frame = first;
    while let Some(parent) = frame.parent {
        if parent.binding().index != current.binding().index {
            let previous = open_lease(directory, parent, ExtentMode::SealedExact)?;
            let eof = File::open(directory.join(header::filename(
                parent.binding().stream,
                parent.binding().index,
            )))?
            .metadata()?
            .len();
            header::validate_continuation(
                previous.header(),
                parent.descriptor(),
                eof,
                lease.header(),
                current.start(),
                current.end() - current.start(),
            )?;
            reverse.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
            reverse.push(previous.clone());
            lease = previous;
        }
        current = parent;
        frame = verify_frame(&lease, current)?;
    }
    if current.start() != 0 || current.ordinal() != 0 || current.payload_length() != 0 {
        return Err(Error::Corrupt("v7 missing genesis"));
    }
    reverse.reverse();
    let source = VerifiedChunks::from_verified(reverse, selected.binding().stream, selected.end())?;
    Ok((source, first))
}
fn reserved_control(control: &File) -> Result<()> {
    let metadata = control.metadata()?;
    if !metadata.is_file() || metadata.len() != protocol::CONTROL_LEN {
        return Err(Error::Corrupt("v7 fixed control EOF"));
    }
    let mut bytes = [0; protocol::BLOCK];
    for at in (protocol::BLOCK as u64..protocol::CONTROL_LEN).step_by(protocol::BLOCK) {
        if protocol::HEAD_OFFSETS.contains(&at) {
            continue;
        }
        read_exact_at(control, &mut bytes, at)?;
        if bytes.iter().any(|byte| *byte != 0) {
            return Err(Error::Corrupt("v7 nonzero control reserved bytes"));
        }
    }
    Ok(())
}
fn read_header(directory: &Path, stream: StreamId, index: u64) -> Result<(File, ChunkHeader)> {
    let file = File::open(directory.join(header::filename(stream, index)))?;
    let mut bytes = [0; HEADER_BYTES as usize];
    read_exact_at(&file, &mut bytes, 0)?;
    let envelope = ChunkHeader::decode(&bytes)?;
    if envelope.stream() != stream || envelope.index() != index {
        return Err(Error::Corrupt("v7 foreign chunk envelope"));
    }
    Ok((file, envelope))
}
fn successor(directory: &Path, anchor: Receipt) -> Result<Receipt> {
    // A single bad head cannot authorize silently rolling back. It requires a
    // complete exact-EOF successor, as in the original two-head protocol.
    let (old_file, old_header) =
        read_header(directory, anchor.binding().stream, anchor.binding().index)?;
    let old_eof = old_file.metadata()?.len();
    let (file, envelope, start) = if old_eof > physical_end(anchor) {
        (old_file, old_header, anchor.end())
    } else if old_eof == physical_end(anchor) {
        let index = anchor
            .binding()
            .index
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        let (file, envelope) = match read_header(directory, anchor.binding().stream, index) {
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::Corrupt("v7 bad head has no complete successor"));
            }
            result => result?,
        };
        let start = envelope.base();
        (file, envelope, start)
    } else {
        return Err(Error::Corrupt("v7 torn-head anchor EOF"));
    };
    let mut block = [0; protocol::BLOCK];
    read_exact_at(&file, &mut block, geometry::locate(start).1)?;
    let candidate = protocol::decode_frame_header_bound(&block, envelope.binding())?;
    let next = candidate.receipt;
    if next.start() != start
        || candidate.parent != Some(anchor)
        || anchor.ordinal().checked_add(1) != Some(next.ordinal())
        || file.metadata()?.len() != physical_end(next)
    {
        return Err(Error::Corrupt("v7 torn-head successor ambiguity"));
    }
    Ok(next)
}
fn orphan(directory: &Path, selected: Receipt) -> Result<Option<std::path::PathBuf>> {
    let Some(index) = selected.binding().index.checked_add(1) else {
        return Ok(None);
    };
    let path = directory.join(header::filename(selected.binding().stream, index));
    let metadata = match fs::metadata(&path) {
        Ok(value) => value,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !metadata.is_file()
        || metadata.len() < HEADER_BYTES
        || metadata.len() > HEADER_BYTES + geometry::DATA_BYTES
    {
        return Err(Error::Corrupt("v7 malformed orphan chunk extent"));
    }
    let (next_file, next) = read_header(directory, selected.binding().stream, index)?;
    // No foreign or malformed next file is ever removed. Only an explicitly
    // bound preparation immediately following the selected parent qualifies.
    let expected = header::Continuation {
        prior_header_hash: selected.binding().header_hash,
        parent: selected.descriptor(),
    };
    if next.continuation() != Some(expected) {
        return Err(Error::Corrupt("v7 foreign or malformed orphan chunk"));
    }
    let mut block = [0; protocol::BLOCK];
    read_exact_at(&next_file, &mut block, HEADER_BYTES)?;
    let frame = protocol::decode_frame_header_bound(&block, next.binding())?;
    if frame.parent != Some(selected)
        || frame.receipt.start() != next.base()
        || geometry::next_frame(
            selected.start(),
            selected.end(),
            frame.receipt.end() - frame.receipt.start(),
        ) != Some(next.base())
        || metadata.len() != physical_end(frame.receipt)
    {
        return Err(Error::Corrupt("v7 orphan full parent or EOF"));
    }
    let lease = open_lease(directory, frame.receipt, ExtentMode::SealedExact)?;
    verify_frame(&lease, frame.receipt)?;
    Ok(Some(path))
}
pub(crate) fn recover_checked<F>(control: &File, directory: &Path, validate: F) -> Result<Recovered>
where
    F: FnOnce(&Recovered) -> Result<()>,
{
    reserved_control(control)?;
    let mut bytes = [0; protocol::BLOCK];
    read_exact_at(control, &mut bytes, 0)?;
    let global = protocol::decode_global(&bytes)?;
    let mut blocks = [[0; protocol::BLOCK]; 2];
    for (bytes, at) in blocks.iter_mut().zip(protocol::HEAD_OFFSETS) {
        read_exact_at(control, bytes, at)?;
    }
    let heads = [
        protocol::decode_head(&blocks[0], global.stream),
        protocol::decode_head(&blocks[1], global.stream),
    ];
    let (selected, slot, heal, expected_parent) = match heads {
        [Ok(left), Ok(right)] => {
            let (latest, slot, older) = if left.ordinal() >= right.ordinal() {
                (left, 0, right)
            } else {
                (right, 1, left)
            };
            if latest.ordinal() == older.ordinal() {
                if latest != older || latest.ordinal() != 0 {
                    return Err(Error::Corrupt("v7 equal head ordinals"));
                }
            } else if older.ordinal().checked_add(1) != Some(latest.ordinal()) {
                return Err(Error::Corrupt("v7 nonadjacent head roots"));
            }
            (
                latest,
                slot,
                false,
                if latest.ordinal() == 0 {
                    None
                } else {
                    Some(older)
                },
            )
        }
        [Ok(anchor), Err(_)] => (successor(directory, anchor)?, 1, true, Some(anchor)),
        [Err(_), Ok(anchor)] => (successor(directory, anchor)?, 0, true, Some(anchor)),
        [Err(_), Err(_)] => return Err(Error::Corrupt("v7 both heads invalid")),
    };
    let (chunks, selected_frame) = verify_chain(directory, selected)?;
    if selected_frame.parent != expected_parent {
        return Err(Error::Corrupt("v7 head/frame parent mismatch"));
    }
    let next_orphan = orphan(directory, selected)?;
    let ready = Recovered {
        global,
        selected,
        slot,
        chunks,
    };
    validate(&ready)?;
    // From this point mutation is authorized. Independent readers hold their
    // own FDs and bounds; only unselected bytes/files can be removed.
    if heal {
        write_all_at(
            control,
            &protocol::encode_head(global.stream, selected),
            protocol::HEAD_OFFSETS[slot],
        )
        .and_then(|()| ulmdb_platform::sync_file(&control))
        .map_err(|_| Error::OutcomeUnknown)?;
    }
    let active = directory.join(header::filename(
        selected.binding().stream,
        selected.binding().index,
    ));
    let file = OpenOptions::new().read(true).write(true).open(active)?;
    if file.metadata()?.len() != physical_end(selected) {
        file.set_len(physical_end(selected))?;
        ulmdb_platform::sync_file(&file)?;
    }
    if let Some(path) = next_orphan {
        fs::remove_file(path)?;
        ulmdb_platform::sync_path(directory)?;
    }
    Ok(ready)
}
