//! Opt-in thread-scoped work observation for cold admission and background work.
//!
//! Ordinary index reads do not consult this scope. The const TLS slot is one
//! pointer and needs no destructor; private lexical guards own every live box.
//! Nested calls and unwinding restore ownership before invoking caller code.
use crate::storage::Result;
#[cfg(target_has_atomic = "64")]
use std::sync::atomic::{AtomicU64, Ordering};
use std::{cell::Cell, marker::PhantomData, mem::ManuallyDrop, rc::Rc, sync::Arc};

pub const CHUNK_BYTES: usize = 64 * 1024;

/// Called before the described unit of native work. Implementations can stop
/// with an error or wait for a caller to grant another cooperative time slice.
/// One kernel call or caller-supplied callback cannot be preempted mid-call.
pub trait WorkObserver: Send + Sync {
    fn checkpoint(&self, units: u64, bytes: u64) -> Result<()>;
}

type Observer = Arc<dyn WorkObserver>;
struct Context {
    pid: u32,
    observer: Option<Observer>,
    critical: bool,
}
type Slot = Cell<ManuallyDrop<Option<Box<Context>>>>;
thread_local! {
    // No TLS destructor or destroyed-key phase on native const TLS. Scopes
    // explicitly free ordinary contexts; inherited contexts are never dropped.
    static CONTEXT: Slot = const { Cell::new(ManuallyDrop::new(None)) };
}

// High 32 bits identify the owning process; low 32 bits count occupied thread
// contexts, including suppressed ones. Nested scopes do not change this count.
// PID and count change in one CAS, so a child reset cannot erase fresh scopes
// installed concurrently by another child thread.
#[cfg(target_has_atomic = "64")]
static ACTIVE: AtomicU64 = AtomicU64::new(0);
#[cfg(target_has_atomic = "64")]
fn packed(pid: u32, count: u32) -> u64 {
    (u64::from(pid) << 32) | u64::from(count)
}
#[cfg(target_has_atomic = "64")]
fn owner(state: u64) -> u32 {
    (state >> 32) as u32
}
#[cfg(target_has_atomic = "64")]
#[inline]
fn any_context() -> bool {
    ACTIVE.load(Ordering::Acquire) as u32 != 0
}
#[cfg(target_has_atomic = "64")]
fn reset_foreign_count(pid: u32) {
    let mut state = ACTIVE.load(Ordering::Acquire);
    while owner(state) != pid {
        match ACTIVE.compare_exchange_weak(
            state,
            packed(pid, 0),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return,
            Err(current) => state = current,
        }
    }
}
#[cfg(target_has_atomic = "64")]
fn add_context(pid: u32) {
    let mut state = ACTIVE.load(Ordering::Acquire);
    loop {
        // Check before publishing anything. Exhaustion cannot wrap to zero and
        // accidentally disable observation for existing contexts.
        let count = if owner(state) == pid {
            (state as u32)
                .checked_add(1)
                .expect("too many active work-observer threads")
        } else {
            1
        };
        match ACTIVE.compare_exchange_weak(
            state,
            packed(pid, count),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return,
            Err(current) => state = current,
        }
    }
}
#[cfg(target_has_atomic = "64")]
fn remove_context(pid: u32) {
    let mut state = ACTIVE.load(Ordering::Acquire);
    loop {
        if owner(state) != pid || state as u32 == 0 {
            return;
        }
        match ACTIVE.compare_exchange_weak(
            state,
            packed(pid, state as u32 - 1),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return,
            Err(current) => state = current,
        }
    }
}

// Without 64-bit atomics, reset ownership is explicit rather than encoded in a
// reserved PID value. Threads in one process share a PID. A child can steal an
// initializer inherited from its parent, while stale current-process contenders
// recheck the published owner before resetting any fresh context count.
#[cfg(any(not(target_has_atomic = "64"), test))]
mod resettable {
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

