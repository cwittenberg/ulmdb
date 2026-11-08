//! Compile-time family-specialized traversal; stored nodes remain exactly96B.
use ulmdb_core::{
    Address, Prefix,
    storage::{Error, ReadStore, Reference, Result},
};
#[derive(Clone, Copy)]
pub(crate) struct ScalarNode<W> {
    pub(crate) network: W,
    pub(crate) length: u8,
    pub(crate) children: [Reference; 2],
    pub(crate) value: Reference,
}
#[derive(Clone, Copy)]
pub(crate) enum Root {
    Empty,
    V4(ScalarNode<u32>),
    V6(ScalarNode<u128>),
}
trait Family {
    type Word: Copy;
    const TAG: u8;
    const BITS: u8;
    fn network(bytes: &[u8]) -> Self::Word;
    fn canonical(network: Self::Word, length: u8) -> bool;
    fn contains(network: Self::Word, length: u8, other: Self::Word) -> bool;
    fn branch(network: Self::Word, depth: u8) -> usize;
    fn address(network: Self::Word) -> Address;
}
struct V4;
struct V6;
macro_rules! family {
    ($family:ty,$word:ty,$tag:literal,$bits:literal,$end:literal,$address:path) => {
        impl Family for $family {
            type Word = $word;
            const TAG: u8 = $tag;
            const BITS: u8 = $bits;
            #[inline]
            fn network(b: &[u8]) -> Self::Word {
                <$word>::from_be_bytes(b[16..$end].try_into().unwrap())
            }
            #[inline]
            fn canonical(network: Self::Word, length: u8) -> bool {
                length <= Self::BITS
                    && (length == Self::BITS || network & (<$word>::MAX >> length) == 0)
            }
            #[inline]
            fn contains(network: Self::Word, length: u8, other: Self::Word) -> bool {
                (network ^ other).leading_zeros() >= u32::from(length)
            }
            #[inline]
            fn branch(network: Self::Word, depth: u8) -> usize {
                ((network >> (Self::BITS - 1 - depth)) & 1) as usize
            }
            #[inline]
            fn address(network: Self::Word) -> Address {
                $address(network)
            }
        }
    };
}
family!(V4, u32, 4, 32, 20, Address::V4);
family!(V6, u128, 6, 128, 32, Address::V6);
fn aligned(at: Reference) -> Result<()> {
    if !at.is_null() && (at.segment == 0 || !at.offset.is_multiple_of(16)) {
        return Err(Error::Corrupt("unaligned radix reference"));
    }
    Ok(())
}
fn bytes<S: ReadStore + ?Sized>(store: &S, at: Reference) -> Result<&[u8]> {
    aligned(at)?;
    if at.is_null() {
        return Err(Error::Corrupt("null radix node"));
    }
    let b = store.read(at, 96)?;
    // Match the original decoder for BOTH short and oversized trait violations.
    if b.len() != 96 {
        return Err(Error::Corrupt("short radix node"));
    }
    if b[..8] != [4, 0, 0, 0, 96, 0, 0, 0] || b[10..16] != [0; 6] || b[80..96] != [0; 16] {
        return Err(Error::Corrupt("invalid radix header"));
    }
    Ok(b)
}
fn decode<F: Family>(b: &[u8]) -> Result<ScalarNode<F::Word>> {
    // Only private callers that checked exact length/header and the actual tag.
    if F::TAG == 4 && b[20..32] != [0; 12] {
        return Err(Error::Corrupt("invalid radix address family"));
    }
    let network = F::network(b);
    let length = b[9];
    if !F::canonical(network, length) {
        return Err(Error::Corrupt("noncanonical radix prefix"));
    }
    // Decode ALL refs before alignment, preserving the original first-error order.
    let children = [
        Reference::decode(&b[32..48])?,
        Reference::decode(&b[48..64])?,
    ];
    let value = Reference::decode(&b[64..80])?;
    for at in children.into_iter().chain([value]) {
        aligned(at)?;
    }
    if length == F::BITS && children.iter().any(|r| !r.is_null()) {
        return Err(Error::Corrupt("full-length radix node has children"));
    }
    Ok(ScalarNode {
        network,
        length,
        children,
        value,
    })
}
fn actual(b: &[u8]) -> Result<Root> {
    match b[8] {
        4 => decode::<V4>(b).map(Root::V4),
        6 => decode::<V6>(b).map(Root::V6),
        _ => Err(Error::Corrupt("invalid radix address family")),
    }
}
pub(crate) fn read_root<S: ReadStore + ?Sized>(store: &S, at: Reference) -> Result<Root> {
    if at.is_null() {
        Ok(Root::Empty)
    } else {
        actual(bytes(store, at)?)
    }
}
fn read_for<F: Family, S: ReadStore + ?Sized>(
    store: &S,
    at: Reference,
) -> Result<Option<ScalarNode<F::Word>>> {
    let b = bytes(store, at)?;
    if b[8] == F::TAG {
        decode::<F>(b).map(Some)
    } else {
        // A valid other-family node is still fully checked before mismatch.
        // Reuse the SAME borrowed96B bytes, never a hidden second store read.
        actual(b)?;
        Ok(None)
    }
}
impl Root {
    pub(crate) fn prefix(&self) -> Option<Prefix> {
        match self {
            Self::Empty => None,
            Self::V4(n) => {
                Some(Prefix::new(Address::V4(n.network), n.length).expect("checked scalar root"))
            }
            Self::V6(n) => {
                Some(Prefix::new(Address::V6(n.network), n.length).expect("checked scalar root"))
            }
        }
    }
}
fn traverse<'a, F: Family, S: ReadStore + ?Sized>(
    store: &'a S,
    root: &ScalarNode<F::Word>,
    query: F::Word,
    limit: u8,
    exact: bool,
) -> Result<Option<(Prefix, &'a [u8])>> {
    if root.length > limit || !F::contains(root.network, root.length, query) {
        return Ok(None);
    }
    let mut node = *root;
    let mut best = None;
    loop {
        if !node.value.is_null() && (!exact || node.length == limit) {
            best = Some((node.network, node.length, node.value));
        }
        if node.length == limit {
            break;
        }
        let side = F::branch(query, node.length);
        let at = node.children[side];
        if at.is_null() {
            break;
        }
        let child = read_for::<F, S>(store, at)?
            .ok_or(Error::Corrupt("invalid radix child relationship"))?;
        if child.length <= node.length
            || !F::contains(node.network, node.length, child.network)
            || F::branch(child.network, node.length) != side
        {
            return Err(Error::Corrupt("invalid radix child relationship"));
        }
        if child.length > limit || !F::contains(child.network, child.length, query) {
            break;
        }
        node = child;
    }
    let Some((network, length, value)) = best else {
        return Ok(None);
    };
    // Decoder-issued private state is canonical; no Prefix is built per visited node.
    let prefix = Prefix::new(F::address(network), length)
        .map_err(|_| Error::Corrupt("noncanonical radix prefix"))?;
    store.payload(value).map(|bytes| Some((prefix, bytes)))
}
pub(crate) fn search_from<'a, S: ReadStore + ?Sized>(
    store: &'a S,
    root: &Root,
    address: Address,
    limit: u8,
    exact: bool,
) -> Result<Option<(Prefix, &'a [u8])>> {
    match (root, address) {
        (Root::V4(n), Address::V4(a)) => traverse::<V4, S>(store, n, a, limit, exact),
        (Root::V6(n), Address::V6(a)) => traverse::<V6, S>(store, n, a, limit, exact),
        _ => Ok(None),
    }
}
pub fn get<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    address: Address,
) -> Result<Option<(Prefix, &[u8])>> {
    longest_match(store, root, address)
}
pub fn longest_match<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    address: Address,
) -> Result<Option<(Prefix, &[u8])>> {
    let root = read_root(store, root)?;
    let limit = match address {
        Address::V4(_) => 32,
        Address::V6(_) => 128,
    };
    search_from(store, &root, address, limit, false)
}
pub fn exact<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    prefix: Prefix,
) -> Result<Option<(Prefix, &[u8])>> {
    search_from(
        store,
        &read_root(store, root)?,
        prefix.network(),
        prefix.length(),
        true,
    )
}
pub fn covering<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    prefix: Prefix,
) -> Result<Option<(Prefix, &[u8])>> {
    search_from(
        store,
        &read_root(store, root)?,
        prefix.network(),
        prefix.length(),
        false,
    )
}
