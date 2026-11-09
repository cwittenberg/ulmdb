//! Opt-in graph verification with checkpoints inside traversal and large-value
//! comparisons. No ordinary verification or query path consults this adapter.
use super::*;
use crate::{
    chunked::admission::ScopedReadStore,
    ordering::{BoundComparator, DatabaseComparators, OrderingSpec},
};
use ulmdb_core::work::WorkObserver;
use ulmdb_kv::ordering::Comparator;

fn read_checked<'a, S: ReadStore, C: WorkObserver + ?Sized>(
    inner: &'a S,
    control: &C,
    at: Reference,
    length: usize,
) -> Result<&'a [u8]> {
    control.checkpoint(1, u64::try_from(length).map_err(|_| Error::LimitExceeded)?)?;
    inner.read(at, length)
}
fn extent_checked<S: ReadStore, C: WorkObserver + ?Sized>(
    inner: &S,
    control: &C,
    at: Reference,
    length: usize,
) -> Result<()> {
    control.checkpoint(1, u64::try_from(length).map_err(|_| Error::LimitExceeded)?)?;
    inner.validate_extent(at, length)
}
struct Controlled<'a, S: ReadStore, C: WorkObserver + ?Sized> {
    inner: &'a S,
    control: &'a C,
}
impl<S: ReadStore, C: WorkObserver + ?Sized> ReadStore for Controlled<'_, S, C> {
    fn page_size(&self) -> usize {
        self.inner.page_size()
    }
    fn max_key_size(&self) -> usize {
        self.inner.max_key_size()
    }
    fn max_value_size(&self) -> usize {
        self.inner.max_value_size()
    }
    fn max_duplicate_key_size(&self) -> usize {
        self.inner.max_duplicate_key_size()
    }
    fn max_duplicate_value_size(&self) -> usize {
        self.inner.max_duplicate_value_size()
    }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        read_checked(self.inner, self.control, at, length)
    }
    fn validate_extent(&self, at: Reference, length: usize) -> Result<()> {
        extent_checked(self.inner, self.control, at, length)
    }
}
struct ControlledScope<'a, S: ReadStore, C: WorkObserver + ?Sized> {
    inner: S,
    control: &'a C,
}
impl<S: ReadStore, C: WorkObserver + ?Sized> ReadStore for ControlledScope<'_, S, C> {
    fn page_size(&self) -> usize {
        self.inner.page_size()
    }
    fn max_key_size(&self) -> usize {
        self.inner.max_key_size()
    }
    fn max_value_size(&self) -> usize {
        self.inner.max_value_size()
    }
    fn max_duplicate_key_size(&self) -> usize {
        self.inner.max_duplicate_key_size()
    }
    fn max_duplicate_value_size(&self) -> usize {
        self.inner.max_duplicate_value_size()
    }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        read_checked(&self.inner, self.control, at, length)
    }
    fn validate_extent(&self, at: Reference, length: usize) -> Result<()> {
        extent_checked(&self.inner, self.control, at, length)
    }
}
impl<S: ScopedReadStore, C: WorkObserver + ?Sized> ScopedReadStore for Controlled<'_, S, C> {
    type Scope<'a>
        = ControlledScope<'a, S::Scope<'a>, C>
    where
        Self: 'a;
    fn scope(&self) -> Result<Self::Scope<'_>> {
        self.control.checkpoint(1, 0)?;
        Ok(ControlledScope {
            inner: self.inner.scope()?,
            control: self.control,
        })
    }
}

