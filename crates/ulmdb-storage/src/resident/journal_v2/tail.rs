//! Fixed-capacity suffix operations. These never grant authority or touch selected bytes.
use crate::{Error, Result};
fn checked(start: u64, end: u64) -> Result<()> {
    if start > end || end > super::wire::MAX_WAL_BYTES {
        return Err(Error::Corrupt("resident private suffix bounds"));
    }
    Ok(())
}
pub(super) fn clear(file: &crate::transform::ResidentFile, start: u64, end: u64) -> Result<()> {
    checked(start, end)?;
    file.clear(start, end)
}
pub(crate) fn verify_zero(
    file: &crate::transform::ResidentFile,
    start: u64,
    end: u64,
) -> Result<()> {
    checked(start, end)?;
    file.verify_zero(start, end)
}
