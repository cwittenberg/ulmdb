//! One-commit borrowed recipe, not source authority. The frozen owner and the
//! exclusively borrowed bounded metadata workspace outlive encoding/publication.
use super::{
    view::{AllocationRef, BodyRef, PatchSpan, Patches, RecordView},
    *,
};
use crate::resident::Prepared;
#[derive(Clone, Copy)]
struct Plan {
    index: usize,
    previous: u64,
    source: Option<VersionKey>,
    start: usize,
    count: usize,
}
#[derive(Default)]
pub struct RecipeWorkspace {
    plans: Vec<Plan>,
    spans: Vec<PatchSpan>,
}
impl RecipeWorkspace {
    pub fn retained_bytes(&self) -> usize {
        self.plans.capacity() * std::mem::size_of::<Plan>()
            + self.spans.capacity() * std::mem::size_of::<PatchSpan>()
    }
}
pub struct BorrowedRecord<'a, 'w> {
    header: Record,
    candidate: &'a Prepared,
    plans: &'w [Plan],
    spans: &'w [PatchSpan],
}
impl BorrowedRecord<'_, '_> {
    pub fn metadata(&self) -> &Record {
        &self.header
    }
}
impl RecordView for BorrowedRecord<'_, '_> {
    fn header(&self) -> &Record {
        &self.header
    }
    fn allocation_count(&self) -> usize {
        self.plans.len()
    }
    fn allocation(&self, i: usize) -> AllocationRef<'_> {
        let p = self.plans[i];
        let f = self
            .candidate
            .slot(p.index)
            .expect("builder-bound frozen slot");
        AllocationRef {
            destination: f.key,
            previous_generation: p.previous,
            ranges: f.ranges,
            body: match p.source {
                None => BodyRef::Literal(f.bytes),
                Some(source) => BodyRef::Copy {
                    source,
                    patches: Patches::Borrowed {
                        spans: &self.spans[p.start..p.start + p.count],
                        after: f.bytes,
                    },
                },
            },
        }
    }
}
pub struct RecipeBuilder<'a, 'w> {
    header: Record,
    candidate: &'a Prepared,
    workspace: &'w mut RecipeWorkspace,
}
impl<'a, 'w> RecipeBuilder<'a, 'w> {
    pub fn new(
        header: Record,
        candidate: &'a Prepared,
        workspace: &'w mut RecipeWorkspace,
    ) -> Result<Self> {
        if !header.allocations.is_empty() {
            return Err(Error::InvalidInput("borrowed RL2 header owns allocations"));
        }
        // Bounded metadata is reserved before mutation and retained only within the
        // protocol's frame cap, including previous larger transactions.
        if workspace.retained_bytes() > MAX_RECORD_BYTES {
            *workspace = RecipeWorkspace::default();
        }
        workspace.plans.clear();
        workspace.spans.clear();
        Ok(Self {
            header,
            candidate,
            workspace,
        })
    }
    pub fn literal(&mut self, index: usize, previous: u64) -> Result<()> {
        self.push(Plan {
            index,
            previous,
            source: None,
            start: 0,
            count: 0,
        })
    }
    pub fn committed(
        &mut self,
        index: usize,
        previous: u64,
        source: VersionKey,
        before: &[u8],
    ) -> Result<()> {
        let after = self
            .candidate
            .slot(index)
            .ok_or(Error::InvalidInput("borrowed RL2 frozen index"))?
            .bytes;
        if before.len() != PAGE || after.len() != PAGE {
            return Err(Error::InvalidInput("borrowed RL2 copy width"));
        }
        let start = self.workspace.spans.len();
        if let Err(e) = self.diff(before, after) {
            self.workspace.spans.truncate(start);
            return Err(e);
        }
        let count = self.workspace.spans.len() - start;
        let cost = self.workspace.spans[start..]
            .iter()
            .try_fold(16usize, |n, p| {
                n.checked_add(8)?
                    .checked_add((p.length as usize).next_multiple_of(8))
            })
            .ok_or(Error::LimitExceeded)?;
        if cost >= PAGE {
            self.workspace.spans.truncate(start);
            self.literal(index, previous)
        } else {
            self.push(Plan {
                index,
                previous,
                source: Some(source),
                start,
                count,
            })
        }
    }
    pub fn finish(self) -> BorrowedRecord<'a, 'w> {
        BorrowedRecord {
            header: self.header,
            candidate: self.candidate,
            plans: &self.workspace.plans,
            spans: &self.workspace.spans,
        }
    }
    fn push(&mut self, p: Plan) -> Result<()> {
        let frozen = self
            .candidate
            .slot(p.index)
            .ok_or(Error::InvalidInput("borrowed RL2 frozen index"))?;
        if self.workspace.plans.len() == MAX_SLOTS
            || self
                .workspace
                .plans
                .last()
                .is_some_and(|a| self.candidate.slot(a.index).unwrap().key.slot >= frozen.key.slot)
        {
            return Err(Error::InvalidInput("borrowed RL2 plan order/count"));
        }
        if self.workspace.plans.len() == self.workspace.plans.capacity() {
            let target = self
                .workspace
                .plans
                .capacity()
                .saturating_mul(2)
                .clamp(16, MAX_SLOTS);
            if target * std::mem::size_of::<Plan>()
                + self.workspace.spans.capacity() * std::mem::size_of::<PatchSpan>()
                > MAX_RECORD_BYTES
            {
                return Err(Error::LimitExceeded);
            }
            self.workspace
                .plans
                .try_reserve_exact(target - self.workspace.plans.len())
                .map_err(|_| Error::LimitExceeded)?;
            if self.workspace.retained_bytes() > MAX_RECORD_BYTES {
                *self.workspace = RecipeWorkspace::default();
                return Err(Error::LimitExceeded);
            }
        }
        self.workspace.plans.push(p);
        Ok(())
    }
    fn diff(&mut self, before: &[u8], after: &[u8]) -> Result<()> {
        let base = self.workspace.spans.len();
        let mut i = 0;
        while i < PAGE {
            let end = i + (PAGE - i).min(32);
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
            while i < PAGE && before[i] != after[i] {
                i += 1;
            }
            if self.workspace.spans.len() - base == PAGE / 2 {
                return Err(Error::LimitExceeded);
            }
            if self.workspace.spans.len() == self.workspace.spans.capacity() {
                let available = (MAX_RECORD_BYTES
                    - self.workspace.plans.capacity() * std::mem::size_of::<Plan>())
                    / std::mem::size_of::<PatchSpan>();
                let target = self
                    .workspace
                    .spans
                    .capacity()
                    .saturating_mul(2)
                    .max(64)
                    .min(available);
                if target <= self.workspace.spans.len() {
                    return Err(Error::LimitExceeded);
                }
                self.workspace
                    .spans
                    .try_reserve_exact(target - self.workspace.spans.len())
                    .map_err(|_| Error::LimitExceeded)?;
                if self.workspace.retained_bytes() > MAX_RECORD_BYTES {
                    *self.workspace = RecipeWorkspace::default();
                    return Err(Error::LimitExceeded);
                }
            }
            self.workspace.spans.push(PatchSpan {
                offset: start as u32,
                length: (i - start) as u32,
            });
        }
        Ok(())
    }
}
/// Private immutable canonical validation token. It cannot outlive either frozen
/// byte owner or the exclusive recipe workspace, and binds the exact Boot.
pub(crate) struct CheckedBorrowed<'r, 'a, 'w> {
    pub(super) boot: Boot,
    pub(super) record: &'r BorrowedRecord<'a, 'w>,
    pub(super) receipt: WalReceipt,
}
pub(crate) fn encode_into<'r, 'a, 'w>(
    boot: Boot,
    record: &'r BorrowedRecord<'a, 'w>,
    limit: usize,
    bytes: &mut Vec<u8>,
) -> Result<CheckedBorrowed<'r, 'a, 'w>> {
    let digest = super::wire::encode_view_into(boot, record, limit, bytes)?;
    let r = record.metadata();
    let receipt = WalReceipt {
        lsn: r.lsn,
        sequence: r.sequence,
        offset: r.parent.end()?,
        length: bytes.len() as u64,
        digest,
    };
    receipt.end()?;
    Ok(CheckedBorrowed {
        boot,
        record,
        receipt,
    })
}

impl CheckedBorrowed<'_, '_, '_> {
    pub(crate) fn receipt(&self) -> WalReceipt {
        self.receipt
    }
}
