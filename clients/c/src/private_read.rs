//! Immutable loans of a writer's private pages to LMDB 1.0 readonly children.
//!
//! The engine owner remains in its separately allocated `Owned` writer or
//! savepoint. A lease blocks every native mutable borrow and every consuming
//! ancestor operation until all views have ended. No private pages are copied.
use super::*;
use std::{
    ops::Deref,
    sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
pub(super) struct PrivateReaders {
    active: AtomicUsize,
    opened_slots: Mutex<Vec<(u32, AliasId)>>,
}
impl PrivateReaders {
    pub(super) fn active(&self) -> bool {
        self.active.load(Ordering::Acquire) != 0
    }
    pub(super) fn opened_slots(&self) -> FfiResult<std::sync::MutexGuard<'_, Vec<(u32, AliasId)>>> {
        self.opened_slots.lock().map_err(|_| BAD_TXN)
    }
}

pub(super) struct PrivateReadView {
    // Must be destroyed before source and before releasing the writer freeze.
    read_scope: Option<ulmdb::DatabaseReadScope<'static>>,
    source: Option<&'static WriteTxn<'static>>,
    readers: Arc<PrivateReaders>,
}
impl PrivateReadView {
    /// The source's heap allocation and all of its ancestors must remain live
    /// and immutably borrowed until this view drops. The caller must gate all
    /// mutable/consuming accesses with `PrivateReaders::active` and check the
    /// entire exclusive-child chain before consuming an ancestor.
    pub(super) unsafe fn new(
        source: &WriteTxn<'static>,
        readers: Arc<PrivateReaders>,
    ) -> FfiResult<Self> {
        readers
            .active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                count.checked_add(1)
            })
            .map_err(|_| -30792)?;
        // SAFETY: the C transaction ownership protocol above pins the source;
        // readers receive only shared references, including through Savepoint.
        let source = unsafe {
            std::mem::transmute::<&WriteTxn<'static>, &'static WriteTxn<'static>>(source)
        };
        let mut view = Self {
            read_scope: None,
            source: Some(source),
            readers,
        };
        // Establish the RAII lease owner first: a failed scope constructor must
        // end the immutable source loan and balance the reader count normally.
        view.read_scope = Some(source.database_read_scope().map_err(code)?);
        Ok(view)
    }
    pub(super) fn readers(&self) -> &Arc<PrivateReaders> {
        &self.readers
    }
    pub(super) fn read_scope(&self) -> &ulmdb::DatabaseReadScope<'static> {
        self.read_scope.as_ref().expect("active private read scope")
    }
}
impl Deref for PrivateReadView {
    type Target = WriteTxn<'static>;
    fn deref(&self) -> &Self::Target {
        self.source.expect("active private read view")
    }
}
impl Drop for PrivateReadView {
    fn drop(&mut self) {
        // End the shared loan before another thread can resume the writer.
        self.read_scope.take();
        self.source.take();
        let previous = self.readers.active.fetch_sub(1, Ordering::Release);
        debug_assert!(previous != 0);
    }
}

// MDB_NOTLS permits a readonly child to be read and ended on another thread.
// The native writer's MutexGuard stays owned/destroyed on its writer thread;
// only immutable access to the writer is shared.
const _: () = {
    fn require_sync<T: Sync>() {}
    let _ = require_sync::<WriteTxn<'static>>;
    let _ = require_sync::<ulmdb::DatabaseReadScope<'static>>;
};
