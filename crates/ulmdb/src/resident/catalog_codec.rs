//! Resident pilot descriptors state their actual per-object limits explicitly.
use crate::catalog::Database;
use crate::{DatabaseKind, Error, Result};
use ulmdb_core::storage::Reference;
pub(super) const MAX_KEY_BYTES: usize = 4080;
pub(super) const MAX_VALUE_BYTES: usize = 4080;
pub(super) const MAX_DUPLICATE_VALUE_BYTES: usize = 65536;
pub(super) const OVERFLOW_VALUE_BYTES: usize = ulmdb_storage::resident::MAX_OVERFLOW_VALUE_BYTES;
const WIDTH: usize = 48;

/// Existing KV/IP descriptor bytes stay unchanged. Kind2 uses the reserved
/// final eight bytes for fixed duplicate width; u64::MAX means variable-width.
/// Ordered databases use version2/64 bytes, with key/value tags at48/49.
pub(super) fn encode(database: &Database) -> Vec<u8> {
    encode_with_limit(database, MAX_VALUE_BYTES)
}
pub(super) fn encode_with_limit(database: &Database, limit: usize) -> Vec<u8> {
    let ordered = database.ordering != Default::default();
    let mut out = vec![0; if ordered { 64 } else { WIDTH }];
    out[..8].copy_from_slice(&database.id.to_le_bytes());
    let kind: u16 = match database.kind {
        DatabaseKind::Kv => 1,
        DatabaseKind::Duplicates { .. } => 2,
        DatabaseKind::Ipv4 => 3,
        DatabaseKind::Ipv6 => 4,
    };
    out[8..10].copy_from_slice(&kind.to_le_bytes());
    out[10..12].copy_from_slice(&(if ordered { 2u16 } else { 1u16 }).to_le_bytes());
    out[12..16].copy_from_slice(&(if limit > MAX_VALUE_BYTES { 2u32 } else { 1u32 }).to_le_bytes());
    out[16..32].copy_from_slice(&database.root.encode());
    out[32..36].copy_from_slice(&(MAX_KEY_BYTES as u32).to_le_bytes());
    out[36..40].copy_from_slice(&(limit as u32).to_le_bytes());
    if let DatabaseKind::Duplicates { width } = database.kind {
        out[40..48].copy_from_slice(&width.map(u64::from).unwrap_or(u64::MAX).to_le_bytes());
    }
    if ordered {
        out[48] = database.ordering.keys.code();
        out[49] = database.ordering.duplicates.code();
    }
    out
}
pub(super) fn decode(bytes: &[u8]) -> Result<Database> {
    if !matches!(bytes.len(), 48 | 64)
        || (bytes.len() == 48 && bytes[10..12] != 1u16.to_le_bytes())
        || (bytes.len() == 64
            && (bytes[10..12] != 2u16.to_le_bytes() || bytes[50..].iter().any(|b| *b != 0)))
        || bytes[32..36] != (MAX_KEY_BYTES as u32).to_le_bytes()
    {
        return Err(Error::Corrupt("resident catalog descriptor contract"));
    }
    let limit = value_limit(bytes)?;
    let id = u64::from_le_bytes(bytes[..8].try_into().unwrap());
    if id == 0 {
        return Err(Error::Corrupt("resident catalog null identity"));
    }
    let kind = match u16::from_le_bytes(bytes[8..10].try_into().unwrap()) {
        2 => {
            let width = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
            DatabaseKind::Duplicates {
                width: if width == u64::MAX {
                    None
                } else {
                    if width > limit as u64 {
                        return Err(Error::Corrupt("resident fixed duplicate width"));
                    }
                    Some(width as u32)
                },
            }
        }
        1 => DatabaseKind::Kv,
        3 => DatabaseKind::Ipv4,
        4 => DatabaseKind::Ipv6,
        _ => return Err(Error::Corrupt("resident catalog unsupported kind")),
    };
    if !matches!(kind, DatabaseKind::Duplicates { .. })
        && bytes[40..48].iter().any(|&byte| byte != 0)
    {
        return Err(Error::Corrupt("resident descriptor reserved bytes"));
    }
    let ordering = if bytes.len() == 64 {
        crate::ordering::DatabaseOrdering {
            keys: crate::ordering::OrderingSpec::decode(bytes[48])?,
            duplicates: crate::ordering::OrderingSpec::decode(bytes[49])?,
        }
    } else {
        Default::default()
    };
    ordering
        .validate(kind)
        .map_err(|_| Error::Corrupt("resident descriptor ordering/kind"))?;
    if bytes.len() == 64 && ordering == Default::default() {
        return Err(Error::Corrupt("noncanonical resident ordered descriptor"));
    }
    Ok(Database {
        ordering,
        id,
        kind,
        root: Reference::decode(&bytes[16..32])?,
    })
}