// Full payload equality can touch many MiB even when a mapped read itself is
// O(1). Keep cancellation/work boundaries inside that CPU/memory work too.
fn equal<C: WorkObserver + ?Sized>(control: &C, left: &[u8], right: &[u8]) -> Result<bool> {
    if left.len() != right.len() {
        control.checkpoint(0, 0)?;
        return Ok(false);
    }
    if std::ptr::eq(left.as_ptr(), right.as_ptr()) || left.is_empty() {
        // Routing normally returns the very same immutable mapped extent.
        // Pointer/length identity proves equality without touching its body:
        // keep the cancellation poll but do not charge nonexistent comparison
        // work. The originating graph reads remain fully precharged.
        control.checkpoint(0, 0)?;
        return Ok(true);
    }
    // The first precharge polls before the first comparison too; a separate
    // zero-work callback immediately before it would add no checkpoint bound.
    for (left, right) in left
        .chunks(ulmdb_core::work::CHUNK_BYTES)
        .zip(right.chunks(ulmdb_core::work::CHUNK_BYTES))
    {
        control.checkpoint(
            1,
            (left.len() as u64)
                .checked_mul(2)
                .ok_or(Error::LimitExceeded)?,
        )?;
        if left != right {
            return Ok(false);
        }
    }
    Ok(true)
}
fn optional_equal<C: WorkObserver + ?Sized>(
    control: &C,
    found: Option<&[u8]>,
    expected: &[u8],
) -> Result<bool> {
    match found {
        Some(found) => equal(control, found, expected),
        None => Ok(false),
    }
}

pub(super) fn validate_with_control(
    store: &SegmentTable,
    manifest: &Manifest,
    structural_custom: bool,
    control: &BackupControl,
) -> Result<()> {
    validate_with_control_and_comparators(
        store,
        manifest,
        structural_custom,
        &BTreeMap::new(),
        control,
    )
}
pub(super) fn validate_with_control_and_comparators(
    store: &SegmentTable,
    manifest: &Manifest,
    structural_custom: bool,
    callbacks: &BTreeMap<Vec<u8>, DatabaseComparators>,
    control: &BackupControl,
) -> Result<()> {
    control.checkpoint(0, 0)?;
    let checked = Controlled {
        inner: store,
        control,
    };
    if store.is_chunked() {
        // The existing short-scope walker keeps its full format, parent-bound,
        // duplicate-count and metadata-memory checks. Its scopes also retain
        // this same control instead of silently dropping observation.
        return crate::chunked::admission::admit_with_policy_unobserved(
            &checked,
            manifest,
            chunk_budget(store),
            structural_custom,
            callbacks,
        )
        .map(|_| ());
    }
    validate_store_with_observer(store, manifest, structural_custom, callbacks, control).map(|_| ())
}

pub(super) fn validate_store_with_observer<S: ReadStore, C: WorkObserver + ?Sized>(
    store: &S,
    manifest: &Manifest,
    structural_custom: bool,
    callbacks: &BTreeMap<Vec<u8>, DatabaseComparators>,
    control: &C,
) -> Result<ValidationWork> {
    let mut bytes = 0u64;
    for entry in &manifest.segments {
        control.checkpoint(1, 0)?;
        bytes = bytes
            .checked_add(entry.length)
            .ok_or(Error::LimitExceeded)?;
    }
    let budget = ValidationBudget::for_geometry(bytes, manifest.page_size.get())?;
    let checked = Controlled {
        inner: store,
        control,
    };
    let bounded = ValidationStore {
        inner: &checked,
        remaining: Cell::new(budget.reads),
        page_size: manifest.page_size.get(),
        value_limit: manifest.value_limit.get(),
    };
    let max_tree_pages = indexes(
        &bounded,
        manifest.catalog,
        manifest.next_database,
        budget.tree_pages,
        structural_custom,
        callbacks,
        control,
    )?;
    Ok(ValidationWork {
        reads: budget.reads - bounded.remaining.get(),
        max_tree_pages,
    })
}

pub(crate) fn admit_scoped_with_observer<S: ScopedReadStore, C: WorkObserver + ?Sized>(
    store: &S,
    manifest: &Manifest,
    limits: crate::chunked::AdmissionLimits,
    structural_custom: bool,
    callbacks: &BTreeMap<Vec<u8>, DatabaseComparators>,
    control: &C,
) -> Result<BTreeMap<Vec<u8>, Database>> {
    let checked = Controlled {
        inner: store,
        control,
    };
    crate::chunked::admission::admit_with_policy_unobserved(
        &checked,
        manifest,
        limits,
        structural_custom,
        callbacks,
    )
}

