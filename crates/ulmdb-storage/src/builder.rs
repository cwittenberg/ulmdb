//! Bounded writer-private memory over a growing set of newly sealed segments.
use crate::{Arena, Error, ReadStore, Reference, Result, SegmentTable, WriteStore};
use std::path::{Path, PathBuf};

/// Generic index-builder store, starting with no references to a source snapshot.
/// At capacity boundaries it seals the current arena, then resolves those now
/// immutable references through the next arena's base table. No dataset clone is
/// required. Files are prepared only; descriptor/CURRENT publication is external.
/// Discard this builder after an error: preceding index writes may be partial.
pub struct SegmentBuilder {
    current: Option<Arena>,
    id: u64,
    directory: PathBuf,
    durability: crate::Durability,
}
impl SegmentBuilder {
    pub fn new(directory: impl AsRef<Path>, first_segment: u64) -> Result<Self> {
        Self::new_with_read_ahead(directory, first_segment, crate::ReadAhead::Normal)
    }
    pub fn new_with_read_ahead(
        directory: impl AsRef<Path>,
        first_segment: u64,
        read_ahead: crate::ReadAhead,
    ) -> Result<Self> {
        Self::new_with_geometry(
            directory,
            first_segment,
            read_ahead,
            ulmdb_core::PageSize::DEFAULT,
        )
    }
    pub fn new_with_geometry(
        directory: impl AsRef<Path>,
        first_segment: u64,
        read_ahead: crate::ReadAhead,
        page_size: ulmdb_core::PageSize,
    ) -> Result<Self> {
        Self::new_with_geometry_and_transform(directory, first_segment, read_ahead, page_size, None)
    }
    pub fn new_with_transform(
        directory: impl AsRef<Path>,
        first_segment: u64,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
    ) -> Result<Self> {
        Self::new_with_geometry_and_transform(
            directory,
            first_segment,
            read_ahead,
            ulmdb_core::PageSize::DEFAULT,
            transform,
        )
    }
    pub fn new_with_geometry_and_transform(
        directory: impl AsRef<Path>,
        first_segment: u64,
        read_ahead: crate::ReadAhead,
        page_size: ulmdb_core::PageSize,
        transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
    ) -> Result<Self> {
        Ok(Self {
            current: Some(Arena::new(
                SegmentTable::empty_with_transform(read_ahead, transform)?
                    .with_page_size(page_size)?,
                first_segment,
            )?),
            id: first_segment,
            directory: directory.as_ref().to_path_buf(),
            durability: crate::Durability::Full,
        })
    }
    /// Apply the data policy to every rolled/final immutable segment.
    /// The existing constructor behavior remains Full.
    pub fn with_durability(mut self, policy: crate::Durability) -> Self {
        self.durability = policy;
        self
    }
    pub fn with_value_limit(mut self, limit: ulmdb_core::ValueLimit) -> Self {
        if let Some(a) = &mut self.current {
            a.set_value_limit(limit);
        }
        self
    }
    fn arena(&self) -> Result<&Arena> {
        self.current
            .as_ref()
            .ok_or(Error::InvalidInput("failed segment builder"))
    }
    fn arena_mut(&mut self) -> Result<&mut Arena> {
        self.current
            .as_mut()
            .ok_or(Error::InvalidInput("failed segment builder"))
    }
    fn roll(&mut self) -> Result<()> {
        // Check ID exhaustion before sealing. A retry never reuses a published
        // segment ID or silently drops an object when the range is exhausted.
        let next = self
            .id
            .checked_add(1)
            .filter(|id| *id != u64::MAX)
            .ok_or(Error::LimitExceeded)?;
        self.arena()?.base().check_insert_id(next)?;
        let current = self
            .current
            .take()
            .ok_or(Error::InvalidInput("failed segment builder"))?;
        let value_limit = current.value_limit();
        let (base, entry, _) = current.seal_with_durability(&self.directory, self.durability)?;
        if entry.is_none() {
            return Err(Error::InvalidInput("cannot roll an empty segment builder"));
        }
        let mut arena = Arena::new(base, next)?;
        arena.set_value_limit(value_limit);
        self.current = Some(arena);
        self.id = next;
        Ok(())
    }
    /// Seal the final arena and return only newly built segments plus the next
    /// unused segment ID. Empty trailing arenas consume no segment identity.
    pub fn finish(mut self) -> Result<(SegmentTable, u64)> {
        let current = self
            .current
            .take()
            .ok_or(Error::InvalidInput("failed segment builder"))?;
        let value_limit = current.value_limit();
        let (segments, entry, _) =
            current.seal_with_durability(&self.directory, self.durability)?;
        let next = if entry.is_some() {
            self.id.checked_add(1).ok_or(Error::LimitExceeded)?
        } else {
            self.id
        };
        Ok((segments.with_value_limit(value_limit)?, next))
    }
    fn retry_allocation(&mut self, length: usize, alignment: usize) -> Result<()> {
        if self.arena()?.can_allocate(length, alignment)? {
            return Ok(());
        }
        // A single oversized object must fail instead of repeatedly sealing
        // empty arenas. All legal engine payloads fit one fresh arena.
        if self.arena()?.dirty_bytes() == 0 {
            return Err(Error::LimitExceeded);
        }
        self.roll()?;
        if !self.arena()?.can_allocate(length, alignment)? {
            return Err(Error::LimitExceeded);
        }
        Ok(())
    }
}
impl ReadStore for SegmentBuilder {
    fn max_value_size(&self) -> usize {
        self.current
            .as_ref()
            .map_or(67_108_864, ReadStore::max_value_size)
    }
    fn page_size(&self) -> usize {
        self.current.as_ref().map_or(4096, ReadStore::page_size)
    }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        self.arena()?.read(at, length)
    }
}
impl WriteStore for SegmentBuilder {
    fn patch(
        &mut self,
        old: Reference,
        object_len: usize,
        offset: usize,
        replacement: &[u8],
        alignment: usize,
    ) -> Result<Reference> {
        let end = offset
            .checked_add(replacement.len())
            .ok_or(Error::LimitExceeded)?;
        if end > object_len {
            return Err(Error::InvalidInput("patch range exceeds object"));
        }
        let result = self
            .arena_mut()?
            .patch(old, object_len, offset, replacement, alignment);
        if matches!(result, Err(Error::LimitExceeded))
            && old.segment != self.id
            && !self.arena()?.can_allocate(object_len, alignment)?
        {
            self.retry_allocation(object_len, alignment)?;
            return self
                .arena_mut()?
                .patch(old, object_len, offset, replacement, alignment);
        }
        result
    }
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.retry_allocation(bytes.len(), alignment)?;
        self.arena_mut()?.allocate(bytes, alignment)
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        let result = self.arena_mut()?.rewrite(old, bytes, alignment);
        // Private same-size rewrites allocate nothing: their LimitExceeded is
        // never a capacity signal (e.g. revision overflow) and must not roll.
        if matches!(result, Err(Error::LimitExceeded))
            && old.segment != self.id
            && !self.arena()?.can_allocate(bytes.len(), alignment)?
        {
            self.retry_allocation(bytes.len(), alignment)?;
            return self.arena_mut()?.rewrite(old, bytes, alignment);
        }
        result
    }
    fn replace_payload(&mut self, old: Reference, value: &[u8]) -> Result<Reference> {
        let allocates = old.is_null()
            || old.segment != self.id
            || self.arena()?.payload(old)?.len() != value.len();
        let result = self.arena_mut()?.replace_payload(old, value);
        if matches!(result, Err(Error::LimitExceeded)) && allocates {
            let length = 16usize
                .checked_add(value.len())
                .ok_or(Error::LimitExceeded)?;
            if !self.arena()?.can_allocate(length, 16)? {
                self.retry_allocation(length, 16)?;
                return self.arena_mut()?.replace_payload(old, value);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("ulmdb-builder-{}", crate::new_identity().unwrap()));
            crate::create_layout(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn sealed_base_references_survive_rolls_and_private_replacements() {
        let dir = Directory::new();
        let mut builder = SegmentBuilder::new(&dir.0, 17).unwrap();
        let old = builder.put_payload(b"first").unwrap();
        builder.roll().unwrap();
        assert_eq!(builder.payload(old).unwrap(), b"first");
        let new = builder.replace_payload(old, b"other").unwrap();
        assert_ne!(new, old);
        assert_eq!(new.segment, 18);
        assert_eq!(builder.replace_payload(new, b"third").unwrap(), new);
        assert_eq!(builder.payload(new).unwrap(), b"third");
        assert_eq!(builder.payload(old).unwrap(), b"first");
        let raw = builder.allocate(b"before", 16).unwrap();
        builder.roll().unwrap();
        let updated = builder.rewrite(raw, b"after!", 16).unwrap();
        assert_eq!(updated.segment, 19);
        assert_eq!(builder.read(raw, 6).unwrap(), b"before");
        let (table, next) = builder.finish().unwrap();
        assert_eq!(next, 20);
        assert_eq!(table.entries().len(), 3);
        assert_eq!(table.payload(old).unwrap(), b"first");
        assert_eq!(table.payload(new).unwrap(), b"third");
        assert_eq!(table.read(updated, 6).unwrap(), b"after!");
    }
    #[test]
    fn empty_finish_and_pure_preflight_do_not_consume_ids_or_change_arena() {
        let dir = Directory::new();
        let builder = SegmentBuilder::new(&dir.0, 123).unwrap();
        let (table, next) = builder.finish().unwrap();
        assert_eq!(next, 123);
        assert!(table.entries().is_empty());
        let arena = Arena::new(SegmentTable::default(), 1).unwrap();
        let before = (arena.dirty_bytes(), arena.revision());
        assert!(arena.can_allocate(268_435_456 - 4096, 16).unwrap());
        assert!(!arena.can_allocate(268_435_456 - 4095, 16).unwrap());
        assert!(matches!(
            arena.can_allocate(usize::MAX, 16),
            Err(Error::LimitExceeded)
        ));
        assert!(matches!(
            arena.can_allocate(1, 3),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!((arena.dirty_bytes(), arena.revision()), before);
    }
    #[test]
    fn id_exhaustion_is_checked_before_sealing_and_failed_io_disables_builder() {
        let dir = Directory::new();
        let mut builder = SegmentBuilder::new(&dir.0, u64::MAX - 1).unwrap();
        let at = builder.allocate(b"data", 16).unwrap();
        assert!(matches!(builder.roll(), Err(Error::LimitExceeded)));
        assert!(
            std::fs::read_dir(dir.0.join("segments"))
                .unwrap()
                .next()
                .is_none()
        );
        assert_eq!(builder.read(at, 4).unwrap(), b"data");
        let (_, next) = builder.finish().unwrap();
        assert_eq!(next, u64::MAX);
        let mut builder = SegmentBuilder::new(&dir.0, 2).unwrap();
        builder.allocate(b"next", 16).unwrap();
        std::fs::rename(dir.0.join("segments"), dir.0.join("old-segments")).unwrap();
        // A missing seal directory yields I/O failure, while replacing it with
        // a regular file is rejected as structural corruption by access checks.
        let result = builder.roll();
        assert!(
            matches!(&result, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound),
            "unexpected seal result: {result:?}"
        );
        assert!(matches!(builder.finish(), Err(Error::InvalidInput(_))));
    }
}

#[cfg(test)]
mod patch_tests {
    use super::*;
    #[test]
    fn private_revision_and_undo_errors_never_roll_or_seal() {
        let path = std::env::temp_dir().join(format!(
            "ulmdb-builder-patch-{}",
            crate::new_identity().unwrap()
        ));
        crate::create_layout(&path).unwrap();
        let mut builder = SegmentBuilder::new(&path, 7).unwrap();
        let size = Arena::MAX_SAVEPOINT_UNDO_BYTES / Arena::MAX_SAVEPOINT_DEPTH + 1;
        let at = builder.allocate(&vec![0; size], 16).unwrap();
        let mut frames = Vec::new();
        for _ in 0..Arena::MAX_SAVEPOINT_DEPTH {
            frames.push(builder.arena_mut().unwrap().checkpoint().unwrap());
        }
        assert!(matches!(
            builder.patch(at, size, 0, b"x", 16),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(builder.id, 7);
        assert_eq!(builder.arena().unwrap().savepoint_undo_bytes(), 0);
        assert!(
            std::fs::read_dir(path.join("segments"))
                .unwrap()
                .next()
                .is_none()
        );
        for frame in frames.iter().rev() {
            builder
                .arena_mut()
                .unwrap()
                .release_checkpoint(frame)
                .unwrap();
        }
        builder.arena_mut().unwrap().set_revision_for_test(u64::MAX);
        assert!(matches!(
            builder.patch(at, size, 0, b"x", 16),
            Err(Error::LimitExceeded)
        ));
        assert!(builder.patch(at, size, usize::MAX, b"xx", 16).is_err());
        assert!(builder.patch(at, size, 0, b"x", 3).is_err());
        assert_eq!(builder.id, 7);
        assert!(
            std::fs::read_dir(path.join("segments"))
                .unwrap()
                .next()
                .is_none()
        );
        assert_eq!(builder.read(at, 1).unwrap(), b"\0");
        drop(builder);
        std::fs::remove_dir_all(path).unwrap();
    }
}
