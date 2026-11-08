//! Every public mutation names its logical binding and genuine native view.
//! These methods preserve the existing parent/private-reader mutation gate.
use super::*;

impl MDB_txn {
    pub(super) fn check_file_value_size(&self, size: usize) -> FfiResult<()> {
        if size > ulmdb::ValueLimit::platform_maximum().get() {
            return Err(BAD_VALSIZE);
        }
        Ok(())
    }
    fn prepare_file_value_size(&mut self, size: usize) -> FfiResult<()> {
        self.check_file_value_size(size)?;
        if size > ulmdb::ValueLimit::EXTENDED.get() {
            let capacity = unsafe { (*self.env).mapsize };
            if size.checked_add(65536).is_none_or(|n| n > capacity) {
                return Err(-30792);
            }
        }
        if size > ulmdb::ValueLimit::EXTENDED.get() {
            self.writer()?
                .set_value_limit(if size > ulmdb::ValueLimit::LARGE.get() {
                    ulmdb::ValueLimit::MAXIMUM
                } else {
                    ulmdb::ValueLimit::LARGE
                })
                .map_err(code)?;
        }
        Ok(())
    }
    fn retained_alias(&self, h: &ResolvedAlias) -> FfiResult<Option<ulmdb::RetainedDatabaseView>> {
        let group = self
            .dbis
            .lock()
            .map_err(|_| BAD_TXN)?
            .retained
            .get(&h.native_id())
            .cloned();
        let Some(group) = group else { return Ok(None) };
        let DataView::Writable(view) = &h.binding.view else {
            return Err(BAD_TXN);
        };
        let retained = match self.state.as_ref() {
            Some(State::Write(w)) => w.retained_database_view(&group, view),
            Some(State::Child(w)) => w.retained_database_view(&group, view),
            _ => return Err(self.shared.readonly_error()),
        }
        .map_err(code)?;
        Ok(Some(retained))
    }
    pub(super) fn reserve_alias(
        &mut self,
        h: &ResolvedAlias,
        key: &[u8],
        length: usize,
    ) -> FfiResult<ulmdb::ValueReservation> {
        self.writable_database(h)?;
        self.prepare_file_value_size(length)?;
        let callbacks = self.bound_ordering_callbacks(h)?;
        if let Some(retained) = self.retained_alias(h)? {
            return self
                .writer()?
                .retained_view_reserve_with_ordering(&retained, key, length, &callbacks)
                .map_err(code);
        }
        match &h.binding.view {
            DataView::Writable(view) => self
                .writer()?
                .view_reserve_with_ordering(view, key, length, &callbacks),
            DataView::Direct => {
                self.writer()?
                    .reserve_handle_with_ordering(h.actual(), key, length, &callbacks)
            }
            DataView::Readonly(_) => return Err(self.shared.readonly_error()),
        }
        .map_err(code)
    }
    pub(super) fn put_alias(
        &mut self,
        h: &ResolvedAlias,
        key: &[u8],
        value: &[u8],
    ) -> FfiResult<()> {
        self.writable_database(h)?;
        self.prepare_file_value_size(value.len())?;
        let callbacks = self.bound_ordering_callbacks(h)?;
        if let Some(retained) = self.retained_alias(h)? {
            return self
                .writer()?
                .retained_view_put_with_ordering(&retained, key, value, &callbacks)
                .map_err(code);
        }
        match &h.binding.view {
            DataView::Writable(view) => self
                .writer()?
                .view_put_with_ordering(view, key, value, &callbacks),
            DataView::Direct => {
                self.writer()?
                    .put_with_ordering(h.actual().name(), key, value, &callbacks)
            }
            DataView::Readonly(_) => return Err(self.shared.readonly_error()),
        }
        .map_err(code)
    }
    pub(super) fn delete_alias(&mut self, h: &ResolvedAlias, key: &[u8]) -> FfiResult<()> {
        self.writable_database(h)?;
        let callbacks = self.bound_ordering_callbacks(h)?;
        if let Some(retained) = self.retained_alias(h)? {
            return self
                .writer()?
                .retained_view_delete_with_ordering(&retained, key, &callbacks)
                .map_err(code);
        }
        match &h.binding.view {
            DataView::Writable(view) => self
                .writer()?
                .view_delete_with_ordering(view, key, &callbacks),
            DataView::Direct => {
                self.writer()?
                    .delete_with_ordering(h.actual().name(), key, &callbacks)
            }
            DataView::Readonly(_) => return Err(self.shared.readonly_error()),
        }
        .map_err(code)
    }
    pub(super) fn insert_alias_duplicate(
        &mut self,
        h: &ResolvedAlias,
        key: &[u8],
        value: &[u8],
    ) -> FfiResult<bool> {
        self.writable_database(h)?;
        let callbacks = self.bound_ordering_callbacks(h)?;
        if let Some(retained) = self.retained_alias(h)? {
            return self
                .writer()?
                .retained_view_mutate_duplicate_with_ordering(
                    &retained,
                    key,
                    Some(value),
                    false,
                    &callbacks,
                )
                .map(|n| n != 0)
                .map_err(code);
        }
        match &h.binding.view {
            DataView::Writable(view) => self
                .writer()?
                .view_mutate_duplicate_with_ordering(view, key, Some(value), false, &callbacks)
                .map(|n| n != 0),
            DataView::Direct => self.writer()?.insert_duplicate_with_ordering(
                h.actual().name(),
                key,
                value,
                &callbacks,
            ),
            DataView::Readonly(_) => return Err(self.shared.readonly_error()),
        }
        .map_err(code)
    }
    pub(super) fn delete_alias_duplicate(
        &mut self,
        h: &ResolvedAlias,
        key: &[u8],
        value: &[u8],
    ) -> FfiResult<bool> {
        self.writable_database(h)?;
        let callbacks = self.bound_ordering_callbacks(h)?;
        if let Some(retained) = self.retained_alias(h)? {
            return self
                .writer()?
                .retained_view_mutate_duplicate_with_ordering(
                    &retained,
                    key,
                    Some(value),
                    true,
                    &callbacks,
                )
                .map(|n| n != 0)
                .map_err(code);
        }
        match &h.binding.view {
            DataView::Writable(view) => self
                .writer()?
                .view_mutate_duplicate_with_ordering(view, key, Some(value), true, &callbacks)
                .map(|n| n != 0),
            DataView::Direct => {
                self.check_native_actual(h.actual()).map_err(code)?;
                self.writer()?.delete_duplicate_with_ordering(
                    h.actual().name(),
                    key,
                    value,
                    &callbacks,
                )
            }
            DataView::Readonly(_) => return Err(self.shared.readonly_error()),
        }
        .map_err(code)
    }
    pub(super) fn delete_alias_duplicates(
        &mut self,
        h: &ResolvedAlias,
        key: &[u8],
    ) -> FfiResult<u64> {
        self.writable_database(h)?;
        let callbacks = self.bound_ordering_callbacks(h)?;
        if let Some(retained) = self.retained_alias(h)? {
            return self
                .writer()?
                .retained_view_mutate_duplicate_with_ordering(
                    &retained, key, None, true, &callbacks,
                )
                .map_err(code);
        }
        match &h.binding.view {
            DataView::Writable(view) => self
                .writer()?
                .view_mutate_duplicate_with_ordering(view, key, None, true, &callbacks),
            DataView::Direct => {
                self.check_native_actual(h.actual()).map_err(code)?;
                self.writer()?
                    .delete_duplicates_with_ordering(h.actual().name(), key, &callbacks)
            }
            DataView::Readonly(_) => return Err(self.shared.readonly_error()),
        }
        .map_err(code)
    }
    pub(super) fn count_alias_duplicates(&self, h: &ResolvedAlias, key: &[u8]) -> FfiResult<u64> {
        let callbacks = self.bound_ordering_callbacks(h)?;
        match (&h.binding.view, self.state.as_ref()) {
            (DataView::Writable(view), Some(State::Write(w))) => {
                w.view_duplicate_count_with_ordering(view, key, &callbacks)
            }
            (DataView::Writable(view), Some(State::Child(w))) => {
                w.view_duplicate_count_with_ordering(view, key, &callbacks)
            }
            (DataView::Readonly(view), Some(State::PrivateRead(r))) => r
                .read_scope()
                .duplicate_count_with_ordering(view, key, &callbacks),
            (DataView::Direct, _) => {
                self.check_native_actual(h.actual()).map_err(code)?;
                match self.state.as_ref() {
                    Some(State::Read(r)) => r
                        .duplicates_with_ordering(h.actual().name(), &callbacks)
                        .and_then(|d| d.count(key)),
                    Some(State::PrivateRead(r)) => {
                        r.duplicate_count_with_ordering(h.actual().name(), key, &callbacks)
                    }
                    Some(State::Write(w)) => {
                        w.duplicate_count_with_ordering(h.actual().name(), key, &callbacks)
                    }
                    Some(State::Child(w)) => {
                        w.duplicate_count_with_ordering(h.actual().name(), key, &callbacks)
                    }
                    _ => return Err(BAD_TXN),
                }
            }
            _ => return Err(BAD_TXN),
        }
        .map_err(code)
    }
    pub(super) fn clear_alias(&mut self, h: &ResolvedAlias) -> FfiResult<()> {
        self.live()?;
        if self.readonly {
            return Err(self.shared.readonly_error());
        }
        if let Some(retained) = self.retained_alias(h)? {
            return self.writer()?.retained_view_clear(&retained).map_err(code);
        }
        self.check_native_actual(h.actual()).map_err(code)?;
        match &h.binding.view {
            DataView::Writable(view) => self.writer()?.view_clear(view).map_err(code),
            DataView::Readonly(_) => Err(self.shared.readonly_error()),
            DataView::Direct => {
                loop {
                    let next = self
                        .seek_database_pair(h, None, false, false)
                        .map_err(code)?
                        .map(|(k, _)| k.to_vec());
                    let Some(key) = next else { break };
                    match h.kind() {
                        DatabaseKind::Kv => self.delete_alias(h, &key)?,
                        DatabaseKind::Duplicates { .. } => {
                            self.delete_alias_duplicates(h, &key)?;
                        }
                        _ => return Err(INCOMPATIBLE),
                    }
                }
                Ok(())
            }
        }
    }
}