pub(super) fn value_limit(bytes: &[u8]) -> Result<usize> {
    if !matches!(bytes.len(), 48 | 64) {
        return Err(Error::Corrupt("resident descriptor width"));
    }
    let version = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    let limit = u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize;
    let kind = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
    let extended = if kind == 2 {
        MAX_DUPLICATE_VALUE_BYTES
    } else {
        OVERFLOW_VALUE_BYTES
    };
    if (version, limit) != (1, MAX_VALUE_BYTES) && (version, limit) != (2, extended) {
        return Err(Error::Corrupt("resident descriptor value profile"));
    }
    Ok(limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_contract_roundtrips_and_rejects_unknown_limits_flags_and_kinds() {
        for kind in [DatabaseKind::Kv, DatabaseKind::Ipv4, DatabaseKind::Ipv6] {
            let database = Database {
                ordering: crate::ordering::DatabaseOrdering::default(),
                id: 17,
                kind,
                root: Reference::NULL,
            };
            let good = encode(&database);
            let decoded = decode(&good).unwrap();
            assert_eq!(
                (decoded.id, decoded.kind, decoded.root),
                (17, kind, Reference::NULL)
            );
            for offset in [8, 10, 12, 13, 14, 15, 32, 36, 40, 47] {
                let mut bad = good.clone();
                bad[offset] ^= 0x80;
                assert!(decode(&bad).is_err(), "offset{offset}");
            }
            for width in [0, 16, 47, 49, 80] {
                assert!(decode(&vec![0; width]).is_err());
            }
        }
        for database in [
            Database {
                ordering: crate::ordering::DatabaseOrdering::default(),
                id: 0,
                kind: DatabaseKind::Kv,
                root: Reference::NULL,
            },
            Database {
                ordering: crate::ordering::DatabaseOrdering::default(),
                id: 1,
                kind: DatabaseKind::Duplicates { width: Some(4081) },
                root: Reference::NULL,
            },
            Database {
                ordering: crate::ordering::DatabaseOrdering::default(),
                id: 1,
                kind: DatabaseKind::Kv,
                root: Reference {
                    segment: 0,
                    offset: 1,
                },
            },
        ] {
            assert!(decode(&encode(&database)).is_err());
        }
    }
}

#[cfg(test)]
mod duplicate_contract_tests {
    use super::*;
    #[test]
    fn duplicate_widths_roundtrip_without_changing_existing_descriptor_shape() {
        for width in [None, Some(0), Some(4), Some(MAX_VALUE_BYTES as u32)] {
            let db = Database {
                id: 3,
                kind: DatabaseKind::Duplicates { width },
                root: Reference::NULL,
                ordering: Default::default(),
            };
            let bytes = encode(&db);
            assert_eq!(bytes.len(), 48);
            assert_eq!(decode(&bytes).unwrap().kind, db.kind);
            assert_eq!(&bytes[8..10], &2u16.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod wide_duplicate_profiles {
    use super::*;
    #[test]
    fn profile2_declares_exact_native_duplicate_bound_and_fixed_width() {
        for width in [None, Some(4081), Some(65536)] {
            let db = Database {
                id: 9,
                kind: DatabaseKind::Duplicates { width },
                root: Reference::NULL,
                ordering: Default::default(),
            };
            let bytes = encode_with_limit(&db, MAX_DUPLICATE_VALUE_BYTES);
            assert_eq!(decode(&bytes).unwrap().kind, db.kind);
            assert_eq!(value_limit(&bytes).unwrap(), 65536);
            assert!(decode(&encode_with_limit(&db, OVERFLOW_VALUE_BYTES)).is_err());
        }
    }
}
