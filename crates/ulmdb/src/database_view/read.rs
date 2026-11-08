//! Immutable G02 loans; no native mutable borrow or writer registry insertion.
use super::*;
use std::sync::Mutex;
struct ReadAuthority {
    id: u64,
    handle: DatabaseHandle,
    origin: DatabaseViewOrigin,
    ordering: crate::ordering::DatabaseOrdering,
    keys: crate::ordering::BoundComparator,
    values: crate::ordering::BoundComparator,
}
struct ReadRecord {
    authority: Weak<ReadAuthority>,
    root: Reference,
    kind: DatabaseKind,
}
#[derive(Default)]
struct ReadRegistry {
    next: u64,
    records: BTreeMap<u64, ReadRecord>,
}
struct Scope {
    creator: u32,
    registry: Mutex<ReadRegistry>,
}
pub struct DatabaseReadScope<'txn> {
    source: &'txn WriteTxn<'txn>,
    scope: Arc<Scope>,
}
#[derive(Clone)]
pub struct DatabaseReadView {
    scope: Weak<Scope>,
    authority: Arc<ReadAuthority>,
}
impl DatabaseReadView {
    pub fn handle(&self) -> &DatabaseHandle {
        &self.authority.handle
    }
    pub fn origin(&self) -> DatabaseViewOrigin {
        self.authority.origin
    }
}
impl WriteTxn<'_> {
    pub fn database_read_scope(&self) -> Result<DatabaseReadScope<'_>> {
        self.view_live()?;
        Ok(DatabaseReadScope {
            source: self,
            scope: Arc::new(Scope {
                creator: self.view_creator_pid,
                registry: Mutex::new(ReadRegistry::default()),
            }),
        })
    }
}
impl DatabaseReadScope<'_> {
    fn live(&self) -> Result<()> {
        self.source.view_live()?;
        if self.scope.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited database read scope"));
        }
        Ok(())
    }
    fn issue(
        &self,
        handle: &DatabaseHandle,
        root: Reference,
        origin: DatabaseViewOrigin,
        ordering: crate::ordering::DatabaseOrdering,
        keys: crate::ordering::BoundComparator,
        values: crate::ordering::BoundComparator,
    ) -> Result<DatabaseReadView> {
        self.live()?;
        let mut registry = self.scope.registry.lock().map_err(|_| Error::Busy)?;
        #[cfg(feature = "fault-injection")]
        ulmdb_storage::injected_fault("database_view.read.locked")?;
        registry
            .records
            .retain(|_, r| r.authority.strong_count() != 0);
        if registry.records.len() >= self.source.catalog_capacity.view_limit() {
            return Err(Error::LimitExceeded);
        }
        let id = registry.next;
        let next = id.checked_add(1).ok_or(Error::LimitExceeded)?;
        let authority = Arc::new(ReadAuthority {
            id,
            handle: handle.clone(),
            origin,
            ordering,
            keys,
            values,
        });
        registry.records.insert(
            id,
            ReadRecord {
                authority: Arc::downgrade(&authority),
                root,
                kind: handle.kind(),
            },
        );
        registry.next = next;
        Ok(DatabaseReadView {
            scope: Arc::downgrade(&self.scope),
            authority,
        })
    }
    pub fn open_database_view(&self, actual: &DatabaseHandle) -> Result<DatabaseReadView> {
        self.open_database_view_with_ordering(actual, &Default::default())
    }
    pub fn open_database_view_with_ordering(
        &self,
        actual: &DatabaseHandle,
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<DatabaseReadView> {
        self.live()?;
        let (root, origin) = self.source.view_seed(actual)?;
        let ordering = self
            .source
            .catalog
            .get(actual.name())
            .ok_or(Error::StaleHandle)?
            .ordering;
        self.issue(
            actual,
            root,
            origin,
            ordering,
            crate::ordering::BoundComparator::deferred(ordering.keys, callbacks.keys.as_ref())?,
            crate::ordering::BoundComparator::deferred(
                ordering.duplicates,
                callbacks.duplicates.as_ref(),
            )?,
        )
    }
    pub fn loan_database_view(&self, parent: &DatabaseView) -> Result<DatabaseReadView> {
        self.live()?;
        let record = self.source.view_record(parent)?;
        self.issue(
            parent.handle(),
            record.root,
            parent.origin(),
            parent.ordering(),
            parent.authority.keys.clone(),
            parent.authority.values.clone(),
        )
    }
    fn root(&self, view: &DatabaseReadView) -> Result<(Reference, DatabaseKind)> {
        self.live()?;
        if !view.scope.ptr_eq(&Arc::downgrade(&self.scope)) {
            return Err(Error::StaleHandle);
        }
        let registry = self.scope.registry.lock().map_err(|_| Error::Busy)?;
        let record = registry
            .records
            .get(&view.authority.id)
            .filter(|r| r.authority.ptr_eq(&Arc::downgrade(&view.authority)))
            .ok_or(Error::StaleHandle)?;
        Ok((record.root, record.kind))
    }
    pub fn get(&self, view: &DatabaseReadView, key: &[u8]) -> Result<Option<&[u8]>> {
        let (root, kind) = self.root(view)?;
        get(
            &self.source.arena,
            root,
            kind,
            key,
            &view.authority.keys,
            &view.authority.values,
        )
    }
    pub fn seek_pair_borrowed(
        &self,
        view: &DatabaseReadView,
        bound: Option<(&[u8], Option<&[u8]>)>,
        inclusive: bool,
        reverse: bool,
    ) -> Result<Option<(&[u8], &[u8])>> {
        let (root, kind) = self.root(view)?;
        crate::duplicates::seek_pair_with(
            &self.source.arena,
            root,
            kind,
            bound,
            inclusive,
            reverse,
            &view.authority.keys,
            &view.authority.values,
        )
    }
    pub fn duplicate_count(&self, view: &DatabaseReadView, key: &[u8]) -> Result<u64> {
        let (root, kind) = self.root(view)?;
        count(
            &self.source.arena,
            root,
            kind,
            key,
            &view.authority.keys,
            &view.authority.values,
        )
    }
}

