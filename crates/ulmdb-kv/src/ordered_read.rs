use crate::{
    ordering::{Comparator, stored},
    page::Page,
};
use ulmdb_core::storage::{Error, ReadStore, Reference, Result};

pub fn get_with<'a, S: ReadStore, C: Comparator + ?Sized>(
    store: &'a S,
    root: Reference,
    key: &[u8],
    order: &C,
) -> Result<Option<&'a [u8]>> {
    Ok(get_pair_with(store, root, key, order)?.map(|(_, value)| value))
}
/// The returned representative is the stored key of the comparator-equality
/// class, independently of the caller's query lifetime.
pub fn get_pair_with<'a, S: ReadStore, C: Comparator + ?Sized>(
    store: &'a S,
    root: Reference,
    key: &[u8],
    order: &C,
) -> Result<Option<(&'a [u8], &'a [u8])>> {
    order.validate(key)?;
    if order.is_bytewise() {
        return crate::get_pair(store, root, key);
    }
    if root.is_null() {
        return Ok(None);
    }
    let mut page = Page::load(store, root)?;
    loop {
        let index = page.search_with(store, key, page.level != 0, order)?;
        if page.level == 0 {
            if index == page.count {
                return Ok(None);
            }
            let cell = page.cell(store, index)?;
            stored(order, cell.key)?;
            return Ok(order
                .compare(cell.key, key)
                .is_eq()
                .then_some((cell.key, cell.value)));
        }
        let expected = page.level - 1;
        page = Page::load(store, page.child(store, index)?)?;
        if page.level != expected {
            return Err(Error::Corrupt("tree level mismatch"));
        }
    }
}
