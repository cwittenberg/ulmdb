//! Explicit removal authority. Ordinary database/view liveness is unchanged.
use super::*;

struct GroupAuthority {
    id: u64,
    original: DatabaseHandle,
}
#[derive(Clone)]
pub(super) struct GroupRecord {
    authority: Weak<GroupAuthority>,
    dirty_owner: Option<Arc<GroupAuthority>>,
}
impl GroupRecord {
    pub(super) fn is_owned(&self) -> bool {
        self.authority.strong_count() != 0
    }
    pub(super) fn retain_dirty(&mut self) {
        if self.dirty_owner.is_none() {
            self.dirty_owner = self.authority.upgrade();
            debug_assert!(self.dirty_owner.is_some());
        }
    }
}
/// Writer-local authority over explicitly retained, already-issued branches.
#[derive(Clone)]
pub struct RetainedViewGroup {
    authority: Arc<GroupAuthority>,
}
impl RetainedViewGroup {
    pub fn original_handle(&self) -> &DatabaseHandle {
        &self.authority.original
    }
}
/// A wrapper around one existing branch, never a new branch or root seed.
#[derive(Clone)]
pub struct RetainedDatabaseView {
    group: RetainedViewGroup,
    prior: DatabaseView,
}
impl RetainedDatabaseView {
    pub fn as_database_view(&self) -> &DatabaseView {
        &self.prior
    }
    pub fn group(&self) -> &RetainedViewGroup {
        &self.group
    }
}
pub struct DatabaseViewTransfer<'a> {
    pub source: &'a RetainedDatabaseView,
    pub destination: &'a DatabaseHandle,
}
impl WriteTxn<'_> {
    fn check_retained_group(&self, group: &RetainedViewGroup) -> Result<()> {
        self.view_live()?;
        self.database_views
            .as_ref()
            .and_then(|r| r.groups.get(&group.authority.id))
            .filter(|r| r.authority.ptr_eq(&Arc::downgrade(&group.authority)))
            .ok_or(Error::StaleHandle)?;
        Ok(())
    }
    fn retained_root(&self, view: &RetainedDatabaseView) -> Result<(Reference, DatabaseKind)> {
        self.check_retained_group(&view.group)?;
        let record = self.view_record(&view.prior)?;
        if record.retained != Some(view.group.authority.id)
            || view.prior.handle() != view.group.original_handle()
        {
            return Err(Error::StaleHandle);
        }
        Ok((record.root, record.kind))
    }
    pub(super) fn has_pending_retained_groups(&self) -> bool {
        self.database_views.as_ref().is_some_and(|registry| {
            registry.records.values().any(|record| {
                record.retained.is_some_and(|id| {
                    (record.dirty() || record.authority.strong_count() != 0)
                        && registry
                            .groups
                            .get(&id)
                            .is_some_and(|group| group.authority.strong_count() != 0)
                })
            })
        })
    }
    /// Atomically unlink a live catalog identity and retain only explicitly
    /// supplied, already-issued branches. Omitted dirty branches are rejected.
    pub fn retain_database_views(
        &mut self,
        actual: &DatabaseHandle,
        existing: &[&DatabaseView],
    ) -> Result<RetainedViewGroup> {
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
        if existing.is_empty() {
            return Err(Error::InvalidInput("empty retained membership"));
        }
        let next = self
            .next_retained_id
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        let mut members = BTreeSet::new();
        for view in existing {
            let record = self.view_record(view)?;
            if view.handle() != actual || record.retained.is_some() {
                return Err(Error::StaleHandle);
            }
            if !members.insert(view.authority.id) {
                return Err(Error::InvalidInput("duplicate retained member"));
            }
        }
        let registry = self.database_views.as_ref().unwrap();
        if registry.records.iter().any(|(id, record)| {
            record
                .dirty_owner
                .as_ref()
                .is_some_and(|a| a.handle == *actual)
                && !members.contains(id)
        }) {
            return Err(Error::InvalidInput("missing dirty retained member"));
        }
        let dirty = members.iter().any(|id| registry.records[id].dirty());
        let authority = Arc::new(GroupAuthority {
            id: self.next_retained_id,
            original: actual.clone(),
        });
        // All recoverable checks and plan allocations precede catalog removal.
        self.mark_changed(actual.name());
        self.catalog.remove(actual.name()).unwrap();
        self.created_database_ids.remove(&actual.id());
        let registry = self.database_views.as_mut().unwrap();
        for id in members {
            registry.records.get_mut(&id).unwrap().retained = Some(authority.id);
        }
        registry.groups.insert(
            authority.id,
            GroupRecord {
                authority: Arc::downgrade(&authority),
                dirty_owner: dirty.then(|| authority.clone()),
            },
        );
        self.next_retained_id = next;
        Ok(RetainedViewGroup { authority })
    }
    pub fn retained_database_view(
        &self,
        group: &RetainedViewGroup,
        prior: &DatabaseView,
    ) -> Result<RetainedDatabaseView> {
        let wrapper = RetainedDatabaseView {
            group: group.clone(),
            prior: prior.clone(),
        };
        self.retained_root(&wrapper)?;
        Ok(wrapper)
    }
    pub(crate) fn retained_reserve_root(
        &self,
        view: &RetainedDatabaseView,
    ) -> Result<(Reference, DatabaseKind)> {
        self.retained_root(view)
    }
    pub fn retained_view_put(
        &mut self,
        view: &RetainedDatabaseView,
        key: &[u8],
        value: &[u8],
    ) -> Result<()> {
        let checked = self.retained_root(view)?;
        self.put_view_checked(&view.prior, checked, key, value)
    }
    pub fn retained_view_delete(&mut self, view: &RetainedDatabaseView, key: &[u8]) -> Result<()> {
        let checked = self.retained_root(view)?;
        self.delete_view_checked(&view.prior, checked, key)
    }
    pub fn retained_view_insert_duplicate(
        &mut self,
        view: &RetainedDatabaseView,
        key: &[u8],
        value: &[u8],
    ) -> Result<bool> {
        let checked = self.retained_root(view)?;
        self.duplicate_view_checked(
            &view.prior,
            checked,
            key,
            crate::duplicates::Mutation::Insert(value),
        )
        .map(|n| n != 0)
    }
    pub fn retained_view_delete_duplicate(
        &mut self,
        view: &RetainedDatabaseView,
        key: &[u8],
        value: &[u8],
    ) -> Result<bool> {
        let checked = self.retained_root(view)?;
        self.duplicate_view_checked(
            &view.prior,
            checked,
            key,
            crate::duplicates::Mutation::Delete(value),
        )
        .map(|n| n != 0)
    }
    pub fn retained_view_delete_duplicates(
        &mut self,
        view: &RetainedDatabaseView,
        key: &[u8],
    ) -> Result<u64> {
        let checked = self.retained_root(view)?;
        self.duplicate_view_checked(
            &view.prior,
            checked,
            key,
            crate::duplicates::Mutation::DeleteAll,
        )
    }
    pub fn retained_view_clear(&mut self, view: &RetainedDatabaseView) -> Result<()> {
        let (old, _) = self.retained_root(view)?;
        self.update_view(&view.prior, old, self.arena.revision(), Reference::NULL);
        Ok(())
    }
    /// Select all live/retained dirty groups in one validated metadata step.
    /// Destinations are genuine current writer-created identities of the same
    /// kind. Every overwritten destination and discarded group is explicit.
    pub fn finalize_database_view_groups(
        &mut self,
        live_winners: &[&DatabaseView],
        transfers: &[DatabaseViewTransfer<'_>],
        superseded_retained: &[&RetainedViewGroup],
        overwritten_destinations: &[&DatabaseHandle],
    ) -> Result<()> {
        self.view_live()?;
        let mut chosen = BTreeMap::new();
        let mut consumed_groups = BTreeSet::new();
        let mut consumed_ids = BTreeSet::new();
        let mut overwrites = BTreeMap::new();
        for handle in overwritten_destinations {
            if self.open_database(handle.name())? != **handle {
                return Err(Error::StaleHandle);
            }
            if overwrites.insert(handle.id(), *handle).is_some() {
                return Err(Error::InvalidInput("duplicate overwrite authority"));
            }
        }
        for view in live_winners {
            let (root, _) = self.writable_view(view)?;
            if chosen
                .insert(
                    view.handle().id(),
                    (view.handle().clone(), root, view.ordering()),
                )
                .is_some()
            {
                return Err(Error::InvalidInput("duplicate destination"));
            }
            consumed_ids.insert(view.handle().id());
        }
        for transfer in transfers {
            let (root, kind) = self.retained_root(transfer.source)?;

            let dest = transfer.destination;
            if self.open_database(dest.name())? != *dest
                || !self.created_database_ids.contains(&dest.id())
            {
                return Err(Error::StaleHandle);
            }
            if dest.kind() != kind {
                return Err(Error::WrongKind);
            }
            if overwrites.remove(&dest.id()) != Some(dest) {
                return Err(Error::InvalidInput("missing overwrite authority"));
            }
            if chosen
                .insert(
                    dest.id(),
                    (dest.clone(), root, transfer.source.prior.ordering()),
                )
                .is_some()
            {
                return Err(Error::InvalidInput("duplicate destination"));
            }
            if !consumed_groups.insert(transfer.source.group.authority.id) {
                return Err(Error::InvalidInput("retained source fanout"));
            }
            consumed_ids.insert(dest.id());
            consumed_ids.insert(transfer.source.group.original_handle().id());
        }
        if !overwrites.is_empty() {
            return Err(Error::InvalidInput("unused overwrite authority"));
        }
        for group in superseded_retained {
            self.check_retained_group(group)?;
            if !consumed_groups.insert(group.authority.id) {
                return Err(Error::InvalidInput("contradictory retained disposition"));
            }
            consumed_ids.insert(group.original_handle().id());
        }
        if let Some(registry) = &self.database_views {
            for record in registry.records.values() {
                if let Some(group) = record.retained {
                    let pending = (record.dirty() || record.authority.strong_count() != 0)
                        && registry
                            .groups
                            .get(&group)
                            .is_some_and(|g| g.authority.strong_count() != 0);
                    if pending && !consumed_groups.contains(&group) {
                        return Err(Error::InvalidInput("missing retained disposition"));
                    }
                } else if let Some(authority) = &record.dirty_owner {
                    if self
                        .catalog
                        .get(authority.handle.name())
                        .is_some_and(|db| db.id == authority.handle.id())
                        && !chosen.contains_key(&authority.handle.id())
                    {
                        return Err(Error::InvalidInput("missing dirty live winner"));
                    }
                }
            }
        }
        // All recoverable errors are above. Revoke every consumed sibling
        // before exposing transferred private pages to ordinary mutation.
        // A retained transfer can move an unprocessed old tree into an already
        // evacuated catalog name. Restart the per-database rewrite frontier;
        // monotonic segment allocation alone cannot certify this region move.
        if !transfers.is_empty() { self.env.maintenance_reset_rewrite(); }
        for (handle, root, ordering) in chosen.values() {
            if self.catalog.get(handle.name()).unwrap().root != *root
                || self.catalog.get(handle.name()).unwrap().ordering != *ordering
            {
                self.mark_changed(handle.name());
            }
        }
        if let Some(registry) = &mut self.database_views {
            registry.records.retain(|_, record| {
                record
                    .authority
                    .upgrade()
                    .is_some_and(|a| !consumed_ids.contains(&a.handle.id()))
            });
            registry
                .groups
                .retain(|id, _| !consumed_groups.contains(id));
        }
        for (handle, root, ordering) in chosen.values() {
            let db = self.catalog.get_mut(handle.name()).unwrap();
            db.root = *root;
            db.ordering = *ordering;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn group_ids_burn_and_failed_provenance_or_forgery_preserves_authority() {
        let path = std::env::temp_dir().join(format!(
            "retained-proof-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let env = crate::Environment::create(&path).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"A", DatabaseKind::Kv).unwrap();
        let a = w.open_database(b"A").unwrap();
        let v = w.open_database_view(&a).unwrap();
        let old_counter = w.next_retained_id;
        w.next_retained_id = u64::MAX;
        assert!(matches!(
            w.retain_database_views(&a, &[&v]),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(w.open_database(b"A").unwrap(), a);
        assert!(w.view_get(&v, b"k").unwrap().is_none());
        w.next_retained_id = old_counter;
        let first;
        {
            let mut child = w.savepoint().unwrap();
            first = child.retain_database_views(&a, &[&v]).unwrap();
            child.abort().unwrap();
        }
        let second = w.retain_database_views(&a, &[&v]).unwrap();
        assert!(second.authority.id > first.authority.id);
        assert!(w.retained_database_view(&first, &v).is_err());
        let r = w.retained_database_view(&second, &v).unwrap();
        let forged = RetainedViewGroup {
            authority: Arc::new(GroupAuthority {
                id: second.authority.id,
                original: a.clone(),
            }),
        };
        assert!(w.retained_database_view(&forged, &v).is_err());
        let forged_v = DatabaseView {
            authority: Arc::new(Authority {
                id: v.authority.id,
                handle: a.clone(),
                origin: v.origin(),
                ordering: v.authority.ordering,
                keys: v.authority.keys.clone(),
                values: v.authority.values.clone(),
            }),
        };
        assert!(w.retained_database_view(&second, &forged_v).is_err());
        w.create_database(b"B", DatabaseKind::Kv).unwrap();
        let b = w.open_database(b"B").unwrap();
        w.created_database_ids.remove(&b.id());
        assert!(
            w.finalize_database_view_groups(
                &[],
                &[DatabaseViewTransfer {
                    source: &r,
                    destination: &b
                }],
                &[],
                &[&b]
            )
            .is_err()
        );
        w.retained_view_put(&r, b"k", b"still valid").unwrap();
        w.created_database_ids.insert(b.id());
        w.finalize_database_view_groups(
            &[],
            &[DatabaseViewTransfer {
                source: &r,
                destination: &b,
            }],
            &[],
            &[&b],
        )
        .unwrap();
        w.commit().unwrap();
        drop(env);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn clean_group_history_is_pruned_without_discarding_dirty_owners() {
        let path = std::env::temp_dir().join(format!(
            "retained-prune-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let env = crate::Environment::create(&path).unwrap();
        let mut w = env.begin_write().unwrap();
        for i in 0..(WriteTxn::MAX_DATABASE_VIEWS + 17) {
            w.create_database(b"A", DatabaseKind::Kv).unwrap();
            let h = w.open_database(b"A").unwrap();
            let v = w.open_database_view(&h).unwrap();
            let g = w.retain_database_views(&h, &[&v]).unwrap();
            assert_eq!(w.database_views.as_ref().unwrap().groups.len(), 1);
            assert_eq!(w.database_views.as_ref().unwrap().records.len(), 1);
            assert_eq!(w.next_retained_id, i as u64 + 2);
            drop(g);
            drop(v);
        }
        w.create_database(b"A", DatabaseKind::Kv).unwrap();
        let h = w.open_database(b"A").unwrap();
        let v = w.open_database_view(&h).unwrap();
        assert_eq!(w.database_views.as_ref().unwrap().groups.len(), 0);
        let g = w.retain_database_views(&h, &[&v]).unwrap();
        let r = w.retained_database_view(&g, &v).unwrap();
        w.retained_view_put(&r, b"k", b"dirty").unwrap();
        drop(r);
        drop(g);
        drop(v);
        assert!(w.has_pending_retained_groups());
        w.abort();
        drop(env);
        std::fs::remove_dir_all(path).unwrap();
    }
}

impl WriteTxn<'_> {
    pub fn retained_view_put_with_ordering(
        &mut self,
        view: &RetainedDatabaseView,
        key: &[u8],
        value: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<()> {
        let (old, kind) = self.retained_root(view)?;
        if kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        let (keys, _) = self.callback_orders(&view.prior, callbacks)?;
        use ulmdb_kv::ordering::Comparator;
        keys.validate(key)?;
        if key.len() > 4096 || value.len() > self.writable_value_limit() {
            return Err(Error::InvalidInput("KV size limit"));
        }
        self.enable_value_limit(value.len())?;
        let revision = self.arena.revision();
        self.failed = true;
        let root = ulmdb_kv::put_with(&mut self.arena, old, key, value, &keys)?;
        self.update_view(&view.prior, old, revision, root);
        self.failed = false;
        Ok(())
    }
    pub fn retained_view_delete_with_ordering(
        &mut self,
        view: &RetainedDatabaseView,
        key: &[u8],
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<()> {
        let (old, kind) = self.retained_root(view)?;
        if kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        let (keys, _) = self.callback_orders(&view.prior, callbacks)?;
        use ulmdb_kv::ordering::Comparator;
        keys.validate(key)?;
        if key.len() > 4096 {
            return Err(Error::InvalidInput("key size limit"));
        }
        let revision = self.arena.revision();
        self.failed = true;
        let root = ulmdb_kv::delete_with(&mut self.arena, old, key, &keys)?;
        self.update_view(&view.prior, old, revision, root);
        self.failed = false;
        Ok(())
    }
    pub fn retained_view_mutate_duplicate_with_ordering(
        &mut self,
        view: &RetainedDatabaseView,
        key: &[u8],
        value: Option<&[u8]>,
        delete: bool,
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<u64> {
        let (old, kind) = self.retained_root(view)?;
        let (keys, values) = self.callback_orders(&view.prior, callbacks)?;
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
        self.update_view(&view.prior, old, revision, root);
        self.failed = false;
        Ok(n)
    }
}