impl DatabaseReadView {
    pub fn ordering(&self) -> crate::ordering::DatabaseOrdering {
        self.authority.ordering
    }
}
impl DatabaseReadScope<'_> {
    pub fn get_with_ordering(
        &self,
        view: &DatabaseReadView,
        key: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<Option<&[u8]>> {
        let (root, kind) = self.root(view)?;
        let spec = view.ordering();
        get(
            &self.source.arena,
            root,
            kind,
            key,
            &crate::ordering::BoundComparator::bind(spec.keys, callbacks.keys.as_ref())?,
            &crate::ordering::BoundComparator::bind(
                spec.duplicates,
                callbacks.duplicates.as_ref(),
            )?,
        )
    }
    pub fn seek_pair_with_ordering(
        &self,
        view: &DatabaseReadView,
        bound: Option<(&[u8], Option<&[u8]>)>,
        inclusive: bool,
        reverse: bool,
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<Option<(&[u8], &[u8])>> {
        let (root, kind) = self.root(view)?;
        let spec = view.ordering();
        crate::duplicates::seek_pair_with(
            &self.source.arena,
            root,
            kind,
            bound,
            inclusive,
            reverse,
            &crate::ordering::BoundComparator::bind(spec.keys, callbacks.keys.as_ref())?,
            &crate::ordering::BoundComparator::bind(
                spec.duplicates,
                callbacks.duplicates.as_ref(),
            )?,
        )
    }
}

impl DatabaseReadScope<'_> {
    pub fn structural_resume_pair(
        &self,
        view: &DatabaseReadView,
        after: Option<(&[u8], &[u8])>,
    ) -> Result<Option<(&[u8], &[u8])>> {
        let (root, kind) = self.root(view)?;
        crate::duplicates::structural::resume(&self.source.arena, root, kind, after)
    }
}

impl DatabaseReadScope<'_> {
    pub fn duplicate_count_with_ordering(
        &self,
        view: &DatabaseReadView,
        key: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<u64> {
        let (root, kind) = self.root(view)?;
        let spec = view.ordering();
        count(
            &self.source.arena,
            root,
            kind,
            key,
            &crate::ordering::BoundComparator::bind(spec.keys, callbacks.keys.as_ref())?,
            &crate::ordering::BoundComparator::bind(
                spec.duplicates,
                callbacks.duplicates.as_ref(),
            )?,
        )
    }
}

impl DatabaseReadScope<'_> {
    pub fn database_stats(&self, view: &DatabaseReadView) -> Result<crate::DatabaseStats> {
        let (root, kind) = self.root(view)?;
        crate::physical_stats::measure(
            &self.source.arena,
            root,
            kind,
            view.ordering(),
            self.source.physical_budget()?,
        )
    }
}
