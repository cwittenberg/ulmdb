//! One full-page CoW copy, or one private revision, for bounded checked spans.
use super::*;
use crate::resident::VersionKey;

impl Arena {
    pub(super) fn patch_many_page(
        &mut self,
        old: Reference,
        patches: &[(usize, &[u8])],
    ) -> Result<Reference> {
        self.pool.ensure_live()?;
        if !old.offset.is_multiple_of(PAGE as u64) {
            return Err(Error::InvalidInput("resident patch alignment"));
        }
        if patches.len() > 4 {
            return Err(Error::InvalidInput("resident multi-patch count"));
        }
        let mut previous_end = 0;
        for &(offset, bytes) in patches {
            let end = offset
                .checked_add(bytes.len())
                .filter(|end| *end <= PAGE)
                .ok_or(Error::InvalidInput("resident patch bounds"))?;
            if offset < previous_end {
                return Err(Error::InvalidInput("resident multi-patch order/overlap"));
            }
            previous_end = end;
        }
        let (slot, generation, start) = unpack(old)?;
        // Aligned tagged references have a zero in-page offset. Every write
        // below therefore satisfies Permit::write's sole offset+length bound.
        debug_assert_eq!(start, 0);
        if let Some(p) = self.private.get(&slot)? {
            if p.generation != generation || !p.exact(start, PAGE) {
                return Err(Error::InvalidInput(
                    "resident patch requires exact allocation",
                ));
            }
            let bytes = p
                .read(start, PAGE)
                .map_err(|_| Error::Corrupt("resident private membership/generation"))?;
            if patches.iter().all(|&(offset, replacement)| {
                bytes[offset..offset + replacement.len()] == *replacement
            }) {
                return Ok(old);
            }
            self.remember_private(slot)?;
            self.revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
            let p = self.private.get_mut(&slot)?.unwrap();
            // All bounds and the single revision tick are checked before the
            // first write. No allocation, lookup or other fallible preflight
            // remains between writes to this exclusively owned private page.
            for &(offset, replacement) in patches {
                p.write(offset, replacement)?;
            }
            return Ok(old);
        }
        let source = self.base.version(slot).ok_or(Error::InvalidInput(
            "resident patch requires exact allocation",
        ))?;
        if !source.exact(generation, start, PAGE) {
            return Err(Error::InvalidInput(
                "resident patch requires exact allocation",
            ));
        }
        let bytes = source
            .read(generation, start, PAGE)
            .map_err(|_| Error::Corrupt("resident reference absent from writer"))?;
        if patches
            .iter()
            .all(|&(offset, replacement)| bytes[offset..offset + replacement.len()] == *replacement)
        {
            return Ok(old);
        }
        // Preserve the existing allocation path's revision-before-reservation
        // order. A failure drops only this unpublished Permit and its owners.
        self.pool.ensure_live()?;
        self.revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
        let mut p = Private::new(self.pool.reserve(false)?);
        p.ranges.try_reserve(1).map_err(|_| Error::LimitExceeded)?;
        p.write(0, bytes)?;
        for &(offset, replacement) in patches {
            p.write(offset, replacement)?;
        }
        p.ranges.push((0, PAGE as u16));
        p.cursor = PAGE;
        if (slot as usize) < self.pool.geometry().0 {
            p.source = Some(VersionKey { slot, generation });
        }
        let at = tagged(p.slot, p.generation, 0);
        self.insert_private(p.slot, p)?;
        Ok(at)
    }
}

#[cfg(test)]
#[path = "patch_many/tests.rs"]
mod tests;
