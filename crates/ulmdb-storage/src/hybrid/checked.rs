//! Internal one-call structural proof. No public constructor, cache, Clone or
//! transaction identity shortcut; Record is immutably borrowed for its lifetime.
use super::{
    redo::{DestinationResolver, Record, SourceResolver},
    view::{self, Object, View},
};
use crate::{Error, Result};
pub(crate) struct CheckedRecord<'a, V: View = Record> {
    record: &'a V,
}
impl<'a, V: View> CheckedRecord<'a, V> {
    pub(super) fn new(record: &'a V) -> Result<Self> {
        view::validate(record)?;
        Ok(Self { record })
    }
    pub(super) fn record(&self) -> &'a V {
        self.record
    }
    /// Authenticate each source once and compare its exact prepared afterimage.
    /// No scratch afterimage or per-object source-reference Vec is allocated.
    pub(super) fn verify_materialized(
        &self,
        sources: &impl SourceResolver,
        destinations: &impl DestinationResolver,
    ) -> Result<()> {
        for index in 0..self.record.object_count() {
            let object = self.record.object(index);
            let dest = object.dest();
            // Source-before-destination ordering preserves individual source failures.
            // Multiple simultaneous corruptions may stop at the first object; old public
            // verify_materialized/codec/Journal entrypoints retain their original checks.
            let before = match &object {
                Object::Literal { .. } => None,
                Object::BaseCopy {
                    source, receipt, ..
                } => {
                    let b = sources.resolve_committed(source, receipt)?;
                    if b.len() != source.length as usize {
                        return Err(Error::Corrupt("hybrid source resolution width"));
                    }
                    Some(b)
                }
            };
            let after = destinations.resolve_prepared(&dest)?;
            if after.len() != dest.length as usize {
                return Err(Error::Corrupt("hybrid materialized width"));
            }
            match object {
                Object::Literal { bytes, .. } => {
                    if after != bytes {
                        return Err(Error::Corrupt("hybrid literal materialization mismatch"));
                    }
                }
                Object::BaseCopy { patches, .. } => {
                    let before = before.expect("checked source for BaseCopy");
                    let mut end = 0;
                    for p in patches {
                        let at = p.offset as usize;
                        if after[end..at] != before[end..at]
                            || after[at..at + p.bytes.len()] != *p.bytes
                        {
                            return Err(Error::Corrupt("hybrid patch materialization mismatch"));
                        }
                        end = at + p.bytes.len();
                    }
                    if after[end..] != before[end..] {
                        return Err(Error::Corrupt("hybrid unpatched materialization mismatch"));
                    }
                }
            }
        }
        Ok(())
    }
}
