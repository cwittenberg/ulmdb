//! One checked private-page mutation or one immutable-page CoW copy.
use super::*;

impl Arena {
    pub(super) fn patch_many_page(
        &mut self,
        old: Reference,
        patches: &[(usize, &[u8])],
    ) -> Result<Reference> {
        let size = self.page_size();
        if old.segment == 0 || old.offset < 4096 || !old.offset.is_multiple_of(size as u64) {
            return Err(Error::InvalidInput(
                "invalid multi-patch reference/alignment",
            ));
        }
        if patches.len() > 4 {
            return Err(Error::InvalidInput("multi-patch count"));
        }
        let mut previous_end = 0;
        for &(offset, replacement) in patches {
            let end = offset
                .checked_add(replacement.len())
                .filter(|end| *end <= size)
                .ok_or(Error::InvalidInput("multi-patch bounds"))?;
            if offset < previous_end {
                return Err(Error::InvalidInput("multi-patch order/overlap"));
            }
            previous_end = end;
        }
        if old.segment == self.id {
            if self.objects.get(&old.offset) != Some(&size) {
                return Err(Error::InvalidInput(
                    "multi-patch requires complete private page",
                ));
            }
            let start = usize::try_from(old.offset).map_err(|_| Error::LimitExceeded)?;
            let end = start.checked_add(size).ok_or(Error::LimitExceeded)?;
            let source = self
                .bytes
                .get(start..end)
                .ok_or(Error::Corrupt("private multi-patch extent"))?;
            if patches.iter().all(|&(offset, replacement)| {
                source[offset..offset + replacement.len()] == *replacement
            }) {
                return Ok(old);
            }
            let revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
            // One complete before-image per active frame, shared with patch
            // and rewrite. Every fallible preflight precedes the first store.
            self.remember(start, size)?;
            for &(offset, replacement) in patches {
                self.bytes[start + offset..start + offset + replacement.len()]
                    .copy_from_slice(replacement);
            }
            self.revision = revision;
            return Ok(old);
        }
        // As with ordinary patch(), immutable stores certify the complete
        // extent, while the caller supplies its checked object size. Never
        // mutate published bytes or retain a borrowed source after this call.
        let source = self.base.read(old, size)?;
        if patches.iter().all(|&(offset, replacement)| {
            source[offset..offset + replacement.len()] == *replacement
        }) {
            return Ok(old);
        }
        let start = self
            .bytes
            .len()
            .checked_add(size - 1)
            .map(|n| n & !(size - 1))
            .ok_or(Error::LimitExceeded)?;
        let end = start.checked_add(size).ok_or(Error::LimitExceeded)?;
        if end > self.capacity_limit() {
            return Err(Error::LimitExceeded);
        }
        let revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
        self.bytes
            .try_reserve_profile(end - self.bytes.len(), self.value_limit)
            .map_err(|_| Error::LimitExceeded)?;
        self.bytes.resize(start, 0)?;
        self.bytes.extend_from_slice(source)?;
        for &(offset, replacement) in patches {
            self.bytes[start + offset..start + offset + replacement.len()]
                .copy_from_slice(replacement);
        }
        self.objects.insert(start as u64, size);
        self.revision = revision;
        Ok(Reference {
            segment: self.id,
            offset: start as u64,
        })
    }
}

#[cfg(test)]
mod tests;
