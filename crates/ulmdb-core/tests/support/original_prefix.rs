//! Exact captured pre-change constructor implementation, compiled as a native control.
use std::net::IpAddr;
use ulmdb_core::kernel;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Address {
    V4(u32),
    V6(u128),
}

impl From<IpAddr> for Address {
    fn from(value: IpAddr) -> Self {
        match value {
            IpAddr::V4(v) => Self::V4(u32::from(v)),
            IpAddr::V6(v) => Self::V6(u128::from(v)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrefixError {
    InvalidLength,
    HostBitsSet,
}

/// Canonical prefix. Private fields prevent invalid lengths and host bits.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Prefix {
    network: Address,
    length: u8,
}

impl Prefix {
    pub fn new(address: Address, length: u8) -> Result<Self, PrefixError> {
        let normalized = Self::normalize(address, length)?;
        if normalized.network != address {
            return Err(PrefixError::HostBitsSet);
        }
        Ok(normalized)
    }

    pub fn normalize(address: Address, length: u8) -> Result<Self, PrefixError> {
        let network = match address {
            Address::V4(value) if length <= 32 => Address::V4(if length == 0 {
                0
            } else {
                value & (u32::MAX << (32 - length))
            }),
            Address::V6(value) if length <= 128 => Address::V6(if length == 0 {
                0
            } else {
                value & (u128::MAX << (128 - length))
            }),
            _ => return Err(PrefixError::InvalidLength),
        };
        Ok(Self { network, length })
    }

    pub const fn network(self) -> Address {
        self.network
    }
    pub const fn length(self) -> u8 {
        self.length
    }

    pub fn contains_address(self, address: Address) -> bool {
        match (self.network, address) {
            (Address::V4(n), Address::V4(a)) => kernel::prefix_equal32(a, n, self.length),
            (Address::V6(n), Address::V6(a)) => kernel::prefix_equal128(a, n, self.length),
            _ => false,
        }
    }

    pub fn contains_prefix(self, other: Self) -> bool {
        self.length <= other.length && self.contains_address(other.network)
    }

    pub fn overlaps(self, other: Self) -> bool {
        self.contains_prefix(other) || other.contains_prefix(self)
    }

    /// Inclusive address bounds. IPv6 /0 does not require representing 2^128.
    pub fn address_bounds(self) -> (Address, Address) {
        let last = match self.network {
            Address::V4(n) => Address::V4(
                n | if self.length == 0 {
                    u32::MAX
                } else if self.length == 32 {
                    0
                } else {
                    u32::MAX >> self.length
                },
            ),
            Address::V6(n) => Address::V6(
                n | if self.length == 0 {
                    u128::MAX
                } else if self.length == 128 {
                    0
                } else {
                    u128::MAX >> self.length
                },
            ),
        };
        (self.network, last)
    }
}
