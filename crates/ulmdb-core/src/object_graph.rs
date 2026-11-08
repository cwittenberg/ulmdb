//! Numeric typed-object descriptions, never allocation/reclaim authority.
use crate::storage::{Error, Reference, Result};
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ObjectKind {
    KvPage { level: u16 },
    RadixNode { family: u8 },
    Payload,
    ResidentExtentSource { kind: u8 },
    CatalogDescriptor { database: u64 },
    CatalogProfile,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ObjectId {
    pub reference: Reference,
    pub length: u64,
    pub kind: ObjectKind,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum EdgeKind {
    KvChild { ordinal: u16 },
    OverflowKey { cell: u16 },
    OverflowValue { cell: u16 },
    RadixChild { side: u8 },
    RadixValue,
    CatalogRoot { database: u64 },
    DuplicateRoot { cell: u16 },
    ExtentSource { ordinal: u32 },
    CatalogDescriptor { cell: u16 },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Edge {
    pub from: ObjectId,
    pub to: ObjectId,
    pub kind: EdgeKind,
}
/// Header and data belong to ONE complete payload allocation. The separate data
/// borrow span is retained because a zero-length endpoint can bind another map.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PayloadExtent {
    pub object: ObjectId,
    pub data: Reference,
    pub data_length: u64,
}
impl PayloadExtent {
    pub fn new(header: Reference, data_length: usize) -> Result<Self> {
        let data_length = u64::try_from(data_length).map_err(|_| Error::LimitExceeded)?;
        let length = data_length.checked_add(16).ok_or(Error::LimitExceeded)?;
        let offset = header
            .offset
            .checked_add(16)
            .ok_or(Error::Corrupt("payload offset overflow"))?;
        Ok(Self {
            object: ObjectId {
                reference: header,
                length,
                kind: ObjectKind::Payload,
            },
            data: Reference {
                segment: header.segment,
                offset,
            },
            data_length,
        })
    }
}
