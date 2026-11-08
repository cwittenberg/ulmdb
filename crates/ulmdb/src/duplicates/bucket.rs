use super::{MAX_VALUE, validate_stored};
use crate::{Error, Result};
use ulmdb_core::storage::{ReadStore, Reference};
use ulmdb_kv::ordering::{Bytewise, Comparator};

pub(crate) const INLINE_BYTES: usize = 512;
pub(super) const INLINE_COUNT: usize = 8;

#[derive(Clone, Copy)]
pub(crate) struct Inline<'a> {
    pub bytes: &'a [u8],
    spans: [(u16, u16); INLINE_COUNT],
    pub count: usize,
}
impl<'a> Inline<'a> {
    pub fn value(self, index: usize) -> &'a [u8] {
        let (start, end) = self.spans[index];
        &self.bytes[usize::from(start)..usize::from(end)]
    }
    pub fn search(self, value: &[u8], upper: bool) -> usize {
        self.search_with(value, upper, &Bytewise)
    }
    pub fn search_with<C: Comparator + ?Sized>(
        self,
        value: &[u8],
        upper: bool,
        order: &C,
    ) -> usize {
        let (mut low, mut high) = (0, self.count);
        while low < high {
            let mid = low + (high - low) / 2;
            let probe = self.value(mid);
            if order.compare(probe, value).is_lt() || (upper && order.compare(probe, value).is_eq())
            {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        low
    }
    fn decode(bytes: &'a [u8], count: u64, width: Option<u32>) -> Result<Self> {
        Self::decode_with(bytes, count, width, Some(&Bytewise))
    }
    fn decode_with(
        bytes: &'a [u8],
        count: u64,
        width: Option<u32>,
        order: Option<&dyn Comparator>,
    ) -> Result<Self> {
        if bytes.len() > INLINE_BYTES || count > INLINE_COUNT as u64 {
            return Err(Error::Corrupt("inline duplicate bucket exceeds limits"));
        }
        let mut bucket = Self {
            bytes,
            spans: [(0, 0); INLINE_COUNT],
            count: count as usize,
        };
        let mut offset = 24usize;
        for index in 0..bucket.count {
            let length = u32::from_le_bytes(
                bytes
                    .get(offset..offset + 4)
                    .ok_or(Error::Corrupt("short inline duplicate frame"))?
                    .try_into()
                    .map_err(|_| Error::Corrupt("inline duplicate length"))?,
            ) as usize;
            offset += 4;
            let end = offset
                .checked_add(length)
                .ok_or(Error::Corrupt("inline duplicate extent overflow"))?;
            let value = bytes
                .get(offset..end)
                .ok_or(Error::Corrupt("short inline duplicate value"))?;
            validate_stored(value, width)?;
            bucket.spans[index] = (offset as u16, end as u16);
            if let Some(order) = order {
                order
                    .validate(value)
                    .map_err(|_| Error::Corrupt("stored duplicate ordering width"))?;
            }
            if index != 0
                && order.is_some_and(|o| !o.compare(bucket.value(index - 1), value).is_lt())
            {
                return Err(Error::Corrupt("unordered inline duplicate values"));
            }
            offset = end;
        }
        if offset != bytes.len() {
            return Err(Error::Corrupt("trailing inline duplicate bytes"));
        }
        Ok(bucket)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Bucket<'a> {
    Inline(Inline<'a>),
    Tree { root: Reference, count: u64 },
}
impl<'a> Bucket<'a> {
    pub fn decode(bytes: &'a [u8], width: Option<u32>) -> Result<Self> {
        Self::decode_with(bytes, width, Some(&Bytewise))
    }
    pub fn decode_with(
        bytes: &'a [u8],
        width: Option<u32>,
        order: Option<&dyn Comparator>,
    ) -> Result<Self> {
        if bytes.len() < 24 {
            return Err(Error::Corrupt("short duplicate bucket"));
        }
        let root = Reference::decode(&bytes[..16])?;
        let count = u64::from_le_bytes(
            bytes[16..24]
                .try_into()
                .map_err(|_| Error::Corrupt("duplicate count"))?,
        );
        if count == 0 {
            return Err(Error::Corrupt("empty duplicate bucket"));
        }
        if root.is_null() {
            Ok(Self::Inline(Inline::decode_with(
                bytes, count, width, order,
            )?))
        } else if bytes.len() == 24 {
            Ok(Self::Tree { root, count })
        } else {
            Err(Error::Corrupt("trailing subtree duplicate bytes"))
        }
    }
    pub fn get<S: ReadStore>(
        store: &'a S,
        root: Reference,
        key: &[u8],
        width: Option<u32>,
    ) -> Result<Option<Self>> {
        ulmdb_kv::get(store, root, key)?
            .map(|bytes| Self::decode(bytes, width))
            .transpose()
    }
    pub fn get_with<S: ReadStore, K: Comparator>(
        store: &'a S,
        root: Reference,
        key: &[u8],
        width: Option<u32>,
        keys: &K,
        values: &dyn Comparator,
    ) -> Result<Option<Self>> {
        ulmdb_kv::get_with(store, root, key, keys)?
            .map(|bytes| Self::decode_with(bytes, width, Some(values)))
            .transpose()
    }
    pub fn count(self) -> u64 {
        match self {
            Self::Inline(inline) => inline.count as u64,
            Self::Tree { count, .. } => count,
        }
    }
    pub fn seek<S: ReadStore>(self, store: &'a S, value: &[u8]) -> Result<Option<&'a [u8]>> {
        self.seek_with(store, value, &Bytewise)
    }
    pub fn seek_with<S: ReadStore, C: Comparator + Clone>(
        self,
        store: &'a S,
        value: &[u8],
        order: &C,
    ) -> Result<Option<&'a [u8]>> {
        order.validate(value)?;
        match self {
            Self::Inline(inline) => {
                let index = inline.search_with(value, false, order);
                Ok(
                    (index < inline.count && order.compare(inline.value(index), value).is_eq())
                        .then(|| inline.value(index)),
                )
            }
            Self::Tree { root, .. } => {
                let Some((stored, empty)) = ulmdb_kv::get_pair_with(store, root, value, order)?
                else {
                    return Ok(None);
                };
                if !empty.is_empty() {
                    return Err(Error::Corrupt("duplicate child value must be empty"));
                }
                if stored.len() > MAX_VALUE {
                    return Err(Error::Corrupt("stored duplicate size exceeds limit"));
                }
                Ok(Some(stored))
            }
        }
    }
}

pub(crate) struct InlineBuffer {
    pub bytes: [u8; INLINE_BYTES],
    length: usize,
    count: usize,
}
impl InlineBuffer {
    pub fn new() -> Self {
        Self {
            bytes: [0; INLINE_BYTES],
            length: 24,
            count: 0,
        }
    }
    pub fn copy(inline: Inline<'_>) -> Self {
        let mut result = Self::new();
        result.bytes[..inline.bytes.len()].copy_from_slice(inline.bytes);
        result.length = inline.bytes.len();
        result.count = inline.count;
        result
    }
    pub fn push(&mut self, value: &[u8]) -> bool {
        if self.count == INLINE_COUNT
            || value.len() > INLINE_BYTES - self.length
            || 4 + value.len() > INLINE_BYTES - self.length
        {
            return false;
        }
        self.bytes[self.length..self.length + 4]
            .copy_from_slice(&(value.len() as u32).to_le_bytes());
        self.length += 4;
        self.bytes[self.length..self.length + value.len()].copy_from_slice(value);
        self.length += value.len();
        self.count += 1;
        self.bytes[16..24].copy_from_slice(&(self.count as u64).to_le_bytes());
        true
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
    pub fn decode(&self, width: Option<u32>) -> Result<Inline<'_>> {
        Inline::decode(self.as_bytes(), self.count as u64, width)
    }
    pub fn decode_structure(&self, width: Option<u32>) -> Result<Inline<'_>> {
        Inline::decode_with(self.as_bytes(), self.count as u64, width, None)
    }
    pub fn decode_with(&self, width: Option<u32>, order: &dyn Comparator) -> Result<Inline<'_>> {
        Inline::decode_with(self.as_bytes(), self.count as u64, width, Some(order))
    }
}

// Keep the bounded inline copy on the stack rather than allocating a Box for
// the overwhelmingly common small-bucket write path.
#[allow(clippy::large_enum_variant)]
pub(super) enum OwnedBucket {
    Missing,
    Inline(InlineBuffer),
    Tree { root: Reference, count: u64 },
}
impl OwnedBucket {
    pub fn copy(bucket: Option<Bucket<'_>>) -> Self {
        match bucket {
            None => Self::Missing,
            Some(Bucket::Inline(inline)) => Self::Inline(InlineBuffer::copy(inline)),
            Some(Bucket::Tree { root, count }) => Self::Tree { root, count },
        }
    }
}
pub(super) fn tree_bytes(root: Reference, count: u64) -> [u8; 24] {
    let mut bytes = [0; 24];
    bytes[..16].copy_from_slice(&root.encode());
    bytes[16..].copy_from_slice(&count.to_le_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inline_decode_rejects_bad_extents_order_count_width_and_tags() {
        let mut good = InlineBuffer::new();
        assert!(good.push(b"a"));
        assert!(good.push(b"b"));
        assert_eq!(Bucket::decode(good.as_bytes(), Some(1)).unwrap().count(), 2);
        for bytes in [
            &good.as_bytes()[..24],
            &good.as_bytes()[..good.as_bytes().len() - 1],
        ] {
            assert!(Bucket::decode(bytes, None).is_err());
        }
        assert!(Bucket::decode(good.as_bytes(), Some(2)).is_err());
        let mut bad = good.as_bytes().to_vec();
        bad.push(0);
        assert!(Bucket::decode(&bad, None).is_err());
        let mut bad = good.as_bytes().to_vec();
        bad[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Bucket::decode(&bad, None).is_err());
        for count in [0u64, 1, 3, 9, u64::MAX] {
            let mut bad = good.as_bytes().to_vec();
            bad[16..24].copy_from_slice(&count.to_le_bytes());
            assert!(Bucket::decode(&bad, None).is_err());
        }
        for last in [b'a', b'0'] {
            let mut bad = good.as_bytes().to_vec();
            *bad.last_mut().unwrap() = last;
            assert!(Bucket::decode(&bad, None).is_err());
        }
        let mut bad = good.as_bytes().to_vec();
        bad[..16].copy_from_slice(
            &Reference {
                segment: 1,
                offset: 4096,
            }
            .encode(),
        );
        assert!(Bucket::decode(&bad, None).is_err());
        let mut bad = good.as_bytes().to_vec();
        bad[8] = 1; // segment zero with a nonzero offset is never NULL.
        assert!(Bucket::decode(&bad, None).is_err());
        assert!(Bucket::decode(&tree_bytes(Reference::NULL, 1), None).is_err());
    }

    #[test]
    fn inline_builder_exact_boundary_and_empty_values() {
        let mut singleton = InlineBuffer::new();
        assert!(singleton.push(&[1; INLINE_BYTES - 28]));
        assert_eq!(singleton.as_bytes().len(), INLINE_BYTES);
        assert!(!singleton.push(b""));
        assert!(Bucket::decode(singleton.as_bytes(), None).is_ok());
        let mut empty = InlineBuffer::new();
        assert!(empty.push(b""));
        assert_eq!(empty.as_bytes().len(), 28);
        assert_eq!(empty.decode(Some(0)).unwrap().value(0), b"");
    }
}
