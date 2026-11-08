//! Process-local comparison authority. No callback or pointer is encoded in pages.
use std::cmp::Ordering;
use ulmdb_core::storage::{Error, Result};

/// Equality defines key identity, not just iteration order. Implementations must
/// provide a consistent strict weak ordering for the lifetime of a tree.
pub trait Comparator {
    fn compare(&self, left: &[u8], right: &[u8]) -> Ordering;
    fn check_ready(&self) -> Result<()> {
        Ok(())
    }
    fn validate(&self, _key: &[u8]) -> Result<()> {
        Ok(())
    }
    fn is_bytewise(&self) -> bool {
        false
    }
}
impl<F: Fn(&[u8], &[u8]) -> Ordering> Comparator for F {
    fn compare(&self, left: &[u8], right: &[u8]) -> Ordering {
        self(left, right)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Bytewise;
impl Comparator for Bytewise {
    fn compare(&self, left: &[u8], right: &[u8]) -> Ordering {
        left.cmp(right)
    }
    fn is_bytewise(&self) -> bool {
        true
    }
}
/// LMDB reverse-key order compares from the final byte; it is not descending
/// ordinary lexical order.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReverseBytes;
impl Comparator for ReverseBytes {
    fn compare(&self, left: &[u8], right: &[u8]) -> Ordering {
        left.iter().rev().cmp(right.iter().rev())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegerWidth {
    U32,
    U64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegerEndian {
    Little,
    Big,
}
#[derive(Clone, Copy, Debug)]
pub struct UnsignedInteger {
    pub width: IntegerWidth,
    pub endian: IntegerEndian,
}
impl Comparator for UnsignedInteger {
    fn compare(&self, left: &[u8], right: &[u8]) -> Ordering {
        match self.endian {
            IntegerEndian::Little => left.iter().rev().cmp(right.iter().rev()),
            IntegerEndian::Big => left.cmp(right),
        }
    }
    fn validate(&self, key: &[u8]) -> Result<()> {
        let expected = match self.width {
            IntegerWidth::U32 => 4,
            IntegerWidth::U64 => 8,
        };
        if key.len() == expected {
            Ok(())
        } else {
            Err(Error::InvalidInput("integer key width mismatch"))
        }
    }
}
pub(crate) fn stored<C: Comparator + ?Sized>(order: &C, key: &[u8]) -> Result<()> {
    order
        .validate(key)
        .map_err(|_| Error::Corrupt("stored ordered key width mismatch"))
}
