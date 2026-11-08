//! Deterministic development workload facade. Generation, encoding and file
//! publication are separate so the oracle stays independent of storage details.

use crate::PrefixStore;
use std::collections::BTreeMap;
use ulmdb_core::Address;

mod encoding;
mod generate;
mod output;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrefixProfile {
    Gapped,
    WithDefaults,
}

impl PrefixProfile {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Gapped => "gapped",
            Self::WithDefaults => "with-defaults",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub seed: u64,
    pub prefixes: usize,
    pub kv: usize,
    pub queries: usize,
    pub prefix_profile: PrefixProfile,
}

pub struct Workload {
    pub config: Config,
    pub prefixes: PrefixStore,
    pub kv: BTreeMap<Vec<u8>, Vec<u8>>,
    pub queries: Vec<Address>,
}

#[cfg(test)]
mod tests;
