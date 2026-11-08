use ulmdb_core::{
    Address, Prefix,
    storage::{Error, ReadStore, Reference, Result},
};

pub(crate) const SIZE: usize = 96;

#[derive(Clone, Copy)]
pub(crate) struct Node {
    pub prefix: Prefix,
    pub children: [Reference; 2],
    pub value: Reference,
}

pub(crate) fn same_family(a: Address, b: Address) -> bool {
    matches!(
        (a, b),
        (Address::V4(_), Address::V4(_)) | (Address::V6(_), Address::V6(_))
    )
}
pub(crate) fn width(a: Address) -> u8 {
    match a {
        Address::V4(_) => 32,
        Address::V6(_) => 128,
    }
}
pub(crate) fn branch(a: Address, depth: u8) -> usize {
    match a {
        Address::V4(v) => ((v >> (31 - depth)) & 1) as usize,
        Address::V6(v) => ((v >> (127 - depth)) & 1) as usize,
    }
}
pub(crate) fn common(a: Prefix, b: Prefix) -> u8 {
    let zeros = match (a.network(), b.network()) {
        (Address::V4(a), Address::V4(b)) => (a ^ b).leading_zeros(),
        (Address::V6(a), Address::V6(b)) => (a ^ b).leading_zeros(),
        _ => 0,
    };
    (zeros as u8).min(a.length()).min(b.length())
}
fn aligned(at: Reference) -> Result<()> {
    if !at.is_null() && (at.segment == 0 || !at.offset.is_multiple_of(16)) {
        return Err(Error::Corrupt("unaligned radix reference"));
    }
    Ok(())
}

impl Node {
    pub fn leaf(prefix: Prefix, value: Reference) -> Self {
        Self {
            prefix,
            children: [Reference::NULL; 2],
            value,
        }
    }
    pub fn read<S: ReadStore + ?Sized>(store: &S, at: Reference) -> Result<Self> {
        aligned(at)?;
        if at.is_null() {
            return Err(Error::Corrupt("null radix node"));
        }
        let b = store.read(at, SIZE)?;
        let decoded = ulmdb_core::radix_wire::decode(b).map_err(|error| {
            use ulmdb_core::radix_wire::DecodeError;
            Error::Corrupt(match error {
                DecodeError::Size => "short radix node",
                DecodeError::Header => "invalid radix header",
                DecodeError::Family => "invalid radix address family",
                DecodeError::Prefix => "noncanonical radix prefix",
                DecodeError::Reference => "malformed null reference",
                DecodeError::Alignment => "unaligned radix reference",
                DecodeError::FullWidthChildren => "full-length radix node has children",
            })
        })?;
        let reference = |(segment, offset)| Reference { segment, offset };
        Ok(Self {
            prefix: decoded.prefix,
            children: [
                reference(decoded.children[0]),
                reference(decoded.children[1]),
            ],
            value: reference(decoded.value),
        })
    }
    pub fn encode(self) -> [u8; SIZE] {
        let mut b = [0; SIZE];
        b[..8].copy_from_slice(&[4, 0, 0, 0, 96, 0, 0, 0]);
        b[9] = self.prefix.length();
        match self.prefix.network() {
            Address::V4(v) => {
                b[8] = 4;
                b[16..20].copy_from_slice(&v.to_be_bytes());
            }
            Address::V6(v) => {
                b[8] = 6;
                b[16..32].copy_from_slice(&v.to_be_bytes());
            }
        }
        b[32..48].copy_from_slice(&self.children[0].encode());
        b[48..64].copy_from_slice(&self.children[1].encode());
        b[64..80].copy_from_slice(&self.value.encode());
        b
    }
    pub fn child<S: ReadStore + ?Sized>(self, store: &S, side: usize) -> Result<Option<Self>> {
        let at = self.children[side];
        if at.is_null() {
            return Ok(None);
        }
        let child = Self::read(store, at)?;
        check_child(self.prefix, child.prefix, side)?;
        Ok(Some(child))
    }
}
pub(crate) fn check_child(parent: Prefix, child: Prefix, side: usize) -> Result<()> {
    if child.length() <= parent.length()
        || !parent.contains_prefix(child)
        || branch(child.network(), parent.length()) != side
    {
        return Err(Error::Corrupt("invalid radix child relationship"));
    }
    Ok(())
}
