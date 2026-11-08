//! Borrowed packed nodes. Validation covers every stored reference before any
//! rank-based access; point readers do not expand 31 references into arrays.
use super::{
    Visits,
    wire::{self, HEADER, MAX_SIZE, TAG},
};
use ulmdb_core::{
    Address, Prefix,
    storage::{Error, ReadStore, Reference, Result},
};

#[derive(Clone, Copy)]
pub(crate) struct NodeView<'a> {
    pub base: Prefix,
    bytes: &'a [u8],
    values: u16,
    children: u16,
}

impl<'a> NodeView<'a> {
    pub fn read<S: ReadStore + ?Sized>(
        store: &'a S,
        at: Reference,
        visits: &mut Visits,
    ) -> Result<Self> {
        visits.take()?;
        wire::valid_ref(at)?;
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
        if !base.length().is_multiple_of(4)
            || base.length() == wire::width(address) && (v != 1 || c != 0)
        {
            return Err(Error::Corrupt("multibit base/terminal mask"));
        }
        let bytes = store.read(at, size)?;
        if bytes.len() != size {
            return Err(Error::Corrupt("short multibit object"));
        }
        // Preserve whole-node validation: unselected packed references are
        // checked as well. Exact length makes this an integral number of refs.
        for encoded in bytes[HEADER..].chunks_exact(16) {
            let reference = Reference::decode(encoded)?;
            wire::valid_ref(reference)?;
            if reference.is_null() {
                return Err(Error::Corrupt("multibit packed null"));
            }
        }
        Ok(Self {
            base,
            bytes,
            values: v,
            children: c,
        })
    }

    fn reference(self, mask: u16, index: usize, preceding: usize) -> Result<Reference> {
        let bit = 1u16 << index;
        if mask & bit == 0 {
            return Ok(Reference::NULL);
        }
        let rank = (mask & (bit - 1)).count_ones() as usize;
        let offset = HEADER + 16 * (preceding + rank);
        Reference::decode(&self.bytes[offset..offset + 16])
    }

    pub fn value(self, index: usize) -> Result<Reference> {
        self.reference(self.values, index, 0)
    }

    pub fn child<S: ReadStore + ?Sized>(
        self,
        store: &'a S,
        side: usize,
        visits: &mut Visits,
    ) -> Result<Option<Self>> {
        let at = self.reference(self.children, side, self.values.count_ones() as usize)?;
        if at.is_null() {
            return Ok(None);
        }
        let child = Self::read(store, at, visits)?;
        if self.base.length() == wire::width(self.base.network())
            || child.base.length() < self.base.length() + 4
            || !self.base.contains_prefix(child.base)
            || wire::nibble(child.base.network(), self.base.length()) != side
        {
            return Err(Error::Corrupt("multibit child family/depth/route"));
        }
        Ok(Some(child))
    }

    #[cfg(test)]
    pub fn expand(self) -> Result<wire::Node> {
        let mut node = wire::Node::empty(self.base);
        for (i, value) in node.values.iter_mut().enumerate() {
            *value = self.value(i)?;
        }
        for (i, child) in node.children.iter_mut().enumerate() {
            *child = self.reference(self.children, i, self.values.count_ones() as usize)?;
        }
        Ok(node)
    }
}
