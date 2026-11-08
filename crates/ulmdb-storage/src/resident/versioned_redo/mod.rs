//! RP2 recipe-block prototype, NOT accepted by the literal Journal or an Env.
//! Future RL2 must bind this block plus all scalar/root/state fields once.
mod wire;
use super::{
    VersionKey, View,
    wal_codec::{self, Boot, LiteralSlot, PAGE, ReplayState, SlotState},
};
use crate::{
    Error, Reference, Result,
    hybrid::redo::{Patch, WalReceipt, final_patches},
};
pub use wire::{decode, encode};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Authority {
    pub boot: Boot,
    pub parent: WalReceipt,
    pub parent_commit: u128,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Body {
    Literal(Vec<u8>),
    Copy {
        source: VersionKey,
        patches: Vec<Patch>,
    },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Allocation {
    pub destination: VersionKey,
    pub previous_generation: u64,
    pub ranges: Vec<(u16, u16)>,
    pub body: Body,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Recipes {
    pub authority: Authority,
    pub allocations: Vec<Allocation>,
}
impl Authority {
    fn validate(self) -> Result<()> {
        self.boot.slots()?;
        if self.parent_commit
            != self
                .boot
                .commit
                .checked_add(self.parent.sequence as u128)
                .ok_or(Error::LimitExceeded)?
        {
            return Err(Error::Corrupt("RP2 parent business identity"));
        }
        if self.parent.lsn == 0 {
            if self.parent != self.boot.genesis()? {
                return Err(Error::Corrupt("RP2 genesis authority"));
            }
        } else if self.parent.sequence > self.parent.lsn
            || self.parent.offset < wal_codec::DATA_START
            || !self.parent.offset.is_multiple_of(8)
            || self.parent.length < (wal_codec::HEADER + wal_codec::FOOTER) as u64
            || self.parent.length > wal_codec::MAX_RECORD_BYTES as u64
            || !self.parent.length.is_multiple_of(8)
        {
            return Err(Error::Corrupt("RP2 authority geometry"));
        }
        self.parent.end()?;
        Ok(())
    }
}
impl Recipes {
    pub fn validate(&self) -> Result<()> {
        self.authority.validate()?;
        let boot = self.authority.boot;
        let count = boot.slots()?;
        if self.allocations.len() > count {
            return Err(Error::LimitExceeded);
        }
        let mut last = None;
        for a in &self.allocations {
            if a.destination.slot as usize >= count
                || last.is_some_and(|n| n >= a.destination.slot)
                || a.destination.generation == 0
                || a.destination.generation > wal_codec::MAX_GENERATION
                || a.previous_generation >= a.destination.generation
                || (a.destination.slot >= boot.kv_pages && a.previous_generation != 0)
            {
                return Err(Error::Corrupt("RP2 destination order/generation"));
            }
            wal_codec::validate_ranges(boot, a.destination.slot, &a.ranges)?;
            match &a.body {
                Body::Literal(b) if b.len() == PAGE => {}
                Body::Copy { source, patches } => {
                    if a.destination.slot >= boot.kv_pages
                        || source.slot >= boot.kv_pages
                        || source.slot == a.destination.slot
                        || source.generation == 0
                        || source.generation > wal_codec::MAX_GENERATION
                        || patches.len() > PAGE / 2
                    {
                        return Err(Error::Corrupt("RP2 source key/count"));
                    }
                    let mut end = 0;
                    for (i, p) in patches.iter().enumerate() {
                        let start = p.offset as usize;
                        let next = start
                            .checked_add(p.bytes.len())
                            .ok_or(Error::Corrupt("RP2 patch overflow"))?;
                        if p.bytes.is_empty() || next > PAGE || (i > 0 && start <= end) {
                            return Err(Error::Corrupt("RP2 nonminimal/overlapping patches"));
                        }
                        end = next;
                    }
                }
                _ => return Err(Error::Corrupt("RP2 literal width")),
            }
            last = Some(a.destination.slot);
        }
        Ok(())
    }
}
/// Private trusted issuance seam: an Env must bind its selected Journal state to
/// the exact immutable physical View after checked materialization. Merely equal
/// raw keys in an unrelated Pool are NOT authority. This prototype has no public
/// issuer and cannot activate a snapshot or logical retirement.
#[allow(dead_code)]
pub(crate) struct Sources<'a> {
    view: &'a View,
    state: &'a ReplayState,
    authority: Authority,
}
impl<'a> Sources<'a> {
    #[allow(dead_code)]
    pub(crate) fn from_selected_materialization(
        view: &'a View,
        state: &'a ReplayState,
    ) -> Result<Self> {
        if view.pool.geometry()
            != (
                state.boot().kv_pages as usize,
                state.boot().tiny_pages as usize,
            )
        {
            return Err(Error::Corrupt("RP2 selected geometry"));
        }
        Ok(Self {
            view,
            state,
            authority: Authority {
                boot: state.boot(),
                parent: state.selected(),
                parent_commit: state.commit(),
            },
        })
    }
    fn page(&self, key: VersionKey) -> Result<&[u8]> {
        if key.slot >= self.authority.boot.kv_pages
            || self.state.slot(key.slot) != Some(SlotState::Live(key.generation))
        {
            return Err(Error::Corrupt(
                "RP2 source not exact committed Live version",
            ));
        }
        let offset = (key.generation << 28) | ((key.slot as u64 + 1) * PAGE as u64);
        let (bytes, actual) = self
            .view
            .read_with_key(Reference { segment: 1, offset }, PAGE)?;
        if actual != key {
            return Err(Error::Corrupt("RP2 physical source membership"));
        }
        Ok(bytes)
    }
    /// Portable32-byte blockskip diff; fallback is a full literal if not smaller.
    #[allow(dead_code)]
    pub(crate) fn choose(
        &self,
        after: &LiteralSlot,
        source: Option<VersionKey>,
    ) -> Result<Allocation> {
        if after.bytes.len() != PAGE {
            return Err(Error::Corrupt("RP2 afterimage width"));
        }
        let body = if let Some(source) = source {
            let patches = final_patches(self.page(source)?, &after.bytes)?;
            let size = 16
                + patches
                    .iter()
                    .map(|p| 8 + p.bytes.len() + (8 - p.bytes.len() % 8) % 8)
                    .sum::<usize>();
            if size < PAGE {
                Body::Copy { source, patches }
            } else {
                Body::Literal(after.bytes.clone())
            }
        } else {
            Body::Literal(after.bytes.clone())
        };
        let result = Allocation {
            destination: after.key,
            previous_generation: after.expected_previous_generation,
            ranges: after.ranges.clone(),
            body,
        };
        Recipes {
            authority: self.authority,
            allocations: vec![result.clone()],
        }
        .validate()?;
        Ok(result)
    }
    #[allow(dead_code)]
    pub(crate) fn materialize<'s>(&'s self, r: &Recipes) -> Result<Materialized<'s, 'a>> {
        r.validate()?;
        if r.authority != self.authority {
            return Err(Error::Corrupt("RP2 stale/foreign parent authority"));
        }
        // Resolve every source and preflight every destination before any output.
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(r.allocations.len())
            .map_err(|_| Error::LimitExceeded)?;
        for a in &r.allocations {
            let previous = match self.state.slot(a.destination.slot) {
                Some(SlotState::Free) => 0,
                Some(SlotState::Retired(g)) => g,
                _ => {
                    return Err(Error::Corrupt(
                        "RP2 destination not previously retired/free",
                    ));
                }
            };
            if previous != a.previous_generation {
                return Err(Error::Corrupt("RP2 destination ABA"));
            }
            sources.push(match &a.body {
                Body::Copy { source, .. } => Some(self.page(*source)?),
                Body::Literal(_) => None,
            });
        }
        let mut out = Vec::new();
        out.try_reserve_exact(r.allocations.len())
            .map_err(|_| Error::LimitExceeded)?;
        for (a, source) in r.allocations.iter().zip(sources) {
            let mut bytes = match (&a.body, source) {
                (Body::Literal(b), None) => b.clone(),
                (Body::Copy { .. }, Some(b)) => b.to_vec(),
                _ => unreachable!(),
            };
            if let Body::Copy { patches, .. } = &a.body {
                for p in patches {
                    let start = p.offset as usize;
                    if bytes[start..start + p.bytes.len()]
                        .iter()
                        .zip(&p.bytes)
                        .any(|(a, b)| a == b)
                    {
                        return Err(Error::Corrupt("RP2 patch includes unchanged bytes"));
                    }
                    bytes[start..start + p.bytes.len()].copy_from_slice(&p.bytes);
                }
            }
            out.push(LiteralSlot {
                key: a.destination,
                expected_previous_generation: a.previous_generation,
                ranges: a.ranges.clone(),
                bytes,
            });
        }
        Ok(Materialized {
            _sources: self,
            allocations: out,
        })
    }
}
/// The exact old selected physical View remains borrowed through materialization.
#[allow(dead_code)]
pub(crate) struct Materialized<'s, 'a> {
    _sources: &'s Sources<'a>,
    allocations: Vec<LiteralSlot>,
}
impl Materialized<'_, '_> {
    #[allow(dead_code)]
    pub(crate) fn allocations(&self) -> &[LiteralSlot] {
        &self.allocations
    }
}
#[cfg(test)]
mod tests;
