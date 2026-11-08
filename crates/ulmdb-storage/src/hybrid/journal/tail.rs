//! Fixed-capacity suffix operations. These never grant authority or touch selected bytes.
use ulmdb_platform::FileExt;
use crate::{Error, Result};
use std::{fs::File};
const CHUNK: usize = 64 * 1024;
fn checked(start: u64, end: u64) -> Result<()> {
    if start > end || end > super::wire::MAX_WAL_BYTES {
        return Err(Error::Corrupt("hybrid private suffix bounds"));
    }
    Ok(())
}
pub(super) fn clear(file: &File, start: u64, end: u64) -> Result<()> {
    checked(start, end)?;
    let zeros = [0; CHUNK];
    let mut at = start;
    while at < end {
        let n = (end - at).min(CHUNK as u64) as usize;
        file.write_all_at(&zeros[..n], at)?;
        at += n as u64;
    }
    Ok(())
}
pub(crate) fn verify_zero(file: &File, start: u64, end: u64) -> Result<()> {
    checked(start, end)?;
    let mut buffer = [0; CHUNK];
    let mut at = start;
    while at < end {
        let n = (end - at).min(CHUNK as u64) as usize;
        file.read_exact_at(&mut buffer[..n], at)?;
        if buffer[..n].iter().any(|b| *b != 0) {
            return Err(Error::Corrupt(
                "hybrid nonzero unused capacity after successor",
            ));
        }
        at += n as u64;
    }
    Ok(())
}
