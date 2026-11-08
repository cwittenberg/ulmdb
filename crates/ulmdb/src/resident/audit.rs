//! Charged complete graph tracing for cold readiness and explicit collection.
//! Ordinary writes must not hide this database-sized work in their commit path.
use super::catalog_codec::{self, MAX_KEY_BYTES, MAX_VALUE_BYTES};
use crate::catalog::Database;
use crate::{Address, DatabaseKind, Error, Prefix, Result};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    ops::Bound,
};
use ulmdb_core::storage::{ReadStore, Reference};
use ulmdb_storage::resident::{VersionKey, View};

pub(super) struct Audit {
    pub catalog: BTreeMap<Vec<u8>, Database>,
    pub catalog_capacity: crate::CatalogCapacity,
    pub live: BTreeSet<VersionKey>,
    pub reads: u64,
    pub kv_pages: u64,
    pub prefix_records: u64,
}
struct Trace<'a> {
    view: &'a View,
    budget: u64,
    reads: Cell<u64>,
    live: RefCell<BTreeSet<VersionKey>>,
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
        // Charge and retain the complete physical source closure of decoded
        // extents, as well as the descriptor owner of the borrowed mapping.
        let (bytes, key, sources) =
            self.view
                .read_with_sources_limited(at, length, (self.budget - count - 1) as usize)?;
        self.reads.set(count + 1 + sources.len() as u64);
        self.live.borrow_mut().insert(key);
        self.live
            .borrow_mut()
            .extend(sources.iter().map(|s| s.version));
        Ok(bytes)
    }
}
fn add(counter: &mut u64, value: u64) -> Result<()> {
    *counter = counter.checked_add(value).ok_or(Error::LimitExceeded)?;
    Ok(())
}
#[cfg(any(test, not(feature = "resident-single-pass-audit")))]
pub(super) fn bounded_values<S: ReadStore>(store: &S, root: Reference) -> Result<()> {
    bounded_values_with_limit(store, root, MAX_VALUE_BYTES)
}
#[cfg(any(test, not(feature = "resident-single-pass-audit")))]
pub(super) fn bounded_values_with_limit<S: ReadStore>(
    store: &S,
    root: Reference,
    limit: usize,
) -> Result<()> {
    for row in ulmdb_kv::Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, false)? {
        let (key, value) = row?;
        if key.len() > MAX_KEY_BYTES || value.len() > limit {
            return Err(Error::Corrupt(
                "resident indexed object exceeds declared limit",
            ));
        }
    }
    Ok(())
}
#[cfg(any(test, feature = "resident-single-pass-audit"))]
pub(super) fn single_pass_kv<S: ReadStore>(
    store: &S,
    root: Reference,
    budget: u64,
    kv_pages: &mut u64,
) -> Result<()> {
    single_pass_kv_with_limit(store, root, budget, kv_pages, MAX_VALUE_BYTES)
}
#[cfg(any(test, feature = "resident-single-pass-audit"))]
pub(super) fn single_pass_kv_with_limit<S: ReadStore>(
    store: &S,
    root: Reference,
    budget: u64,
    kv_pages: &mut u64,
    limit: usize,
) -> Result<()> {
    let mut too_large = false;
    let tree = ulmdb_kv::audit_tree_with_leaf_visitor(store, root, budget, |key, value| {
        too_large |= key.len() > MAX_KEY_BYTES || value.len() > limit;
    })?;
    // Preserve complete graph/overflow/budget and checked-page-counter errors
    // before reporting the additional Resident leaf size constraint.
    add(kv_pages, tree.pages)?;
    if too_large {
        return Err(Error::Corrupt(
            "resident indexed object exceeds declared limit",
        ));
    }
    Ok(())
}

/// Complete bucket and child-tree ordering/routing/count/width audit. Trace
/// charges every actual read and captures all child pages and overflow payloads.
pub(super) fn audit_duplicates<S: ReadStore>(
    store: &S,
    root: Reference,
    width: Option<u32>,
    budget: u64,
    limit: usize,
) -> Result<u64> {
    use crate::duplicates::bucket::Bucket;
    let mut pages = ulmdb_kv::audit_tree(store, root, budget)?.pages;
    for row in ulmdb_kv::Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, false)? {
        let (key, bytes) = row?;
        if key.len() > MAX_KEY_BYTES || ulmdb_kv::get(store, root, key)? != Some(bytes) {
            return Err(Error::Corrupt("resident duplicate parent size/routing"));
        }
        match Bucket::decode(bytes, width)? {
            Bucket::Inline(bucket) => {
                for i in 0..bucket.count {
                    if bucket.value(i).len() > limit {
                        return Err(Error::Corrupt("resident duplicate value limit"));
                    }
                }
            }
            Bucket::Tree {
                root,
                count: expected,
            } => {
                add(&mut pages, ulmdb_kv::audit_tree(store, root, budget)?.pages)?;
                let mut count = 0u64;
                for row in
                    ulmdb_kv::Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, false)?
                {
                    let (value, empty) = row?;
                    if value.len() > limit
                        || width.is_some_and(|w| value.len() != w as usize)
                        || !empty.is_empty()
                        || ulmdb_kv::get(store, root, value)? != Some(empty)
                    {
                        return Err(Error::Corrupt(
                            "resident duplicate child size/width/data/routing",
                        ));
                    }
                    add(&mut count, 1)?;
                }
                if count != expected {
                    return Err(Error::Corrupt("resident duplicate count mismatch"));
                }
            }
        }
    }
    Ok(pages)
}

