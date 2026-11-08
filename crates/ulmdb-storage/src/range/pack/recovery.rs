//! Read/validate first; heal or trim only after a complete selection is proven.
use crate::range::pack::{
    BLOCK, DATA_START, Error, HEAD_OFFSETS, MAX_FILE_BYTES, Receipt, Result, codec,
    io::{read_exact_at, write_all_at},
    point,
};
use std::fs::File;

pub(crate) fn verify_envelope(
    file: &File,
    file_id: [u8; 16],
    receipt: Receipt,
) -> Result<codec::Frame> {
    receipt.validate()?;
    let end = receipt.frame_end()?;
    if file.metadata()?.len() < end {
        return Err(Error::Corrupt("packed selected frame is truncated"));
    }
    let mut header = [0; BLOCK];
    read_exact_at(file, &mut header, receipt.offset)?;
    let frame = codec::decode_frame_header(&header, file_id)?;
    if frame.receipt != receipt {
        return Err(Error::Corrupt("packed selected frame identity mismatch"));
    }
    let mut footer = [0; BLOCK];
    read_exact_at(file, &mut footer, end - BLOCK as u64)?;
    codec::decode_frame_footer(&footer, file_id, &frame)?;
    Ok(frame)
}

pub(crate) fn verify_frame(
    file: &File,
    file_id: [u8; 16],
    receipt: Receipt,
) -> Result<codec::Frame> {
    let frame = verify_envelope(file, file_id, receipt)?;
    codec::validate_frame_payload(file, &frame)?;
    Ok(frame)
}

pub(crate) fn recover_checked<F: FnOnce(Receipt) -> Result<()>>(
    file: &File,
    file_id: [u8; 16],
    validate: F,
) -> Result<(Receipt, usize)> {
    let length = file.metadata()?.len();
    if !(DATA_START + crate::range::pack::ALIGNMENT..=MAX_FILE_BYTES).contains(&length) {
        return Err(Error::Corrupt("packed file extent"));
    }
    verify_reserved_prefix(file)?;
    let mut blocks = [[0; BLOCK]; 2];
    for (block, offset) in blocks.iter_mut().zip(HEAD_OFFSETS) {
        read_exact_at(file, block, offset)?;
    }
    let heads = [
        codec::decode_head(&blocks[0], file_id),
        codec::decode_head(&blocks[1], file_id),
    ];
    let (selected, slot, heal) = match heads {
        [Ok(left), Ok(right)] => {
            let (latest, latest_slot, older) = if left.ordinal >= right.ordinal {
                (left, 0, right)
            } else {
                (right, 1, left)
            };
            if latest.ordinal == older.ordinal {
                if latest != older || latest.ordinal != 0 {
                    return Err(Error::Corrupt("packed equal-ordinal heads disagree"));
                }
                verify_frame(file, file_id, latest)?;
            } else {
                if older.ordinal.checked_add(1) != Some(latest.ordinal) {
                    return Err(Error::Corrupt("packed head ordinals are not adjacent"));
                }
                // Never normalize storage after either authenticated anchor is
                // corrupt, even when the newest opaque payload is independent.
                verify_frame(file, file_id, older)?;
                let frame = verify_frame(file, file_id, latest)?;
                if frame.parent != Some(older) {
                    return Err(Error::Corrupt("packed head/frame parent mismatch"));
                }
            }
            (latest, latest_slot, false)
        }
        [Ok(anchor), Err(_)] => (successor(file, file_id, anchor, length)?, 1, true),
        [Err(_), Ok(anchor)] => (successor(file, file_id, anchor, length)?, 0, true),
        [Err(_), Err(_)] => return Err(Error::Corrupt("both packed heads are invalid")),
    };
    // No write/truncate occurs until every required checksum, field, parent,
    // payload and padding check above succeeds.
    validate(selected)?;
    if heal {
        point("pack.recovery.before_heal")?;
        write_all_at(
            file,
            &codec::encode_head(file_id, selected),
            HEAD_OFFSETS[slot],
        )?;
        point("pack.recovery.after_heal_write")?;
        ulmdb_platform::sync_file(&file)?;
        point("pack.recovery.after_heal_sync")?;
    }
    let end = selected.frame_end()?;
    if length != end {
        // Safe-public snapshots can cover only published frames <= this end.
        // Removing an unselected tail prevents orphan-fork ambiguity on the
        // next attempt; the 64KiB boundary never overlaps an old mapped page.
        point("pack.recovery.before_trim")?;
        file.set_len(end)?;
        point("pack.recovery.after_trim")?;
        ulmdb_platform::sync_file(&file)?;
        point("pack.recovery.after_trim_sync")?;
    }
    Ok((selected, slot))
}

pub(in crate::range) fn verify_reserved_prefix(file: &File) -> Result<()> {
    let mut block = [0; BLOCK];
    for start in (0..DATA_START).step_by(BLOCK) {
        if start == 0 || HEAD_OFFSETS.contains(&start) {
            continue;
        }
        read_exact_at(file, &mut block, start)?;
        if block != [0; BLOCK] {
            return Err(Error::Corrupt("packed reserved prefix is nonzero"));
        }
    }
    Ok(())
}

fn successor(file: &File, file_id: [u8; 16], anchor: Receipt, length: u64) -> Result<Receipt> {
    verify_frame(file, file_id, anchor)?;
    let offset = anchor.frame_end()?;
    if length
        < offset
            .checked_add(BLOCK as u64)
            .ok_or(Error::LimitExceeded)?
    {
        return Err(Error::Corrupt(
            "invalid packed head has no complete successor",
        ));
    }
    let mut header = [0; BLOCK];
    read_exact_at(file, &mut header, offset)?;
    let frame = codec::decode_frame_header(&header, file_id)?;
    if frame.receipt.offset != offset
        || frame.parent != Some(anchor)
        || anchor.ordinal.checked_add(1) != Some(frame.receipt.ordinal)
        || frame.receipt.frame_end()? != length
    {
        return Err(Error::Corrupt(
            "packed torn head has ambiguous or corrupt tail",
        ));
    }
    verify_frame(file, file_id, frame.receipt)?;
    Ok(frame.receipt)
}