    pub(super) struct Counter {
        owner: AtomicU32,
        reset_owner: AtomicU32,
        initializing: AtomicBool,
        count: AtomicUsize,
    }
    impl Counter {
        pub(super) const fn new() -> Self {
            Self {
                owner: AtomicU32::new(0),
                reset_owner: AtomicU32::new(0),
                initializing: AtomicBool::new(false),
                count: AtomicUsize::new(0),
            }
        }
        pub(super) fn any(&self) -> bool {
            self.count.load(Ordering::Acquire) != 0
        }
        fn finish_reset(&self, pid: u32, inherited: bool) {
            // A delayed ordinary contender may acquire the flag after another
            // initializer has published this PID and admitted fresh scopes.
            // Only an inherited ownership claim resets unconditionally.
            if inherited || self.owner.load(Ordering::Acquire) != pid {
                self.count.store(0, Ordering::Release);
                self.owner.store(pid, Ordering::Release);
            }
            self.initializing.store(false, Ordering::Release);
        }
        fn try_normal_reset(&self, pid: u32) -> bool {
            // Publish ownership before acquiring the flag. A child must never
            // mistake a current-process initializer for an inherited one.
            self.reset_owner.store(pid, Ordering::Release);
            if self
                .initializing
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return false;
            }
            self.finish_reset(pid, false);
            true
        }
        fn try_inherited_reset(&self, pid: u32, inherited_owner: u32) -> bool {
            if inherited_owner == pid
                || self
                    .reset_owner
                    .compare_exchange(inherited_owner, pid, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
            {
                return false;
            }
            // Only one child thread can replace the foreign ownership marker.
            // Other child threads wait until this publication is complete.
            self.finish_reset(pid, true);
            true
        }
        pub(super) fn reset_for(&self, pid: u32) {
            let mut spins = 0u8;
            loop {
                let owner = self.owner.load(Ordering::Acquire);
                let initializing = self.initializing.load(Ordering::Acquire);
                if owner == pid && !initializing {
                    return;
                }
                if !initializing {
                    if self.try_normal_reset(pid) {
                        return;
                    }
                } else {
                    let inherited_owner = self.reset_owner.load(Ordering::Acquire);
                    if self.try_inherited_reset(pid, inherited_owner) {
                        return;
                    }
                }
                if spins < 16 {
                    spins += 1;
                    std::hint::spin_loop();
                } else {
                    std::thread::yield_now();
                }
            }
        }
        pub(super) fn add(&self, pid: u32) {
            self.reset_for(pid);
            let mut count = self.count.load(Ordering::Acquire);
            loop {
                let next = count
                    .checked_add(1)
                    .expect("too many active work-observer threads");
                match self.count.compare_exchange_weak(
                    count,
                    next,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => return,
                    Err(current) => count = current,
                }
            }
        }
        pub(super) fn remove(&self, pid: u32) {
            self.reset_for(pid);
            let mut count = self.count.load(Ordering::Acquire);
            loop {
                if count == 0 {
                    return;
                }
                match self.count.compare_exchange_weak(
                    count,
                    count - 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => return,
                    Err(current) => count = current,
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::{Arc, Barrier};

        fn inherited(owner: u32, count: usize, initializing: bool) -> Counter {
            Counter {
                owner: AtomicU32::new(owner),
                reset_owner: AtomicU32::new(owner),
                initializing: AtomicBool::new(initializing),
                count: AtomicUsize::new(count),
            }
        }
        fn count(counter: &Counter) -> usize {
            counter.count.load(Ordering::Acquire)
        }

        #[test]
        fn ordinary_first_and_last_contexts_enable_and_disable_the_gate() {
            let counter = Counter::new();
            assert!(!counter.any());
            // Neither zero nor any other PID value is reserved for a lock.
            for pid in [0, u32::MAX] {
                counter.add(pid);
                counter.add(pid);
                counter.remove(pid);
                assert!(counter.any());
                counter.remove(pid);
                assert!(!counter.any());
                counter.remove(pid);
                assert!(!counter.any());
                assert_eq!(counter.owner.load(Ordering::Acquire), pid);
            }
        }

        #[test]
        fn foreign_retirement_disables_the_gate_before_another_suppressed_scope() {
            let counter = inherited(u32::MAX, 7, false);
            counter.reset_for(0);
            assert_eq!(count(&counter), 0);
            assert!(!counter.any());
            // without_observer uses this false gate to execute directly, so an
            // unscoped critical call cannot acquire a suppressed context.
            counter.add(0);
            assert_eq!(count(&counter), 1);
            counter.remove(0);
            assert!(!counter.any());
        }

        #[test]
        fn overflow_fails_before_publication_without_a_permanent_sentinel() {
            let counter = inherited(2, usize::MAX, false);
            let body_ran = AtomicBool::new(false);
            let stopped = std::panic::catch_unwind(|| {
                counter.add(2);
                body_ran.store(true, Ordering::Release);
            });
            assert!(stopped.is_err());
            assert!(!body_ran.load(Ordering::Acquire));
            assert_eq!(count(&counter), usize::MAX);
            counter.remove(2);
            assert_eq!(count(&counter), usize::MAX - 1);
            counter.add(2);
            assert_eq!(count(&counter), usize::MAX);
        }

        #[test]
        fn concurrent_first_child_entries_reset_once_and_retain_all_live_scopes() {
            let counter = Arc::new(inherited(1, 17, false));
            let barrier = Arc::new(Barrier::new(7));
            let mut workers = Vec::new();
            for _ in 0..6 {
                let counter = counter.clone();
                let barrier = barrier.clone();
                workers.push(std::thread::spawn(move || {
                    barrier.wait();
                    counter.add(2);
                    barrier.wait();
                    barrier.wait();
                    counter.remove(2);
                }));
            }
            barrier.wait();
            barrier.wait();
            assert_eq!(count(&counter), 6);
            assert_eq!(counter.owner.load(Ordering::Acquire), 2);
            barrier.wait();
            for worker in workers {
                worker.join().unwrap();
            }
            assert!(!counter.any());
        }

        #[test]
        fn child_recovers_an_initializer_inherited_while_busy() {
            let counter = inherited(1, 7, true);
            counter.reset_for(2);
            assert_eq!(counter.owner.load(Ordering::Acquire), 2);
            assert_eq!(counter.reset_owner.load(Ordering::Acquire), 2);
            assert!(!counter.initializing.load(Ordering::Acquire));
            assert!(!counter.any());
            counter.add(2);
            counter.remove(2);
            assert!(!counter.any());
        }

        #[test]
        fn stale_normal_initializer_cannot_erase_a_published_child_scope() {
            let counter = inherited(1, 7, false);
            // The delayed contender previously saw foreign owner + false flag.
            // Another child initializer completes and admits one live context.
            counter.add(2);
            assert!(counter.try_normal_reset(2));
            assert_eq!(count(&counter), 1);
            counter.remove(2);
            assert!(!counter.any());
        }

        #[test]
        fn stale_inherited_steal_cannot_erase_a_published_child_scope() {
            let counter = inherited(1, 7, true);
            let observed_foreign_owner = counter.reset_owner.load(Ordering::Acquire);
            assert!(counter.try_inherited_reset(2, observed_foreign_owner));
            counter.add(2);
            assert!(!counter.try_inherited_reset(2, observed_foreign_owner));
            assert_eq!(count(&counter), 1);
            counter.remove(2);
            assert!(!counter.any());
        }

        #[test]
        fn marker_prestore_cannot_steal_another_current_process_initializer() {
            let counter = inherited(1, 7, true);
            counter.reset_owner.store(2, Ordering::Release);
            // A same-PID initializer owns the flag and is paused before finish.
            assert!(!counter.try_normal_reset(2));
            assert!(counter.initializing.load(Ordering::Acquire));
            assert_eq!(counter.reset_owner.load(Ordering::Acquire), 2);
            assert_eq!(count(&counter), 7);
            counter.finish_reset(2, false);
            counter.add(2);
            assert_eq!(count(&counter), 1);
            counter.remove(2);
            assert!(!counter.any());
        }
    }
}
#[cfg(not(target_has_atomic = "64"))]
static ACTIVE_RESETTABLE: resettable::Counter = resettable::Counter::new();
#[cfg(not(target_has_atomic = "64"))]
#[inline]
fn any_context() -> bool {
    ACTIVE_RESETTABLE.any()
}
#[cfg(not(target_has_atomic = "64"))]
fn reset_foreign_count(pid: u32) {
    ACTIVE_RESETTABLE.reset_for(pid);
}
#[cfg(not(target_has_atomic = "64"))]
fn add_context(pid: u32) {
    ACTIVE_RESETTABLE.add(pid);
}
#[cfg(not(target_has_atomic = "64"))]
fn remove_context(pid: u32) {
    ACTIVE_RESETTABLE.remove(pid);
}

// Moving the Box out avoids RefCell borrows and unchecked pointers. This guard
// restores the slot even if an internal checked operation unwinds. No caller
// callback or user-owned destructor runs while the slot is taken.
struct Taken<'a> {
    slot: &'a Slot,
    context: Option<Box<Context>>,
}
impl<'a> Taken<'a> {
    fn new(slot: &'a Slot) -> Self {
        Self {
            slot,
            context: ManuallyDrop::into_inner(slot.replace(ManuallyDrop::new(None))),
        }
    }
    fn forget_foreign(&mut self, pid: u32) {
        if self
            .context
            .as_ref()
            .is_some_and(|context| context.pid != pid)
        {
            // A child must not run inherited observer destructors, which may
            // own gates held by threads that disappeared at fork.
            std::mem::forget(self.context.take());
        }
    }
}
impl Drop for Taken<'_> {
    fn drop(&mut self) {
        self.slot.set(ManuallyDrop::new(self.context.take()));
    }
}

fn forget_foreign_context(pid: u32) {
    let _ = CONTEXT.try_with(|slot| {
        let mut taken = Taken::new(slot);
        taken.forget_foreign(pid);
    });
    reset_foreign_count(pid);
}

/// A short namespace/pin bootstrap section may stop on cancellation or limits,
/// but must not park a worker or invoke caller code while owning native gates.
/// Only an already scoped operation needs critical-section bookkeeping.
pub fn in_critical() -> bool {
    if !any_context() {
        return false;
    }
    let pid = std::process::id();
    reset_foreign_count(pid);
    CONTEXT
        .try_with(|slot| {
            let mut taken = Taken::new(slot);
            taken.forget_foreign(pid);
            taken
                .context
                .as_ref()
                .is_some_and(|context| context.critical)
        })
        .expect("work-observer TLS is unavailable")
}

struct Critical {
    pid: u32,
    previous: bool,
    _same_thread: PhantomData<Rc<()>>,
}
impl Drop for Critical {
    fn drop(&mut self) {
        let pid = std::process::id();
        if self.pid != pid {
            forget_foreign_context(pid);
            return;
        }
        let _ = CONTEXT.try_with(|slot| {
            let mut taken = Taken::new(slot);
            taken.forget_foreign(pid);
            if let Some(context) = &mut taken.context {
                context.critical = self.previous;
            }
        });
    }
}

/// Mark an already scoped thread's short native-gate section. Unscoped work
/// runs directly, without creating a context or initializing observation.
pub fn critical<T>(body: impl FnOnce() -> T) -> T {
    if !any_context() {
        return body();
    }
    let pid = std::process::id();
    reset_foreign_count(pid);
    let previous = CONTEXT
        .try_with(|slot| {
            let mut taken = Taken::new(slot);
            taken.forget_foreign(pid);
            taken
                .context
                .as_mut()
                .map(|context| std::mem::replace(&mut context.critical, true))
        })
        .expect("work-observer TLS is unavailable");
    let _restore = previous.map(|previous| Critical {
        pid,
        previous,
        _same_thread: PhantomData,
    });
    body()
}

struct Scope {
    pid: u32,
    // None owns the first context. Some(previous) restores a nested scope,
    // including Some(None) for an already suppressed observer.
    previous: Option<Option<Observer>>,
    _same_thread: PhantomData<Rc<()>>,
}
impl Scope {
    fn enter(mut observer: Option<Observer>) -> Self {
        let pid = std::process::id();
        let previous = CONTEXT
            .try_with(|slot| {
                let mut taken = Taken::new(slot);
                taken.forget_foreign(pid);
                match &mut taken.context {
                    Some(context) => {
                        Some(std::mem::replace(&mut context.observer, observer.take()))
                    }
                    None => {
                        // Allocate an empty box before checked publication. If
                        // the count check unwinds, only this empty box drops
                        // here; the incoming observer remains outside TLS.
                        let mut context = Box::new(Context {
                            pid,
                            observer: None,
                            critical: false,
                        });
                        add_context(pid);
                        // These moves cannot unwind after count publication.
                        context.observer = observer.take();
                        taken.context = Some(context);
                        None
                    }
                }
            })
            .unwrap_or_else(|_| panic!("work-observer TLS is unavailable"));
        Self {
            pid,
            previous,
            _same_thread: PhantomData,
        }
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        let pid = std::process::id();
        let mut previous = self.previous.take();
        if self.pid != pid {
            std::mem::forget(previous);
            forget_foreign_context(pid);
            return;
        }
        let first = previous.is_none();
        let outgoing = CONTEXT.try_with(|slot| {
            let mut taken = Taken::new(slot);
            taken.forget_foreign(pid);
            if first {
                (None, taken.context.take())
            } else if let Some(context) = &mut taken.context {
                (
                    std::mem::replace(&mut context.observer, previous.take().flatten()),
                    None,
                )
            } else {
                (None, None)
            }
        });
        if first {
            remove_context(pid);
        }
        // Destructors may immediately create another scope. The TLS slot and
        // global count must already describe the restored state at that point.
        drop(outgoing);
        drop(previous);
    }
}

#[inline]
pub fn current_observer() -> Option<Arc<dyn WorkObserver>> {
    if !any_context() {
        return None;
    }
    current_observer_active()
}

// Keep ownership and TLS access off the ordinary admission path, including
// builds without cross-crate inlining.
#[cold]
#[inline(never)]
fn current_observer_active() -> Option<Arc<dyn WorkObserver>> {
    let pid = std::process::id();
    reset_foreign_count(pid);
    CONTEXT
        .try_with(|slot| {
            let mut taken = Taken::new(slot);
            taken.forget_foreign(pid);
            taken
                .context
                .as_ref()
                .and_then(|context| context.observer.clone())
        })
        .expect("work-observer TLS is unavailable")
}

pub fn with_observer<T>(observer: Arc<dyn WorkObserver>, body: impl FnOnce() -> T) -> T {
    let _scope = Scope::enter(Some(observer));
    body()
}

/// Use only around an explicitly documented terminal publication boundary or
/// a progress callback; validation and copying must remain inside observation.
pub fn without_observer<T>(body: impl FnOnce() -> T) -> T {
    if !any_context() {
        return body();
    }
    let _scope = Scope::enter(None);
    body()
}

#[inline]
pub fn checkpoint(units: u64, bytes: u64) -> Result<()> {
    if !any_context() {
        return Ok(());
    }
    checkpoint_active(units, bytes)
}

#[cold]
#[inline(never)]
fn checkpoint_active(units: u64, bytes: u64) -> Result<()> {
    match current_observer_active() {
        Some(observer) => observer.checkpoint(units, bytes),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Barrier, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    };
    // These tests inspect the process-wide empty fast path as well as scoped
    // behavior. Serialize the test cases; concurrency inside a case is real.
    static SCOPES: Mutex<()> = Mutex::new(());
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SCOPES.lock().unwrap_or_else(|p| p.into_inner())
    }
    struct Counter(AtomicU64);
    impl WorkObserver for Counter {
        fn checkpoint(&self, units: u64, _: u64) -> Result<()> {
            self.0.fetch_add(units, Ordering::Relaxed);
            Ok(())
        }
    }
    #[test]
    fn nested_scopes_restore_on_return_and_unwind_without_cross_thread_leaks() {
        let _serial = serial();
        let first = Arc::new(Counter(AtomicU64::new(0)));
        let second = Arc::new(Counter(AtomicU64::new(0)));
        with_observer(first.clone(), || {
            checkpoint(1, 0).unwrap();
            with_observer(second.clone(), || checkpoint(2, 0).unwrap());
            without_observer(|| checkpoint(100, 0).unwrap());
            let _ = std::panic::catch_unwind(|| {
                with_observer(second.clone(), || panic!("scope unwind"));
            });
            checkpoint(3, 0).unwrap();
            std::thread::spawn(|| assert!(current_observer().is_none()))
                .join()
                .unwrap();
        });
        assert!(current_observer().is_none());
        assert_eq!(first.0.load(Ordering::Relaxed), 4);
        assert_eq!(second.0.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn checkpoint_callbacks_can_replace_and_suppress_the_observer_reentrantly() {
        let _serial = serial();
        struct Reentrant {
            total: AtomicU64,
            nested: Arc<Counter>,
        }
        impl WorkObserver for Reentrant {
            fn checkpoint(&self, units: u64, _: u64) -> Result<()> {
                self.total.fetch_add(units, Ordering::Relaxed);
                // The outer checkpoint must restore the TLS slot before
                // caller code installs another owned scope.
                with_observer(self.nested.clone(), || checkpoint(2, 0)).unwrap();
                without_observer(|| {
                    assert!(current_observer().is_none());
                    checkpoint(100, 0).unwrap();
                });
                assert!(current_observer().is_some());
                Ok(())
            }
        }
        let nested = Arc::new(Counter(AtomicU64::new(0)));
        let outer = Arc::new(Reentrant {
            total: AtomicU64::new(0),
            nested: nested.clone(),
        });
        with_observer(outer.clone(), || checkpoint(3, 0)).unwrap();
        assert!(current_observer().is_none());
        assert_eq!(outer.total.load(Ordering::Relaxed), 3);
        assert_eq!(nested.0.load(Ordering::Relaxed), 2);
    }

    fn has_context() -> bool {
        CONTEXT.with(|slot| Taken::new(slot).context.is_some())
    }
    fn count() -> u32 {
        ACTIVE.load(Ordering::Acquire) as u32
    }

    #[test]
    fn inactive_calls_do_not_create_a_context() {
        let _serial = serial();
        assert!(!any_context());
        assert!(!has_context());
        let before = ACTIVE.load(Ordering::Acquire);
        for _ in 0..8 {
            assert!(current_observer().is_none());
            checkpoint(100, 100).unwrap();
            without_observer(|| assert!(current_observer().is_none()));
            critical(|| assert!(!in_critical()));
        }
        assert_eq!(ACTIVE.load(Ordering::Acquire), before);
        assert!(!has_context());
        assert!(!any_context());
    }

    #[test]
    fn concurrent_scopes_remain_thread_specific_through_suppression_and_removal() {
        let _serial = serial();
        assert!(!any_context());
        assert!(!has_context());
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for units in [1, 2] {
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                let counter = Arc::new(Counter(AtomicU64::new(0)));
                with_observer(counter.clone(), || {
                    barrier.wait();
                    checkpoint(units, 0).unwrap();
                    without_observer(|| {
                        assert!(current_observer().is_none());
                        checkpoint(100, 0).unwrap();
                        barrier.wait();
                    });
                    checkpoint(units + 10, 0).unwrap();
                    barrier.wait();
                });
                assert!(current_observer().is_none());
                counter.0.load(Ordering::Relaxed)
            }));
        }
        for _ in 0..3 {
            barrier.wait();
            assert!(current_observer().is_none());
        }
        assert_eq!(workers.remove(0).join().unwrap(), 12);
        assert_eq!(workers.remove(0).join().unwrap(), 14);
        assert!(!any_context());
        assert!(!has_context());
    }

    #[test]
    fn replaced_observer_destructors_reenter_after_unlocking() {
        let _serial = serial();
        struct OnDrop;
        impl WorkObserver for OnDrop {
            fn checkpoint(&self, _: u64, _: u64) -> Result<()> {
                Ok(())
            }
        }
        impl Drop for OnDrop {
            fn drop(&mut self) {
                assert!(current_observer().is_some());
                checkpoint(7, 0).unwrap();
            }
        }
        let counter = Arc::new(Counter(AtomicU64::new(0)));
        with_observer(counter.clone(), || {
            with_observer(Arc::new(OnDrop), || checkpoint(100, 0).unwrap());
            checkpoint(2, 0).unwrap();
        });
        assert_eq!(counter.0.load(Ordering::Relaxed), 9);
        assert!(!any_context());
    }

    struct OnDrop(Arc<AtomicBool>);
    impl WorkObserver for OnDrop {
        fn checkpoint(&self, _: u64, _: u64) -> Result<()> {
            Ok(())
        }
    }
    impl Drop for OnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    fn install_foreign(dropped: &Arc<AtomicBool>) -> u32 {
        assert!(!has_context());
        let pid = std::process::id().wrapping_add(1);
        CONTEXT.with(|slot| {
            slot.set(ManuallyDrop::new(Some(Box::new(Context {
                pid,
                observer: Some(Arc::new(OnDrop(dropped.clone()))),
                critical: true,
            }))));
        });
        ACTIVE.store(packed(pid, 1), Ordering::Release);
        pid
    }

    #[test]
    fn foreign_context_and_scope_retire_without_dropping_inherited_values() {
        let _serial = serial();
        let dropped = Arc::new(AtomicBool::new(false));
        let pid = install_foreign(&dropped);
        drop(Scope {
            pid,
            previous: Some(Some(Arc::new(OnDrop(dropped.clone())))),
            _same_thread: PhantomData,
        });
        drop(Critical {
            pid,
            previous: false,
            _same_thread: PhantomData,
        });
        assert!(current_observer().is_none());
        assert!(!has_context());
        assert!(!any_context());
        assert_eq!(owner(ACTIVE.load(Ordering::Acquire)), std::process::id());
        assert!(!dropped.load(Ordering::Acquire));
    }

    #[test]
    fn scoped_critical_state_survives_suppression_nesting_and_unwinding() {
        let _serial = serial();
        assert!(!in_critical());
        critical(|| assert!(!in_critical()));
        let counter = Arc::new(Counter(AtomicU64::new(0)));
        with_observer(counter.clone(), || {
            assert!(!in_critical());
            critical(|| {
                assert!(in_critical());
                critical(|| assert!(in_critical()));
                assert!(in_critical());
                without_observer(|| {
                    assert!(current_observer().is_none());
                    assert!(in_critical());
                    critical(|| assert!(in_critical()));
                    with_observer(counter.clone(), || assert!(in_critical()));
                    assert!(in_critical());
                });
                std::thread::spawn(|| {
                    assert!(!in_critical());
                    critical(|| assert!(!in_critical()));
                })
                .join()
                .unwrap();
                let _ = std::panic::catch_unwind(|| critical(|| panic!("critical unwind")));
                assert!(in_critical());
            });
            assert!(!in_critical());
        });
        assert!(!in_critical());
        assert!(!any_context());
    }

    #[test]
    fn pointer_slot_needs_no_drop_and_first_last_scopes_own_the_context() {
        let _serial = serial();
        assert!(!std::mem::needs_drop::<Slot>());
        assert_eq!(std::mem::size_of::<Slot>(), std::mem::size_of::<usize>());
        assert_eq!(count(), 0);
        assert!(!has_context());
        let counter = Arc::new(Counter(AtomicU64::new(0)));
        with_observer(counter.clone(), || {
            assert_eq!(count(), 1);
            assert!(has_context());
            let first = CONTEXT.with(|slot| {
                let taken = Taken::new(slot);
                std::ptr::from_ref(taken.context.as_deref().unwrap())
            });
            for _ in 0..8 {
                with_observer(counter.clone(), || {
                    assert_eq!(count(), 1);
                    without_observer(|| {
                        assert_eq!(count(), 1);
                        assert!(current_observer().is_none());
                    });
                });
            }
            CONTEXT.with(|slot| {
                let taken = Taken::new(slot);
                assert_eq!(std::ptr::from_ref(taken.context.as_deref().unwrap()), first);
            });
            std::thread::spawn(|| {
                assert!(!has_context());
                // Another thread is observed: suppression still owns a context
                // here, while nested critical bookkeeping creates none.
                critical(|| assert_eq!(count(), 1));
                assert!(!has_context());
                without_observer(|| {
                    assert!(has_context());
                    assert_eq!(count(), 2);
                    critical(|| assert!(in_critical()));
                });
                assert_eq!(count(), 1);
                assert!(!has_context());
            })
            .join()
            .unwrap();
            assert_eq!(count(), 1);
        });
        assert_eq!(count(), 0);
        assert!(!has_context());
    }

    struct ExitProbe(Arc<Counter>);
    impl Drop for ExitProbe {
        fn drop(&mut self) {
            assert!(current_observer().is_none());
            with_observer(self.0.clone(), || {
                critical(|| {
                    assert!(in_critical());
                    checkpoint(3, 0).unwrap();
                    without_observer(|| {
                        assert!(in_critical());
                        assert!(current_observer().is_none());
                    });
                });
                with_observer(self.0.clone(), || checkpoint(4, 0).unwrap());
            });
            assert!(current_observer().is_none());
            assert!(!has_context());
        }
    }
    thread_local! {
        static EXIT_PROBE: Cell<Option<ExitProbe>> = const { Cell::new(None) };
    }

    #[test]
    fn observer_scopes_work_in_real_tls_destructors_in_both_registration_orders() {
        let _serial = serial();
        for exit_key_first in [true, false] {
            let counter = Arc::new(Counter(AtomicU64::new(0)));
            let thread_counter = counter.clone();
            std::thread::spawn(move || {
                let register = || {
                    EXIT_PROBE.with(|slot| slot.set(Some(ExitProbe(thread_counter.clone()))));
                };
                if exit_key_first {
                    register();
                }
                with_observer(thread_counter.clone(), || checkpoint(1, 0).unwrap());
                if !exit_key_first {
                    register();
                }
                assert!(!has_context());
            })
            .join()
            .unwrap();
            assert_eq!(counter.0.load(Ordering::Relaxed), 8);
            assert_eq!(count(), 0);
        }
    }

    #[test]
    fn observer_destructor_can_reenter_and_panic_after_restoration() {
        let _serial = serial();
        struct ReenterThenPanic(Arc<Counter>);
        impl WorkObserver for ReenterThenPanic {
            fn checkpoint(&self, _: u64, _: u64) -> Result<()> {
                Ok(())
            }
        }
        impl Drop for ReenterThenPanic {
            fn drop(&mut self) {
                assert!(current_observer().is_some());
                assert_eq!(count(), 1);
                checkpoint(2, 0).unwrap();
                with_observer(self.0.clone(), || checkpoint(3, 0).unwrap());
                without_observer(|| assert!(current_observer().is_none()));
                panic!("observer destructor unwind");
            }
        }
        let counter = Arc::new(Counter(AtomicU64::new(0)));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_observer(counter.clone(), || {
                with_observer(Arc::new(ReenterThenPanic(counter.clone())), || {});
            });
        }));
        assert!(result.is_err());
        assert_eq!(counter.0.load(Ordering::Relaxed), 5);
        assert_eq!(count(), 0);
        assert!(!has_context());
        with_observer(counter.clone(), || checkpoint(7, 0).unwrap());
        assert_eq!(counter.0.load(Ordering::Relaxed), 12);
        assert_eq!(count(), 0);
    }

    #[test]
    fn foreign_guards_preserve_a_new_child_context_and_critical_state() {
        let _serial = serial();
        let dropped = Arc::new(AtomicBool::new(false));
        let foreign_pid = install_foreign(&dropped);
        let old_scope = Scope {
            pid: foreign_pid,
            previous: Some(Some(Arc::new(OnDrop(dropped.clone())))),
            _same_thread: PhantomData,
        };
        let old_critical = Critical {
            pid: foreign_pid,
            previous: false,
            _same_thread: PhantomData,
        };
        let counter = Arc::new(Counter(AtomicU64::new(0)));
        with_observer(counter.clone(), || {
            assert_eq!(count(), 1);
            critical(|| {
                drop(old_scope);
                drop(old_critical);
                assert!(in_critical());
                assert_eq!(count(), 1);
                checkpoint(9, 0).unwrap();
            });
            assert!(!in_critical());
            assert!(current_observer().is_some());
        });
        assert_eq!(counter.0.load(Ordering::Relaxed), 9);
        assert_eq!(count(), 0);
        assert!(!has_context());
        assert!(!dropped.load(Ordering::Acquire));
    }

    #[test]
    fn taken_context_is_restored_on_internal_unwind() {
        let _serial = serial();
        let counter = Arc::new(Counter(AtomicU64::new(0)));
        with_observer(counter.clone(), || {
            let result = std::panic::catch_unwind(|| {
                CONTEXT.with(|slot| {
                    let _taken = Taken::new(slot);
                    panic!("internal unwind while slot is taken");
                });
            });
            assert!(result.is_err());
            assert_eq!(count(), 1);
            checkpoint(6, 0).unwrap();
        });
        assert_eq!(counter.0.load(Ordering::Relaxed), 6);
        assert_eq!(count(), 0);
        assert!(!has_context());
    }

    #[test]
    fn exhausted_thread_counter_cannot_wrap_or_run_an_unobserved_body() {
        let _serial = serial();
        assert_eq!(count(), 0);
        assert!(!has_context());
        let pid = std::process::id();
        let counter = Arc::new(Counter(AtomicU64::new(0)));
        let ran = AtomicBool::new(false);
        ACTIVE.store(packed(pid, u32::MAX), Ordering::Release);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_observer(counter, || ran.store(true, Ordering::Release));
        }));
        let retained = ACTIVE.swap(packed(pid, 0), Ordering::AcqRel);
        assert!(result.is_err());
        assert_eq!(retained, packed(pid, u32::MAX));
        assert!(!ran.load(Ordering::Acquire));
        assert!(!has_context());
    }
}
