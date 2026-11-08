//! WriteStore directly in an owned unpublished mapped tail; no heap data arena.
use super::{
    ChunkWriter, ReadSpan,
    image::{FrozenPermit, TailLease},
    undo::{ImageCheckpoint, Journal},
};
use crate::{Error, ReadStore, Reference, Result, WriteStore};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone, Copy, Debug)]
pub(super) struct Object {
    length: usize,
    source: Option<Reference>,
}
#[derive(Clone, Copy, Debug)]
pub struct Allocation {
    pub reference: Reference,
    pub length: usize,
    pub base_source: Option<Reference>,
}
pub struct ImageArena<S: ReadStore> {
    lease: Option<TailLease>,
    base: S,
    objects: BTreeMap<usize, Object>,
    revision: u64,
    journal: Option<Journal>,
}
pub struct FrozenExtent {
    permit: FrozenPermit,
    objects: BTreeMap<usize, Object>,
    revision: u64,
}
impl ChunkWriter {
    pub fn begin<S: ReadStore>(&mut self, base: S) -> Result<ImageArena<S>> {
        Ok(ImageArena {
            lease: Some(self.acquire()?),
            base,
            objects: BTreeMap::new(),
            revision: 0,
            journal: None,
        })
    }
}
impl FrozenExtent {
    pub fn chunk_id(&self) -> u64 {
        self.permit.view.chunk_id()
    }
    pub fn image_incarnation(&self) -> u128 {
        self.permit.view.image_incarnation()
    }
    pub fn start(&self) -> usize {
        self.permit.start()
    }
    pub fn end(&self) -> usize {
        self.permit.view.end()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn view(&self) -> Arc<ReadSpan> {
        Arc::clone(&self.permit.view)
    }
    pub fn bytes(&self) -> Result<&[u8]> {
        self.permit
            .view
            .read(self.start(), self.end() - self.start())
    }
    pub fn allocation_count(&self) -> usize {
        self.objects.len()
    }
    pub fn allocations(&self) -> impl Iterator<Item = Allocation> + '_ {
        self.objects.iter().map(|(&offset, o)| Allocation {
            reference: Reference {
                segment: self.chunk_id(),
                offset: offset as u64,
            },
            length: o.length,
            base_source: o.source,
        })
    }
    /// No durability is implied. The facade calls this only after its journal
    /// authority is durable; this step allocates nothing and cannot fail.
    pub fn accept(self) -> Arc<ReadSpan> {
        self.permit.accept()
    }
}
impl<S: ReadStore> ImageArena<S> {
    pub const MAX_SAVEPOINT_DEPTH: usize = super::undo::MAX_DEPTH;
    pub const MAX_SAVEPOINT_UNDO_BYTES: usize = super::undo::MAX_UNDO_BYTES;
    pub const MAX_ALLOCATIONS: usize = 1_048_576;
    fn lease(&self) -> &TailLease {
        self.lease.as_ref().expect("active image arena")
    }
    fn lease_mut(&mut self) -> &mut TailLease {
        self.lease.as_mut().expect("active image arena")
    }
    /// Materialize a verified redo destination in a fresh/unpublished tail.
    /// This checks storage ownership/geometry; the caller verifies journal authority.
    pub fn allocate_at(&mut self, at: Reference, bytes: &[u8]) -> Result<Reference> {
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        let end = start.checked_add(bytes.len()).ok_or(Error::LimitExceeded)?;
        if at.segment != self.lease().chunk_id() || start < self.lease().cursor {
            return Err(Error::InvalidInput(
                "redo destination overlaps existing image",
            ));
        }
        if end > self.lease().capacity() || self.objects.len() >= Self::MAX_ALLOCATIONS {
            return Err(Error::LimitExceeded);
        }
        let revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
        let prior = self.lease().cursor;
        let lease = self.lease_mut();
        lease.ensure_reserved(end)?;
        lease.zero(prior, start)?;
        lease.write(start, bytes)?;
        lease.cursor = end;
        self.objects.insert(
            start,
            Object {
                length: bytes.len(),
                source: None,
            },
        );
        self.revision = revision;
        Ok(at)
    }
    /// Advance only unpublished allocation slack, preserving exact authenticated bounds.
    pub fn advance_to(&mut self, end: usize) -> Result<()> {
        let start = self.lease().cursor;
        if end < start {
            return Err(Error::InvalidInput("image bound cannot move backward"));
        }
        if end == start {
            return Ok(());
        }
        if end > self.lease().capacity() {
            return Err(Error::LimitExceeded);
        }
        let revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
        let lease = self.lease_mut();
        lease.ensure_reserved(end)?;
        lease.zero(start, end)?;
        lease.cursor = end;
        self.revision = revision;
        Ok(())
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn private_bytes(&self) -> usize {
        self.lease().cursor - self.lease().start
    }
    pub fn private_extent(&self, at: Reference) -> Option<usize> {
        if at.segment != self.lease().chunk_id() {
            return None;
        }
        let offset = usize::try_from(at.offset).ok()?;
        if offset < self.lease().start || offset > self.lease().cursor {
            return None;
        }
        self.objects.get(&offset).map(|o| o.length)
    }
    pub fn checkpoint(&mut self) -> Result<ImageCheckpoint> {
        let end = self.lease().cursor;
        let boundary = self.objects.get(&end).copied();
        self.journal
            .get_or_insert_with(Journal::new)
            .checkpoint(end, self.revision, boundary)
    }
    pub fn release_checkpoint(&mut self, token: &ImageCheckpoint) -> Result<()> {
        self.journal
            .as_mut()
            .ok_or(Error::InvalidInput("inactive image checkpoint"))?
            .release(token)
    }
    pub fn rollback_checkpoint(&mut self, token: &ImageCheckpoint) -> Result<()> {
        let frame = self
            .journal
            .as_mut()
            .ok_or(Error::InvalidInput("inactive image checkpoint"))?
            .rollback(token)?;
        for (offset, bytes) in frame.before {
            self.lease_mut().write(offset, &bytes)?;
        }
        self.lease_mut().cursor = frame.length;
        drop(self.objects.split_off(&frame.length));
        if let Some(o) = frame.boundary_object {
            self.objects.insert(frame.length, o);
        }
        self.revision = frame.revision;
        Ok(())
    }
    pub fn savepoint_depth(&self) -> usize {
        self.journal.as_ref().map_or(0, Journal::depth)
    }
    pub fn savepoint_undo_bytes(&self) -> usize {
        self.journal.as_ref().map_or(0, Journal::bytes)
    }
    pub fn freeze(mut self) -> Result<FrozenExtent> {
        if self.savepoint_depth() != 0 {
            return Err(Error::InvalidInput("active image savepoints"));
        }
        let permit = self.lease.take().expect("active image arena").freeze()?;
        Ok(FrozenExtent {
            permit,
            objects: std::mem::take(&mut self.objects),
            revision: self.revision,
        })
    }
    fn remember(&mut self, start: usize, length: usize) -> Result<()> {
        if let Some(journal) = &mut self.journal
            && journal.depth() != 0
        {
            let source = self
                .lease
                .as_ref()
                .expect("active image arena")
                .read(start, length)?;
            journal.remember(start, source)?;
        }
        Ok(())
    }
    fn payload_parts(&mut self, value: &[u8], source: Option<Reference>) -> Result<Reference> {
        let mut header = [0; 16];
        header[..2].copy_from_slice(&1u16.to_le_bytes());
        header[4..8].copy_from_slice(&16u32.to_le_bytes());
        header[8..].copy_from_slice(&(value.len() as u64).to_le_bytes());
        self.parts(&header, value, 16, source)
    }
    fn parts(
        &mut self,
        prefix: &[u8],
        body: &[u8],
        alignment: usize,
        source: Option<Reference>,
    ) -> Result<Reference> {
        let length = prefix
            .len()
            .checked_add(body.len())
            .ok_or(Error::LimitExceeded)?;
        let (start, end) = position(self.lease(), length, alignment)?;
        if !self.objects.contains_key(&start) && self.objects.len() >= Self::MAX_ALLOCATIONS {
            return Err(Error::LimitExceeded);
        }
        let revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
        let prior = self.lease().cursor;
        let lease = self.lease_mut();
        lease.ensure_reserved(end)?;
        lease.zero(prior, start)?;
        lease.write(start, prefix)?;
        lease.write(start + prefix.len(), body)?;
        lease.cursor = end;
        let segment = lease.chunk_id();
        self.objects.insert(start, Object { length, source });
        self.revision = revision;
        Ok(Reference {
            segment,
            offset: start as u64,
        })
    }
    fn copy_patch(
        &mut self,
        old: Reference,
        length: usize,
        offset: usize,
        value: &[u8],
        alignment: usize,
    ) -> Result<Reference> {
        let (start, end) = position(self.lease(), length, alignment)?;
        let revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
        if !self.objects.contains_key(&start) && self.objects.len() >= Self::MAX_ALLOCATIONS {
            return Err(Error::LimitExceeded);
        }
        let source = self.base.read(old, length)?;
        if source.len() != length {
            return Err(Error::Corrupt("short committed image object"));
        }
        let lease = self.lease.as_mut().expect("active image arena");
        let prior = lease.cursor;
        lease.ensure_reserved(end)?;
        lease.zero(prior, start)?;
        lease.write(start, source)?;
        lease.write(start + offset, value)?;
        lease.cursor = end;
        let segment = lease.chunk_id();
        self.objects.insert(
            start,
            Object {
                length,
                source: Some(old),
            },
        );
        self.revision = revision;
        Ok(Reference {
            segment,
            offset: start as u64,
        })
    }
}
fn position(lease: &TailLease, length: usize, alignment: usize) -> Result<(usize, usize)> {
    if !alignment.is_power_of_two() || alignment > 16384 {
        return Err(Error::InvalidInput("invalid image alignment"));
    }
    let start = lease
        .cursor
        .checked_add(alignment - 1)
        .map(|n| n & !(alignment - 1))
        .ok_or(Error::LimitExceeded)?;
    let end = start.checked_add(length).ok_or(Error::LimitExceeded)?;
    if end > lease.capacity() {
        return Err(Error::LimitExceeded);
    }
    Ok((start, end))
}
impl<S: ReadStore> ReadStore for ImageArena<S> {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        let lease = self.lease();
        let offset =
            usize::try_from(at.offset).map_err(|_| Error::Corrupt("image offset overflow"))?;
        if at.segment == lease.chunk_id() && offset >= lease.start {
            return lease.read(offset, length);
        }
        self.base.read(at, length)
    }
}
impl<S: ReadStore> WriteStore for ImageArena<S> {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.parts(&[], bytes, alignment, None)
    }
    fn put_payload(&mut self, value: &[u8]) -> Result<Reference> {
        self.payload_parts(value, None)
    }
    fn replace_payload(&mut self, old: Reference, value: &[u8]) -> Result<Reference> {
        if old.is_null() {
            return self.put_payload(value);
        }
        let prior = self.payload(old)?;
        if prior == value {
            return Ok(old);
        }
        let length = prior.len();
        if self.private_extent(old) == Some(length + 16) && length == value.len() {
            return self.patch(old, length + 16, 16, value, 16);
        }
        if length == value.len() && self.private_extent(old).is_none() {
            return self.payload_parts(value, Some(old));
        }
        self.put_payload(value)
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        if !alignment.is_power_of_two() || alignment > 16384 {
            return Err(Error::InvalidInput("invalid image rewrite alignment"));
        }
        if let Some(length) = self.private_extent(old) {
            if length != bytes.len() || !old.offset.is_multiple_of(alignment as u64) {
                return Err(Error::InvalidInput("private image rewrite geometry"));
            }
            if self.read(old, length)? == bytes {
                return Ok(old);
            }
            let offset = old.offset as usize;
            let revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
            self.remember(offset, length)?;
            self.lease_mut().write(offset, bytes)?;
            self.revision = revision;
            return Ok(old);
        }
        let prior = self.base.read(old, bytes.len())?;
        if prior.len() != bytes.len() {
            return Err(Error::Corrupt("short committed rewrite object"));
        }
        if prior == bytes {
            return Ok(old);
        }
        // Complete afterimage from existing checked codec, directly into image.
        self.parts(&[], bytes, alignment, Some(old))
    }
    fn patch(
        &mut self,
        old: Reference,
        length: usize,
        offset: usize,
        value: &[u8],
        alignment: usize,
    ) -> Result<Reference> {
        if !alignment.is_power_of_two()
            || alignment > 16384
            || old.is_null()
            || !old.offset.is_multiple_of(alignment as u64)
        {
            return Err(Error::InvalidInput("invalid image patch alignment"));
        }
        let end = offset
            .checked_add(value.len())
            .ok_or(Error::LimitExceeded)?;
        if end > length {
            return Err(Error::InvalidInput("image patch exceeds object"));
        }
        if let Some(actual) = self.private_extent(old) {
            if actual != length {
                return Err(Error::InvalidInput(
                    "image patch must target complete object",
                ));
            }
            if &self.read(old, length)?[offset..end] == value {
                return Ok(old);
            }
            let start = old.offset as usize;
            let revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
            self.remember(start, length)?;
            self.lease_mut().write(start + offset, value)?;
            self.revision = revision;
            return Ok(old);
        }
        let source = self.base.read(old, length)?;
        if source.len() != length {
            return Err(Error::Corrupt("short committed patch object"));
        }
        if &source[offset..end] == value {
            return Ok(old);
        }
        self.copy_patch(old, length, offset, value, alignment)
    }
}
