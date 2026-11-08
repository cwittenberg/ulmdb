//! Owned default-index state and snapshot/write oracle.

use crate::PrefixStore;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Bound,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct State {
    pub kv: BTreeMap<Vec<u8>, Vec<u8>>,
    pub duplicates: BTreeSet<(Vec<u8>, Vec<u8>)>,
    pub prefixes: PrefixStore,
}

impl State {
    pub fn range(
        &self,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Vec<(&[u8], &[u8])> {
        // Filtering rather than BTreeMap::range also defines contradictory bounds as empty.
        let mut result: Vec<_> = self
            .kv
            .iter()
            .filter(|(k, _)| {
                let k = k.as_slice();
                (match lower {
                    Bound::Included(x) => k >= x,
                    Bound::Excluded(x) => k > x,
                    Bound::Unbounded => true,
                }) && (match upper {
                    Bound::Included(x) => k <= x,
                    Bound::Excluded(x) => k < x,
                    Bound::Unbounded => true,
                })
            })
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect();
        if reverse {
            result.reverse();
        }
        result
    }
    pub fn duplicate_values(&self, key: &[u8]) -> Vec<&[u8]> {
        self.duplicates
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.as_slice())
            .collect()
    }
    pub fn delete_duplicate_key(&mut self, key: &[u8]) {
        self.duplicates.retain(|(k, _)| k != key);
    }
}

/// An owned snapshot/write oracle; private state is discarded on abort/drop.
#[derive(Default)]
pub struct Environment {
    current: State,
    sequence: u64,
}

impl Environment {
    pub fn snapshot(&self) -> (u64, State) {
        (self.sequence, self.current.clone())
    }
    pub fn begin_write(&mut self) -> Write<'_> {
        let private = self.current.clone();
        Write { env: self, private }
    }
}

pub struct Write<'a> {
    env: &'a mut Environment,
    pub private: State,
}
impl Write<'_> {
    pub fn commit(self) -> u64 {
        self.env.current = self.private;
        self.env.sequence += 1;
        self.env.sequence
    }
    pub fn abort(self) {}
}

#[cfg(test)]
mod tests;
