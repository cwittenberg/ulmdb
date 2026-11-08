//! Writer-private pending bucket heads. Point readers never access this table.
//!
//! Reserve precedes find/set preparation. Once terminal draining starts, callers
//! must use only take until every touched slot is empty; removing one colliding
//! entry creates a probe hole. No find/set may interleave with that drain. The
//! enclosing writer mutex and Ready publish/abort phases enforce this contract.
//!
//! The existing external ticket belongs exclusively to this buffer. Its object
//! header stays in Writer's already charged storage; only requested array bytes
//! are charged here. T owns/charges its payload separately, as Arc<Node> did in
//! the former Vec<Option<Arc<Node>>>.
use super::super::index::MetadataReservation;
use crate::{Error, Result};
use std::mem::size_of;

pub(super) struct Slot<T> {
    bucket: usize,
    value: Option<T>,
}

/// Both variants contain only one fat Box pointer:24B on our64-bit target,
/// matching the replaced Vec. Small batches use <=half-full sparse slots;
/// after promotion, the dense directory is retained for subsequent batches.
pub(super) enum Pending<T> {
    Dense(Box<[Option<T>]>),
    Sparse(Box<[Slot<T>]>),
}
impl<T> Pending<T> {
    pub(super) fn new() -> Self {
        Self::Sparse(Box::new([]))
    }
    pub(super) fn capacity_slots(&self) -> usize {
        match self {
            Self::Dense(slots) => slots.len(),
            Self::Sparse(slots) => slots.len(),
        }
    }
    pub(super) fn capacity_bytes(&self) -> usize {
        match self {
            Self::Dense(slots) => std::mem::size_of_val(slots.as_ref()),
            Self::Sparse(slots) => std::mem::size_of_val(slots.as_ref()),
        }
    }
    pub(super) fn is_empty(&self) -> bool {
        match self {
            Self::Dense(slots) => slots.iter().all(Option::is_none),
            Self::Sparse(slots) => slots.iter().all(|slot| slot.value.is_none()),
        }
    }
    /// Bound counts insert operations, so it also bounds distinct buckets.
    /// Compatible capacity returns immediately without scanning/clearing slots.
    /// Resizing is allowed only after complete terminal draining. Old storage
    /// is dropped and refunded before admitting/allocating its replacement.
    pub(super) fn reserve(
        &mut self,
        buckets: usize,
        insertion_bound: usize,
        ticket: &mut MetadataReservation,
    ) -> Result<()> {
        if buckets == 0 || !buckets.is_power_of_two() {
            return Err(Error::InvalidInput("pending bucket capacity"));
        }
        if insertion_bound == 0 {
            return Ok(());
        }
        let sparse = insertion_bound <= buckets / 8;
        let capacity = if sparse {
            insertion_bound
                .checked_mul(2)
                .and_then(usize::checked_next_power_of_two)
                .ok_or(Error::LimitExceeded)?
        } else {
            buckets
        };
        match self {
            Self::Dense(slots) if slots.len() == buckets => return Ok(()),
            Self::Sparse(slots) if sparse && slots.len() >= capacity => return Ok(()),
            _ => {}
        }
        if !self.is_empty() {
            return Err(Error::InvalidInput("pending resize before drain"));
        }
        let dense = matches!(self, Self::Dense(_)) || !sparse;
        let capacity = if dense { buckets } else { capacity };
        let bytes = capacity
            .checked_mul(if dense {
                size_of::<Option<T>>()
            } else {
                size_of::<Slot<T>>()
            })
            .ok_or(Error::LimitExceeded)?;
        self.release_empty(ticket)?;
        ticket.resize(bytes)?;
        let next = if dense {
            filled(capacity, || None).map(Self::Dense)
        } else {
            filled(capacity, || Slot {
                bucket: 0,
                value: None,
            })
            .map(Self::Sparse)
        };
        match next {
            Ok(next) => {
                *self = next;
                Ok(())
            }
            Err(error) => {
                ticket.resize(0).expect("quota shrink cannot fail");
                Err(error)
            }
        }
    }
    /// Locate exactly once before allocating the new Node. The returned slot
    /// remains valid while no pending mutation/reserve occurs. Core::node only
    /// allocates independent payload, so get_at/set_at need not probe again.
    #[inline(always)]
    pub(super) fn locate(&self, bucket: usize) -> Result<usize> {
        match self {
            Self::Dense(slots) => {
                if bucket < slots.len() {
                    Ok(bucket)
                } else {
                    Err(Error::LimitExceeded)
                }
            }
            Self::Sparse(slots) => {
                if slots.is_empty() {
                    return Err(Error::LimitExceeded);
                }
                let mask = slots.len() - 1;
                let start = bucket & mask;
                // Core bucket IDs are already derived from its secret hash.
                // Capacity bounds even hostile/colliding input; reserved load
                // is <=1/2 because insertion_bound bounds distinct buckets.
                for distance in 0..slots.len() {
                    let at = start.wrapping_add(distance) & mask;
                    let slot = &slots[at];
                    if slot.value.is_none() || slot.bucket == bucket {
                        return Ok(at);
                    }
                }
                Err(Error::LimitExceeded)
            }
        }
    }
    #[inline(always)]
    pub(super) fn get_at(&self, slot: usize) -> Option<&T> {
        match self {
            Self::Dense(slots) => slots.get(slot)?.as_ref(),
            Self::Sparse(slots) => slots.get(slot)?.value.as_ref(),
        }
    }
    /// True only for the first value in this slot; only then add it to touched.
    #[inline(always)]
    pub(super) fn set_at(&mut self, slot: usize, bucket: usize, value: T) -> Result<bool> {
        match self {
            Self::Dense(slots) => {
                if slot != bucket {
                    return Err(Error::InvalidInput("pending dense slot"));
                }
                let target = slots.get_mut(slot).ok_or(Error::LimitExceeded)?;
                Ok(target.replace(value).is_none())
            }
            Self::Sparse(slots) => {
                let target = slots.get_mut(slot).ok_or(Error::LimitExceeded)?;
                if target.value.is_some() && target.bucket != bucket {
                    return Err(Error::InvalidInput("pending occupied slot"));
                }
                target.bucket = bucket;
                Ok(target.value.replace(value).is_none())
            }
        }
    }
    /// Terminal drain only. Find/set may resume after ALL touched slots drain.
    #[inline(always)]
    pub(super) fn take(&mut self, slot: usize) -> Option<(usize, T)> {
        match self {
            Self::Dense(slots) => slots.get_mut(slot)?.take().map(|value| (slot, value)),
            Self::Sparse(slots) => {
                let entry = slots.get_mut(slot)?;
                entry.value.take().map(|value| (entry.bucket, value))
            }
        }
    }
    /// Caller has completed terminal draining, including iterative unlinking of
    /// any Arc<Node> tails. Drop the now-empty buffer before refunding charge.
    pub(super) fn release_empty(&mut self, ticket: &mut MetadataReservation) -> Result<()> {
        debug_assert!(self.is_empty(), "pending release before complete drain");
        let old = std::mem::replace(self, Self::new());
        drop(old);
        ticket.resize(0)
    }
}
fn filled<T>(capacity: usize, mut make: impl FnMut() -> T) -> Result<Box<[T]>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| Error::LimitExceeded)?;
    for _ in 0..capacity {
        values.push(make());
    }
    Ok(values.into_boxed_slice())
}

