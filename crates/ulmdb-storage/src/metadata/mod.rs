//! Persistent checked Patricia directory for format 0.4 segment metadata.
mod codec;
mod inventory;
mod origin_cursor;
pub use origin_cursor::MetadataOriginCursor;
#[cfg(test)]
mod inventory_tests;
pub use inventory::Inventory;
pub(crate) mod resolver;
#[cfg(test)]
mod tests;
mod tree;
use crate::{Error, Manifest, Result, SegmentEntry};
pub(crate) use resolver::Resolver;
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, OnceLock},
};
use tree::{Kind, Node};
pub const MAX_DESCRIPTOR_BYTES: u64 = 268_435_456;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct NodeRef {
    pub origin: u128,
    pub offset: u64,
    pub length: u32,
    pub hash: [u8; 32],
}
#[derive(Clone, Debug)]
pub(crate) struct Index {
    pub generation: u128,
    root: Option<Arc<Node>>,
    pub blob: Arc<[u8]>,
    pub local_nodes: u32,
    pub origins: OnceLock<BTreeSet<u128>>,
    pub inventory_stamp: u64,
    pub certified_table: Option<u64>,
    pub own_layout: Option<Option<SegmentEntry>>,
}
pub(crate) fn validate_entries(entries: &[SegmentEntry]) -> Result<()> {
    if entries.len() >= 1_048_576 {
        return Err(Error::LimitExceeded);
    }
    let mut prior = 0;
    let first = entries.first().map_or(0, |e| e.id >> 8);
    for e in entries {
        if e.id <= prior
            || e.id == u64::MAX
            || (e.id >> 8) - first >= crate::table::MAX_DIRECTORY_CHUNKS
            || !(4096..=(ulmdb_core::ValueLimit::MAXIMUM.segment_limit())).contains(&e.length)
        {
            return Err(Error::Corrupt("invalid indexed segment inventory"));
        }
        prior = e.id;
    }
    Ok(())
}
fn origins(root: &Option<Arc<Node>>) -> BTreeSet<u128> {
    fn walk(node: &Node, out: &mut BTreeSet<u128>) {
        out.insert(node.location.unwrap().origin);
        if let Kind::Branch(left, right) = &node.kind {
            walk(left, out);
            walk(right, out);
        }
    }
    let mut out = BTreeSet::new();
    if let Some(root) = root {
        walk(root, &mut out);
    }
    out
}
pub(crate) fn prepare(manifest: &Manifest, base: Option<&Manifest>) -> Result<Index> {
    validate_entries(&manifest.segments)?;
    if manifest.generation == 0 {
        return Err(Error::InvalidInput("zero indexed generation"));
    }
    let root = if let Some(base) = base.filter(|b| b.format_minor == 4) {
        let old = base
            .index
            .as_ref()
            .ok_or(Error::InvalidInput("base indexed manifest is unprepared"))?;
        if old.generation != base.generation || !tree::matches(&old.root, &base.segments) {
            return Err(Error::InvalidInput("base indexed inventory was mutated"));
        }
        if manifest.generation == base.generation {
            return Err(Error::InvalidInput("indexed generation must be fresh"));
        }
        let mut root = old.root.clone();
        let mut a = 0;
        let mut b = 0;
        while a < base.segments.len() || b < manifest.segments.len() {
            let old = base.segments.get(a);
            let new = manifest.segments.get(b);
            match (old, new) {
                (Some(old), Some(new)) if old.id == new.id => {
                    if !tree::equal(old, new) {
                        root = tree::insert(root, new.clone());
                    }
                    a += 1;
                    b += 1;
                }
                (Some(old), Some(new)) if old.id < new.id => {
                    root = tree::delete(root, old.id);
                    a += 1;
                }
                (Some(old), None) => {
                    root = tree::delete(root, old.id);
                    a += 1;
                }
                (_, Some(new)) => {
                    root = tree::insert(root, new.clone());
                    b += 1;
                }
                (None, None) => break,
            }
        }
        root
    } else {
        tree::build(&manifest.segments)
    };
    let mut blob = Vec::new();
    let mut count = 0;
    let root = root
        .map(|n| codec::serialize(n, manifest.generation, true, &mut blob, &mut count))
        .transpose()?;
    Ok(Index {
        generation: manifest.generation,
        root,
        blob: blob.into(),
        local_nodes: count,
        origins: OnceLock::new(),
        inventory_stamp: 0,
        certified_table: None,
        own_layout: None,
    })
}
impl Index {
    pub(crate) fn adopt_inventory(&mut self, old: Inventory) -> Result<Inventory> {
        let inventory = Inventory::canonical(self.root.clone(), old.into_cache())?;
        self.inventory_stamp = inventory.stamp();
        self.certified_table = None;
        Ok(inventory)
    }
    pub(crate) fn canonical_matches(&self, manifest: &Manifest) -> bool {
        self.inventory_stamp != 0
            && self.inventory_stamp == manifest.segments.stamp()
            && self.generation == manifest.generation
    }
    pub(crate) fn append_eligible(&self, manifest: &Manifest, table: u64) -> bool {
        ulmdb_core::inventory_stamp::append_eligible(
            manifest.segments.stamp(),
            self.inventory_stamp,
            self.generation,
            manifest.generation,
            self.certified_table.unwrap_or(0),
            table,
        )
    }
    pub fn origins(&self) -> &BTreeSet<u128> {
        self.origins.get_or_init(|| origins(&self.root))
    }

