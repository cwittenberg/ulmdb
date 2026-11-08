//! Transaction-local branches seeded only from immutable transaction-start state.
//! No operation accepts a raw root, and root selection revokes sibling views.
use crate::{DatabaseHandle, DatabaseKind, Error, ReadTxn, Result, WriteTxn};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Weak},
};
use ulmdb_core::storage::Reference;
mod read;
mod retained;
pub use read::{DatabaseReadScope, DatabaseReadView};
pub use retained::{DatabaseViewTransfer, RetainedDatabaseView, RetainedViewGroup};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatabaseViewOrigin {
    TransactionBase,
    CreatedEmpty,
}
struct Authority {
    id: u64,
    handle: DatabaseHandle,
    origin: DatabaseViewOrigin,
    ordering: crate::ordering::DatabaseOrdering,
    keys: crate::ordering::BoundComparator,
    values: crate::ordering::BoundComparator,
}
#[derive(Clone)]
pub struct DatabaseView {
    authority: Arc<Authority>,
}
impl std::fmt::Debug for DatabaseView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DatabaseView")
            .field(&self.authority.handle)
            .finish()
    }
}
impl DatabaseView {
    pub fn handle(&self) -> &DatabaseHandle {
        &self.authority.handle
    }
    pub fn ordering(&self) -> crate::ordering::DatabaseOrdering {
        self.authority.ordering
    }
    pub fn origin(&self) -> DatabaseViewOrigin {
        self.authority.origin
    }
}
#[derive(Clone)]
struct Record {
    authority: Weak<Authority>,
    dirty_owner: Option<Arc<Authority>>,
    root: Reference,
    kind: DatabaseKind,
    retained: Option<u64>,
}
impl Record {
    fn dirty(&self) -> bool {
        self.dirty_owner.is_some()
    }
}
#[derive(Clone, Default)]
pub(crate) struct ViewRegistry {
    records: BTreeMap<u64, Record>,
    groups: BTreeMap<u64, retained::GroupRecord>,
}
impl ViewRegistry {
    pub(crate) fn has_identity(&self, id: u64) -> bool {
        self.records
            .values()
            .any(|r| r.authority.upgrade().is_some_and(|a| a.handle.id() == id))
    }
    fn prune(&mut self) {
        self.records
            .retain(|_, record| record.dirty() || record.authority.strong_count() != 0);
        let active: BTreeSet<_> = self
            .records
            .values()
            .filter_map(|record| record.retained)
            .collect();
        self.groups
            .retain(|id, group| active.contains(id) && group.is_owned());
        for record in self.records.values_mut() {
            if record
                .retained
                .is_some_and(|id| !self.groups.contains_key(&id))
            {
                record.retained = None;
            }
        }
    }
}
impl WriteTxn<'_> {
    pub const MAX_DATABASE_VIEWS: usize = 8194;
    fn view_live(&self) -> Result<()> {
        if self.view_creator_pid != std::process::id() {
            return Err(Error::InvalidInput("inherited database view owner"));
        }
        self.live()
    }
    fn view_seed(&self, actual: &DatabaseHandle) -> Result<(Reference, DatabaseViewOrigin)> {
        self.view_live()?;
        if self.open_database(actual.name())? != *actual {
            return Err(Error::StaleHandle);
        }
        if !matches!(
            actual.kind(),
            DatabaseKind::Kv | DatabaseKind::Duplicates { .. }
        ) {
            return Err(Error::WrongKind);
        }
        if let Some(base) = self
            .base
            .catalog
            .get(actual.name())
            .filter(|db| db.id == actual.id() && db.kind == actual.kind())
        {
            return Ok((base.root, DatabaseViewOrigin::TransactionBase));
        }
        if self.created_database_ids.contains(&actual.id()) {
            Ok((Reference::NULL, DatabaseViewOrigin::CreatedEmpty))
        } else {
            Err(Error::StaleHandle)
        }
    }
    pub fn transaction_start_snapshot(&self) -> Result<ReadTxn> {
        self.view_live()?;
        Ok(ReadTxn {
            snapshot: self.base.clone(),
        })
    }
    /// Preflight view admission before an enclosing facade creates a catalog
    /// entry or writes name/settings metadata. The caller retains exclusive
    /// writer access until actual issuance; no slot or data is changed here.
    pub fn preflight_database_view(&self) -> Result<()> {
        self.view_live()?;
        self.next_view_id
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        if self.database_views.as_ref().is_some_and(|registry| {
            // The physical record count bounds every live/dirty authority.
            // Count liveness only at pressure; all admission checks above stay
            // ahead of the facade's catalog or metadata mutation.
            registry.records.len() >= self.catalog_capacity.view_limit()
                && registry
                    .records
                    .values()
                    .filter(|record| record.dirty() || record.authority.strong_count() != 0)
                    .count()
                    >= self.catalog_capacity.view_limit()
        }) {
            return Err(Error::LimitExceeded);
        }
        Ok(())
    }
    pub fn open_database_view(&mut self, actual: &DatabaseHandle) -> Result<DatabaseView> {
        self.open_database_view_with_ordering(actual, &Default::default())
    }
    pub fn open_database_view_with_ordering(
        &mut self,
        actual: &DatabaseHandle,
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<DatabaseView> {
        self.preflight_database_view()?;
        let (root, origin) = self.view_seed(actual)?;
        let ordering = self
            .catalog
            .get(actual.name())
            .ok_or(Error::StaleHandle)?
            .ordering;
        let keys =
            crate::ordering::BoundComparator::deferred(ordering.keys, callbacks.keys.as_ref())?;
        let values = crate::ordering::BoundComparator::deferred(
            ordering.duplicates,
            callbacks.duplicates.as_ref(),
        )?;
        let next = self
            .next_view_id
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        let registry = self.database_views.get_or_insert_with(Default::default);
        // Dead clean records may wait within the existing bounded allowance.
        // Retained groups still reconcile at every admission as before.
        if registry.records.len() >= self.catalog_capacity.view_limit()
            || !registry.groups.is_empty()
        {
            registry.prune();
        }
        if registry.records.len() >= self.catalog_capacity.view_limit() {
            return Err(Error::LimitExceeded);
        }
        let authority = Arc::new(Authority {
            id: self.next_view_id,
            handle: actual.clone(),
            origin,
            ordering,
            keys,
            values,
        });
        registry.records.insert(
            authority.id,
            Record {
                authority: Arc::downgrade(&authority),
                dirty_owner: None,
                retained: None,
                root,
                kind: actual.kind(),
            },
        );
        self.next_view_id = next;
        Ok(DatabaseView { authority })
    }
    fn view_record(&self, view: &DatabaseView) -> Result<&Record> {
        self.view_live()?;
        self.database_views
            .as_ref()
            .and_then(|r| r.records.get(&view.authority.id))
            .filter(|r| r.authority.ptr_eq(&Arc::downgrade(&view.authority)))
            .ok_or(Error::StaleHandle)
    }
    pub(crate) fn writable_view(&self, view: &DatabaseView) -> Result<(Reference, DatabaseKind)> {
        let record = self.view_record(view)?;
        if self.open_database(view.handle().name())? != *view.handle() {
            return Err(Error::StaleHandle);
        }
        Ok((record.root, record.kind))
    }
    pub fn view_get(&self, view: &DatabaseView, key: &[u8]) -> Result<Option<&[u8]>> {
        let r = self.view_record(view)?;
        get(
            &self.arena,
            r.root,
            r.kind,
            key,
            &view.authority.keys,
            &view.authority.values,
        )
    }
    pub fn view_seek_pair_borrowed(
        &self,
        view: &DatabaseView,
        bound: Option<(&[u8], Option<&[u8]>)>,
        inclusive: bool,
        reverse: bool,
    ) -> Result<Option<(&[u8], &[u8])>> {
        let r = self.view_record(view)?;
        crate::duplicates::seek_pair_with(
            &self.arena,
            r.root,
            r.kind,
            bound,
            inclusive,
            reverse,
            &view.authority.keys,
            &view.authority.values,
        )
    }
    pub fn view_duplicate_count(&self, view: &DatabaseView, key: &[u8]) -> Result<u64> {
        let r = self.view_record(view)?;
        count(
            &self.arena,
            r.root,
            r.kind,
            key,
            &view.authority.keys,
            &view.authority.values,
        )
    }
    pub(crate) fn update_view(
        &mut self,
        view: &DatabaseView,
        old: Reference,
        revision: u64,
        root: Reference,
    ) {
        let record = self
            .database_views
            .as_mut()
            .unwrap()
            .records
            .get_mut(&view.authority.id)
            .unwrap();
        record.root = root;
        if old != root || self.arena.revision() != revision {
            record.dirty_owner = Some(view.authority.clone());
            if let Some(group) = record.retained {
                self.database_views
                    .as_mut()
                    .unwrap()
                    .groups
                    .get_mut(&group)
                    .unwrap()
                    .retain_dirty();
            }
        }
    }
    pub fn view_put(&mut self, view: &DatabaseView, key: &[u8], value: &[u8]) -> Result<()> {
        let checked = self.writable_view(view)?;
        self.put_view_checked(view, checked, key, value)
    }
    fn put_view_checked(
        &mut self,
        view: &DatabaseView,
        (old, kind): (Reference, DatabaseKind),
        key: &[u8],
        value: &[u8],
    ) -> Result<()> {
        if kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if key.len() > self.base.manifest.page_size.native_key_limit()
            || value.len() > self.writable_value_limit()
        {
            return Err(Error::InvalidInput("KV size limit exceeded"));
        }
        self.enable_value_limit(value.len())?;
        let revision = self.arena.revision();
        self.failed = true;
        let root = ulmdb_kv::put_with(&mut self.arena, old, key, value, &view.authority.keys)?;
        self.update_view(view, old, revision, root);
        self.failed = false;
        Ok(())
    }
    pub fn view_delete(&mut self, view: &DatabaseView, key: &[u8]) -> Result<()> {
        let checked = self.writable_view(view)?;
        self.delete_view_checked(view, checked, key)
    }
    fn delete_view_checked(
        &mut self,
        view: &DatabaseView,
        (old, kind): (Reference, DatabaseKind),
        key: &[u8],
    ) -> Result<()> {
        if kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if key.len() > self.base.manifest.page_size.native_key_limit() {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        let revision = self.arena.revision();
        self.failed = true;
        let root = ulmdb_kv::delete_with(&mut self.arena, old, key, &view.authority.keys)?;
        self.update_view(view, old, revision, root);
        self.failed = false;
        Ok(())
    }
    fn mutate_view_duplicate(
        &mut self,
        view: &DatabaseView,
        key: &[u8],
        mutation: crate::duplicates::Mutation<'_>,
    ) -> Result<u64> {
        let checked = self.writable_view(view)?;
        self.duplicate_view_checked(view, checked, key, mutation)
    }
    fn duplicate_view_checked(
        &mut self,
        view: &DatabaseView,
        (old, kind): (Reference, DatabaseKind),
        key: &[u8],
        mutation: crate::duplicates::Mutation<'_>,
    ) -> Result<u64> {
        if !matches!(kind, DatabaseKind::Duplicates { .. }) {
            return Err(Error::WrongKind);
        }
        let revision = self.arena.revision();
        self.failed = true;
        let (root, count) = crate::duplicates::ordered_write::mutate_root_with(
            &mut self.arena,
            old,
            kind,
            key,
            mutation,
            &view.authority.keys,
            &view.authority.values,
        )?;
        self.update_view(view, old, revision, root);
        self.failed = false;
        Ok(count)
    }
    pub fn view_insert_duplicate(
        &mut self,
        view: &DatabaseView,
        key: &[u8],
        value: &[u8],
    ) -> Result<bool> {
        self.mutate_view_duplicate(view, key, crate::duplicates::Mutation::Insert(value))
            .map(|n| n != 0)
    }
    pub fn view_delete_duplicate(
        &mut self,
        view: &DatabaseView,
        key: &[u8],
        value: &[u8],
    ) -> Result<bool> {
        self.mutate_view_duplicate(view, key, crate::duplicates::Mutation::Delete(value))
            .map(|n| n != 0)
    }
    pub fn view_delete_duplicates(&mut self, view: &DatabaseView, key: &[u8]) -> Result<u64> {
        self.mutate_view_duplicate(view, key, crate::duplicates::Mutation::DeleteAll)
    }
    pub fn view_clear(&mut self, view: &DatabaseView) -> Result<()> {
        let (old, _) = self.writable_view(view)?;
        let revision = self.arena.revision();
        self.update_view(view, old, revision, Reference::NULL);
        Ok(())
    }
    /// Catalog removal intentionally supersedes publication of this identity.
    /// Existing external caps retain readable roots; no live ID means no write
    /// authority. Dead records may now prune instead of retaining dirty history.
    pub(crate) fn remove_database_view_identity(&mut self, id: u64) {
        if let Some(registry) = &mut self.database_views {
            for record in registry.records.values_mut() {
                if record
                    .authority
                    .upgrade()
                    .is_some_and(|authority| authority.handle.id() == id)
                {
                    record.dirty_owner = None;
                    record.retained = None;
                }
            }
        }
    }
    pub(crate) fn ensure_database_views_selected(&self) -> Result<()> {
        if self.database_views.is_none() {
            return Ok(());
        }
        self.view_live()?;
        if self.has_pending_retained_groups() {
            return Err(Error::InvalidInput("retained groups require finalization"));
        }
        if self.database_views.as_ref().is_some_and(|r| {
            r.records.values().any(|record| {
                record.dirty()
                    && record.dirty_owner.as_ref().is_some_and(|a| {
                        self.catalog
                            .get(a.handle.name())
                            .is_some_and(|db| db.id == a.handle.id())
                    })
            })
        }) {
            return Err(Error::InvalidInput(
                "dirty database views require selection",
            ));
        }
        Ok(())
    }
    pub fn select_database_views(&mut self, winners: &[&DatabaseView]) -> Result<()> {
        self.view_live()?;
        if self.has_pending_retained_groups() {
            return Err(Error::InvalidInput(
                "retained groups require batch finalization",
            ));
        }
        let mut chosen = BTreeMap::new();
        for view in winners {
            let (root, _) = self.writable_view(view)?;
            if chosen
                .insert(
                    view.handle().id(),
                    (view.handle().clone(), root, view.ordering()),
                )
                .is_some()
            {
                return Err(Error::InvalidInput("duplicate database view winner"));
            }
        }
        let dirty: BTreeSet<_> = self
            .database_views
            .as_ref()
            .map(|r| {
                r.records
                    .values()
                    .filter_map(|r| r.dirty_owner.as_ref())
                    .filter(|a| {
                        self.catalog
                            .get(a.handle.name())
                            .is_some_and(|db| db.id == a.handle.id())
                    })
                    .map(|a| a.handle.id())
                    .collect()
            })
            .unwrap_or_default();
        if dirty.iter().any(|id| !chosen.contains_key(id)) {
            return Err(Error::InvalidInput("missing dirty database view winner"));
        }
        // All fallible validation/allocation precedes any root assignment.
        for (handle, root, _) in chosen.values() {
            if self.catalog.get(handle.name()).unwrap().root != *root
                || self.catalog.get(handle.name()).unwrap().ordering != chosen[&handle.id()].2
            {
                self.mark_changed(handle.name());
            }
        }
        for (handle, root, _) in chosen.values() {
            let db = self.catalog.get_mut(handle.name()).unwrap();
            db.root = *root;
            db.ordering = chosen[&handle.id()].2;
        }
        if let Some(registry) = &mut self.database_views {
            registry.records.retain(|_, r| {
                r.authority
                    .upgrade()
                    .is_some_and(|a| !chosen.contains_key(&a.handle.id()))
            });
        }
        Ok(())
    }
}
fn get<'a, S: ulmdb_core::storage::ReadStore>(
    store: &'a S,
    root: Reference,
    kind: DatabaseKind,
    key: &[u8],
    keys: &crate::ordering::BoundComparator,
    values: &crate::ordering::BoundComparator,
) -> Result<Option<&'a [u8]>> {
    if key.len() > store.max_key_size() {
        return Err(Error::InvalidInput("key size limit exceeded"));
    }
    match kind {
        DatabaseKind::Kv => ulmdb_kv::get_with(store, root, key, keys),
        DatabaseKind::Duplicates { .. } => crate::duplicates::seek_pair_with(
            store,
            root,
            kind,
            Some((key, None)),
            true,
            false,
            keys,
            values,
        )
        .map(|pair| {
            pair.filter(|(stored, _)| {
                ulmdb_kv::ordering::Comparator::compare(keys, stored, key).is_eq()
            })
            .map(|(_, v)| v)
        }),
        _ => Err(Error::WrongKind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_creation_proof_is_rejected_and_rollback_does_not_reuse_view_ids() {
        let path = std::env::temp_dir().join(format!(
            "database-view-proof-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let env = crate::Environment::create(&path).unwrap();
        let mut writer = env.begin_write().unwrap();
        writer.create_database(b"new", DatabaseKind::Kv).unwrap();
        let handle = writer.open_database(b"new").unwrap();
        assert!(writer.created_database_ids.remove(&handle.id()));
        assert!(matches!(
            writer.open_database_view(&handle),
            Err(Error::StaleHandle)
        ));
        {
            let scope = writer.database_read_scope().unwrap();
            assert!(matches!(
                scope.open_database_view(&handle),
                Err(Error::StaleHandle)
            ));
        }
        writer.created_database_ids.insert(handle.id());
        let old_id;
        {
            let mut child = writer.savepoint().unwrap();
            let first = child.open_database_view(&handle).unwrap();
            old_id = first.authority.id;
            child.rollback().unwrap();
            let next = child.open_database_view(&handle).unwrap();
            assert!(next.authority.id > old_id);
            assert!(matches!(
                child.view_get(&first, b"k"),
                Err(Error::StaleHandle)
            ));
            child.abort().unwrap();
        }
        writer.abort();
        drop(env);
        std::fs::remove_dir_all(path).unwrap();
    }
}

fn count<S: ulmdb_core::storage::ReadStore>(
    store: &S,
    root: Reference,
    kind: DatabaseKind,
    key: &[u8],
    keys: &crate::ordering::BoundComparator,
    values: &crate::ordering::BoundComparator,
) -> Result<u64> {
    let DatabaseKind::Duplicates { width } = kind else {
        return Err(Error::WrongKind);
    };
    Ok(
        crate::duplicates::bucket::Bucket::get_with(store, root, key, width, keys, values)?
            .map_or(0, |b| b.count()),
    )
}

impl WriteTxn<'_> {
    pub fn configure_empty_database_view(
        &mut self,
        view: &DatabaseView,
        ordering: crate::ordering::DatabaseOrdering,
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<DatabaseView> {
        self.view_live()?;
        self.next_view_id
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        let (root, kind) = self.writable_view(view)?;
        ordering.validate(kind)?;
        let prior = self.view_record(view)?;
        if !root.is_null() || prior.retained.is_some() {
            return Err(Error::InvalidInput(
                "ordering change requires empty unretained view",
            ));
        }
        let mut record = prior.clone();
        let keys =
            crate::ordering::BoundComparator::deferred(ordering.keys, callbacks.keys.as_ref())?;
        let values = crate::ordering::BoundComparator::deferred(
            ordering.duplicates,
            callbacks.duplicates.as_ref(),
        )?;
        let id = self.next_view_id;
        let next = id.checked_add(1).ok_or(Error::LimitExceeded)?;
        let authority = Arc::new(Authority {
            id,
            handle: view.handle().clone(),
            origin: view.origin(),
            ordering,
            keys,
            values,
        });
        record.authority = Arc::downgrade(&authority);
        if record.dirty() {
            record.dirty_owner = Some(authority.clone());
        }
        let registry = self.database_views.as_mut().unwrap();
        registry.records.remove(&view.authority.id);
        registry.records.insert(id, record);
        self.next_view_id = next;
        Ok(DatabaseView { authority })
    }
    fn callback_orders(
        &self,
        view: &DatabaseView,
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<(
        crate::ordering::BoundComparator,
        crate::ordering::BoundComparator,
    )> {
        self.view_record(view)?;
        let spec = view.ordering();
        Ok((
            crate::ordering::BoundComparator::bind(spec.keys, callbacks.keys.as_ref())?,
            crate::ordering::BoundComparator::bind(spec.duplicates, callbacks.duplicates.as_ref())?,
        ))
    }
    pub fn view_get_with_ordering(
        &self,
        view: &DatabaseView,
        key: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<Option<&[u8]>> {
        let (keys, values) = self.callback_orders(view, callbacks)?;
        let r = self.view_record(view)?;
        get(&self.arena, r.root, r.kind, key, &keys, &values)
    }
    pub fn view_seek_pair_with_ordering(
        &self,
        view: &DatabaseView,
        bound: Option<(&[u8], Option<&[u8]>)>,
        inclusive: bool,
        reverse: bool,
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<Option<(&[u8], &[u8])>> {
        let (keys, values) = self.callback_orders(view, callbacks)?;
        let r = self.view_record(view)?;
        crate::duplicates::seek_pair_with(
            &self.arena,
            r.root,
            r.kind,
            bound,
            inclusive,
            reverse,
            &keys,
            &values,
        )
    }
    pub fn view_put_with_ordering(
        &mut self,
        view: &DatabaseView,
        key: &[u8],
        value: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<()> {
        let (keys, _) = self.callback_orders(view, callbacks)?;
        let (old, kind) = self.writable_view(view)?;
        if kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if key.len() > 4096 || value.len() > self.writable_value_limit() {
            return Err(Error::InvalidInput("KV size limit"));
        }
        use ulmdb_kv::ordering::Comparator;
        keys.validate(key)?;
        self.enable_value_limit(value.len())?;
        let revision = self.arena.revision();
        self.failed = true;
        let root = ulmdb_kv::put_with(&mut self.arena, old, key, value, &keys)?;
        self.update_view(view, old, revision, root);
        self.failed = false;
        Ok(())
    }
    pub fn view_delete_with_ordering(
        &mut self,
        view: &DatabaseView,
        key: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<()> {
        let (keys, _) = self.callback_orders(view, callbacks)?;
        let (old, kind) = self.writable_view(view)?;
        if kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if key.len() > 4096 {
            return Err(Error::InvalidInput("key size limit"));
        }
        use ulmdb_kv::ordering::Comparator;
        keys.validate(key)?;
        let revision = self.arena.revision();
        self.failed = true;
        let root = ulmdb_kv::delete_with(&mut self.arena, old, key, &keys)?;
        self.update_view(view, old, revision, root);
        self.failed = false;
        Ok(())
    }
    pub fn view_mutate_duplicate_with_ordering(
        &mut self,
        view: &DatabaseView,
        key: &[u8],
        value: Option<&[u8]>,
        delete: bool,
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<u64> {
        let (keys, values) = self.callback_orders(view, callbacks)?;
        let (old, kind) = self.writable_view(view)?;
        let mutation = match (value, delete) {
            (Some(v), false) => crate::duplicates::Mutation::Insert(v),
            (Some(v), true) => crate::duplicates::Mutation::Delete(v),
            (None, true) => crate::duplicates::Mutation::DeleteAll,
            _ => return Err(Error::InvalidInput("duplicate operation")),
        };
        let revision = self.arena.revision();
        self.failed = true;
        let (root, n) = crate::duplicates::ordered_write::mutate_root_with(
            &mut self.arena,
            old,
            kind,
            key,
            mutation,
            &keys,
            &values,
        )?;
        self.update_view(view, old, revision, root);
        self.failed = false;
        Ok(n)
    }
}

impl WriteTxn<'_> {
    pub fn view_structural_resume_pair(
        &self,
        view: &DatabaseView,
        after: Option<(&[u8], &[u8])>,
    ) -> Result<Option<(&[u8], &[u8])>> {
        let r = self.view_record(view)?;
        crate::duplicates::structural::resume(&self.arena, r.root, r.kind, after)
    }
}

impl WriteTxn<'_> {
    pub fn view_duplicate_count_with_ordering(
        &self,
        view: &DatabaseView,
        key: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<u64> {
        let (keys, values) = self.callback_orders(view, callbacks)?;
        let r = self.view_record(view)?;
        count(&self.arena, r.root, r.kind, key, &keys, &values)
    }
}

impl WriteTxn<'_> {
    /// Statistics observe the exact loaded alias branch, including retained roots.
    pub fn view_database_stats(&self, view: &DatabaseView) -> Result<crate::DatabaseStats> {
        let r = self.view_record(view)?;
        crate::physical_stats::measure(
            &self.arena,
            r.root,
            r.kind,
            view.ordering(),
            self.physical_budget()?,
        )
    }
}
