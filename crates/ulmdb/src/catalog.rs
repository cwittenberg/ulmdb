use crate::{Error, Result};
use ulmdb_core::storage::Reference;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatabaseKind {
    Kv,
    Duplicates { width: Option<u32> },
    Ipv4,
    Ipv6,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_descriptor_round_trip_and_invalid_combinations() {
        for width in [None, Some(0), Some(4), Some(65_536)] {
            let db = Database {
                ordering: crate::ordering::DatabaseOrdering::default(),
                id: 1,
                kind: DatabaseKind::Duplicates { width },
                root: Reference::NULL,
            };
            let bytes = db.encode();
            assert_eq!(&bytes[8..14], &[2, 0, 1, 0, 3, 0]);
            assert_eq!(Database::decode(&bytes).unwrap().kind, db.kind);
            for (offset, replacement) in [(10, 2), (12, 0), (12, 2), (14, 1), (64, 1)] {
                let mut corrupt = bytes;
                corrupt[offset] = replacement;
                assert!(Database::decode(&corrupt).is_err());
            }
            let mut corrupt = bytes;
            corrupt[56..64].copy_from_slice(&65_537u64.to_le_bytes());
            assert!(Database::decode(&corrupt).is_err());
        }
        for kind in [DatabaseKind::Kv, DatabaseKind::Ipv4, DatabaseKind::Ipv6] {
            let bytes = Database {
                ordering: crate::ordering::DatabaseOrdering::default(),
                id: 1,
                kind,
                root: Reference::NULL,
            }
            .encode();
            assert_eq!(Database::decode(&bytes).unwrap().kind, kind);
            let mut corrupt = bytes;
            corrupt[12] = 3;
            assert!(Database::decode(&corrupt).is_err());
            let mut corrupt = bytes;
            corrupt[56..64].copy_from_slice(&0u64.to_le_bytes());
            assert!(Database::decode(&corrupt).is_err());
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Database {
    pub ordering: crate::ordering::DatabaseOrdering,
    pub id: u64,
    pub kind: DatabaseKind,
    pub root: Reference,
}
impl Database {
    pub fn encode(&self) -> [u8; 80] {
        self.encode_for_page_size(crate::PageSize::DEFAULT)
    }
    pub fn encode_for_page_size(&self, page_size: crate::PageSize) -> [u8; 80] {
        self.encode_for_profile(page_size, crate::ValueLimit::DEFAULT)
    }
    pub fn encode_for_profile(
        &self,
        page_size: crate::PageSize,
        value_limit: crate::ValueLimit,
    ) -> [u8; 80] {
        let mut b = [0; 80];
        b[..8].copy_from_slice(&self.id.to_le_bytes());
        let kind = match self.kind {
            DatabaseKind::Kv => 1u16,
            DatabaseKind::Duplicates { width } => {
                b[12..14].copy_from_slice(&3u16.to_le_bytes());
                b[56..64].copy_from_slice(&width.map(u64::from).unwrap_or(u64::MAX).to_le_bytes());
                2
            }
            DatabaseKind::Ipv4 => 3,
            DatabaseKind::Ipv6 => 4,
        };
        b[8..10].copy_from_slice(&kind.to_le_bytes());
        b[10..12].copy_from_slice(&1u16.to_le_bytes());
        if self.ordering != crate::ordering::DatabaseOrdering::default() {
            b[10..12].copy_from_slice(&2u16.to_le_bytes());
            b[64] = self.ordering.keys.code();
            b[65] = self.ordering.duplicates.code();
        }
        b[16..32].copy_from_slice(&self.root.encode());
        b[32..40].copy_from_slice(&(page_size.native_key_limit() as u64).to_le_bytes());
        b[40..48].copy_from_slice(&(value_limit.get() as u64).to_le_bytes());
        b[48..56].copy_from_slice(&65_536u64.to_le_bytes());
        if !matches!(self.kind, DatabaseKind::Duplicates { .. }) {
            b[56..64].copy_from_slice(&u64::MAX.to_le_bytes());
        }
        b
    }
    pub fn declared_value_limit(b: &[u8]) -> Result<crate::ValueLimit> {
        let raw = b
            .get(40..48)
            .ok_or(Error::Corrupt("short database value limit"))?;
        let raw = u64::from_le_bytes(raw.try_into().unwrap());
        let raw = usize::try_from(raw).map_err(|_| Error::Corrupt("value limit overflow"))?;
        crate::ValueLimit::new(raw).map_err(|_| Error::Corrupt("invalid database value limit"))
    }
    pub fn decode(b: &[u8]) -> Result<Self> {
        Self::decode_for_page_size(b, crate::PageSize::DEFAULT)
    }
    pub fn decode_for_page_size(b: &[u8], page_size: crate::PageSize) -> Result<Self> {
        Self::decode_for_profile(b, page_size, crate::ValueLimit::DEFAULT)
    }
    pub fn decode_for_profile(
        b: &[u8],
        page_size: crate::PageSize,
        value_limit: crate::ValueLimit,
    ) -> Result<Self> {
        if b.len() != 80
            || !matches!(u16::from_le_bytes([b[10], b[11]]), 1 | 2)
            || b[14..16] != [0; 2]
            || b[32..40] != (page_size.native_key_limit() as u64).to_le_bytes()
            || !Self::declared_value_limit(b).is_ok_and(|n| n <= value_limit)
            || b[48..56] != 65_536u64.to_le_bytes()
            || b[66..].iter().any(|n| *n != 0)
            || (b[10..12] == 1u16.to_le_bytes() && b[64..].iter().any(|n| *n != 0))
        {
            return Err(Error::Corrupt("invalid database descriptor"));
        }
        let id = u64::from_le_bytes(
            b[..8]
                .try_into()
                .map_err(|_| Error::Corrupt("database ID"))?,
        );
        let kind = match u16::from_le_bytes(
            b[8..10]
                .try_into()
                .map_err(|_| Error::Corrupt("database kind"))?,
        ) {
            1 => DatabaseKind::Kv,
            2 => {
                let width = u64::from_le_bytes(
                    b[56..64]
                        .try_into()
                        .map_err(|_| Error::Corrupt("duplicate width"))?,
                );
                DatabaseKind::Duplicates {
                    width: if width == u64::MAX {
                        None
                    } else if width <= 65_536 {
                        Some(width as u32)
                    } else {
                        return Err(Error::Corrupt("unsupported duplicate width"));
                    },
                }
            }
            3 => DatabaseKind::Ipv4,
            4 => DatabaseKind::Ipv6,
            _ => return Err(Error::Corrupt("unsupported database kind")),
        };
        let expected_codec = if matches!(kind, DatabaseKind::Duplicates { .. }) {
            3u16
        } else {
            0
        };
        if b[12..14] != expected_codec.to_le_bytes()
            || (!matches!(kind, DatabaseKind::Duplicates { .. })
                && b[56..64] != u64::MAX.to_le_bytes())
        {
            return Err(Error::Corrupt("invalid duplicate descriptor codec/width"));
        }
        if id == 0 {
            return Err(Error::Corrupt("null database identity"));
        }
        let ordering = crate::ordering::DatabaseOrdering {
            keys: crate::ordering::OrderingSpec::decode(b[64])?,
            duplicates: crate::ordering::OrderingSpec::decode(b[65])?,
        };
        ordering
            .validate(kind)
            .map_err(|_| Error::Corrupt("database ordering/kind mismatch"))?;
        if b[10..12] == 2u16.to_le_bytes()
            && ordering == crate::ordering::DatabaseOrdering::default()
        {
            return Err(Error::Corrupt("noncanonical ordered database descriptor"));
        }
        Ok(Self {
            ordering,
            id,
            kind,
            root: Reference::decode(&b[16..32])?,
        })
    }
}
