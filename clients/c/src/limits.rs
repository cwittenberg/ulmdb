//! Adapter map-capacity admission, separate from native allocation policies.
use super::*;
impl MDB_txn {
    fn logical_size(&self) -> FfiResult<usize> {
        let mut size = 8192usize;
        for name in self.database_names()? {
            if name != MAIN && !names::is_alias(&name) && name.first().is_some_and(|b| *b == 0) {
                continue;
            }
            let h = self.handle_by_name(&name)?;
            if !matches!(h.kind(), DatabaseKind::Kv | DatabaseKind::Duplicates { .. }) {
                continue;
            }
            let name_bytes = if names::is_alias(&name) {
                self.external_database_name(&name)?.ok_or(-30796)?.len()
            } else {
                name.len()
            };
            size = size.checked_add(name_bytes + 48).ok_or(-30792)?;
            let logical = match self.state.as_ref() {
                Some(State::Read(r)) => r.logical_database_size(&name),
                Some(State::PrivateRead(r)) => r.logical_database_size(&name),
                Some(State::Write(w)) => w.logical_database_size(&name),
                Some(State::Child(w)) => w.logical_database_size(&name),
                Some(State::Prepared(_)) | None => return Err(BAD_TXN),
            }
            .map_err(code)?;
            let bytes =
                usize::try_from(logical.tuple_bytes().map_err(code)?).map_err(|_| -30792)?;
            size = size.checked_add(bytes).ok_or(-30792)?;
        }
        Ok(size)
    }
    /// Pure quota projection for a catalog create. Install the cached total
    /// only after every fallible alias registration step succeeds.
    pub(super) fn quota_project(&mut self, added: usize, removed: usize) -> FfiResult<usize> {
        let result = (|| {
            let used = match self.quota_used {
                Some(size) => size,
                None => self.logical_size()?,
            };
            let next = used
                .saturating_sub(removed)
                .checked_add(added)
                .ok_or(-30792)?;
            let limit = unsafe { get_ref(self.env)? }.mapsize;
            if next > limit {
                return Err(-30792);
            }
            Ok(next)
        })();
        if matches!(result, Err(-30792)) {
            self.capacity_failed = true;
        }
        result
    }
    pub(super) fn quota_adjust(&mut self, added: usize, removed: usize) -> FfiResult<()> {
        self.quota_used = Some(self.quota_project(added, removed)?);
        Ok(())
    }
    /// Reconcile the actual winning native roots before any publication.
    /// Creation-only transactions and retained alias transfers cannot bypass
    /// mapsize merely because no ordinary value put was required.
    pub(super) fn admit_map_capacity(&mut self) -> FfiResult<()> {
        let result = self.logical_size();
        let used = match result {
            Ok(used) => used,
            Err(error) => {
                if error == -30792 {
                    self.capacity_failed = true;
                }
                return Err(error);
            }
        };
        if used > unsafe { get_ref(self.env)? }.mapsize {
            self.capacity_failed = true;
            return Err(-30792);
        }
        self.quota_used = Some(used);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
