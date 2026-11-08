//! Kernel-enforced immutable decoded objects in genuine owned fixed slots.
//! Source identity derives from the actual opaque Arc; retaining it prevents
//! pointer ABA and preserves original quota/handle ownership through final unmap.
use super::*;
use crate::transform::ReadOnlyDecodedObject;
impl Reservation {
    pub(crate) fn map_decoded(
        self: &Arc<Self>,
        object: Arc<ReadOnlyDecodedObject>,
        region_offset: usize,
        read_ahead: crate::ReadAhead,
    ) -> Result<ByteLease> {
        self.check_creator()?;
        object.check_creator()?;
        read_ahead.validate()?;
        if !region_offset.is_multiple_of(self.page) {
            return Err(invalid("unaligned fixed decoded mapping"));
        }
        let length = object.len();
        let map_length = rounded(length, self.page)?;
        let end = region_offset
            .checked_add(map_length)
            .filter(|n| *n <= self.length)
            .ok_or(Error::LimitExceeded)?;
        let identity = Identity::Decoded(Arc::as_ptr(&object) as usize);
        let mut registry = self.slots.lock().map_err(|_| Error::OutcomeUnknown)?;
        self.check_creator()?;
        if let Some((&start, entry)) = registry.range(..end).next_back() {
            if entry.end > region_offset {
                if !entry.writable
                    && start == region_offset
                    && entry.end == end
                    && entry.file == identity
                    && entry.file_offset == 0
                    && entry.length == length
                {
                    if let Some(existing) = entry.live.upgrade() {
                        if existing.read_ahead == read_ahead {
                            return Ok(ByteLease(existing));
                        }
                    }
                }
                return Err(Error::Busy);
            }
        }
        let id = self
            .serial
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::LimitExceeded)?;
        let address = self.base.as_ptr().wrapping_add(region_offset);
        #[cfg(windows)]
        let pointer = self
            .backend
            .map_section(object.section()?, address, map_length)?;
        #[cfg(target_os = "macos")]
        let pointer = {
            // SAFETY: the registry proves this rounded range is an unused part
            // of this exact reservation. Mach replacement never releases it.
            unsafe {
                object.map_at(address as usize, length)?;
            }
            address
        };
        if pointer != address {
            self.poisoned.store(true, Ordering::Release);
            return Err(Error::OutcomeUnknown);
        }
        if read_ahead == crate::ReadAhead::Random {
            if let Err(error) = sys::random(pointer, map_length) {
                if self.backend.restore(address, map_length).is_err() {
                    self.poisoned.store(true, Ordering::Release);
                    return Err(Error::OutcomeUnknown);
                }
                return Err(error.into());
            }
        }
        let resources = Arc::new(SlotResources {
            decoded_object: Some(object),
            ..Default::default()
        });
        let mapping = Arc::new(Mapping {
            reservation: self.clone(),
            file_offset: 0,
            pointer: NonNull::new(pointer).ok_or(Error::OutcomeUnknown)?,
            read_ahead,
            locks: Mutex::new(LockCounts {
                count: 0,
                healthy: true,
            }),
            identity,
            resources: CreatorOwned::new(resources.clone()),
            offset: region_offset,
            length,
            rounded: map_length,
            id,
        });
        registry.insert(
            region_offset,
            Record {
                id,
                end,
                file: identity,
                file_offset: 0,
                length,
                live: Arc::downgrade(&mapping),
                writable: false,
                _resources: resources,
            },
        );
        Ok(ByteLease(mapping))
    }
}
