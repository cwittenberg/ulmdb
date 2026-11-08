//! Destruct mutable process-local state only in the process that created it.
//!
//! After fork another thread's mutex or container mutation may be inherited.
//! An early return from an owner's Drop does not suppress automatic field Drop.
//! Keep those fields manually owned instead of walking inherited contents. The
//! child intentionally leaks them until exec/exit; its own VMAs may still unmap.
use std::{
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
};

pub(crate) struct CreatorOwned<T> {
    creator: u32,
    value: ManuallyDrop<T>,
}
impl<T> CreatorOwned<T> {
    pub(crate) fn new(value: T) -> Self {
        Self {
            creator: std::process::id(),
            value: ManuallyDrop::new(value),
        }
    }
}
impl<T> Deref for CreatorOwned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}
impl<T> DerefMut for CreatorOwned<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}
#[allow(unsafe_code)]
impl<T> Drop for CreatorOwned<T> {
    fn drop(&mut self) {
        if self.creator == std::process::id() {
            // SAFETY: this is the unique automatic Drop of this field. Only
            // its creator may inspect or destroy the process-local contents.
            unsafe { ManuallyDrop::drop(&mut self.value) };
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
pub(crate) mod tests;
