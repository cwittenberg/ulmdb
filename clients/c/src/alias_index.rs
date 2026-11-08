//! Bounded lookup metadata for the existing public slot authority.
//! No strong alias/capability is retained here and no Native identity is minted.
use super::*;
use std::collections::BTreeMap;
impl dbi_state::AliasStates {
    fn active_named(&self, slot: u32, handle: Option<&AliasHandle>) -> bool {
        handle.is_some_and(|h| !h.is_main())
            && !self.dormant.contains(&slot)
            && !self.reserved.contains_key(&slot)
    }
    fn account_active(&mut self, before: bool, after: bool) {
        match (before, after) {
            (true, false) => {
                self.active_count = self
                    .active_count
                    .checked_sub(1)
                    .expect("active alias count")
            }
            (false, true) => self.active_count += 1,
            _ => {}
        }
    }
    fn update_free(&mut self, slot: u32, empty: bool) {
        if slot >= 2 && empty && !self.reserved.contains_key(&slot) {
            self.free_slots.insert(slot);
        } else {
            self.free_slots.remove(&slot);
        }
    }
    pub(super) fn replace_handle(
        &mut self,
        slot: u32,
        target: &mut Option<AliasHandle>,
        replacement: Option<AliasHandle>,
    ) -> Option<AliasHandle> {
        let before = self.active_named(slot, target.as_ref());
        let old = std::mem::replace(target, replacement);
        if let Some(handle) = old.as_ref().filter(|h| !h.is_main()) {
            let slots = self
                .names
                .get_mut(handle.name())
                .expect("indexed alias name");
            assert_eq!(
                slots.remove(&slot),
                Some(handle.id()),
                "indexed alias generation"
            );
            if slots.is_empty() {
                self.names.remove(handle.name());
            }
        }
        if let Some(handle) = target.as_ref().filter(|h| !h.is_main()) {
            let prior = self
                .names
                .entry(handle.name().into())
                .or_default()
                .insert(slot, handle.id());
            assert!(prior.is_none(), "alias index slot already occupied");
        }
        self.update_free(slot, target.is_none());
        self.account_active(before, self.active_named(slot, target.as_ref()));
        old
    }
    pub(super) fn set_dormant(&mut self, slot: u32, handle: Option<&AliasHandle>, dormant: bool) {
        let before = self.active_named(slot, handle);
        if dormant {
            self.dormant.insert(slot);
        } else {
            self.dormant.remove(&slot);
        }
        self.account_active(before, self.active_named(slot, handle));
    }
    pub(super) fn set_reserved(
        &mut self,
        slot: u32,
        handle: Option<&AliasHandle>,
        reserved: Option<dbi_state::AliasReservation>,
    ) {
        let before = self.active_named(slot, handle);
        if let Some(reserved) = reserved {
            self.reserved.insert(slot, reserved);
        } else {
            self.reserved.remove(&slot);
        }
        self.update_free(slot, handle.is_none());
        self.account_active(before, self.active_named(slot, handle));
    }
    pub(super) fn candidates(&self, name: &[u8]) -> Option<&BTreeMap<u32, AliasId>> {
        self.names.get(name)
    }
    pub(super) fn free_slot(&self) -> Option<usize> {
        self.free_slots.first().copied().map(|n| n as usize)
    }
}
