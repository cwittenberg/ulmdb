//! Opaque writer-owned initialized value reservations. Bytes already belong to
//! the actual value payload; ending a reservation performs no value copy.
use crate::ordering::{BoundComparator, DatabaseComparators};
use crate::{DatabaseKind, DatabaseView, Error, Result, RetainedDatabaseView, WriteTxn};
use ulmdb_kv::ordering::Comparator;
use ulmdb_storage::ArenaReservation;
pub struct ValueReservation {
    private: ArenaReservation,
}
impl ValueReservation {
    pub fn len(&self) -> usize {
        self.private.len()
    }
    pub fn is_empty(&self) -> bool {
        self.private.is_empty()
    }
}
impl WriteTxn<'_> {
    fn reserve_live(&self) -> Result<()> {
        if self.view_creator_pid != std::process::id() {
            return Err(Error::StaleHandle);
        }
        self.live()?;
        self.env.require_file_backend()
    }
    /// Reserve a fresh initialized payload; previously borrowed slices and
    /// reservations cease to be valid at this writer update. No value is copied.
    pub fn reserve(&mut self, name: &[u8], key: &[u8], length: usize) -> Result<ValueReservation> {
        self.reserve_with_ordering(name, key, length, &DatabaseComparators::default())
    }
    pub fn reserve_with_ordering(
        &mut self,
        name: &[u8],
        key: &[u8],
        length: usize,
        callbacks: &DatabaseComparators,
    ) -> Result<ValueReservation> {
        self.reserve_live()?;
        let old = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        if old.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if key.len() > self.base.manifest.page_size.native_key_limit()
            || length > self.writable_value_limit()
        {
            return Err(Error::InvalidInput("KV size limit exceeded"));
        }
        let order = BoundComparator::bind(old.ordering.keys, callbacks.keys.as_ref())?;
        order.validate(key)?;
        let old = old.root;
        self.enable_value_limit(length)?;
        self.failed = true;
        let mut private = self.arena.reserve_value(length)?;
        let root =
            ulmdb_kv::put_reserved_with(&mut self.arena, old, key, private.payload(), &order)?;
        self.catalog.get_mut(name).unwrap().root = root;
        self.mark_changed(name);
        self.arena.bind_reservation(&mut private)?;
        self.failed = false;
        Ok(ValueReservation { private })
    }
    pub fn reserve_handle(
        &mut self,
        handle: &crate::DatabaseHandle,
        key: &[u8],
        length: usize,
    ) -> Result<ValueReservation> {
        self.check_handle(handle)?;
        self.reserve(handle.name(), key, length)
    }
    pub fn reserve_handle_with_ordering(
        &mut self,
        handle: &crate::DatabaseHandle,
        key: &[u8],
        length: usize,
        callbacks: &DatabaseComparators,
    ) -> Result<ValueReservation> {
        self.check_handle(handle)?;
        self.reserve_with_ordering(handle.name(), key, length, callbacks)
    }
    pub fn view_reserve(
        &mut self,
        view: &DatabaseView,
        key: &[u8],
        length: usize,
    ) -> Result<ValueReservation> {
        self.view_reserve_with_ordering(view, key, length, &DatabaseComparators::default())
    }
    pub fn view_reserve_with_ordering(
        &mut self,
        view: &DatabaseView,
        key: &[u8],
        length: usize,
        callbacks: &DatabaseComparators,
    ) -> Result<ValueReservation> {
        self.reserve_live()?;
        let checked = self.writable_view(view)?;
        let order = BoundComparator::bind(view.ordering().keys, callbacks.keys.as_ref())?;
        self.reserve_view_checked(view, checked, key, length, order)
    }
    pub fn retained_view_reserve(
        &mut self,
        view: &RetainedDatabaseView,
        key: &[u8],
        length: usize,
    ) -> Result<ValueReservation> {
        self.retained_view_reserve_with_ordering(view, key, length, &DatabaseComparators::default())
    }
    pub fn retained_view_reserve_with_ordering(
        &mut self,
        view: &RetainedDatabaseView,
        key: &[u8],
        length: usize,
        callbacks: &DatabaseComparators,
    ) -> Result<ValueReservation> {
        self.reserve_live()?;
        let checked = self.retained_reserve_root(view)?;
        let order = BoundComparator::bind(
            view.as_database_view().ordering().keys,
            callbacks.keys.as_ref(),
        )?;
        self.reserve_view_checked(view.as_database_view(), checked, key, length, order)
    }
    pub(super) fn reserve_view_checked(
        &mut self,
        view: &DatabaseView,
        (old, kind): (ulmdb_storage::Reference, DatabaseKind),
        key: &[u8],
        length: usize,
        order: BoundComparator,
    ) -> Result<ValueReservation> {
        order.validate(key)?;
        if kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if key.len() > self.base.manifest.page_size.native_key_limit()
            || length > self.writable_value_limit()
        {
            return Err(Error::InvalidInput("KV size limit exceeded"));
        }
        let revision = self.arena.revision();
        self.enable_value_limit(length)?;
        self.failed = true;
        let mut private = self.arena.reserve_value(length)?;
        let root =
            ulmdb_kv::put_reserved_with(&mut self.arena, old, key, private.payload(), &order)?;
        self.update_view(view, old, revision, root);
        self.arena.bind_reservation(&mut private)?;
        self.failed = false;
        Ok(ValueReservation { private })
    }
    pub fn reservation_value(&self, reservation: &ValueReservation) -> Result<&[u8]> {
        self.reserve_live()?;
        self.arena.reservation_value(&reservation.private)
    }
    pub fn reservation_value_mut(&mut self, reservation: &ValueReservation) -> Result<&mut [u8]> {
        self.reserve_live()?;
        self.arena.reservation_value_mut(&reservation.private)
    }
    pub fn end_reservation(&mut self, reservation: ValueReservation) -> Result<()> {
        self.reserve_live()?;
        self.arena.end_reservation(reservation.private)
    }
}