#[allow(clippy::too_many_arguments)]
fn indexes<S: ReadStore, C: WorkObserver + ?Sized>(
    store: &S,
    catalog: Reference,
    next_database: u64,
    page_budget: u64,
    structural_custom: bool,
    callbacks: &BTreeMap<Vec<u8>, DatabaseComparators>,
    control: &C,
) -> Result<u64> {
    let mut max_tree_pages = ulmdb_kv::audit_tree(store, catalog, page_budget)?.pages;
    let mut identities = BTreeSet::new();
    let capacity = crate::catalog_profile::read(store, catalog)?;
    let mut databases = 0usize;
    let mut prior = None;
    for item in cursor(store, catalog)? {
        control.checkpoint(1, 0)?;
        let (name, bytes) = item?;
        if name.is_empty() {
            crate::CatalogCapacity::decode_marker(bytes)?;
            continue;
        }
        if name.len() > 255
            || prior.is_some_and(|p: &[u8]| p >= name)
            || databases >= capacity.get()
        {
            return Err(Error::Corrupt("invalid backup catalog ordering/name"));
        }
        prior = Some(name);
        databases += 1;
        let db = Database::decode_for_profile(
            bytes,
            crate::PageSize::new(store.page_size())?,
            crate::ValueLimit::new(store.max_value_size())?,
        )?;
        if db.id >= next_database
            || !identities.insert(db.id)
            || !optional_equal(control, ulmdb_kv::get(store, catalog, name)?, bytes)?
        {
            return Err(Error::Corrupt("invalid backup catalog identity/routing"));
        }
        let limited = ValueBoundStore {
            inner: store,
            limit: Database::declared_value_limit(bytes)?.get(),
        };
        let store = &limited;
        match db.kind {
            DatabaseKind::Kv | DatabaseKind::Duplicates { .. } => {
                if db.ordering == Default::default() {
                    match db.kind {
                        DatabaseKind::Kv => {
                            max_tree_pages = max_tree_pages.max(kv(
                                store,
                                db.root,
                                page_budget,
                                None,
                                false,
                                None,
                                control,
                            )?);
                        }
                        DatabaseKind::Duplicates { width } => {
                            max_tree_pages = max_tree_pages.max(crate::duplicates::audit_buckets(
                                store,
                                db.root,
                                width,
                                page_budget,
                            )?);
                        }
                        _ => unreachable!(),
                    }
                } else {
                    let default = DatabaseComparators::default();
                    max_tree_pages = max_tree_pages.max(ordered_database(
                        store,
                        &db,
                        page_budget,
                        callbacks.get(name).unwrap_or(&default),
                        structural_custom,
                        control,
                    )?);
                }
            }
            DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => prefixes(store, db.root, db.kind, control)?,
        }
    }
    Ok(max_tree_pages)
}
fn bind(
    spec: OrderingSpec,
    callback: Option<&crate::ordering::CustomComparator>,
    structural_custom: bool,
) -> Result<Option<BoundComparator>> {
    if spec == OrderingSpec::Custom && callback.is_none() && structural_custom {
        Ok(None)
    } else {
        BoundComparator::bind(spec, callback).map(Some)
    }
}
#[allow(clippy::too_many_arguments)]
fn kv<S: ReadStore, C: WorkObserver + ?Sized>(
    store: &S,
    root: Reference,
    budget: u64,
    order: Option<&BoundComparator>,
    duplicate: bool,
    width: Option<u32>,
    control: &C,
) -> Result<u64> {
    if order.is_none() {
        let mut invalid = false;
        // The full audit already proves strict local ordering, every ancestor
        // separator bound, child levels and every payload extent. Inspect the
        // checked leaves in that same traversal instead of walking the tree
        // again and looking every key up from the root.
        let audit = ulmdb_kv::audit_tree_with_leaf_visitor(store, root, budget, |key, value| {
            // The store precharges each page/payload before parsing it. This
            // nonfallible visitor only checks those already admitted bytes;
            // inline record CPU work is bounded by one native page (<=64 KiB).
            invalid |= key.len()
                > if duplicate {
                    65536
                } else {
                    store.max_key_size()
                }
                || value.len() > store.max_value_size()
                || (duplicate
                    && (!value.is_empty() || width.is_some_and(|w| key.len() != w as usize)));
        });
        let pages = audit?.pages;
        if invalid {
            return Err(Error::Corrupt("ordered KV size/duplicate width"));
        }
        return Ok(pages);
    }
    let order = order.expect("bytewise audit returned above");
    let pages = ulmdb_kv::audit_tree_with(store, root, budget, order)?.pages;
    let mut prior: Option<&[u8]> = None;
    for pair in cursor(store, root)? {
        control.checkpoint(1, 0)?;
        let (key, value) = pair?;
        if key.len()
            > if duplicate {
                65536
            } else {
                store.max_key_size()
            }
            || value.len() > store.max_value_size()
            || (duplicate && (!value.is_empty() || width.is_some_and(|w| key.len() != w as usize)))
            || prior.is_some_and(|old| !order.compare(old, key).is_lt())
        {
            return Err(Error::Corrupt("ordered KV size/duplicate width"));
        }
        prior = Some(key);
        order
            .validate(key)
            .map_err(|_| Error::Corrupt("stored ordering width"))?;
        let found = ulmdb_kv::get_with(store, root, key, order)?;
        if !optional_equal(control, found, value)? {
            return Err(Error::Corrupt("invalid backup KV ordering/size/routing"));
        }
    }
    Ok(pages)
}
fn structural_kv<S: ReadStore, C: WorkObserver + ?Sized>(
    store: &S,
    root: Reference,
    budget: u64,
    duplicate: bool,
    width: Option<u32>,
    control: &C,
) -> Result<u64> {
    let pages = ulmdb_kv::audit_tree_structure(store, root, budget)?.pages;
    for pair in cursor(store, root)? {
        control.checkpoint(1, 0)?;
        let (key, value) = pair?;
        if key.len()
            > if duplicate {
                65536
            } else {
                store.max_key_size()
            }
            || value.len() > store.max_value_size()
            || (duplicate && (!value.is_empty() || width.is_some_and(|w| key.len() != w as usize)))
        {
            return Err(Error::Corrupt("ordered KV size/duplicate width"));
        }
    }
    Ok(pages)
}
fn ordered_database<S: ReadStore, C: WorkObserver + ?Sized>(
    store: &S,
    db: &Database,
    budget: u64,
    callbacks: &DatabaseComparators,
    structural_custom: bool,
    control: &C,
) -> Result<u64> {
    let keys = bind(db.ordering.keys, callbacks.keys.as_ref(), structural_custom)?;
    let values = bind(
        db.ordering.duplicates,
        callbacks.duplicates.as_ref(),
        structural_custom,
    )?;
    if db.kind == DatabaseKind::Kv {
        return match keys.as_ref() {
            Some(order) => kv(store, db.root, budget, Some(order), false, None, control),
            None => structural_kv(store, db.root, budget, false, None, control),
        };
    }
    let DatabaseKind::Duplicates { width } = db.kind else {
        return Err(Error::WrongKind);
    };
    let mut pages = match keys.as_ref() {
        Some(order) => ulmdb_kv::audit_tree_with(store, db.root, budget, order)?,
        None => ulmdb_kv::audit_tree_structure(store, db.root, budget)?,
    }
    .pages;
    for pair in cursor(store, db.root)? {
        control.checkpoint(1, 0)?;
        let (key, bytes) = pair?;
        if key.len() > store.max_key_size() {
            return Err(Error::Corrupt("duplicate key too large"));
        }
        if let Some(keys) = &keys {
            keys.validate(key)
                .map_err(|_| Error::Corrupt("stored key width"))?;
            if !optional_equal(
                control,
                ulmdb_kv::get_with(store, db.root, key, keys)?,
                bytes,
            )? {
                return Err(Error::Corrupt("duplicate key routing"));
            }
        }
        match crate::duplicates::bucket::Bucket::decode_with(
            bytes,
            width,
            values.as_ref().map(|v| v as &dyn Comparator),
        )? {
            crate::duplicates::bucket::Bucket::Inline(_) => (),
            crate::duplicates::bucket::Bucket::Tree {
                root,
                count: expected,
            } => {
                pages = pages.max(match values.as_ref() {
                    Some(order) => kv(store, root, budget, Some(order), true, width, control)?,
                    None => structural_kv(store, root, budget, true, width, control)?,
                });
                let mut count = 0u64;
                for item in cursor(store, root)? {
                    control.checkpoint(1, 0)?;
                    item?;
                    count = count.checked_add(1).ok_or(Error::LimitExceeded)?;
                }
                if count != expected {
                    return Err(Error::Corrupt("duplicate count mismatch"));
                }
            }
        }
    }
    Ok(pages)
}
fn prefixes<S: ReadStore, C: WorkObserver + ?Sized>(
    store: &S,
    root: Reference,
    kind: DatabaseKind,
    control: &C,
) -> Result<()> {
    let address = if kind == DatabaseKind::Ipv4 {
        Address::V4(0)
    } else {
        Address::V6(0)
    };
    let all = Prefix::new(address, 0).map_err(|_| Error::Corrupt("backup prefix family"))?;
    let mut count = 0u64;
    let mut prior = None;
    // A /0 traversal visits every node in this family. Node::child validates
    // strict prefix growth, containment and the selected side on every edge;
    // those properties prove unique routing without a second exact lookup.
    for item in ulmdb_prefix::subnets(store, root, all)? {
        control.checkpoint(1, 0)?;
        let (prefix, value) = item?;
        let order = (prefix.network(), prefix.length());
        if value.len() > store.max_value_size() || prior.is_some_and(|previous| previous >= order) {
            return Err(Error::Corrupt("invalid backup prefix value/routing"));
        }
        prior = Some(order);
        count = count.checked_add(1).ok_or(Error::LimitExceeded)?;
    }
    if !root.is_null() && count == 0 {
        return Err(Error::Corrupt("empty/wrong-family backup prefix root"));
    }
    Ok(())
}

