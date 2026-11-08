//! Source-observed pure metadata on a live dirty pending owner. No data access.
//! Upstream's public preparation prose only guarantees final commit/abort.
use super::*;
use std::{cmp::Ordering, collections::BTreeMap};

pub(super) struct Metadata {
    id: AliasId,
    flags: u32,
    main: bool,
}
pub(super) struct PreparedOwner {
    pub transaction: PreparedWriteTxn<'static>,
    pub metadata: BTreeMap<u32, Metadata>,
}
impl MDB_txn {
    pub(super) fn capture_prepared_metadata(&self) -> FfiResult<BTreeMap<u32, Metadata>> {
        self.live()?;
        let valid = self.dbis.lock().map_err(|_| BAD_TXN)?.valid.clone();
        let bindings: Vec<_> = self
            .shared
            .handles
            .lock()
            .map_err(|_| BAD_TXN)?
            .iter()
            .enumerate()
            .filter_map(|(slot, handle)| {
                handle
                    .as_ref()
                    .filter(|h| h.name() == MAIN || valid.get(&(slot as u32)) == Some(&h.id()))
                    .map(|h| (slot as u32, h.clone()))
            })
            .collect();
        let mut result = BTreeMap::new();
        for (slot, handle) in bindings {
            let flags = self.db_flags(&handle)?;
            result.insert(
                slot,
                Metadata {
                    id: handle.id(),
                    flags,
                    main: handle.name() == MAIN,
                },
            );
        }
        Ok(result)
    }
    fn pending_metadata(&self, slot: u32) -> FfiResult<Option<&Metadata>> {
        let Some(State::Prepared(owner)) = &self.state else {
            return Ok(None);
        };
        if !self.correct_thread()
            || self.readonly
            || !self.parent.is_null()
            || !self.child.is_null()
            || self.capacity_failed
            || self.private_readers.as_ref().is_some_and(|r| r.active())
        {
            return Err(BAD_TXN);
        }
        let metadata = owner.metadata.get(&slot).ok_or(BAD_DBI)?;
        if !self
            .shared
            .handles
            .lock()
            .map_err(|_| BAD_TXN)?
            .get(slot as usize)
            .and_then(Option::as_ref)
            .is_some_and(|h| h.id() == metadata.id)
        {
            return Err(BAD_DBI);
        }
        Ok(Some(metadata))
    }
    pub(super) fn pending_flags(&self, slot: u32) -> FfiResult<Option<u32>> {
        Ok(self.pending_metadata(slot)?.map(|m| m.flags))
    }
    pub(super) fn pending_order(
        &self,
        slot: u32,
        a: &[u8],
        b: &[u8],
        duplicate: bool,
    ) -> FfiResult<Option<Ordering>> {
        let Some(metadata) = self.pending_metadata(slot)? else {
            return Ok(None);
        };
        // Callbacks remain environment-wide. Never hold this lock while calling
        // application code, and never consult a native tree in pending state.
        let callback_key = if metadata.main {
            compat_features::MAIN_CALLBACK_KEY
        } else {
            metadata.id
        };
        let options = self
            .shared
            .options
            .lock()
            .map_err(|_| BAD_TXN)?
            .get(&callback_key)
            .cloned()
            .unwrap_or_default();
        let callback = if duplicate {
            options.dup_cmp
        } else {
            options.key_cmp
        };
        if let Some(callback) = callback {
            return Ok(Some(compat_features::invoke(callback, a, b)));
        }
        let flags = if metadata.main {
            metadata.flags
        } else {
            options.flags.unwrap_or(metadata.flags)
        };
        Ok(Some(if !duplicate && flags & 2 != 0 {
            a.iter().rev().cmp(b.iter().rev())
        } else if flags & if duplicate { 0x20 } else { 8 } != 0 {
            compat_features::integer(a).cmp(&compat_features::integer(b))
        } else if flags & if duplicate { 0x40 } else { 2 } != 0 {
            a.iter().rev().cmp(b.iter().rev())
        } else {
            a.cmp(b)
        }))
    }
}
