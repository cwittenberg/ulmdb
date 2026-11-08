//! Immutable sealed directory plus O(1) per-snapshot active-prefix replacement.
use super::{ReadSpan, image::HEADER};
use crate::{Error, ReadStore, Reference, Result};
use std::sync::Arc;
type Chunk = [Option<Arc<ReadSpan>>; 256];
#[derive(Clone, Default)]
struct Directory {
    first: u64,
    chunks: Vec<Option<Arc<Chunk>>>,
    count: usize,
}
impl Directory {
    #[inline]
    fn get(&self, id: u64) -> Option<&ReadSpan> {
        let relative = usize::try_from((id >> 8).wrapping_sub(self.first)).ok()?;
        self.chunks.get(relative)?.as_ref()?[(id & 255) as usize].as_deref()
    }
    fn insert(&mut self, span: Arc<ReadSpan>) -> Result<()> {
        let id = span.chunk_id();
        let chunk = id >> 8;
        if self.get(id).is_some() {
            return Err(Error::Corrupt("duplicate image chunk"));
        }
        if self.count >= 1_048_575 {
            return Err(Error::LimitExceeded);
        }
        if self.chunks.is_empty() {
            self.first = chunk;
        }
        if chunk < self.first {
            return Err(Error::InvalidInput("nonmonotone image chunk ID"));
        }
        let index = usize::try_from(chunk - self.first).map_err(|_| Error::LimitExceeded)?;
        if index >= 4097 {
            return Err(Error::LimitExceeded);
        }
        self.chunks
            .resize_with(self.chunks.len().max(index + 1), || None);
        let slot =
            self.chunks[index].get_or_insert_with(|| Arc::new(std::array::from_fn(|_| None)));
        Arc::make_mut(slot)[(id & 255) as usize] = Some(span);
        self.count += 1;
        Ok(())
    }
}
#[derive(Clone, Default)]
pub struct ImageTable {
    sealed: Arc<Directory>,
    active: Option<Arc<ReadSpan>>,
    active_id: u64,
}
impl ImageTable {
    pub fn with_active(&self, span: Arc<ReadSpan>) -> Result<Self> {
        let mut sealed = Arc::clone(&self.sealed);
        if let Some(old) = &self.active {
            if old.chunk_id() == span.chunk_id() {
                if !old.same_owner(&span) || span.end() < old.end() {
                    return Err(Error::InvalidInput("image active owner/bound changed"));
                }
                return Ok(Self {
                    sealed,
                    active_id: span.chunk_id(),
                    active: Some(span),
                });
            }
            if span.chunk_id() <= old.chunk_id() {
                return Err(Error::InvalidInput("nonmonotone image rollover"));
            }
            let first = if sealed.chunks.is_empty() {
                old.chunk_id() >> 8
            } else {
                sealed.first
            };
            if (span.chunk_id() >> 8) - first >= 4097 || sealed.count >= 1_048_574 {
                return Err(Error::LimitExceeded);
            }
            Arc::make_mut(&mut sealed).insert(Arc::clone(old))?;
        }
        if sealed.get(span.chunk_id()).is_some() {
            return Err(Error::Corrupt("active image chunk already sealed"));
        }
        Ok(Self {
            sealed,
            active_id: span.chunk_id(),
            active: Some(span),
        })
    }
    pub fn active(&self) -> Option<&Arc<ReadSpan>> {
        self.active.as_ref()
    }
    /// Read-only physical binding for receipt/source validation at H3.
    pub fn chunk(&self, id: u64) -> Option<&ReadSpan> {
        self.span(id)
    }
    pub fn sealed_chunks(&self) -> usize {
        self.sealed.count
    }
    #[inline]
    fn span(&self, id: u64) -> Option<&ReadSpan> {
        if id == self.active_id {
            return self.active.as_deref();
        }
        self.sealed.get(id)
    }
}
impl ReadStore for ImageTable {
    #[inline]
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment == 0 {
            return Err(Error::Corrupt("null image reference"));
        }
        let offset =
            usize::try_from(at.offset).map_err(|_| Error::Corrupt("image offset overflow"))?;
        if offset < HEADER {
            return Err(Error::Corrupt("image reference targets header"));
        }
        self.span(at.segment)
            .ok_or(Error::Corrupt("missing image chunk"))?
            .read(offset, length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        WriteStore,
        hybrid::{ChunkLimits, ChunkWriter},
    };
    #[test]
    fn repeated_active_commits_share_directory_and_old_bounds() {
        let root = std::env::temp_dir().join(format!(
            "ulmdb-image-directory-{}",
            crate::new_identity().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        let limits = ChunkLimits {
            capacity: 1024 * 1024,
            reserve_quantum: 65536,
            max_burned_bytes: 1024 * 1024,
        };
        let mut writer = ChunkWriter::create(root.join("active"), 1u64 << 40, limits).unwrap();
        let mut table = ImageTable::default()
            .with_active(writer.read_view())
            .unwrap();
        let initial = table.clone();
        let directory = Arc::clone(&table.sealed);
        for n in 0..128u64 {
            let mut arena = writer.begin(table.clone()).unwrap();
            let at = arena.allocate(&n.to_le_bytes(), 16).unwrap();
            let frozen = arena.freeze().unwrap();
            table = table.with_active(frozen.view()).unwrap();
            frozen.accept();
            assert!(Arc::ptr_eq(&table.sealed, &directory));
            assert!(initial.read(at, 8).is_err());
            assert_eq!(table.read(at, 8).unwrap(), n.to_le_bytes());
        }
        assert_eq!(table.sealed_chunks(), 0);
        let mut roots = Vec::new();
        for n in 1..=16u64 {
            let mut w =
                ChunkWriter::create(root.join(format!("next{n}")), (1u64 << 40) + n, limits)
                    .unwrap();
            let mut a = w.begin(table.clone()).unwrap();
            let at = a.allocate(&n.to_le_bytes(), 16).unwrap();
            let frozen = a.freeze().unwrap();
            table = table.with_active(frozen.view()).unwrap();
            frozen.accept();
            roots.push((at, n));
        }
        assert_eq!(table.sealed_chunks(), 16);
        for (at, n) in roots {
            assert_eq!(table.read(at, 8).unwrap(), n.to_le_bytes());
        }
        let far =
            ChunkWriter::create(root.join("far"), (1u64 << 40) + (4097 * 256), limits).unwrap();
        assert!(matches!(
            table.with_active(far.read_view()),
            Err(Error::LimitExceeded)
        ));
        drop(far);
        drop(table);
        drop(initial);
        drop(directory);
        drop(writer);
        std::fs::remove_dir_all(root).unwrap();
    }
}