/// Builtins receive full ordering/routing validation; custom orders receive
/// graph/extent/width/count validation without an invented callback.
pub(super) fn ordered_database<S: ReadStore>(
    store: &S,
    db: &Database,
    budget: u64,
    limit: usize,
) -> Result<u64> {
    use crate::ordering::{BoundComparator, OrderingSpec};
    use ulmdb_kv::ordering::Comparator;
    fn bind(spec: OrderingSpec) -> Result<Option<BoundComparator>> {
        if spec == OrderingSpec::Custom {
            Ok(None)
        } else {
            BoundComparator::bind(spec, None).map(Some)
        }
    }
    fn tree<S: ReadStore>(
        s: &S,
        root: Reference,
        budget: u64,
        order: Option<&BoundComparator>,
    ) -> Result<u64> {
        Ok(match order {
            Some(o) => ulmdb_kv::audit_tree_with(s, root, budget, o)?,
            None => ulmdb_kv::audit_tree_structure(s, root, budget)?,
        }
        .pages)
    }
    let keys = bind(db.ordering.keys)?;
    let values = bind(db.ordering.duplicates)?;
    let mut pages = tree(store, db.root, budget, keys.as_ref())?;
    for row in ulmdb_kv::Cursor::new(store, db.root, Bound::Unbounded, Bound::Unbounded, false)? {
        let (key, value) = row?;
        if key.len() > MAX_KEY_BYTES || (db.kind == DatabaseKind::Kv && value.len() > limit) {
            return Err(Error::Corrupt("resident ordered size limit"));
        }
        if let Some(o) = &keys {
            o.validate(key)
                .map_err(|_| Error::Corrupt("resident stored integer width"))?;
            if ulmdb_kv::get_with(store, db.root, key, o)? != Some(value) {
                return Err(Error::Corrupt("resident ordered routing"));
            }
        }
        if let DatabaseKind::Duplicates { width } = db.kind {
            match crate::duplicates::bucket::Bucket::decode_with(
                value,
                width,
                values.as_ref().map(|x| x as &dyn Comparator),
            )? {
                crate::duplicates::bucket::Bucket::Inline(b) => {
                    for i in 0..b.count {
                        if b.value(i).len() > limit {
                            return Err(Error::Corrupt("resident ordered duplicate limit"));
                        }
                    }
                }
                crate::duplicates::bucket::Bucket::Tree {
                    root,
                    count: expected,
                } => {
                    add(&mut pages, tree(store, root, budget, values.as_ref())?)?;
                    let mut count = 0u64;
                    for row in ulmdb_kv::Cursor::new(
                        store,
                        root,
                        Bound::Unbounded,
                        Bound::Unbounded,
                        false,
                    )? {
                        let (v, empty) = row?;
                        if v.len() > limit
                            || width.is_some_and(|w| v.len() != w as usize)
                            || !empty.is_empty()
                        {
                            return Err(Error::Corrupt("resident ordered duplicate width/data"));
                        }
                        if let Some(o) = &values {
                            o.validate(v).map_err(|_| {
                                Error::Corrupt("resident stored duplicate integer width")
                            })?;
                            if ulmdb_kv::get_with(store, root, v, o)? != Some(empty) {
                                return Err(Error::Corrupt("resident ordered duplicate routing"));
                            }
                        }
                        add(&mut count, 1)?;
                    }
                    if count != expected {
                        return Err(Error::Corrupt("resident ordered duplicate count"));
                    }
                }
            }
        }
    }
    Ok(pages)
}