/// Fully verify one already-admitted environment with cooperative cancellation
/// and total work limits. Limits abort the operation; a partial verification is
/// never returned as a successful summary. Custom comparators require the
/// explicit comparator variant, just as for ordinary verification. Plain mapped
/// Legacy/Cohort/Indexed snapshots are supported; Range, chunked, transformed
/// and externally synchronized snapshots reject before traversal.
pub fn verify_with_control(environment: &Environment, control: &BackupControl) -> Result<Summary> {
    let read = environment.begin_read();
    verify_snapshot_with_control(environment, &read, control)
}
/// Verify an exact pinned read from this environment even if CURRENT advances.
pub fn verify_snapshot_with_control(
    environment: &Environment,
    read: &ReadTxn,
    control: &BackupControl,
) -> Result<Summary> {
    control.scope(|| {
        super::controlled_export::check_supported(&read.snapshot)?;
        if read.snapshot.owner != environment.current.load().owner {
            return Err(Error::InvalidInput(
                "snapshot belongs to another environment",
            ));
        }
        read.snapshot.check_external()?;
        validate_with_control(
            &read.snapshot.segments,
            &read.snapshot.manifest,
            false,
            control,
        )?;
        let summary = super::controlled_export::summary_with_control(
            &environment.directory,
            &read.snapshot.manifest,
            control,
        )?;
        control.checkpoint(0, 0)?;
        Ok(summary)
    })
}
pub fn verify_with_comparators_and_control(
    environment: &Environment,
    callbacks: &BTreeMap<Vec<u8>, DatabaseComparators>,
    control: &BackupControl,
) -> Result<Summary> {
    control.scope(|| {
        let snapshot = environment.current.load_full();
        super::controlled_export::check_supported(&snapshot)?;
        snapshot.check_external()?;
        validate_with_control_and_comparators(
            &snapshot.segments,
            &snapshot.manifest,
            false,
            callbacks,
            control,
        )?;
        let summary = super::controlled_export::summary_with_control(
            &environment.directory,
            &snapshot.manifest,
            control,
        )?;
        control.checkpoint(0, 0)?;
        Ok(summary)
    })
}
/// Verify an independently pinned file read without taking a source writer
/// lease. Control also observes cold descriptor/header operations in this call.
pub fn verify_independent_snapshot_with_control(
    source: impl AsRef<Path>,
    read: &ReadTxn,
    control: &BackupControl,
) -> Result<Summary> {
    control.scope(|| {
        let snapshot = &read.snapshot;
        super::controlled_export::check_supported(snapshot)?;
        snapshot.check_external()?;
        if snapshot.range_backed || snapshot.external.is_some() {
            return Err(Error::InvalidInput(
                "independent verification requires coordinated files",
            ));
        }
        let pin = snapshot.physical_pin.as_ref().ok_or(Error::InvalidInput(
            "independent verification requires a physical pin",
        ))?;
        control.checkpoint(1, 0)?;
        let source = source.as_ref().canonicalize()?;
        // Descriptor hashing may itself need many work slices. Do it before
        // taking Capture, then bind only the fixed namespace under the gate.
        if pin.root().manifest().uuid != snapshot.manifest.uuid
            || pin.root().checkpoint() != &ulmdb_storage::checkpoint_bytes(&snapshot.manifest)?
        {
            return Err(Error::InvalidInput(
                "physical snapshot pin belongs to another source",
            ));
        }
        check_namespace(pin, &source, control)?;
        validate_with_control(&snapshot.segments, &snapshot.manifest, false, control)?;
        let summary =
            super::controlled_export::summary_with_control(&source, &snapshot.manifest, control)?;
        check_namespace(pin, &source, control)?;
        control.checkpoint(0, 0)?;
        Ok(summary)
    })
}

