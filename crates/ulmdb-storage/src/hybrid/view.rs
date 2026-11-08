//! One structural validator for owned public records and one-call borrowed views.
use super::redo::*;
use crate::{Error, Reference, Result};

#[derive(Clone, Copy, Debug)]
pub(crate) struct PatchSpan {
    pub offset: u32,
    pub length: u32,
}
pub(crate) enum Patches<'a> {
    Owned(&'a [Patch]),
    Borrowed {
        spans: &'a [PatchSpan],
        after: &'a [u8],
    },
}
impl Patches<'_> {
    pub fn len(&self) -> usize {
        match self {
            Self::Owned(p) => p.len(),
            Self::Borrowed { spans, .. } => spans.len(),
        }
    }
}
pub(crate) struct PatchView<'a> {
    pub offset: u32,
    pub bytes: &'a [u8],
}
pub(crate) struct PatchIter<'a> {
    inner: Patches<'a>,
    at: usize,
}
impl<'a> IntoIterator for Patches<'a> {
    type Item = PatchView<'a>;
    type IntoIter = PatchIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        PatchIter { inner: self, at: 0 }
    }
}
impl<'a> Iterator for PatchIter<'a> {
    type Item = PatchView<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.at == self.inner.len() {
            return None;
        }
        let i = self.at;
        self.at += 1;
        Some(match self.inner {
            Patches::Owned(p) => PatchView {
                offset: p[i].offset,
                bytes: &p[i].bytes,
            },
            // Only the private diff builder can issue these checked spans. Their
            // workspace stays immutably borrowed until this recipe is dropped.
            Patches::Borrowed { spans, after } => {
                let p = spans[i];
                PatchView {
                    offset: p.offset,
                    bytes: &after[p.offset as usize..p.offset as usize + p.length as usize],
                }
            }
        })
    }
}
pub(crate) enum Object<'a> {
    Literal {
        dest: Extent,
        bytes: &'a [u8],
    },
    BaseCopy {
        dest: Extent,
        source: &'a Extent,
        receipt: &'a SourceReceipt,
        patches: Patches<'a>,
    },
}
impl Object<'_> {
    pub fn dest(&self) -> Extent {
        match self {
            Self::Literal { dest, .. } | Self::BaseCopy { dest, .. } => *dest,
        }
    }
}
pub(crate) trait View {
    fn header(&self) -> &Record;
    fn object_count(&self) -> usize;
    fn object(&self, index: usize) -> Object<'_>;
    fn partition_at(&self, at: Reference) -> usize {
        let mut lo = 0;
        let mut hi = self.object_count();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.object(mid).dest().at < at {
                lo = mid + 1
            } else {
                hi = mid
            }
        }
        lo
    }
}
impl View for Record {
    fn header(&self) -> &Record {
        self
    }
    fn object_count(&self) -> usize {
        self.objects.len()
    }
    fn object(&self, index: usize) -> Object<'_> {
        match &self.objects[index] {
            ObjectRedo::Literal { dest, bytes } => Object::Literal { dest: *dest, bytes },
            ObjectRedo::BaseCopy {
                dest,
                source,
                receipt,
                patches,
            } => Object::BaseCopy {
                dest: *dest,
                source,
                receipt,
                patches: Patches::Owned(patches),
            },
        }
    }
}
pub(crate) fn validate(view: &impl View) -> Result<()> {
    let header = view.header();

    if header.uuid == [0; 16]
        || header.epoch == 0
        || header.commit == 0
        || header.parent_commit == 0
        || header.commit == header.parent_commit
        || header.lsn
            != header
                .parent
                .lsn
                .checked_add(1)
                .ok_or(Error::LimitExceeded)?
        || header.sequence
            != header
                .parent
                .sequence
                .checked_add(1)
                .ok_or(Error::LimitExceeded)?
        || header.root.next_chunk == 0
        || header.root.next_database == 0
        || view.object_count() > MAX_OBJECTS
        || header.chunks.len() > MAX_CHUNKS
    {
        return Err(Error::Corrupt("hybrid record identity/counts"));
    }
    if !header.root.catalog.is_null()
        && (header.root.catalog.segment == 0
            || header.root.catalog.segment >= header.root.next_chunk
            || header.root.catalog.offset < IMAGE_DATA_START)
    {
        return Err(Error::Corrupt("hybrid root reference bounds"));
    }
    let mut previous = 0;
    for c in &header.chunks {
        if c.chunk <= previous
            || c.chunk >= header.root.next_chunk
            || c.logical_generation == 0
            || c.before < IMAGE_DATA_START
            || c.before >= c.after
            || c.after > c.capacity
            || c.capacity > MAX_CHUNK_BYTES
        {
            return Err(Error::Corrupt("hybrid chunk delta bounds/order"));
        }
        previous = c.chunk;
    }
    let mut previous_dest: Option<Extent> = None;
    let mut materialized_bytes = 0u64;
    for index in 0..view.object_count() {
        let object = view.object(index);
        let d = object.dest();
        d.validate()?;
        materialized_bytes = materialized_bytes
            .checked_add(u64::from(d.length))
            .filter(|n| *n <= MAX_MATERIALIZED_BYTES)
            .ok_or(Error::LimitExceeded)?;
        if let Some(p) = previous_dest
            && (p.at >= d.at || p.overlaps(d)?)
        {
            return Err(Error::Corrupt("hybrid destination order/overlap"));
        }
        previous_dest = Some(d);
        let i = header
            .chunks
            .binary_search_by_key(&d.at.segment, |c| c.chunk)
            .map_err(|_| Error::Corrupt("hybrid destination has no highwater delta"))?;
        let c = header.chunks[i];
        if d.logical_generation != c.logical_generation
            || d.at.offset < c.before
            || d.end()? > c.after
        {
            return Err(Error::Corrupt("hybrid destination is not private tail"));
        }
    }
    for index in 0..view.object_count() {
        let object = view.object(index);
        let d = object.dest();
        match object {
            Object::Literal { bytes, .. } => {
                if bytes.len() != d.length as usize {
                    return Err(Error::Corrupt("hybrid literal length"));
                }
            }
            Object::BaseCopy {
                source,
                receipt,
                patches,
                ..
            } => {
                source.validate()?;
                if source.length != d.length {
                    return Err(Error::Corrupt("hybrid base copy width"));
                }
                validate_source(header, *receipt)?;
                if let Ok(i) = header
                    .chunks
                    .binary_search_by_key(&source.at.segment, |c| c.chunk)
                {
                    let c = header.chunks[i];
                    if source.logical_generation != c.logical_generation || source.end()? > c.before
                    {
                        return Err(Error::Corrupt(
                            "hybrid source is private/foreign generation",
                        ));
                    }
                }
                if patches.len() > MAX_PATCHES {
                    return Err(Error::LimitExceeded);
                }
                let mut end = 0u64;
                for p in patches {
                    let start = u64::from(p.offset);
                    let next = start
                        .checked_add(p.bytes.len() as u64)
                        .ok_or(Error::LimitExceeded)?;
                    if p.bytes.is_empty() || start < end || next > u64::from(d.length) {
                        return Err(Error::Corrupt("hybrid patch order/bounds"));
                    }
                    end = next;
                }
                // Object order makes earlier destinations before this source check relevant
                // too. Generation inequality never authorizes physical range aliasing.
                let at = view.partition_at(source.at);
                if (at < view.object_count() && source.overlaps(view.object(at).dest())?)
                    || (at > 0 && source.overlaps(view.object(at - 1).dest())?)
                {
                    return Err(Error::Corrupt(
                        "hybrid source aliases transaction destinations",
                    ));
                }
            }
        }
    }
    Ok(())
}