pub(super) fn audit(
    view: &View,
    root: Reference,
    next_database: u64,
    budget: u64,
) -> Result<Audit> {
    if next_database == 0 {
        return Err(Error::Corrupt("resident next database identity"));
    }
    let store = Trace {
        view,
        budget,
        reads: Cell::new(0),
        live: RefCell::new(BTreeSet::new()),
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
        let database = catalog_codec::decode(bytes)?;
        value_limits.insert(database.id, catalog_codec::value_limit(bytes)?);
        if database.id >= next_database || !identities.insert(database.id) {
            return Err(Error::Corrupt("resident catalog identity range/alias"));
        }
        if catalog.insert(name.to_vec(), database).is_some() {
            return Err(Error::Corrupt("resident catalog duplicate name"));
        }
    }
    let mut prefix_records = 0;
    for database in catalog.values() {
        let limit = value_limits[&database.id];
        if database.ordering != Default::default() {
            add(
                &mut kv_pages,
                ordered_database(&store, database, budget, limit)?,
            )?;
            continue;
        }
        match database.kind {
            DatabaseKind::Kv => {
                #[cfg(not(feature = "resident-single-pass-audit"))]
                {
                    add(
                        &mut kv_pages,
                        ulmdb_kv::audit_tree(&store, database.root, budget)?.pages,
                    )?;
                    bounded_values_with_limit(&store, database.root, limit)?;
                }
                #[cfg(feature = "resident-single-pass-audit")]
                {
                    single_pass_kv_with_limit(&store, database.root, budget, &mut kv_pages, limit)?;
                }
            }
            kind @ (DatabaseKind::Ipv4 | DatabaseKind::Ipv6) => {
                let address = if kind == DatabaseKind::Ipv4 {
                    Address::V4(0)
                } else {
                    Address::V6(0)
                };
                let query =
                    Prefix::new(address, 0).map_err(|_| Error::Corrupt("resident family root"))?;
                let reader = ulmdb_prefix::Reader::new(&store, database.root)?;
                if reader
                    .root_prefix()
                    .is_some_and(|prefix| !query.contains_prefix(prefix))
                {
                    return Err(Error::Corrupt("resident prefix descriptor family mismatch"));
                }
                // A whole-family traversal loads every node (including valueless
                // branches), checks every child relationship and payload extent.
                for row in ulmdb_prefix::subnets(&store, database.root, query)? {
                    let (prefix, value) = row?;
                    if !query.contains_prefix(prefix) || value.len() > limit {
                        return Err(Error::Corrupt("resident prefix family/value limit"));
                    }
                    add(&mut prefix_records, 1)?;
                }
            }
            DatabaseKind::Duplicates { width } => {
                add(
                    &mut kv_pages,
                    audit_duplicates(&store, database.root, width, budget, limit)?,
                )?;
            }
        }
    }
    Ok(Audit {
        catalog,
        catalog_capacity: capacity,
        live: store.live.into_inner(),
        reads: store.reads.get(),
        kv_pages,
        prefix_records,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ulmdb_core::storage::WriteStore;
    use ulmdb_storage::resident::{Arena, Config, Pool};
    fn arena() -> Arena {
        let pool = Pool::create(Config {
            kv_pages: 64,
            tiny_pages: 64,
            max_reserved_bytes: 64 << 20,
            max_metadata_bytes: 64 << 20,
            max_views: 32,
        })
        .unwrap();
        let view = pool.empty().unwrap();
        pool.begin(view).unwrap()
    }
    fn descriptor(
        arena: &mut Arena,
        catalog: Reference,
        name: &[u8],
        id: u64,
        kind: DatabaseKind,
        root: Reference,
    ) -> Reference {
        ulmdb_kv::put(
            arena,
            catalog,
            name,
            &catalog_codec::encode(&Database {
                ordering: crate::ordering::DatabaseOrdering::default(),
                id,
                kind,
                root,
            }),
        )
        .unwrap()
    }
    #[test]
    fn traces_actual_kv_and_both_prefix_families_excludes_unreachable_version() {
        let mut arena = arena();
        let dead = arena.allocate(&[7; 4096], 4096).unwrap();
        let mut kv = Reference::NULL;
        // More than one leaf even when fixed-width compact pages are enabled.
        for key in 0..512u32 {
            kv = ulmdb_kv::put(&mut arena, kv, &key.to_be_bytes(), &key.to_le_bytes()).unwrap();
        }
        let mut catalog = descriptor(&mut arena, Reference::NULL, b"kv", 1, DatabaseKind::Kv, kv);
        for (id, kind, address, length, name) in [
            (
                2,
                DatabaseKind::Ipv4,
                Address::V4(0x0a00_0000),
                8,
                b"v4".as_slice(),
            ),
            (
                3,
                DatabaseKind::Ipv6,
                Address::V6(0x2001u128 << 112),
                16,
                b"v6".as_slice(),
            ),
        ] {
            let prefix = Prefix::new(address, length).unwrap();
            let root =
                ulmdb_prefix::insert(&mut arena, Reference::NULL, prefix, b"network").unwrap();
            catalog = descriptor(&mut arena, catalog, name, id, kind, root);
        }
        let view = arena.freeze().unwrap().activate();
        let result = audit(&view, catalog, 4, 100_000).unwrap();
        assert_eq!((result.catalog.len(), result.prefix_records), (3, 2));
        assert!(result.kv_pages >= 4);
        assert!(result.reads > result.kv_pages);
        let dead_key = view.read_with_key(dead, 4096).unwrap().1;
        assert!(!result.live.contains(&dead_key));
        assert!(view.members().count() > result.live.len());
        let minimal = view.retain(&result.live).unwrap();
        let repeated = audit(&minimal, catalog, 4, 100_000).unwrap();
        assert_eq!(repeated.live, result.live);
        assert!(matches!(
            audit(&view, catalog, 4, result.reads - 1),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(
            audit(&view, catalog, 4, result.reads).unwrap().reads,
            result.reads
        );
    }
    #[test]
    fn wrong_family_duplicate_ids_bad_names_and_small_identity_bound_reject() {
        let mut arena = arena();
        let root = ulmdb_prefix::insert(
            &mut arena,
            Reference::NULL,
            Prefix::new(Address::V6(0), 0).unwrap(),
            b"v6",
        )
        .unwrap();
        let wrong_family = descriptor(
            &mut arena,
            Reference::NULL,
            b"v4",
            1,
            DatabaseKind::Ipv4,
            root,
        );
        let mut duplicate_id = descriptor(
            &mut arena,
            Reference::NULL,
            b"a",
            1,
            DatabaseKind::Kv,
            Reference::NULL,
        );
        duplicate_id = descriptor(
            &mut arena,
            duplicate_id,
            b"b",
            1,
            DatabaseKind::Kv,
            Reference::NULL,
        );
        let bad_name = descriptor(
            &mut arena,
            Reference::NULL,
            b"",
            1,
            DatabaseKind::Kv,
            Reference::NULL,
        );
        let bound = descriptor(
            &mut arena,
            Reference::NULL,
            b"bound",
            2,
            DatabaseKind::Kv,
            Reference::NULL,
        );
        let view = arena.freeze().unwrap().activate();
        for root in [wrong_family, duplicate_id, bad_name, bound] {
            assert!(audit(&view, root, 2, 100_000).is_err());
        }
        assert!(audit(&view, Reference::NULL, 0, 0).is_err());
        assert_eq!(audit(&view, Reference::NULL, 1, 0).unwrap().reads, 0);
    }
    #[test]
    fn full_audit_rejects_unqueried_child_and_malformed_payload_header() {
        let mut arena = arena();
        let left = ulmdb_prefix::insert(
            &mut arena,
            Reference::NULL,
            Prefix::new(Address::V4(0), 32).unwrap(),
            b"left",
        )
        .unwrap();
        let root = ulmdb_prefix::insert(
            &mut arena,
            left,
            Prefix::new(Address::V4(u32::MAX), 32).unwrap(),
            b"right",
        )
        .unwrap();
        // Keep the left-hand lookup valid while the unqueried right link names
        // an unmapped/stale version. Whole-family audit must still read it.
        let stale = Reference {
            segment: 1,
            offset: (17 << 28) | 4096,
        };
        let root = arena.patch(root, 96, 48, &stale.encode(), 16).unwrap();
        let catalog = descriptor(
            &mut arena,
            Reference::NULL,
            b"broken",
            1,
            DatabaseKind::Ipv4,
            root,
        );
        let view = arena.freeze().unwrap().activate();
        assert_eq!(
            ulmdb_prefix::get(&view, root, Address::V4(0))
                .unwrap()
                .unwrap()
                .1,
            b"left"
        );
        assert!(audit(&view, catalog, 2, 100_000).is_err());

        let mut arena = super::tests::arena();
        let root = ulmdb_prefix::insert(
            &mut arena,
            Reference::NULL,
            Prefix::new(Address::V4(0), 0).unwrap(),
            b"body",
        )
        .unwrap();
        let value = Reference::decode(&arena.read(root, 96).unwrap()[64..80]).unwrap();
        let value = arena.patch(value, 20, 0, &[99], 16).unwrap();
        let root = arena.patch(root, 96, 64, &value.encode(), 16).unwrap();
        let catalog = descriptor(
            &mut arena,
            Reference::NULL,
            b"badbody",
            1,
            DatabaseKind::Ipv4,
            root,
        );
        let view = arena.freeze().unwrap().activate();
        assert!(audit(&view, catalog, 2, 100_000).is_err());
    }
}

#[cfg(test)]
#[path = "audit_single_pass_tests.rs"]
mod single_pass_tests;
