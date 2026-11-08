//! Small catalogs embed a direct map (at most 64 names). Large catalogs share
//! immutable names and stable, chunked database slots. Transactions keep only
//! changed names; large root publication detaches the fixed top and touched
//! chunks, without copying names. Topology changes may detach the name map,
//! search bounded slots, or cross the representation threshold. Persisted
//! descriptors are unchanged; this is private materialized catalog metadata.
use crate::{Error, Result, catalog::Database};
use std::{collections::BTreeMap, sync::Arc};

const BLOCK: usize = 64;
const SLOTS: usize = crate::CatalogCapacity::EXTENDED.get();

#[derive(Clone, Copy)]
struct Handle {
    slot: u16,
    id: u64,
}
type Chunk = [Option<Database>; BLOCK];
#[derive(Clone)]
struct Directory {
    chunks: Vec<Option<Arc<Chunk>>>,
}
impl Default for Directory {
    fn default() -> Self {
        Self {
            chunks: vec![None; crate::CatalogCapacity::DEFAULT.get() / BLOCK],
        }
    }
}

#[derive(Clone, Default)]
struct Large {
    names: Arc<BTreeMap<Vec<u8>, Handle>>,
    directory: Arc<Directory>,
    #[cfg(test)]
    work: Work,
}
impl Large {
    pub fn len(&self) -> usize {
        self.names.len()
    }
    pub fn get(&self, name: &[u8]) -> Option<&Database> {
        self.names.get(name).map(|h| self.resolve(*h))
    }
    fn resolve(&self, h: Handle) -> &Database {
        let db = self.directory.chunks[usize::from(h.slot) / BLOCK]
            .as_ref()
            .expect("resident catalog chunk invariant")[usize::from(h.slot) % BLOCK]
            .as_ref()
            .expect("resident catalog slot invariant");
        assert_eq!(db.id, h.id, "resident catalog identity invariant");
        db
    }
    fn iter(&self) -> LargeIter<'_> {
        LargeIter {
            names: self.names.iter(),
            catalog: self,
        }
    }
    pub fn from_map(map: BTreeMap<Vec<u8>, Database>) -> Result<Self> {
        if map.len() > SLOTS {
            return Err(Error::LimitExceeded);
        }
        let mut names = BTreeMap::new();
        let mut next = Self::default();
        for (slot, (name, db)) in map.into_iter().enumerate() {
            names.insert(
                name,
                Handle {
                    slot: slot as u16,
                    id: db.id,
                },
            );
            next.set(slot, Some(db));
        }
        next.names = Arc::new(names);
        Ok(next)
    }
    fn set(&mut self, slot: usize, db: Option<Database>) {
        #[cfg(test)]
        if Arc::get_mut(&mut self.directory).is_none() {
            self.work.directory_detaches += 1;
        }
        let directory = Arc::make_mut(&mut self.directory);
        if directory.chunks.len() <= slot / BLOCK {
            directory.chunks.resize_with(
                (slot / BLOCK + 1).next_power_of_two().min(SLOTS / BLOCK),
                || None,
            );
        }
        let chunk = directory.chunks[slot / BLOCK]
            .get_or_insert_with(|| Arc::new(std::array::from_fn(|_| None)));
        #[cfg(test)]
        if Arc::get_mut(chunk).is_none() {
            self.work.chunk_detaches += 1;
        }
        Arc::make_mut(chunk)[slot % BLOCK] = db;
        #[cfg(test)]
        {
            self.work.slot_writes += 1;
        }
    }
    fn names_mut(&mut self) -> &mut BTreeMap<Vec<u8>, Handle> {
        #[cfg(test)]
        if Arc::get_mut(&mut self.names).is_none() {
            self.work.names_detaches += 1;
        }
        Arc::make_mut(&mut self.names)
    }
    fn free_slot(&self) -> Result<usize> {
        for (i, chunk) in self.directory.chunks.iter().enumerate() {
            match chunk {
                None => return Ok(i * BLOCK),
                Some(c) => {
                    if let Some(j) = c.iter().position(Option::is_none) {
                        return Ok(i * BLOCK + j);
                    }
                }
            }
        }
        if self.directory.chunks.len() < SLOTS / BLOCK {
            return Ok(self.directory.chunks.len() * BLOCK);
        }
        Err(Error::LimitExceeded)
    }
    fn prepare(&self, changes: &BTreeMap<Vec<u8>, Option<Database>>) -> Result<Self> {
        let mut next = self.clone();
        #[cfg(test)]
        {
            next.work = Work::default();
        }
        // Remove first: a full catalog can drop and create in either name order.
        for (name, after) in changes {
            if let Some(old) = next.names.get(name).copied() {
                if after
                    .as_ref()
                    .is_some_and(|db| db.id == old.id && db.kind != next.get(name).unwrap().kind)
                {
                    return Err(Error::Corrupt(
                        "resident catalog kind changed without identity",
                    ));
                }
                if after.as_ref().is_none_or(|db| db.id != old.id) {
                    next.set(usize::from(old.slot), None);
                    next.names_mut().remove(name);
                }
            }
        }
        for (name, after) in changes {
            if let Some(db) = after {
                if db.id == 0 {
                    return Err(Error::Corrupt("resident catalog zero identity"));
                }
                let slot = if let Some(h) = next.names.get(name) {
                    usize::from(h.slot)
                } else {
                    let slot = next.free_slot()?;
                    next.names_mut().insert(
                        name.clone(),
                        Handle {
                            slot: slot as u16,
                            id: db.id,
                        },
                    );
                    slot
                };
                next.set(slot, Some(db.clone()));
            }
        }
        Ok(next)
    }
}
struct LargeIter<'a> {
    names: std::collections::btree_map::Iter<'a, Vec<u8>, Handle>,
    catalog: &'a Large,
}
impl<'a> Iterator for LargeIter<'a> {
    type Item = (&'a Vec<u8>, &'a Database);
    fn next(&mut self) -> Option<Self::Item> {
        self.names
            .next()
            .map(|(name, h)| (name, self.catalog.resolve(*h)))
    }
}
impl<'a> IntoIterator for &'a Large {
    type Item = (&'a Vec<u8>, &'a Database);
    type IntoIter = LargeIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Small catalogs preserve direct map binding. Their clone is explicitly
/// bounded to 64 entries; large catalogs share names and detach touched chunks.
#[derive(Clone)]
pub(super) struct Catalog {
    repr: Representation,
}
#[derive(Clone)]
enum Representation {
    Small {
        map: BTreeMap<Vec<u8>, Database>,
        #[cfg(test)]
        work: Work,
    },
    Large(Large),
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            repr: Representation::Small {
                map: BTreeMap::new(),
                #[cfg(test)]
                work: Work::default(),
            },
        }
    }
}
impl Catalog {
    pub fn len(&self) -> usize {
        match &self.repr {
            Representation::Small { map, .. } => map.len(),
            Representation::Large(c) => c.len(),
        }
    }
    pub fn get(&self, name: &[u8]) -> Option<&Database> {
        match &self.repr {
            Representation::Small { map, .. } => map.get(name),
            Representation::Large(c) => c.get(name),
        }
    }
    pub fn iter(&self) -> Iter<'_> {
        Iter(match &self.repr {
            Representation::Small { map, .. } => IterInner::Small(map.iter()),
            Representation::Large(c) => IterInner::Large(c.iter()),
        })
    }
    pub fn from_map(map: BTreeMap<Vec<u8>, Database>) -> Result<Self> {
        Self::from_map_with_capacity(map, crate::CatalogCapacity::DEFAULT)
    }
    pub fn from_map_with_capacity(
        map: BTreeMap<Vec<u8>, Database>,
        capacity: crate::CatalogCapacity,
    ) -> Result<Self> {
        if map.len() > capacity.get() {
            return Err(Error::LimitExceeded);
        }
        if map.len() > SLOTS {
            return Err(Error::LimitExceeded);
        }
        let mut ids = std::collections::BTreeSet::new();
        if map.values().any(|db| db.id == 0 || !ids.insert(db.id)) {
            return Err(Error::Corrupt("resident catalog identity alias"));
        }
        if map.len() <= BLOCK {
            Ok(Self {
                repr: Representation::Small {
                    map,
                    #[cfg(test)]
                    work: Work::default(),
                },
            })
        } else {
            Ok(Self {
                repr: Representation::Large(Large::from_map(map)?),
            })
        }
    }
    fn prepare(&self, changes: &BTreeMap<Vec<u8>, Option<Database>>) -> Result<Self> {
        if changes.is_empty() {
            return Ok(self.clone());
        }
        match &self.repr {
            Representation::Small { map, .. } => {
                let mut next = map.clone();
                #[cfg(test)]
                let work = Work {
                    small_entries_copied_bound: map.len(),
                    small_map_clones: 1,
                    ..Work::default()
                };
                let mutable = &mut next;
                for (name, after) in changes {
                    if let Some(db) = after {
                        if db.id == 0
                            || mutable
                                .get(name)
                                .is_some_and(|old| old.id == db.id && old.kind != db.kind)
                        {
                            return Err(Error::Corrupt("resident catalog small identity/kind"));
                        }
                        mutable.insert(name.clone(), db.clone());
                    } else {
                        mutable.remove(name);
                    }
                }
                if mutable.len() > BLOCK {
                    // Crossing the threshold requires a topology change. At most
                    // 64 old names are cloned; changed names are already bounded.
                    Self::from_map_with_capacity(next, crate::CatalogCapacity::EXTENDED)
                } else {
                    Ok(Self {
                        repr: Representation::Small {
                            map: next,
                            #[cfg(test)]
                            work,
                        },
                    })
                }
            }
            Representation::Large(c) => {
                let next = c.prepare(changes)?;
                if next.len() <= BLOCK {
                    // Demotion is topology work; ordinary updates never iterate.
                    Self::from_map(
                        next.iter()
                            .map(|(name, db)| (name.clone(), db.clone()))
                            .collect(),
                    )
                } else {
                    Ok(Self {
                        repr: Representation::Large(next),
                    })
                }
            }
        }
    }
    #[cfg(test)]
    fn work(&self) -> Work {
        match &self.repr {
            Representation::Small { work, .. } => *work,
            Representation::Large(c) => c.work,
        }
    }
}
pub(super) struct Iter<'a>(IterInner<'a>);
enum IterInner<'a> {
    Small(std::collections::btree_map::Iter<'a, Vec<u8>, Database>),
    Large(LargeIter<'a>),
}
impl<'a> Iterator for Iter<'a> {
    type Item = (&'a Vec<u8>, &'a Database);
    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            IterInner::Small(i) => i.next(),
            IterInner::Large(i) => i.next(),
        }
    }
}
impl<'a> IntoIterator for &'a Catalog {
    type Item = (&'a Vec<u8>, &'a Database);
    type IntoIter = Iter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Overlay only: the WriteTxn already pins its source Snapshot. No catalog,
/// page or reader owner is cloned at begin. Dropping the edit cannot change it.
#[derive(Clone)]
pub(super) struct Edit {
    changes: BTreeMap<Vec<u8>, Option<Database>>,
    len: usize,
}
impl Edit {
    pub fn new(base: &Catalog) -> Self {
        Self {
            changes: BTreeMap::new(),
            len: base.len(),
        }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn get<'a>(&'a self, base: &'a Catalog, name: &[u8]) -> Option<&'a Database> {
        match self.changes.get(name) {
            Some(db) => db.as_ref(),
            None => base.get(name),
        }
    }
    pub fn contains_key(&self, base: &Catalog, name: &[u8]) -> bool {
        self.get(base, name).is_some()
    }
    pub fn insert(&mut self, base: &Catalog, name: Vec<u8>, db: Database) {
        if self.get(base, &name).is_none() {
            self.len += 1;
        }
        self.changes.insert(name, Some(db));
    }
    pub fn remove(&mut self, base: &Catalog, name: &[u8]) -> Option<Database> {
        let old = self.get(base, name)?.clone();
        self.len -= 1;
        match self.changes.get_mut(name) {
            Some(entry) => *entry = None,
            None => {
                self.changes.insert(name.to_vec(), None);
            }
        }
        Some(old)
    }
    pub fn set_root(&mut self, base: &Catalog, name: &[u8], root: ulmdb_storage::Reference) {
        if let Some(entry) = self.changes.get_mut(name) {
            entry.as_mut().unwrap().root = root;
        } else {
            let mut db = base.get(name).unwrap().clone();
            db.root = root;
            self.changes.insert(name.to_vec(), Some(db));
        }
    }
    pub fn changed(&self) -> impl Iterator<Item = &Vec<u8>> {
        self.changes.keys()
    }
    pub fn is_unchanged(&self) -> bool {
        self.changes.is_empty()
    }
    pub fn prepare(&self, base: &Catalog) -> Result<Catalog> {
        self.prepare_with_capacity(base, crate::CatalogCapacity::DEFAULT)
    }
    pub fn prepare_with_capacity(
        &self,
        base: &Catalog,
        capacity: crate::CatalogCapacity,
    ) -> Result<Catalog> {
        if self.len > capacity.get() {
            return Err(Error::LimitExceeded);
        }
        base.prepare(&self.changes)
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Default, Debug, Eq, PartialEq)]
struct Work {
    small_entries_copied_bound: usize,
    small_map_clones: usize,
    directory_detaches: usize,
    chunk_detaches: usize,
    names_detaches: usize,
    slot_writes: usize,
}
#[cfg(test)]
mod tests;
