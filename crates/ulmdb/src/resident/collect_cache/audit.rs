//! The catalog and KV remain full audited. Prefix reuse substitutes only a
//! previously complete typed closure whose current memberships all revalidate.
use super::*;
use crate::{Address, Prefix};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    ops::Bound,
};
use ulmdb_core::storage::ReadStore;
struct Trace<'a> {
    view: &'a View,
    budget: u64,
    reads: Cell<u64>,
    live: RefCell<BTreeSet<VersionKey>>,
    capture: RefCell<Option<Capture>>,
}
impl ReadStore for Trace<'_> {
    fn max_key_size(&self) -> usize {
        65536
    }
    fn max_duplicate_key_size(&self) -> usize {
        4080
    }
    fn max_duplicate_value_size(&self) -> usize {
        65536
    }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        let count = self.reads.get();
        if count >= self.budget {
            return Err(Error::LimitExceeded);
        }
        let (bytes, key, sources) =
            self.view
                .read_with_sources_limited(at, length, (self.budget - count - 1) as usize)?;
        self.reads.set(count + 1 + sources.len() as u64);
        self.live
            .borrow_mut()
            .extend(sources.iter().map(|s| s.version));
        self.live.borrow_mut().insert(key);
        let mut capture = self.capture.borrow_mut();
        if capture.as_mut().is_some_and(|c| {
            !c.record(at, length, key)
                || sources
                    .iter()
                    .any(|s| !c.record(s.reference, s.length, s.version))
        }) {
            *capture = None;
        }
        Ok(bytes)
    }
}
fn add(counter: &mut u64, n: u64) -> Result<()> {
    *counter = counter.checked_add(n).ok_or(Error::LimitExceeded)?;
    Ok(())
}
fn prefix(store: &Trace<'_>, db: &Database, count: &mut u64, limit: usize) -> Result<u64> {
    let address = if db.kind == DatabaseKind::Ipv4 {
        Address::V4(0)
    } else {
        Address::V6(0)
    };
    let query = Prefix::new(address, 0).map_err(|_| Error::Corrupt("resident family root"))?;
    let reader = ulmdb_prefix::Reader::new(store, db.root)?;
    if reader
        .root_prefix()
        .is_some_and(|p| !query.contains_prefix(p))
    {
        return Err(Error::Corrupt("resident prefix descriptor family mismatch"));
    }
    let before = *count;
    for row in ulmdb_prefix::subnets(store, db.root, query)? {
        let (p, value) = row?;
        if !query.contains_prefix(p) || value.len() > limit {
            return Err(Error::Corrupt("resident prefix family/value limit"));
        }
        add(count, 1)?;
    }
    Ok(*count - before)
}
pub(super) fn run(
    cache: &Cache,
    pool: &Arc<Pool>,
    view: &View,
    root: Reference,
    next_database: u64,
    budget: u64,
) -> Result<(super::super::audit::Audit, Cache)> {
    if next_database == 0 {
        return Err(Error::Corrupt("resident next database identity"));
    }
    let mut next = Cache::new(pool, cache.limits)?;
    let store = Trace {
        view,
        budget,
        reads: Cell::new(0),
        live: RefCell::new(BTreeSet::new()),
        capture: RefCell::new(None),
    };
    let mut kv_pages = ulmdb_kv::audit_tree(&store, root, budget)?.pages;
    let capacity = crate::catalog_profile::read(&store, root)?;
    let mut catalog = BTreeMap::new();
    let mut identities = BTreeSet::new();
    let mut value_limits = BTreeMap::new();
    for row in ulmdb_kv::Cursor::new(&store, root, Bound::Unbounded, Bound::Unbounded, false)? {
        let (name, bytes) = row?;
        if name.is_empty() {
            crate::CatalogCapacity::decode_marker(bytes)?;
            continue;
        }
        if name.len() > 255 || catalog.len() >= capacity.get() {
            return Err(Error::Corrupt("resident catalog name/count limit"));
        }
        let db = catalog_codec::decode(bytes)?;
        value_limits.insert(db.id, catalog_codec::value_limit(bytes)?);
        if db.id >= next_database || !identities.insert(db.id) {
            return Err(Error::Corrupt("resident catalog identity range/alias"));
        }
        if catalog.insert(name.to_vec(), db).is_some() {
            return Err(Error::Corrupt("resident catalog duplicate name"));
        }
    }
    let mut prefix_records = 0;
    for (name, db) in &catalog {
        let limit = value_limits[&db.id];
        if db.ordering != Default::default() {
            add(
                &mut kv_pages,
                super::super::audit::ordered_database(&store, db, budget, limit)?,
            )?;
            continue;
        }
        match db.kind {
            DatabaseKind::Kv => {
                #[cfg(not(feature = "resident-single-pass-audit"))]
                {
                    add(
                        &mut kv_pages,
                        ulmdb_kv::audit_tree(&store, db.root, budget)?.pages,
                    )?;
                    super::super::audit::bounded_values_with_limit(&store, db.root, limit)?;
                }
                #[cfg(feature = "resident-single-pass-audit")]
                {
                    super::super::audit::single_pass_kv_with_limit(
                        &store,
                        db.root,
                        budget,
                        &mut kv_pages,
                        limit,
                    )?;
                }
            }
            DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => {
                if let Some(entry) = cache
                    .find(pool, name, db)
                    .filter(|e| cache.usable(view, e, &mut next.stats))
                {
                    add(&mut prefix_records, entry.records)?;
                    store.live.borrow_mut().extend(entry.keys.iter().copied());
                    next.stats.hits += 1;
                    if next.admit(Arc::clone(entry)) {
                        next.stats.admitted += 1;
                    } else {
                        next.stats.admission_skips += 1;
                    }
                    continue;
                }
                next.stats.misses += 1;
                *store.capture.borrow_mut() = if next.entries.len() < next.limits.max_entries {
                    Capture::new(view.slot_capacity(), cache.limits.phase())
                } else {
                    None
                };
                let records = prefix(&store, db, &mut prefix_records, limit)?;
                let captured = store.capture.borrow_mut().take();
                let available = next.limits.phase().saturating_sub(next.charged);
                if let Some(entry) = captured.and_then(|c| c.finish(name, db, records, available)) {
                    if next.admit(entry) {
                        next.stats.admitted += 1;
                    } else {
                        next.stats.admission_skips += 1;
                    }
                } else {
                    next.stats.admission_skips += 1;
                }
            }
            DatabaseKind::Duplicates { width } => {
                add(
                    &mut kv_pages,
                    super::super::audit::audit_duplicates(&store, db.root, width, budget, limit)?,
                )?;
            }
        }
    }
    next.entries.sort_unstable_by_key(|e| e.id);
    next.stats.entries = next.entries.len();
    next.stats.retained_bytes = next.charged;
    Ok((
        super::super::audit::Audit {
            catalog,
            catalog_capacity: capacity,
            live: store.live.into_inner(),
            reads: store.reads.get(),
            kv_pages,
            prefix_records,
        },
        next,
    ))
}
