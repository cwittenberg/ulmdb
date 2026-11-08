//! Private copy iterators own at most one bounded record. Actual borrowed C
//! values keep transaction pins; these internal streaming copies release each
//! temporary source scope before moving to the next record.
use crate::{Prefix, Result};
use ulmdb_core::storage::Reference;
use ulmdb_storage::SegmentTable;
pub(crate) struct Pairs<'a> {
    table: &'a SegmentTable,
    position: ulmdb_kv::InOrderPosition,
    done: bool,
}
impl<'a> Pairs<'a> {
    pub(crate) fn new(table: &'a SegmentTable, root: Reference) -> Self {
        Self {
            table,
            position: ulmdb_kv::InOrderPosition::new(root),
            done: false,
        }
    }
}
impl Iterator for Pairs<'_> {
    type Item = Result<(Vec<u8>, Vec<u8>)>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let result = (|| {
            let scope = self.table.fresh_read_scope()?;
            Ok(self
                .position
                .next(&scope)?
                .map(|(k, v)| (k.to_vec(), v.to_vec())))
        })();
        match result {
            Ok(Some(pair)) => Some(Ok(pair)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}
pub(crate) struct Subnets<'a> {
    table: &'a SegmentTable,
    position: ulmdb_prefix::SubnetPosition,
    done: bool,
}
impl<'a> Subnets<'a> {
    pub(crate) fn new(table: &'a SegmentTable, root: Reference, query: Prefix) -> Self {
        Self {
            table,
            position: ulmdb_prefix::SubnetPosition::new(root, query),
            done: false,
        }
    }
}
impl Iterator for Subnets<'_> {
    type Item = Result<(Prefix, Vec<u8>)>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let result = (|| {
            let scope = self.table.fresh_read_scope()?;
            Ok(self.position.next(&scope)?.map(|(k, v)| (k, v.to_vec())))
        })();
        match result {
            Ok(Some(pair)) => Some(Ok(pair)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}
