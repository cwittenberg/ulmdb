//! Ordered transaction deltas with an append path and a bounded tree fallback.
//! Entries are the pending SQL write set, never cached committed query results.
use super::Key;
use std::{collections::BTreeMap, mem, ops::Bound};

enum Storage<V> {
    Sorted(Vec<(Key, V)>),
    Tree(BTreeMap<Key, V>),
}

pub(super) struct Pending<V> {
    storage: Storage<V>,
}

impl<V> Pending<V> {
    pub(super) fn new() -> Self {
        Self {
            storage: Storage::Sorted(Vec::new()),
        }
    }

    pub(super) fn len(&self) -> usize {
        match &self.storage {
            Storage::Sorted(entries) => entries.len(),
            Storage::Tree(entries) => entries.len(),
        }
    }

    pub(super) fn get(&self, key: &Key) -> Option<&V> {
        match &self.storage {
            Storage::Sorted(entries) => {
                let (last_key, last_value) = entries.last()?;
                match key.cmp(last_key) {
                    std::cmp::Ordering::Greater => None,
                    std::cmp::Ordering::Equal => Some(last_value),
                    std::cmp::Ordering::Less => entries
                        .binary_search_by_key(key, |(key, _)| *key)
                        .ok()
                        .map(|index| &entries[index].1),
                }
            }
            Storage::Tree(entries) => entries.get(key),
        }
    }

    fn tree(&mut self) -> &mut BTreeMap<Key, V> {
        if let Storage::Sorted(entries) = &mut self.storage {
            // Each transaction converts at most once. No values are cloned,
            // and random insertions never shift an ever-growing vector.
            let entries = mem::take(entries).into_iter().collect();
            self.storage = Storage::Tree(entries);
        }
        match &mut self.storage {
            Storage::Tree(entries) => entries,
            Storage::Sorted(_) => unreachable!("conversion installs tree storage"),
        }
    }

    pub(super) fn insert(&mut self, key: Key, value: V) -> Option<V> {
        if let Storage::Sorted(entries) = &mut self.storage {
            if let Some((last_key, last_value)) = entries.last_mut() {
                match key.cmp(last_key) {
                    std::cmp::Ordering::Greater => {
                        entries.push((key, value));
                        return None;
                    }
                    std::cmp::Ordering::Equal => return Some(mem::replace(last_value, value)),
                    std::cmp::Ordering::Less => {
                        if let Ok(index) = entries.binary_search_by_key(&key, |(key, _)| *key) {
                            return Some(mem::replace(&mut entries[index].1, value));
                        }
                    }
                }
            } else {
                entries.push((key, value));
                return None;
            }
        }
        self.tree().insert(key, value)
    }

    pub(super) fn remove(&mut self, key: &Key) -> Option<V> {
        if let Storage::Sorted(entries) = &mut self.storage {
            let (last_key, _) = entries.last()?;
            match key.cmp(last_key) {
                std::cmp::Ordering::Greater => return None,
                std::cmp::Ordering::Equal => return entries.pop().map(|(_, value)| value),
                std::cmp::Ordering::Less => {
                    if entries.binary_search_by_key(key, |(key, _)| *key).is_err() {
                        return None;
                    }
                }
            }
        }
        self.tree().remove(key)
    }

    pub(super) fn iter(&self) -> Iter<'_, V> {
        match &self.storage {
            Storage::Sorted(entries) => Iter::Sorted(entries.iter()),
            Storage::Tree(entries) => Iter::Tree(entries.iter()),
        }
    }

    pub(super) fn first_from(&self, bound: Bound<Key>) -> Option<(&Key, &V)> {
        match &self.storage {
            Storage::Sorted(entries) => {
                let index = match bound {
                    Bound::Unbounded => 0,
                    Bound::Included(key) => {
                        entries.partition_point(|(candidate, _)| candidate < &key)
                    }
                    Bound::Excluded(key) => {
                        entries.partition_point(|(candidate, _)| candidate <= &key)
                    }
                };
                entries.get(index).map(|(key, value)| (key, value))
            }
            Storage::Tree(entries) => entries.range((bound, Bound::Unbounded)).next(),
        }
    }
}