    pub fn root_bytes(&self, manifest: &Manifest) -> Result<[u8; 64]> {
        if self.generation != manifest.generation
            || (!self.canonical_matches(manifest) && !tree::matches(&self.root, &manifest.segments))
        {
            return Err(Error::InvalidInput(
                "indexed manifest context does not match inventory",
            ));
        }
        Ok(self
            .root
            .as_ref()
            .map_or([0; 64], |n| n.location.unwrap().encode()))
    }
}
pub(crate) fn decode(
    bytes: &[u8],
    generation: u128,
    count: usize,
    local: u32,
    root: &[u8],
    resolver: Option<&mut Resolver>,
) -> Result<(Index, Vec<SegmentEntry>)> {
    codec::decode(bytes, generation, count, local, root, resolver)
}
pub(crate) fn decode_manifest(
    bytes: &[u8],
    directory: &Path,
    fallback: Option<&Path>,
) -> Result<Manifest> {
    decode_manifest_with_mode(
        bytes,
        directory,
        fallback,
        crate::read_mode::ReadMode::Ordinary,
    )
}
pub(crate) fn decode_manifest_with_mode(
    bytes: &[u8],
    directory: &Path,
    fallback: Option<&Path>,
    mode: crate::read_mode::ReadMode,
) -> Result<Manifest> {
    let mut resolver = Resolver::new_with_mode(directory, fallback, mode);
    Manifest::decode_resolved(bytes, Some(&mut resolver))
}

pub(crate) fn same_entry(a: &SegmentEntry, b: &SegmentEntry) -> bool {
    tree::equal(a, b)
}

/// The existing format, prepared from a certified immutable base plus one new
/// logical segment. A failed eligibility check takes the existing public path.
fn prepare_append_kind(
    manifest: &mut Manifest,
    base: &Manifest,
    table_identity: u64,
    entry: Option<&SegmentEntry>,
    cohort: u128,
    file_layout: bool,
) -> Result<bool> {
    let Some(old) = base.index.as_ref() else {
        return Ok(false);
    };
    if base.format_minor != 4 || !old.append_eligible(base, table_identity) {
        return Ok(false);
    }
    if manifest.generation == 0 || manifest.generation == base.generation {
        return Err(Error::InvalidInput("indexed generation must be fresh"));
    }
    if let Some(entry) = entry {
        if entry.id == 0
            || entry.id == u64::MAX
            || base.segments.last().is_some_and(|last| last.id >= entry.id)
            || base.segments.len() >= 1_048_575
            || !(4096..=(ulmdb_core::ValueLimit::MAXIMUM.segment_limit())).contains(&entry.length)
            || entry.cohort != cohort
        {
            return Ok(false);
        }
        let first = base.segments.first().map_or(entry.id >> 8, |e| e.id >> 8);
        if (entry.id >> 8) - first >= crate::table::MAX_DIRECTORY_CHUNKS {
            return Err(Error::LimitExceeded);
        }
        manifest.next_segment = manifest
            .next_segment
            .max(entry.id.checked_add(1).ok_or(Error::LimitExceeded)?);
    }
    let root = match entry {
        Some(entry) => tree::insert(old.root.clone(), entry.clone()),
        None => old.root.clone(),
    };
    let mut blob = Vec::new();
    let mut local_nodes = 0;
    let root = root
        .map(|n| codec::serialize(n, manifest.generation, true, &mut blob, &mut local_nodes))
        .transpose()?;
    let inventory = Inventory::canonical(root.clone(), None)?;
    let inventory_stamp = inventory.stamp();
    manifest.segments = inventory;
    manifest.index = Some(Index {
        generation: manifest.generation,
        root,
        blob: blob.into(),
        local_nodes,
        origins: OnceLock::new(),
        inventory_stamp,
        certified_table: None,
        own_layout: if file_layout {
            Some(entry.cloned())
        } else {
            None
        },
    });
    manifest.format_minor = 4;
    Ok(true)
}

pub(crate) fn prepare_append(
    manifest: &mut Manifest,
    base: &Manifest,
    table: u64,
    entry: Option<&SegmentEntry>,
) -> Result<bool> {
    let cohort = manifest.generation;
    prepare_append_kind(manifest, base, table, entry, cohort, true)
}
pub(crate) fn prepare_range_append(
    manifest: &mut Manifest,
    base: &Manifest,
    table: u64,
    entry: Option<&SegmentEntry>,
) -> Result<bool> {
    prepare_append_kind(manifest, base, table, entry, 0, false)
}
