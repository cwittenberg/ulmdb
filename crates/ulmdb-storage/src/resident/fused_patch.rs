//! Resolve a patch once. No token escapes this call or survives a mutation.
use super::*;
#[cfg(feature = "resident-direct-page-patch")]
use crate::resident::{VersionKey, pool::tagged};

impl Arena {
    pub(super) fn fused_patch(
        &mut self,
        old: Reference,
        object_len: usize,
        offset: usize,
        replacement: &[u8],
        alignment: usize,
    ) -> Result<Reference> {
        self.pool.ensure_live()?;
        if !alignment.is_power_of_two()
            || alignment > PAGE
            || !old.offset.is_multiple_of(alignment as u64)
        {
            return Err(Error::InvalidInput("resident patch alignment"));
        }
        let end = offset
            .checked_add(replacement.len())
            .filter(|n| *n <= object_len)
            .ok_or(Error::InvalidInput("resident patch bounds"))?;
        let (slot, generation, start) = unpack(old)?;
        if let Some(p) = self.private.get(&slot)? {
            if p.generation != generation || !p.exact(start, object_len) {
                return Err(Error::InvalidInput(
                    "resident patch requires exact allocation",
                ));
            }
            let bytes = p
                .read(start, object_len)
                .map_err(|_| Error::Corrupt("resident private membership/generation"))?;
            if bytes[offset..end] == *replacement {
                return Ok(old);
            }
            // Admit undo before the checked revision tick and private write.
            self.remember_private(slot)?;
            self.revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
            let p = self.private.get_mut(&slot)?.unwrap();
            p.write(start + offset, replacement)?;
            return Ok(old);
        }
        let source = self.base.version(slot).ok_or(Error::InvalidInput(
            "resident patch requires exact allocation",
        ))?;
        if !source.exact(generation, start, object_len) {
            return Err(Error::InvalidInput(
                "resident patch requires exact allocation",
            ));
        }
        let bytes = source
            .read(generation, start, object_len)
            .map_err(|_| Error::Corrupt("resident reference absent from writer"))?;
        if bytes[offset..end] == *replacement {
            return Ok(old);
        }
        #[cfg(feature = "resident-direct-page-patch")]
        if object_len == PAGE && alignment == PAGE {
            self.pool.ensure_live()?;
            self.revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
            let mut p = Private {
                permit: self.pool.reserve(false)?,
                source: None,
                born: 0,
                protected: false,
            };
            p.ranges.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
            p.write(0, bytes)?;
            p.write(offset, replacement)?;
            p.ranges.push((0, PAGE as u16));
            p.cursor = PAGE;
            if (slot as usize) < self.pool.geometry().0 {
                p.source = Some(VersionKey { slot, generation });
            }
            let at = tagged(p.slot, p.generation, 0);
            self.insert_private(p.slot, p)?;
            return Ok(at);
        }
        // The generic size/alignment path keeps the original owned fallback.
        let mut bytes = bytes.to_vec();
        bytes[offset..end].copy_from_slice(replacement);
        self.allocate_copy(old, &bytes, alignment)
    }
}

#[cfg(test)]
#[path = "fused_patch/tests.rs"]
mod tests;
