use super::*;
use std::sync::Arc;
use ulmdb_core::storage::ReservedPayload;
/// Actual newly allocated private payload authority. No public raw constructor.
pub struct ArenaReservation {
    owner: Arc<()>,
    epoch: u64,
    creator: u32,
    revision: u64,
    payload: ReservedPayload,
}
impl ArenaReservation {
    pub fn payload(&self) -> &ReservedPayload {
        &self.payload
    }
    pub fn len(&self) -> usize {
        self.payload.len()
    }
    pub fn is_empty(&self) -> bool {
        self.payload.is_empty()
    }
}
impl Arena {
    pub(super) fn invalidate_reservations(&mut self) -> Result<()> {
        if self.reservation_owner.is_some() {
            self.reservation_epoch = self
                .reservation_epoch
                .checked_add(1)
                .ok_or(Error::LimitExceeded)?;
        }
        Ok(())
    }
    pub fn reserve_value(&mut self, length: usize) -> Result<ArenaReservation> {
        if length > self.max_value_size() {
            return Err(Error::InvalidInput("reservation value size limit"));
        }
        if self.reservation_owner.is_none() {
            self.reservation_owner = Some(Arc::new(()));
        }
        self.invalidate_reservations()?;
        let payload = WriteStore::reserve_payload(self, length)?;
        Ok(ArenaReservation {
            owner: self.reservation_owner.as_ref().unwrap().clone(),
            epoch: self.reservation_epoch,
            creator: std::process::id(),
            revision: self.revision,
            payload,
        })
    }
    fn check_reservation_owner(&self, cap: &ArenaReservation) -> Result<()> {
        if cap.creator != std::process::id() {
            return Err(Error::StaleHandle);
        }
        if self.reservation_epoch != cap.epoch
            || !self
                .reservation_owner
                .as_ref()
                .is_some_and(|o| Arc::ptr_eq(o, &cap.owner))
        {
            return Err(Error::StaleHandle);
        }
        let at = cap.payload.reference();
        if at.segment != self.id || self.objects.get(&at.offset) != Some(&(cap.len() + 16)) {
            return Err(Error::StaleHandle);
        }
        cap.payload.check(self)
    }
    /// Called after all tree and metadata allocations, immediately before the
    /// private bytes are lent. Subsequent mutations invalidate the revision.
    pub fn bind_reservation(&self, cap: &mut ArenaReservation) -> Result<()> {
        self.check_reservation_owner(cap)?;
        cap.revision = self.revision;
        Ok(())
    }
    pub fn reservation_value(&self, cap: &ArenaReservation) -> Result<&[u8]> {
        self.check_reservation_owner(cap)?;
        if cap.revision != self.revision {
            return Err(Error::StaleHandle);
        }
        let start =
            usize::try_from(cap.payload.reference().offset).map_err(|_| Error::LimitExceeded)? + 16;
        self.bytes
            .get(start..start + cap.len())
            .ok_or(Error::Corrupt("private reservation extent"))
    }
    pub fn reservation_value_mut(&mut self, cap: &ArenaReservation) -> Result<&mut [u8]> {
        self.check_reservation_owner(cap)?;
        if cap.revision != self.revision {
            return Err(Error::StaleHandle);
        }
        // Fresh payloads are always tail allocations. A savepoint transition
        // revokes the cap before a rollback can truncate/reuse that location.
        let start =
            usize::try_from(cap.payload.reference().offset).map_err(|_| Error::LimitExceeded)? + 16;
        self.bytes
            .get_mut(start..start + cap.len())
            .ok_or(Error::Corrupt("private reservation extent"))
    }
    pub fn end_reservation(&mut self, cap: ArenaReservation) -> Result<()> {
        self.check_reservation_owner(&cap)?;
        self.invalidate_reservations()
    }
}
