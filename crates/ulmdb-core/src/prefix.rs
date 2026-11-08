// Actual checked public prefix constructors; contracts erase under Cargo.
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

verus! {
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Address {
    V4(u32),
    V6(u128),
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

pub open spec fn width(address: Address) -> u8 {
    match address { Address::V4(_) => 32, Address::V6(_) => 128 }
}

pub open spec fn normalized(address: Address, length: u8) -> Address {
    match address {
        Address::V4(a) => Address::V4(if length == 0 { 0 }
            else if length <= 32 { a & (u32::MAX << (32 - length)) } else { a }),
        Address::V6(a) => Address::V6(if length == 0 { 0 }
            else if length <= 128 { a & (u128::MAX << (128 - length)) } else { a }),
    }
}

pub open spec fn last_address(network: Address, length: u8) -> Address {
    match network {
        Address::V4(n) => Address::V4(n | if length == 0 { u32::MAX }
            else if length >= 32 { 0 } else { u32::MAX >> length }),
        Address::V6(n) => Address::V6(n | if length == 0 { u128::MAX }
            else if length >= 128 { 0 } else { u128::MAX >> length }),
    }
}

pub open spec fn interval_member(value: Address, first: Address, last: Address) -> bool {
    match (value,first,last) {
        (Address::V4(v),Address::V4(a),Address::V4(z)) => a <= v && v <= z,
        (Address::V6(v),Address::V6(a),Address::V6(z)) => a <= v && v <= z,
        _ => false,
    }
}

proof fn membership32(network:u32, value:u32, length:u8)
    requires length <= 32,
        network == (if length == 0 { 0 } else { network & (u32::MAX << (32-length)) }),
    ensures (if length == 0 { true } else { (value >> (32-length)) == (network >> (32-length)) })
        == (network <= value && value <= (network | if length == 0 { u32::MAX }
            else if length == 32 { 0 } else { u32::MAX >> length })),
{
    if length == 0 {
        assert(network == 0);
        assert((network | u32::MAX) == u32::MAX) by(bit_vector);
    } else {
        let shift:u8 = (32-length) as u8;
        let host:u32 = if length == 32 { 0 } else { u32::MAX >> length };
        assert(((value >> shift) == (network >> shift)) == (network <= value && value <= (network | host)))
            by(bit_vector) requires 0 < length, length <= 32, shift == 32-length,
                host == (if length == 32 { 0u32 } else { u32::MAX >> length }),
                (network & (u32::MAX << shift)) == network;
    }
}

proof fn membership128(network:u128, value:u128, length:u8)
    requires length <= 128,
        network == (if length == 0 { 0 } else { network & (u128::MAX << (128-length)) }),
    ensures (if length == 0 { true } else { (value >> (128-length)) == (network >> (128-length)) })
        == (network <= value && value <= (network | if length == 0 { u128::MAX }
            else if length == 128 { 0 } else { u128::MAX >> length })),
{
    if length == 0 {
        assert(network == 0);
        assert((network | u128::MAX) == u128::MAX) by(bit_vector);
    } else {
        let shift:u8 = (128-length) as u8;
        let host:u128 = if length == 128 { 0 } else { u128::MAX >> length };
        assert(((value >> shift) == (network >> shift)) == (network <= value && value <= (network | host)))
            by(bit_vector) requires 0 < length, length <= 128, shift == 128-length,
                host == (if length == 128 { 0u128 } else { u128::MAX >> length }),
                (network & (u128::MAX << shift)) == network;
    }
}

/// Pure actual-datatype extensionality; erased from native production code.
pub proof fn prefix_extensionality(left: Prefix, right: Prefix)
    requires left.address() == right.address(), left.depth() == right.depth(),
    ensures left == right,
{
    reveal(Prefix::address); reveal(Prefix::depth);
}

pub open spec fn interval_subset(a: Address, z: Address, b: Address, y: Address) -> bool {
    match (a,z,b,y) {
        (Address::V4(a),Address::V4(z),Address::V4(b),Address::V4(y)) => a <= b && y <= z,
        (Address::V6(a),Address::V6(z),Address::V6(b),Address::V6(y)) => a <= b && y <= z,
        _ => false,
    }
}

pub open spec fn interval_overlap(a: Address, z: Address, b: Address, y: Address) -> bool {
    match (a,z,b,y) {
        (Address::V4(a),Address::V4(z),Address::V4(b),Address::V4(y)) => a <= y && b <= z,
        (Address::V6(a),Address::V6(z),Address::V6(b),Address::V6(y)) => a <= y && b <= z,
        _ => false,
    }
}

proof fn cidr_relations32(a:u32, al:u8, b:u32, bl:u8)
    requires al <= 32, bl <= 32,
        a == (if al == 0 { 0 } else { a & (u32::MAX << ((32-al) as u8)) }),
        b == (if bl == 0 { 0 } else { b & (u32::MAX << ((32-bl) as u8)) }),
    ensures
        ((al <= bl) && a <= b && b <= (a | if al == 0 { u32::MAX } else if al == 32 { 0 } else { u32::MAX >> al }))
            == (a <= b && (b | if bl == 0 { u32::MAX } else if bl == 32 { 0 } else { u32::MAX >> bl })
                    <= (a | if al == 0 { u32::MAX } else if al == 32 { 0 } else { u32::MAX >> al })),
        (a <= (b | if bl == 0 { u32::MAX } else if bl == 32 { 0 } else { u32::MAX >> bl })
            && b <= (a | if al == 0 { u32::MAX } else if al == 32 { 0 } else { u32::MAX >> al }))
        == ((a <= b && (b | if bl == 0 { u32::MAX } else if bl == 32 { 0 } else { u32::MAX >> bl })
                    <= (a | if al == 0 { u32::MAX } else if al == 32 { 0 } else { u32::MAX >> al }))
            || (b <= a && (a | if al == 0 { u32::MAX } else if al == 32 { 0 } else { u32::MAX >> al })
                    <= (b | if bl == 0 { u32::MAX } else if bl == 32 { 0 } else { u32::MAX >> bl }))),
{
    let ah:u32 = if al == 0 { u32::MAX } else if al == 32 { 0 } else { u32::MAX >> al };
    let bh:u32 = if bl == 0 { u32::MAX } else if bl == 32 { 0 } else { u32::MAX >> bl };
    assert(((al <= bl) && a <= b && b <= (a | ah)) == (a <= b && (b | bh) <= (a | ah)))
        by(bit_vector) requires al <= 32, bl <= 32,
            a == (if al == 0 { 0u32 } else { a & (u32::MAX << ((32-al) as u8)) }),
            b == (if bl == 0 { 0u32 } else { b & (u32::MAX << ((32-bl) as u8)) }),
            ah == (if al == 0 { u32::MAX } else if al == 32 { 0u32 } else { u32::MAX >> al }),
            bh == (if bl == 0 { u32::MAX } else if bl == 32 { 0u32 } else { u32::MAX >> bl });
    assert((a <= (b | bh) && b <= (a | ah)) == ((a <= b && (b | bh) <= (a | ah)) || (b <= a && (a | ah) <= (b | bh))))
        by(bit_vector) requires al <= 32, bl <= 32,
            a == (if al == 0 { 0u32 } else { a & (u32::MAX << ((32-al) as u8)) }),
            b == (if bl == 0 { 0u32 } else { b & (u32::MAX << ((32-bl) as u8)) }),
            ah == (if al == 0 { u32::MAX } else if al == 32 { 0u32 } else { u32::MAX >> al }),
            bh == (if bl == 0 { u32::MAX } else if bl == 32 { 0u32 } else { u32::MAX >> bl });
}

proof fn cidr_relations128(a:u128, al:u8, b:u128, bl:u8)
    requires al <= 128, bl <= 128,
        a == (if al == 0 { 0 } else { a & (u128::MAX << ((128-al) as u8)) }),
        b == (if bl == 0 { 0 } else { b & (u128::MAX << ((128-bl) as u8)) }),
    ensures
        ((al <= bl) && a <= b && b <= (a | if al == 0 { u128::MAX } else if al == 128 { 0 } else { u128::MAX >> al }))
            == (a <= b && (b | if bl == 0 { u128::MAX } else if bl == 128 { 0 } else { u128::MAX >> bl })
                    <= (a | if al == 0 { u128::MAX } else if al == 128 { 0 } else { u128::MAX >> al })),
        (a <= (b | if bl == 0 { u128::MAX } else if bl == 128 { 0 } else { u128::MAX >> bl })
            && b <= (a | if al == 0 { u128::MAX } else if al == 128 { 0 } else { u128::MAX >> al }))
        == ((a <= b && (b | if bl == 0 { u128::MAX } else if bl == 128 { 0 } else { u128::MAX >> bl })
                    <= (a | if al == 0 { u128::MAX } else if al == 128 { 0 } else { u128::MAX >> al }))
            || (b <= a && (a | if al == 0 { u128::MAX } else if al == 128 { 0 } else { u128::MAX >> al })
                    <= (b | if bl == 0 { u128::MAX } else if bl == 128 { 0 } else { u128::MAX >> bl }))),
{
    let ah:u128 = if al == 0 { u128::MAX } else if al == 128 { 0 } else { u128::MAX >> al };
    let bh:u128 = if bl == 0 { u128::MAX } else if bl == 128 { 0 } else { u128::MAX >> bl };
    assert(((al <= bl) && a <= b && b <= (a | ah)) == (a <= b && (b | bh) <= (a | ah)))
        by(bit_vector) requires al <= 128, bl <= 128,
            a == (if al == 0 { 0u128 } else { a & (u128::MAX << ((128-al) as u8)) }),
            b == (if bl == 0 { 0u128 } else { b & (u128::MAX << ((128-bl) as u8)) }),
            ah == (if al == 0 { u128::MAX } else if al == 128 { 0u128 } else { u128::MAX >> al }),
            bh == (if bl == 0 { u128::MAX } else if bl == 128 { 0u128 } else { u128::MAX >> bl });
    assert((a <= (b | bh) && b <= (a | ah)) == ((a <= b && (b | bh) <= (a | ah)) || (b <= a && (a | ah) <= (b | bh))))
        by(bit_vector) requires al <= 128, bl <= 128,
            a == (if al == 0 { 0u128 } else { a & (u128::MAX << ((128-al) as u8)) }),
            b == (if bl == 0 { 0u128 } else { b & (u128::MAX << ((128-bl) as u8)) }),
            ah == (if al == 0 { u128::MAX } else if al == 128 { 0u128 } else { u128::MAX >> al }),
            bh == (if bl == 0 { u128::MAX } else if bl == 128 { 0u128 } else { u128::MAX >> bl });
}

impl Prefix {
    pub closed spec fn valid(self) -> bool {
        self.length <= width(self.network) && normalized(self.network,self.length) == self.network
    }
    pub closed spec fn address(self) -> Address { self.network }
    pub closed spec fn depth(self) -> u8 { self.length }

    pub fn new(address: Address, length: u8) -> (result: Result<Self, PrefixError>)
        ensures match result {
            Ok(p) => length <= width(address) && normalized(address,length) == address
                && p.valid() && p.address() == address && p.depth() == length,
            Err(PrefixError::InvalidLength) => length > width(address),
            Err(PrefixError::HostBitsSet) => length <= width(address) && normalized(address,length) != address,
        },
    {
        proof { reveal(Prefix::address); reveal(Prefix::depth); reveal(Prefix::valid); }
        let normalized = Self::normalize(address, length)?;
        let same = match (normalized.network, address) {
            (Address::V4(a), Address::V4(b)) => a == b,
            (Address::V6(a), Address::V6(b)) => a == b,
            _ => false,
        };
        if !same {
            return Err(PrefixError::HostBitsSet);
        }
        Ok(normalized)
    }

    pub fn normalize(address: Address, length: u8) -> (result: Result<Self, PrefixError>)
        ensures match result {
            Ok(p) => length <= width(address) && p.valid()
                && p.address() == normalized(address,length) && p.depth() == length,
            Err(error) => length > width(address) && error == PrefixError::InvalidLength,
        },
    {
        proof { reveal(Prefix::address); reveal(Prefix::depth); reveal(Prefix::valid); }
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
        proof {
            match address {
                Address::V4(value) => {
                    if length > 0 {
                        let mask = u32::MAX << (32 - length);
                        assert((value & mask) & mask == value & mask) by(bit_vector);
                    }
                },
                Address::V6(value) => {
                    if length > 0 {
                        let mask = u128::MAX << (128 - length);
                        assert((value & mask) & mask == value & mask) by(bit_vector);
                    }
                },
            }
        }
        Ok(Self { network, length })
    }

    pub fn contains_address(self, address: Address) -> (result: bool)
        requires self.valid(),
        ensures result == interval_member(address,self.address(),last_address(self.address(),self.depth())),
    {
        proof {
            reveal(Prefix::valid); reveal(Prefix::address); reveal(Prefix::depth);
            match (self.network,address) {
                (Address::V4(n),Address::V4(a)) => membership32(n,a,self.length),
                (Address::V6(n),Address::V6(a)) => membership128(n,a,self.length),
                _ => {},
            }
        }
        match (self.network, address) {
            (Address::V4(n), Address::V4(a)) => crate::kernel::prefix_equal32(a, n, self.length),
            (Address::V6(n), Address::V6(a)) => crate::kernel::prefix_equal128(a, n, self.length),
            _ => false,
        }
    }

    pub fn contains_prefix(self, other: Self) -> (result: bool)
        requires self.valid(), other.valid(),
        ensures result == interval_subset(self.address(),last_address(self.address(),self.depth()),other.address(),last_address(other.address(),other.depth())),
    {
        proof {
            reveal(Prefix::valid); reveal(Prefix::address); reveal(Prefix::depth);
            match (self.network,other.network) {
                (Address::V4(a),Address::V4(b)) => cidr_relations32(a,self.length,b,other.length),
                (Address::V6(a),Address::V6(b)) => cidr_relations128(a,self.length,b,other.length),
                _ => {},
            }
        }
        self.length <= other.length && self.contains_address(other.network)
    }

    pub fn overlaps(self, other: Self) -> (result: bool)
        requires self.valid(), other.valid(),
        ensures result == interval_overlap(self.address(),last_address(self.address(),self.depth()),other.address(),last_address(other.address(),other.depth())),
    {
        proof {
            reveal(Prefix::valid); reveal(Prefix::address); reveal(Prefix::depth);
            match (self.network,other.network) {
                (Address::V4(a),Address::V4(b)) => cidr_relations32(a,self.length,b,other.length),
                (Address::V6(a),Address::V6(b)) => cidr_relations128(a,self.length,b,other.length),
                _ => {},
            }
        }
        self.contains_prefix(other) || other.contains_prefix(self)
    }

    /// Inclusive address bounds. IPv6 /0 does not require representing 2^128.
    pub fn address_bounds(self) -> (result: (Address, Address))
        requires self.valid(),
        ensures result.0 == self.address(), result.1 == last_address(self.address(),self.depth()),
            interval_member(result.0,result.0,result.1), interval_member(result.1,result.0,result.1),
    {
        proof { reveal(Prefix::address); reveal(Prefix::depth); reveal(Prefix::valid); }
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
        proof {
            match (self.network,last) {
                (Address::V4(n),Address::V4(_)) => {
                    let mask: u32 = if self.length == 0 { u32::MAX }
                        else if self.length == 32 { 0 } else { u32::MAX >> self.length };
                    assert(n <= (n | mask)) by(bit_vector);
                },
                (Address::V6(n),Address::V6(_)) => {
                    let mask: u128 = if self.length == 0 { u128::MAX }
                        else if self.length == 128 { 0 } else { u128::MAX >> self.length };
                    assert(n <= (n | mask)) by(bit_vector);
                },
                _ => {},
            }
        }
        (self.network, last)
    }
}


}

impl Prefix {
    pub const fn network(self) -> Address {
        self.network
    }
    pub const fn length(self) -> u8 {
        self.length
    }
}
