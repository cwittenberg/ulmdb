//! Aggregate budget for retained and active private buffers; owns no Pool/maps.
use crate::{Error, Result};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
pub(super) struct Quota {
    limit: u64,
    used: AtomicU64,
}
impl Quota {
    pub fn new(limit: u64) -> Arc<Self> {
        Arc::new(Self {
            limit,
            used: AtomicU64::new(0),
        })
    }
    pub fn used(&self) -> u64 {
        self.used.load(Ordering::Acquire)
    }
    pub fn limit(&self) -> u64 {
        self.limit
    }
    fn acquire(&self, bytes: u64) -> Result<()> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|n| *n <= self.limit)
            })
            .map(|_| ())
            .map_err(|_| Error::LimitExceeded)
    }
    pub fn reserve(self: &Arc<Self>, bytes: u64) -> Result<Reservation> {
        self.acquire(bytes)?;
        Ok(Reservation {
            quota: Arc::clone(self),
            bytes,
        })
    }
}
pub(super) struct Reservation {
    quota: Arc<Quota>,
    bytes: u64,
}
impl Reservation {
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn reconcile(&mut self, actual: u64) -> Result<()> {
        if actual > self.bytes {
            self.quota.acquire(actual - self.bytes)?;
        } else {
            self.quota
                .used
                .fetch_sub(self.bytes - actual, Ordering::AcqRel);
        }
        self.bytes = actual;
        Ok(())
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.quota.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
