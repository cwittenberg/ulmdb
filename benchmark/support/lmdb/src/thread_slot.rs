//! Enforce lmdb.h's default one-transaction-per-thread precondition before FFI.
use crate::{Error, Result};
use std::{cell::Cell, marker::PhantomData, rc::Rc};

thread_local! {static DEFAULT_ACTIVE:Cell<bool>=const{Cell::new(false)};}

/// MDB_NOTLS exempts read-only transactions, never writers.
pub(crate) struct Slot {
    claimed: bool,
    _thread: PhantomData<Rc<()>>,
}
impl Slot {
    pub(crate) fn acquire(notls_read: bool) -> Result<Self> {
        if notls_read {
            return Ok(Self {
                claimed: false,
                _thread: PhantomData,
            });
        }
        DEFAULT_ACTIVE
            .try_with(|active| {
                if active.get() {
                    return Err(Error::TransactionConflict);
                }
                active.set(true);
                Ok(Self {
                    claimed: true,
                    _thread: PhantomData,
                })
            })
            .map_err(|_| Error::InvalidInput("thread transaction state unavailable"))?
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        if self.claimed {
            let _ = DEFAULT_ACTIVE.try_with(|active| active.set(false));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_nested_default_and_writer_but_allows_notls_reads() {
        let first = Slot::acquire(false).unwrap();
        assert!(matches!(
            Slot::acquire(false),
            Err(Error::TransactionConflict)
        ));
        let exempt = Slot::acquire(true).unwrap();
        assert!(matches!(
            Slot::acquire(false),
            Err(Error::TransactionConflict)
        ));
        drop(exempt);
        drop(first);
        assert!(Slot::acquire(false).is_ok());
    }
    #[test]
    fn failing_start_scope_releases_its_reservation() {
        fn fail_after_reservation() -> Result<()> {
            let _slot = Slot::acquire(false)?;
            Err(Error::InvalidInput(
                "simulated native start error before Transaction construction",
            ))
        }
        assert!(fail_after_reservation().is_err());
        assert!(Slot::acquire(false).is_ok());
    }
    #[test]
    fn reservations_are_thread_local() {
        let _main = Slot::acquire(false).unwrap();
        std::thread::spawn(|| assert!(Slot::acquire(false).is_ok()))
            .join()
            .unwrap();
        assert!(matches!(
            Slot::acquire(false),
            Err(Error::TransactionConflict)
        ));
    }
}
