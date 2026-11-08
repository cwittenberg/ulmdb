//! Read-only strict capture. Never heals, truncates, locks or syncs the source.
use super::{
    BLOCK, Error, HEAD_OFFSETS, MAX_FILE_BYTES, Receipt, Result, codec, io::read_exact_at, recovery,
};
use std::fs::File;
pub(in crate::range) fn select(file: &File, id: [u8; 16]) -> Result<(Receipt, Option<Receipt>)> {
    let length = file.metadata()?.len();
    if length > MAX_FILE_BYTES {
        return Err(Error::Corrupt("range snapshot physical capacity"));
    }
    recovery::verify_reserved_prefix(file)?;
    let mut first = [[0; BLOCK]; 2];
    let mut second = [[0; BLOCK]; 2];
    for (block, offset) in first.iter_mut().zip(HEAD_OFFSETS) {
        read_exact_at(file, block, offset)?;
    }
    for (block, offset) in second.iter_mut().zip(HEAD_OFFSETS) {
        read_exact_at(file, block, offset)?;
    }
    if first != second {
        return Err(Error::Busy);
    }
    let left = codec::decode_head(&first[0], id)?;
    let right = codec::decode_head(&first[1], id)?;
    let (new, old) = if left.ordinal >= right.ordinal {
        (left, right)
    } else {
        (right, left)
    };
    if new.ordinal == old.ordinal {
        if new != old || new.ordinal != 0 {
            return Err(Error::Corrupt("range snapshot equal heads disagree"));
        }
    } else if old.ordinal.checked_add(1) != Some(new.ordinal) {
        return Err(Error::Corrupt("range snapshot heads are not adjacent"));
    }
    recovery::verify_frame(file, id, old)?;
    let frame = recovery::verify_frame(file, id, new)?;
    if new.ordinal != 0 && frame.parent != Some(old) {
        return Err(Error::Corrupt("range snapshot parent mismatch"));
    }
    if new.frame_end()? > file.metadata()?.len() {
        return Err(Error::Corrupt("range snapshot selected prefix truncated"));
    }
    Ok((new, frame.parent))
}