fn check_namespace(
    pin: &ulmdb_storage::coordination::PhysicalPin,
    source: &Path,
    control: &BackupControl,
) -> Result<()> {
    control.checkpoint(1, 0)?;
    // This bounded ownership check has no callback/pause while Capture is
    // held. The read's physical pin retains every immutable root dependency
    // throughout the graph traversal and every later cooperative pause.
    ulmdb_core::work::without_observer(|| {
        let capture = pin.coordination().try_capture()?;
        let same = pin.matches_directory(source)?;
        drop(capture);
        if same {
            Ok(())
        } else {
            Err(Error::InvalidInput(
                "physical snapshot pin belongs to another source",
            ))
        }
    })
}

#[cfg(test)]
#[path = "controlled_verify_tests.rs"]
mod traversal_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ops::ControlFlow, sync::Arc};
    #[test]
    fn identical_immutable_extents_skip_comparison_work_but_still_poll_cancellation() {
        let bytes = vec![17; 4 * 1024 * 1024];
        let control = BackupControl::default();
        assert!(equal(&control, &bytes, &bytes).unwrap());
        assert_eq!(control.progress(), BackupProgress::default());
        let token = CancellationToken::new();
        let cancelled = BackupControl::default().with_cancel_token(token.clone());
        token.cancel();
        assert!(matches!(
            equal(&cancelled, &bytes, &bytes),
            Err(ref error) if error.is_cancelled()
        ));
        assert_eq!(cancelled.progress(), BackupProgress::default());
    }
    #[test]
    fn large_value_comparison_checks_control_instead_of_one_unbounded_memcmp() {
        let left = vec![7; 4 * 1024 * 1024];
        let right = left.clone();
        let control = BackupControl::default().with_progress_callback(Arc::new(|p| {
            if p.units >= 3 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }));
        assert!(matches!(
            equal(&control, &left, &right),
            Err(ref error) if error.is_cancelled()
        ));
        assert_eq!(control.progress().units, 3);
        assert_eq!(
            control.progress().bytes,
            (3 * 2 * ulmdb_core::work::CHUNK_BYTES) as u64
        );
        assert!(control.progress().bytes < (left.len() + right.len()) as u64);
    }
}