#[cfg(test)]
mod tests {
    use super::super::super::index::Index;
    use super::*;
    use std::sync::Arc;
    type Value = Arc<u64>;
    #[test]
    fn header_layout_and_small_batch_storage_reuse_match_old_vec() {
        assert_eq!(size_of::<Pending<Value>>(), size_of::<Vec<Option<Value>>>());
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(size_of::<Pending<Value>>(), 24);
            assert_eq!(size_of::<Slot<Value>>(), 16);
        }
        let index = Index::new(1 << 20).unwrap();
        let baseline = index.stats().charged_bytes;
        let mut ticket = index.reserve_metadata(0).unwrap();
        let mut pending = Pending::<Value>::new();
        pending.reserve(32768, 256, &mut ticket).unwrap();
        assert!(matches!(pending, Pending::Sparse(_)));
        assert_eq!(pending.capacity_slots(), 512);
        assert_eq!(ticket.bytes(), 512 * size_of::<Slot<Value>>());
        let peak = index.stats().peak_bytes;
        let address = match &pending {
            Pending::Sparse(slots) => slots.as_ptr(),
            _ => unreachable!(),
        };
        pending.reserve(32768, 64, &mut ticket).unwrap();
        assert_eq!(
            match &pending {
                Pending::Sparse(slots) => slots.as_ptr(),
                _ => unreachable!(),
            },
            address
        );
        assert_eq!(index.stats().peak_bytes, peak);
        pending.release_empty(&mut ticket).unwrap();
        assert_eq!(ticket.bytes(), 0);
        assert_eq!(index.stats().charged_bytes, baseline);
    }
    #[test]
    fn collisions_repeat_bucket_and_terminal_drain_preserve_bucket_labels() {
        let index = Index::new(1 << 20).unwrap();
        let mut ticket = index.reserve_metadata(0).unwrap();
        let mut pending = Pending::<Value>::new();
        pending.reserve(1024, 4, &mut ticket).unwrap();
        let buckets = [1, 9, 17, 25];
        let mut touched = Vec::new();
        for bucket in buckets {
            let at = pending.locate(bucket).unwrap();
            assert!(pending.get_at(at).is_none());
            assert!(pending.set_at(at, bucket, Arc::new(bucket as u64)).unwrap());
            touched.push(at);
        }
        for bucket in buckets {
            let at = pending.locate(bucket).unwrap();
            assert_eq!(**pending.get_at(at).unwrap(), bucket as u64);
        }
        let repeated = pending.locate(9).unwrap();
        assert!(!pending.set_at(repeated, 9, Arc::new(99)).unwrap());
        assert!(pending.set_at(touched[0], 999, Arc::new(1)).is_err());
        // No locate/get/set is performed during this terminal draining phase.
        for (at, bucket) in touched.into_iter().zip(buckets) {
            let (actual, value) = pending.take(at).unwrap();
            assert_eq!(actual, bucket);
            assert_eq!(*value, if bucket == 9 { 99 } else { bucket as u64 });
            assert!(pending.take(at).is_none());
        }
        assert!(pending.is_empty());
        pending.reserve(1024, 4, &mut ticket).unwrap();
        let at = pending.locate(25).unwrap();
        assert!(pending.get_at(at).is_none());
        assert!(pending.set_at(at, 25, Arc::new(3)).unwrap());
        assert_eq!(pending.take(at).unwrap().0, 25);
        pending.release_empty(&mut ticket).unwrap();
    }
    #[test]
    fn empty_growth_promotes_once_and_retains_dense_for_later_small_batches() {
        let index = Index::new(1 << 20).unwrap();
        let mut ticket = index.reserve_metadata(0).unwrap();
        let mut pending = Pending::<Value>::new();
        pending.reserve(1024, 2, &mut ticket).unwrap();
        assert_eq!(pending.capacity_slots(), 4);
        pending.reserve(1024, 8, &mut ticket).unwrap();
        assert_eq!(pending.capacity_slots(), 16);
        pending.reserve(1024, 129, &mut ticket).unwrap();
        assert!(matches!(pending, Pending::Dense(_)));
        assert_eq!(pending.capacity_slots(), 1024);
        assert_eq!(ticket.bytes(), pending.capacity_bytes());
        let at = pending.locate(1023).unwrap();
        assert_eq!(at, 1023);
        assert!(pending.set_at(at, 1023, Arc::new(7)).unwrap());
        assert_eq!(pending.take(at).unwrap().0, 1023);
        let peak = index.stats().peak_bytes;
        pending.reserve(1024, 1, &mut ticket).unwrap();
        assert!(matches!(pending, Pending::Dense(_)));
        assert_eq!(index.stats().peak_bytes, peak);
        pending.release_empty(&mut ticket).unwrap();
    }
    #[test]
    fn refused_promotion_refunds_old_buffer_and_leaves_valid_empty_retry() {
        let index = Index::new(4096).unwrap();
        let baseline = index.stats().charged_bytes;
        let mut ticket = index.reserve_metadata(0).unwrap();
        let mut pending = Pending::<Value>::new();
        pending.reserve(1024, 4, &mut ticket).unwrap();
        assert!(ticket.bytes() > 0);
        assert!(matches!(
            pending.reserve(1024, 129, &mut ticket),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(ticket.bytes(), 0);
        assert_eq!(pending.capacity_slots(), 0);
        assert!(pending.is_empty());
        assert_eq!(index.stats().charged_bytes, baseline);
        pending.reserve(1024, 4, &mut ticket).unwrap();
        let at = pending.locate(99).unwrap();
        pending.set_at(at, 99, Arc::new(1)).unwrap();
        assert_eq!(pending.take(at).unwrap().0, 99);
        pending.release_empty(&mut ticket).unwrap();
    }
    #[test]
    fn resizing_live_entries_is_definite_and_does_not_drop_their_charge() {
        let index = Index::new(1 << 20).unwrap();
        let mut ticket = index.reserve_metadata(0).unwrap();
        let mut pending = Pending::<Value>::new();
        pending.reserve(1024, 1, &mut ticket).unwrap();
        let at = pending.locate(4).unwrap();
        pending.set_at(at, 4, Arc::new(77)).unwrap();
        let charged = index.stats().charged_bytes;
        assert!(pending.reserve(1024, 8, &mut ticket).is_err());
        assert_eq!(index.stats().charged_bytes, charged);
        assert_eq!(**pending.get_at(at).unwrap(), 77);
        pending.take(at).unwrap();
        pending.release_empty(&mut ticket).unwrap();
    }
}