impl<V> Default for Pending<V> {
    fn default() -> Self {
        Self::new()
    }
}

pub(super) enum Iter<'a, V> {
    Sorted(std::slice::Iter<'a, (Key, V)>),
    Tree(std::collections::btree_map::Iter<'a, Key, V>),
}

impl<'a, V> Iterator for Iter<'a, V> {
    type Item = (&'a Key, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Sorted(entries) => entries.next().map(|(key, value)| (key, value)),
            Self::Tree(entries) => entries.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Sorted(entries) => entries.size_hint(),
            Self::Tree(entries) => entries.size_hint(),
        }
    }
}

impl<V> DoubleEndedIterator for Iter<'_, V> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            Self::Sorted(entries) => entries.next_back().map(|(key, value)| (key, value)),
            Self::Tree(entries) => entries.next_back(),
        }
    }
}

impl<V> ExactSizeIterator for Iter<'_, V> {}

impl<'a, V> IntoIterator for &'a Pending<V> {
    type Item = (&'a Key, &'a V);
    type IntoIter = Iter<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Owned staging bytes. Small SQL values need no separate heap allocation.
/// Deliberately not Clone: replacing values moves the old owner into undo.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Value {
    Inline { length: u8, bytes: [u8; 64] },
    Heap(Vec<u8>),
}

impl Value {
    pub(super) fn from_slice(input: &[u8]) -> Self {
        if input.len() <= 64 {
            let mut bytes = [0; 64];
            bytes[..input.len()].copy_from_slice(input);
            Self::Inline {
                length: input.len() as u8,
                bytes,
            }
        } else {
            Self::Heap(input.to_vec())
        }
    }

    pub(super) fn as_slice(&self) -> &[u8] {
        match self {
            Self::Inline { length, bytes } => &bytes[..usize::from(*length)],
            Self::Heap(bytes) => bytes,
        }
    }

