//! Copy one checked shared page straight into a new, privately owned mapping.
//!
//! The parent already checked alignment, exact allocation, bounds, generation,
//! no-op behavior and absence from its private pages. Borrowing the base field
//! keeps its immutable owner alive while mutating the separate permit; no View
//! clone or object-sized temporary allocation is needed. The destination cannot
//! alias a live source: Pool::reserve transfers an exclusively owned free map.
use super::{Arena, Private};
use crate::resident::{
    VersionKey,
    pool::{PAGE, tagged, unpack},
};
use crate::{ReadStore, Reference, Result};

impl Arena {
    pub(super) fn direct_page_patch(
        &mut self,
        old: Reference,
        offset: usize,
        replacement: &[u8],
    ) -> Result<Reference> {
        let (source, generation, within) = unpack(old)?;
        debug_assert_eq!(within, 0);
        self.pool.ensure_live()?;
        // Keep the same revision-before-reservation order as normal allocation.
        self.tick()?;
        let mut p = Private {
            permit: self.pool.reserve(false)?,
            source: None,
            born: 0,
            protected: false,
        };
        p.ranges
            .try_reserve(1)
            .map_err(|_| crate::Error::LimitExceeded)?;
        p.write(0, self.base.read(old, PAGE)?)?;
        p.write(offset, replacement)?;
        p.ranges.push((0, PAGE as u16));
        p.cursor = PAGE;
        if (source as usize) < self.pool.geometry().0 {
            p.source = Some(VersionKey {
                slot: source,
                generation,
            });
        }
        let at = tagged(p.slot, p.generation, 0);
        self.insert_private(p.slot, p)?;
        Ok(at)
    }
}

#[cfg(test)]
#[path = "direct_patch/tests.rs"]
mod tests;
