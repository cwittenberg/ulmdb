//! Contracts and scalar address primitives. This crate is not a storage engine.

mod catalog_capacity;
#[cfg(feature = "range-chunks")]
pub mod chunk_geometry;
pub mod compact_page_geometry;
pub use catalog_capacity::CatalogCapacity;
pub mod descriptor_count;
pub mod endian;
pub mod hex;
pub mod inventory_stamp;
pub mod kernel;
pub mod object_graph;
pub mod page_geometry;
pub mod radix_wire;
pub mod storage;

use std::net::IpAddr;

include!("prefix.rs");

impl From<IpAddr> for Address {
    fn from(value: IpAddr) -> Self {
        match value {
            IpAddr::V4(v) => Self::V4(u32::from(v)),
            IpAddr::V6(v) => Self::V6(u128::from(v)),
        }
    }
}

/// The required public surface, also used to detect missing verification entries.
pub const REQUIRED_OPERATIONS: &[&str] = &[
    "open",
    "begin_read",
    "begin_write",
    "create_database",
    "open_database",
    "drop_database",
    "kv_get",
    "kv_put",
    "kv_delete",
    "kv_range",
    "kv_reverse_range",
    "duplicate_first",
    "duplicate_seek_pair",
    "duplicate_next",
    "duplicate_next_key",
    "duplicate_insert",
    "duplicate_delete_pair",
    "duplicate_delete_all",
    "duplicate_count",
    "prefix_exact",
    "prefix_longest_match",
    "prefix_covering",
    "prefix_ancestors",
    "prefix_subnets",
    "prefix_overlaps",
    "prefix_insert",
    "prefix_delete",
    "commit",
    "abort",
    "stats",
    "check",
    "snapshot_export",
    "snapshot_restore",
    "compact",
    "replica_transfer",
    "replica_activate",
    "hostname_lookup",
    "ip_lookup",
    "cidr_observations",
    "savepoint",
    "rollback_savepoint",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_boundaries_and_canonicalization() {
        for length in 0..=32 {
            let p = Prefix::normalize(Address::V4(u32::MAX), length).unwrap();
            assert_eq!(Prefix::new(p.network(), length), Ok(p));
            assert!(p.contains_address(Address::V4(u32::MAX)));
            assert_eq!(p.address_bounds().1, Address::V4(u32::MAX));
        }
        for length in 0..=128 {
            let p = Prefix::normalize(Address::V6(u128::MAX), length).unwrap();
            assert_eq!(Prefix::new(p.network(), length), Ok(p));
            assert!(p.contains_address(Address::V6(u128::MAX)));
            assert_eq!(p.address_bounds().1, Address::V6(u128::MAX));
        }
        assert_eq!(
            Prefix::new(Address::V4(1), 0),
            Err(PrefixError::HostBitsSet)
        );
        assert_eq!(
            Prefix::new(Address::V4(0), 33),
            Err(PrefixError::InvalidLength)
        );
        assert_eq!(
            Prefix::new(Address::V6(0), 129),
            Err(PrefixError::InvalidLength)
        );
        assert!(
            !Prefix::new(Address::V4(0), 0)
                .unwrap()
                .contains_address(Address::V6(0))
        );
    }

    #[test]
    fn decoder_range_handles_overflow_and_empty_extents() {
        for &(offset, length, total) in &[
            (0, 0, 0),
            (0, 1, 0),
            (1, 0, 0),
            (u64::MAX, 0, u64::MAX),
            (u64::MAX, 1, u64::MAX),
            (1, u64::MAX, u64::MAX),
        ] {
            assert_eq!(
                kernel::range_fits(offset, length, total),
                u128::from(offset) + u128::from(length) <= u128::from(total)
            );
        }
    }
}

pub mod range_generation;

pub mod page_size;
pub use page_size::PageSize;

pub mod value_limit;
pub use value_limit::ValueLimit;
