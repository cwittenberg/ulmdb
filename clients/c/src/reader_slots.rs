//! LMDB's thread-owned reader slots and MDB_NOTLS transaction-owned slots.
use super::*;
use std::cell::RefCell;
use std::sync::Weak;

pub(super) struct ThreadSlot {
    thread: ThreadId,
    lease: process::ReaderLease,
    active: bool,
}

struct ThreadCleanup(Vec<(Weak<Shared>, ThreadId)>);
impl Drop for ThreadCleanup {
    fn drop(&mut self) {
        for (environment, thread) in &self.0 {
            if let Some(shared) = environment.upgrade()
                && let Ok(mut slots) = shared.reader_threads.lock()
            {
                slots.retain(|slot| slot.thread != *thread);
            }
        }
    }
}
thread_local! {
    static CLEANUP: RefCell<ThreadCleanup> = const { RefCell::new(ThreadCleanup(Vec::new())) };
}

pub(super) fn acquire(
    shared: &Arc<Shared>,
    notls: bool,
    nolock: bool,
    sequence: u64,
    transaction_lease: &mut Option<process::ReaderLease>,
) -> FfiResult<()> {
    if nolock {
        return Ok(());
    }
    if notls {
        if let Some(lease) = transaction_lease {
            lease.update(Some(sequence))?;
        } else {
            *transaction_lease = Some(process::ReaderLease::acquire(shared, sequence)?);
        }
    } else {
        let thread = std::thread::current().id();
        let mut slots = shared.reader_threads.lock().map_err(|_| BAD_TXN)?;
        if let Some(slot) = slots.iter_mut().find(|slot| slot.thread == thread) {
            if slot.active {
                return Err(-30783); // MDB_BAD_RSLOT: this thread already owns a reader.
            }
            slot.lease.update(Some(sequence))?;
            slot.active = true;
        } else {
            let lease = process::ReaderLease::acquire(shared, sequence)?;
            slots.push(ThreadSlot {
                thread,
                lease,
                active: true,
            });
            CLEANUP.with(|cleanup| {
                let mut cleanup = cleanup.borrow_mut();
                cleanup
                    .0
                    .retain(|(environment, _)| environment.strong_count() != 0);
                cleanup.0.push((Arc::downgrade(shared), thread));
            });
        }
    }
    shared
        .readers
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Once a slot is acquired, every capacity-check Result/unwind exit must
/// release its active authority while retaining the published high-water mark.
pub(super) struct Setup<'a> {
    shared: &'a Arc<Shared>,
    lease: &'a mut Option<process::ReaderLease>,
    notls: bool,
    nolock: bool,
    finished: bool,
    armed: bool,
}
pub(super) fn setup<'a>(
    shared: &'a Arc<Shared>,
    notls: bool,
    nolock: bool,
    sequence: u64,
    lease: &'a mut Option<process::ReaderLease>,
) -> FfiResult<Setup<'a>> {
    let finished = lease.is_none();
    acquire(shared, notls, nolock, sequence, lease)?;
    Ok(Setup {
        shared,
        lease,
        notls,
        nolock,
        finished,
        armed: true,
    })
}
impl Setup<'_> {
    pub(super) fn finish(mut self) {
        self.armed = false;
    }
}
impl Drop for Setup<'_> {
    fn drop(&mut self) {
        if self.armed && self.shared.creator == std::process::id() {
            release_fields(
                self.shared,
                std::thread::current().id(),
                self.notls,
                self.nolock,
                self.lease,
                self.finished,
            );
        }
    }
}
fn release_fields(
    shared: &Shared,
    thread: ThreadId,
    notls: bool,
    nolock: bool,
    reader_lease: &mut Option<process::ReaderLease>,
    finished: bool,
) {
    if nolock {
        return;
    }
    if notls {
        if finished {
            reader_lease.take();
        } else if let Some(lease) = reader_lease {
            let _ = lease.update(None);
        }
    } else if let Ok(mut slots) = shared.reader_threads.lock()
        && let Some(slot) = slots.iter_mut().find(|slot| slot.thread == thread)
    {
        slot.active = false;
        let _ = slot.lease.update(None);
    }
    shared
        .readers
        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
}
pub(super) fn release(txn: &mut MDB_txn, finished: bool) {
    release_fields(
        &txn.shared,
        txn.thread,
        txn.notls,
        unsafe { (*txn.env).flags() } & 0x400000 != 0,
        &mut txn.reader_lease,
        finished,
    );
}
