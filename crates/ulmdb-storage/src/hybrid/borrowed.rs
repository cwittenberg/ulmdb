//! Private-byte live recipe. No source authority is issued by this builder.
//! Objects borrow a frozen destination; only compact range metadata is retained.
use super::{
    codec::MAX_RECORD_BYTES,
    redo::{Extent, MAX_OBJECT_BYTES, MAX_OBJECTS, MAX_PATCHES, Record, SourceReceipt},
    view::{Object, PatchSpan, Patches, View},
};
use crate::{Error, Result};

#[derive(Default)]
pub struct RecipeWorkspace {
    spans: Vec<PatchSpan>,
}
impl RecipeWorkspace {
    pub fn retained_bytes(&self) -> usize {
        self.spans.capacity() * std::mem::size_of::<PatchSpan>()
    }
}
// Keep a single contiguous metadata allocation; boxing copy metadata would
// reintroduce one allocation and pointer chase per committed object.
#[allow(clippy::large_enum_variant)]
enum RecipeObject<'a> {
    Literal {
        dest: Extent,
        after: &'a [u8],
    },
    BaseCopy {
        dest: Extent,
        source: Extent,
        receipt: SourceReceipt,
        after: &'a [u8],
        start: usize,
        count: usize,
    },
}
/// Only a builder can construct this opaque one-call recipe. It cannot outlive
/// frozen afterimages or the exclusively borrowed patch workspace. It is not a
/// validation/source capability and is rechecked by Journal before every IO.
pub struct BorrowedRecord<'a, 'w> {
    header: Record,
    objects: Vec<RecipeObject<'a>>,
    spans: &'w [PatchSpan],
}
impl BorrowedRecord<'_, '_> {
    pub fn metadata(&self) -> &Record {
        &self.header
    }
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }
    pub fn destinations(&self) -> impl ExactSizeIterator<Item = Extent> + '_ {
        (0..self.objects.len()).map(|i| self.object(i).dest())
    }
}
impl View for BorrowedRecord<'_, '_> {
    fn header(&self) -> &Record {
        &self.header
    }
    fn object_count(&self) -> usize {
        self.objects.len()
    }
    fn object(&self, index: usize) -> Object<'_> {
        match &self.objects[index] {
            RecipeObject::Literal { dest, after } => Object::Literal {
                dest: *dest,
                bytes: after,
            },
            RecipeObject::BaseCopy {
                dest,
                source,
                receipt,
                after,
                start,
                count,
            } => Object::BaseCopy {
                dest: *dest,
                source,
                receipt,
                patches: Patches::Borrowed {
                    spans: &self.spans[*start..*start + *count],
                    after,
                },
            },
        }
    }
}
pub struct RecipeBuilder<'a, 'w> {
    header: Record,
    objects: Vec<RecipeObject<'a>>,
    workspace: &'w mut RecipeWorkspace,
    span_limit: usize,
}
impl<'a, 'w> RecipeBuilder<'a, 'w> {
    /// Header must carry no owned objects. Its scalar/chunk values are validated
    /// together with this view, never encoded as a standalone empty Record.
    /// Workspace policy bounds retained patch metadata, including failed builds.
    pub fn new(
        header: Record,
        workspace: &'w mut RecipeWorkspace,
        objects: usize,
        workspace_bytes: usize,
    ) -> Result<Self> {
        if !header.objects.is_empty() {
            return Err(Error::InvalidInput(
                "borrowed header contains owned objects",
            ));
        }
        if objects > MAX_OBJECTS {
            return Err(Error::LimitExceeded);
        }
        let limit = workspace_bytes.min(MAX_RECORD_BYTES) / std::mem::size_of::<PatchSpan>();
        if workspace.spans.capacity() > limit {
            workspace.spans = Vec::new();
        }
        workspace.spans.clear();
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(objects)
            .map_err(|_| Error::LimitExceeded)?;
        Ok(Self {
            header,
            objects: entries,
            workspace,
            span_limit: limit,
        })
    }
    pub fn literal(&mut self, dest: Extent, after: &'a [u8]) -> Result<()> {
        self.push(RecipeObject::Literal { dest, after })
    }
    /// Only exact committed source provenance is accepted by the later resolver.
    /// Before bytes are used for an ordinary final-diff, not retained or trusted
    /// as authority. Private sources must use a literal.
    pub fn committed(
        &mut self,
        dest: Extent,
        source: Extent,
        receipt: SourceReceipt,
        before: &[u8],
        after: &'a [u8],
    ) -> Result<()> {
        if before.len() != source.length as usize
            || after.len() != dest.length as usize
            || source.length != dest.length
        {
            return Err(Error::InvalidInput("hybrid committed copy widths"));
        }
        let start = self.workspace.spans.len();
        let result = self.diff(before, after);
        if result.is_err() {
            self.workspace.spans.truncate(start);
        }
        result?;
        let count = self.workspace.spans.len() - start;
        let cost = self.workspace.spans[start..]
            .iter()
            .try_fold(208usize, |n, p| {
                n.checked_add(8)?.checked_add(p.length as usize)
            })
            .ok_or(Error::LimitExceeded)?;
        if cost >= after.len() {
            self.workspace.spans.truncate(start);
            self.literal(dest, after)
        } else {
            self.push(RecipeObject::BaseCopy {
                dest,
                source,
                receipt,
                after,
                start,
                count,
            })
        }
    }
    pub fn finish(self) -> BorrowedRecord<'a, 'w> {
        BorrowedRecord {
            header: self.header,
            objects: self.objects,
            spans: &self.workspace.spans,
        }
    }
    fn push(&mut self, object: RecipeObject<'a>) -> Result<()> {
        if self.objects.len() == MAX_OBJECTS {
            return Err(Error::LimitExceeded);
        }
        self.objects
            .try_reserve(1)
            .map_err(|_| Error::LimitExceeded)?;
        self.objects.push(object);
        Ok(())
    }
    fn diff(&mut self, before: &[u8], after: &[u8]) -> Result<()> {
        if before.len() != after.len() || after.len() > MAX_OBJECT_BYTES as usize {
            return Err(Error::InvalidInput("hybrid diff widths"));
        }
        let base = self.workspace.spans.len();
        let mut i = 0;
        while i < after.len() {
            let end = i + (after.len() - i).min(32);
            if end - i == 32 && before[i..end] == after[i..end] {
                i = end;
                continue;
            }
            while i < end && before[i] == after[i] {
                i += 1;
            }
            if i == end {
                continue;
            }
            let start = i;
            i += 1;
            while i < after.len() && before[i] != after[i] {
                i += 1;
            }
            if self.workspace.spans.len() - base == MAX_PATCHES
                || self.workspace.spans.len() == self.span_limit
            {
                return Err(Error::LimitExceeded);
            }
            if self.workspace.spans.len() == self.workspace.spans.capacity() {
                let target = self
                    .workspace
                    .spans
                    .capacity()
                    .saturating_mul(2)
                    .max(64)
                    .min(self.span_limit);
                self.workspace
                    .spans
                    .try_reserve_exact(target - self.workspace.spans.len())
                    .map_err(|_| Error::LimitExceeded)?;
            }
            self.workspace.spans.push(PatchSpan {
                offset: start as u32,
                length: (i - start) as u32,
            });
        }
        Ok(())
    }
}
