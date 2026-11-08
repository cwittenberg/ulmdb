use crate::{Result, ordering::BoundComparator};

/// Native ordered cursor selected outside the index algorithm. Both variants
/// borrow the same transaction as their prepared lookup; no index is rebuilt.
pub enum Cursor<'a> {
    File(ulmdb_kv::Cursor<'a, ulmdb_storage::SegmentTable, BoundComparator>),
    Resident(ulmdb_kv::Cursor<'a, ulmdb_storage::resident::View, BoundComparator>),
}

impl<'a> Iterator for Cursor<'a> {
    type Item = Result<(&'a [u8], &'a [u8])>;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::File(cursor) => cursor.next(),
            Self::Resident(cursor) => cursor.next(),
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::File(cursor) => cursor.size_hint(),
            Self::Resident(cursor) => cursor.size_hint(),
        }
    }

    /// Resolve the backend once for consumers that use fold/for_each, rather
    /// than forwarding every item through the cursor enum.
    #[inline]
    fn fold<B, F>(self, initial: B, f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        match self {
            Self::File(cursor) => cursor.fold(initial, f),
            Self::Resident(cursor) => cursor.fold(initial, f),
        }
    }
}