    pub(super) fn len(&self) -> usize {
        match self {
            Self::Inline { length, .. } => usize::from(*length),
            Self::Heap(bytes) => bytes.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(number: u64) -> Key {
        number.to_be_bytes()
    }

    fn check_model(pending: &Pending<i64>, expected: &BTreeMap<Key, i64>) {
        assert_eq!(pending.len(), expected.len());
        assert_eq!(pending.iter().len(), expected.len());
        assert!(pending.iter().eq(expected.iter()));
        assert!(pending.iter().rev().eq(expected.iter().rev()));
        for number in [0, 1, 2, 17, 63, 127, 128, 255, 256, 257, u64::MAX] {
            assert_eq!(pending.get(&key(number)), expected.get(&key(number)));
            for bound in [Bound::Included(key(number)), Bound::Excluded(key(number))] {
                assert_eq!(
                    pending.first_from(bound),
                    expected.range((bound, Bound::Unbounded)).next()
                );
            }
        }
        assert_eq!(
            pending.first_from(Bound::Unbounded),
            expected.first_key_value()
        );
    }

    #[test]
    fn monotonic_appends_replacements_and_tail_removals_keep_append_storage() {
        let mut pending = Pending::new();
        let mut expected = BTreeMap::new();
        check_model(&pending, &expected);
        for number in 0..4096 {
            let value = number as i64 * 3;
            assert_eq!(
                pending.insert(key(number), value),
                expected.insert(key(number), value)
            );
        }
        assert!(matches!(pending.storage, Storage::Sorted(_)));
        for number in [2048, 1, 4095, 0] {
            assert_eq!(
                pending.insert(key(number), -17),
                expected.insert(key(number), -17)
            );
        }
        assert!(matches!(pending.storage, Storage::Sorted(_)));
        for number in [4095, 4094, 9999] {
            assert_eq!(pending.remove(&key(number)), expected.remove(&key(number)));
        }
        assert!(matches!(pending.storage, Storage::Sorted(_)));
        check_model(&pending, &expected);
    }

    #[test]
    fn new_out_of_order_key_and_non_tail_removal_promote_once() {
        for remove_middle in [false, true] {
            let mut pending = Pending::new();
            let mut expected = BTreeMap::new();
            for number in [10, 20, 30] {
                pending.insert(key(number), number as i64);
                expected.insert(key(number), number as i64);
            }
            assert_eq!(pending.remove(&key(19)), None);
            assert!(matches!(pending.storage, Storage::Sorted(_)));
            if remove_middle {
                assert_eq!(pending.remove(&key(20)), expected.remove(&key(20)));
            } else {
                assert_eq!(pending.insert(key(15), 15), expected.insert(key(15), 15));
            }
            assert!(matches!(pending.storage, Storage::Tree(_)));
            check_model(&pending, &expected);
            for number in [10, 15, 20, 30] {
                assert_eq!(pending.remove(&key(number)), expected.remove(&key(number)));
            }
            assert_eq!(pending.len(), 0);
            assert!(matches!(pending.storage, Storage::Tree(_)));
            check_model(&pending, &expected);
        }
    }

    #[test]
    fn arbitrary_operations_match_btree_model_including_duplicate_replacements() {
        let mut pending = Pending::new();
        let mut expected = BTreeMap::new();
        let mut random = 0x8341_ace2_79b5_603du64;
        for operation in 0..5000 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let number = (random >> 19) % 257;
            match random % 5 {
                0 => assert_eq!(pending.remove(&key(number)), expected.remove(&key(number))),
                1 => assert_eq!(pending.get(&key(number)), expected.get(&key(number))),
                _ => assert_eq!(
                    pending.insert(key(number), operation),
                    expected.insert(key(number), operation)
                ),
            }
            if operation % 41 == 0 {
                check_model(&pending, &expected);
            }
        }
        check_model(&pending, &expected);
        assert!(matches!(pending.storage, Storage::Tree(_)));
    }

    #[test]
    fn bounds_handle_full_key_range_without_overflow() {
        let mut pending = Pending::new();
        pending.insert([0; 8], 1);
        pending.insert([255; 8], 2);
        assert_eq!(
            pending.first_from(Bound::Included([0; 8])),
            Some((&[0; 8], &1))
        );
        assert_eq!(
            pending.first_from(Bound::Excluded([0; 8])),
            Some((&[255; 8], &2))
        );
        assert_eq!(
            pending.first_from(Bound::Included([255; 8])),
            Some((&[255; 8], &2))
        );
        assert_eq!(pending.first_from(Bound::Excluded([255; 8])), None);
        pending.insert(key(1), 3);
        assert!(matches!(pending.storage, Storage::Tree(_)));
        assert_eq!(pending.first_from(Bound::Excluded([255; 8])), None);
    }

    #[test]
    fn inline_and_large_values_preserve_bytes_and_move_without_cloning() {
        let mut pending = Pending::new();
        for length in [0, 1, 63, 64, 65, 1024] {
            let bytes: Vec<u8> = (0..length).map(|i| (i * 11) as u8).collect();
            let value = Value::from_slice(&bytes);
            assert_eq!(value.len(), length);
            assert_eq!(value.as_slice(), bytes);
            assert_eq!(matches!(value, Value::Inline { .. }), length <= 64);
            assert!(pending.insert(key(length as u64), value).is_none());
        }
        let previous = pending
            .insert(key(64), Value::from_slice(b"replacement"))
            .unwrap();
        assert_eq!(previous.len(), 64);
        let removed = pending.remove(&key(1024)).unwrap();
        assert_eq!(removed.len(), 1024);
        let borrowed: Vec<&[u8]> = (&pending)
            .into_iter()
            .map(|(_, value)| value.as_slice())
            .collect();
        assert_eq!(borrowed.len(), 5);
        assert_eq!(pending.get(&key(64)).unwrap().as_slice(), b"replacement");
    }
}
