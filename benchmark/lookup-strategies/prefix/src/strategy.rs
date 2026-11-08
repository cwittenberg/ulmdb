use crate::arena::{Arena, Trace};
use std::cell::RefCell;
use ulmdb_core::{
    Prefix,
    storage::{Reference, Result},
};
use ulmdb_prefix::{self as binary, multibit};

pub enum Algorithm {
    Binary,
    Multibit,
}
impl Algorithm {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Binary => {
                if cfg!(feature = "scalar") {
                    "binary_scalar"
                } else {
                    "binary_generic"
                }
            }
            Self::Multibit => "multibit4_checked",
        }
    }
    pub fn build(&self, entries: &[(Prefix, [u8; 16])]) -> Result<(Arena, Reference)> {
        let mut arena = Arena::default();
        let root = match self {
            Self::Binary => {
                binary::build_sorted(&mut arena, entries.iter().map(|(p, v)| Ok((*p, v))))?.root
            }
            Self::Multibit => {
                let mut root = Reference::NULL;
                for (p, v) in entries {
                    root = multibit::insert(&mut arena, root, *p, v)?;
                }
                root
            }
        };
        Ok((arena, root))
    }
    pub fn reader<'a>(&self, arena: &'a Arena, root: Reference) -> Result<Reader<'a>> {
        Ok(match self {
            Self::Binary => Reader::Binary(binary::Reader::new(arena, root)?),
            Self::Multibit => Reader::Multibit(multibit::Reader::new(arena, root)?),
        })
    }
    pub fn ancestors<'a>(
        &self,
        arena: &'a Arena,
        root: Reference,
        query: Prefix,
    ) -> Result<Vec<(Prefix, &'a [u8])>> {
        match self {
            Self::Binary => binary::ancestors(arena, root, query)?.collect(),
            Self::Multibit => multibit::ancestors(arena, root, query)?.collect(),
        }
    }
    pub fn reachable(&self, arena: &Arena, root: Reference, full: Prefix) -> Result<usize> {
        let trace = Trace {
            arena,
            ranges: RefCell::new(Vec::new()),
        };
        let scan: Vec<_> = match self {
            Self::Binary => binary::subnets(&trace, root, full)?.collect::<Result<Vec<_>>>()?,
            Self::Multibit => {
                // This untimed accounting pass scans the entire fixture. The
                // public default budget intentionally bounds ordinary cursors
                // and can be smaller than a million-record full traversal.
                // Arena bytes /16 conservatively bounds total node visits,
                // including the cursor's child validation and rereads.
                let budget = multibit::Budget {
                    max_nodes: (arena.bytes.len() / 16).saturating_add(1),
                };
                multibit::Cursor::new(&trace, root, full, multibit::Selection::Subnets, budget)?
                    .collect::<Result<Vec<_>>>()?
            }
        };
        std::hint::black_box(scan);
        Ok(trace.reachable_bytes())
    }
    pub fn ancestors_checksum(&self, arena: &Arena, root: Reference, query: Prefix) -> Result<u64> {
        let mut checksum = 0u64;
        // Same allocation-free streaming consumption for both cursor strategies.
        match self {
            Self::Binary => {
                for item in binary::ancestors(arena, root, query)? {
                    let (p, v) = item?;
                    checksum = checksum.wrapping_add(fingerprint(p, v));
                }
            }
            Self::Multibit => {
                for item in multibit::ancestors(arena, root, query)? {
                    let (p, v) = item?;
                    checksum = checksum.wrapping_add(fingerprint(p, v));
                }
            }
        }
        Ok(checksum)
    }
}
pub enum Reader<'a> {
    Binary(binary::Reader<'a, Arena>),
    Multibit(multibit::Reader<'a, Arena>),
}
impl<'a> Reader<'a> {
    pub fn exact(&self, p: Prefix) -> Result<Option<(Prefix, &'a [u8])>> {
        match self {
            Self::Binary(r) => r.exact(p),
            Self::Multibit(r) => r.exact(p),
        }
    }
    pub fn lpm(&self, p: Prefix) -> Result<Option<(Prefix, &'a [u8])>> {
        match self {
            Self::Binary(r) => r.longest_match(p.network()),
            Self::Multibit(r) => r.longest_match(p.network()),
        }
    }
}
pub fn fingerprint(p: Prefix, v: &[u8]) -> u64 {
    // Includes both payload words and full matched prefix, not just presence.
    let address = match p.network() {
        ulmdb_core::Address::V4(n) => n as u64,
        ulmdb_core::Address::V6(n) => (n as u64) ^ ((n >> 64) as u64).rotate_left(17),
    };
    address
        ^ (p.length() as u64).rotate_left(7)
        ^ u64::from_le_bytes(v[..8].try_into().unwrap())
        ^ u64::from_le_bytes(v[8..16].try_into().unwrap()).rotate_left(31)
}
