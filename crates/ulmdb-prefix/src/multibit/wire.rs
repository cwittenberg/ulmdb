use super::Visits;
use ulmdb_core::{
    Address, Prefix,
    storage::{Error, ReadStore, Reference, Result, WriteStore},
};
pub const HEADER: usize = 64;
pub const MAX_SIZE: usize = HEADER + 31 * 16;
pub const TAG: u16 = 0x0104;
#[derive(Clone, Copy)]
pub(crate) struct Node {
    pub base: Prefix,
    pub values: [Reference; 15],
    pub children: [Reference; 16],
}
pub(crate) fn width(a: Address) -> u8 {
    if matches!(a, Address::V4(_)) { 32 } else { 128 }
}
pub(crate) fn family(a: Address, b: Address) -> bool {
    matches!(
        (a, b),
        (Address::V4(_), Address::V4(_)) | (Address::V6(_), Address::V6(_))
    )
}
pub(crate) fn number(a: Address) -> u128 {
    match a {
        Address::V4(n) => u128::from(n),
        Address::V6(n) => n,
    }
}
pub(crate) fn nibble(a: Address, depth: u8) -> usize {
    ((number(a) >> (width(a) - depth - 4)) & 15) as usize
}
pub(crate) fn aligned_prefix(p: Prefix) -> Prefix {
    Prefix::normalize(p.network(), p.length() / 4 * 4).unwrap()
}
pub(crate) fn slot(p: Prefix) -> usize {
    let k = p.length() % 4;
    if k == 0 {
        0
    } else {
        (1usize << k) - 1
            + ((number(p.network()) >> (width(p.network()) - p.length())) & ((1u128 << k) - 1))
                as usize
    }
}
pub(crate) fn common(a: Prefix, b: Prefix) -> u8 {
    let n = match (a.network(), b.network()) {
        (Address::V4(a), Address::V4(b)) => (a ^ b).leading_zeros(),
        (Address::V6(a), Address::V6(b)) => (a ^ b).leading_zeros(),
        _ => 0,
    };
    (n as u8).min(a.length()).min(b.length())
}
pub(crate) fn valid_ref(r: Reference) -> Result<()> {
    if !r.is_null() && (r.segment == 0 || !r.offset.is_multiple_of(16)) {
        return Err(Error::Corrupt("multibit reference alignment/null"));
    }
    Ok(())
}
fn mask(refs: &[Reference]) -> u16 {
    refs.iter()
        .enumerate()
        .fold(0, |a, (i, r)| a | if r.is_null() { 0 } else { 1u16 << i })
}
impl Node {
    pub fn empty(base: Prefix) -> Self {
        Self {
            base,
            values: [Reference::NULL; 15],
            children: [Reference::NULL; 16],
        }
    }
    pub fn masks(self) -> (u16, u16) {
        (mask(&self.values), mask(&self.children))
    }
    pub fn len(self) -> usize {
        let (v, c) = self.masks();
        HEADER + 16 * (v.count_ones() + c.count_ones()) as usize
    }
    pub fn prefix(self, index: usize) -> Prefix {
        let k = (usize::BITS - (index + 1).leading_zeros() - 1) as u8;
        let bits = index + 1 - (1usize << k);
        let n = number(self.base.network())
            | if k == 0 {
                0
            } else {
                (bits as u128) << (width(self.base.network()) - self.base.length() - k)
            };
        let a = if matches!(self.base.network(), Address::V4(_)) {
            Address::V4(n as u32)
        } else {
            Address::V6(n)
        };
        Prefix::new(a, self.base.length() + k).unwrap()
    }
    pub fn region(self, side: usize) -> Prefix {
        let n = number(self.base.network())
            | ((side as u128) << (width(self.base.network()) - self.base.length() - 4));
        let a = if matches!(self.base.network(), Address::V4(_)) {
            Address::V4(n as u32)
        } else {
            Address::V6(n)
        };
        Prefix::new(a, self.base.length() + 4).unwrap()
    }
    pub fn child<S: ReadStore + ?Sized>(
        self,
        store: &S,
        side: usize,
        visits: &mut Visits,
    ) -> Result<Option<(Reference, Self)>> {
        let at = self.children[side];
        if at.is_null() {
            return Ok(None);
        }
        let child = Self::read(store, at, visits)?;
        if self.base.length() == width(self.base.network())
            || !self.region(side).contains_prefix(child.base)
        {
            return Err(Error::Corrupt("multibit child family/depth/route"));
        }
        Ok(Some((at, child)))
    }
    pub fn read<S: ReadStore + ?Sized>(
        store: &S,
        at: Reference,
        visits: &mut Visits,
    ) -> Result<Self> {
        visits.take()?;
        valid_ref(at)?;
        if at.is_null() {
            return Err(Error::Corrupt("null multibit node"));
        }
        let h = store.read(at, HEADER)?;
        if h.len() != HEADER {
            return Err(Error::Corrupt("short multibit header"));
        }
        let u16at = |p| u16::from_le_bytes(h[p..p + 2].try_into().unwrap());
        let size = u32::from_le_bytes(h[4..8].try_into().unwrap()) as usize;
        let (v, c) = (u16at(12), u16at(14));
        if u16at(0) != TAG
            || u16at(2) != 1
            || h[10] != 4
            || h[11] != 0
            || h[36..64] != [0; 28]
            || v & 0x8000 != 0
            || u16at(32) != v.count_ones() as u16
            || u16at(34) != c.count_ones() as u16
            || size != HEADER + 16 * (v.count_ones() + c.count_ones()) as usize
            || size > MAX_SIZE
            || v == 0 && c.count_ones() < 2
        {
            return Err(Error::Corrupt("multibit tag/version/masks/count/length"));
        }
        let address = match h[8] {
            4 if h[20..32] == [0; 12] => {
                Address::V4(u32::from_be_bytes(h[16..20].try_into().unwrap()))
            }
            6 => Address::V6(u128::from_be_bytes(h[16..32].try_into().unwrap())),
            _ => return Err(Error::Corrupt("multibit family")),
        };
        let base =
            Prefix::new(address, h[9]).map_err(|_| Error::Corrupt("multibit canonical base"))?;
        if !base.length().is_multiple_of(4) || base.length() == width(address) && (v != 1 || c != 0)
        {
            return Err(Error::Corrupt("multibit base/terminal mask"));
        }
        let b = store.read(at, size)?;
        if b.len() != size {
            return Err(Error::Corrupt("short multibit object"));
        }
        let mut node = Self::empty(base);
        let mut offset = HEADER;
        for (refs, bits) in [(&mut node.values[..], v), (&mut node.children[..], c)] {
            for (i, r) in refs.iter_mut().enumerate() {
                if bits & (1 << i) != 0 {
                    *r = Reference::decode(&b[offset..offset + 16])?;
                    valid_ref(*r)?;
                    if r.is_null() {
                        return Err(Error::Corrupt("multibit packed null"));
                    }
                    offset += 16;
                }
            }
        }
        Ok(node)
    }
    pub fn persist<S: WriteStore + ?Sized>(
        self,
        store: &mut S,
        old: Option<(Reference, usize)>,
    ) -> Result<Reference> {
        let (v, c) = self.masks();
        let len = self.len();
        let mut b = [0u8; MAX_SIZE];
        b[..2].copy_from_slice(&TAG.to_le_bytes());
        b[2..4].copy_from_slice(&1u16.to_le_bytes());
        b[4..8].copy_from_slice(&(len as u32).to_le_bytes());
        b[9] = self.base.length();
        b[10] = 4;
        match self.base.network() {
            Address::V4(n) => {
                b[8] = 4;
                b[16..20].copy_from_slice(&n.to_be_bytes());
            }
            Address::V6(n) => {
                b[8] = 6;
                b[16..32].copy_from_slice(&n.to_be_bytes());
            }
        }
        b[12..14].copy_from_slice(&v.to_le_bytes());
        b[14..16].copy_from_slice(&c.to_le_bytes());
        b[32..34].copy_from_slice(&(v.count_ones() as u16).to_le_bytes());
        b[34..36].copy_from_slice(&(c.count_ones() as u16).to_le_bytes());
        let mut offset = HEADER;
        for r in self.values.into_iter().chain(self.children) {
            if !r.is_null() {
                valid_ref(r)?;
                b[offset..offset + 16].copy_from_slice(&r.encode());
                offset += 16;
            }
        }
        let at = if let Some((at, old_len)) = old.filter(|(_, n)| *n == len) {
            if store.read(at, old_len)? == &b[..len] {
                return Ok(at);
            }
            store.rewrite(at, &b[..len], 16)?
        } else {
            store.allocate(&b[..len], 16)?
        };
        valid_ref(at)?;
        if at.is_null() {
            return Err(Error::Corrupt("allocator returned null multibit node"));
        }
        Ok(at)
    }
}
