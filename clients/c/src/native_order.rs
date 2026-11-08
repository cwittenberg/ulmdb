//! Native ordering admission. Old byte-ordered adapter files retain the legacy
//! scan strategy until their actual native descriptor matches this alias order.
use super::*;
use ulmdb::ordering::{DatabaseComparators, DatabaseOrdering, Endian, OrderingSpec, Width};
impl MDB_txn {
    pub(super) fn ordering_callbacks(
        &self,
        h: &impl LogicalAlias,
    ) -> FfiResult<DatabaseComparators> {
        let options = self.shared.options.lock().map_err(|_| BAD_TXN)?;
        Ok(options
            .get(&h.logical().id())
            .map(|o| DatabaseComparators {
                keys: o.native_key.clone(),
                duplicates: o.native_dup.clone(),
            })
            .unwrap_or_default())
    }
    /// An explicit MAIN reopen selects default callbacks even when its physical
    /// tree was built under an equivalent custom function. Supply that runtime
    /// default authority only for a persisted Custom axis; ordinary byte reads
    /// keep None and allocate no comparison object.
    pub(super) fn bound_ordering_callbacks(
        &self,
        h: &ResolvedAlias,
    ) -> FfiResult<DatabaseComparators> {
        self.bound_ordering_callbacks_with_settings(h, None)
    }
    /// An operation may reuse settings it just checked, provided no callback
    /// or mutation occurred since that observation. Global callbacks and actual
    /// Native ordering are still read afresh; no authority is cached here.
    pub(super) fn bound_ordering_callbacks_with_settings(
        &self,
        h: &ResolvedAlias,
        settings: Option<compat_features::Settings>,
    ) -> FfiResult<DatabaseComparators> {
        // Immutable view descriptors and coordinated-read catalog ordering
        // cannot perform I/O or invoke a transform callback. External and
        // writable Direct sources retain the original callback snapshot order.
        // Native built-in axes never consume runtime comparison callbacks.
        let view_order = match &h.binding.view {
            DataView::Writable(view) => Some(view.ordering()),
            DataView::Readonly(view) => Some(view.ordering()),
            DataView::Direct => match self.state.as_ref() {
                // With no external authority this query is only an immutable
                // catalog lookup: it cannot touch segments or run transforms.
                Some(State::Read(read)) if read.external_synchronization().is_none() => {
                    read.database_ordering(h.actual().name()).ok()
                }
                _ => None,
            },
        };
        if view_order.is_some_and(|order| {
            order.keys != OrderingSpec::Custom && order.duplicates != OrderingSpec::Custom
        }) {
            // Preserve the original poisoned-options error even though this
            // operation needs no callback snapshot or options lock.
            if self.shared.options.is_poisoned() {
                return Err(BAD_TXN);
            }
            if settings.is_none() {
                self.settings(h)?;
            }
            return Ok(DatabaseComparators::default());
        }
        let mut callbacks = self.ordering_callbacks(h)?;
        let actual = self.actual_native_order(h)?;
        let flags = match settings {
            Some(settings) => settings.flags,
            None => self.settings(h)?.flags,
        };
        if actual.keys == OrderingSpec::Custom && callbacks.keys.is_none() {
            callbacks.keys = Some(default_callback(flags, false));
        }
        if actual.duplicates == OrderingSpec::Custom && callbacks.duplicates.is_none() {
            callbacks.duplicates = Some(default_callback(flags, true));
        }
        Ok(callbacks)
    }
    pub(super) fn desired_native_order(
        &self,
        h: &impl LogicalAlias,
    ) -> FfiResult<Option<DatabaseOrdering>> {
        let settings = self.settings(h)?;
        let options = self.shared.options.lock().map_err(|_| BAD_TXN)?;
        let o = options.get(&h.logical().id());
        let integer = |width: u32| match width {
            4 => Some(OrderingSpec::UnsignedInteger {
                width: Width::U32,
                endian: if cfg!(target_endian = "little") {
                    Endian::Little
                } else {
                    Endian::Big
                },
            }),
            8 => Some(OrderingSpec::UnsignedInteger {
                width: Width::U64,
                endian: if cfg!(target_endian = "little") {
                    Endian::Little
                } else {
                    Endian::Big
                },
            }),
            _ => None,
        };
        let keys = if o.is_some_and(|o| o.key_cmp.is_some()) {
            OrderingSpec::Custom
        } else if settings.flags & 2 != 0 {
            OrderingSpec::ReverseBytes
        } else if settings.flags & 8 != 0 {
            let Some(order) = integer(settings.key_width) else {
                return Ok(None);
            };
            order
        } else {
            OrderingSpec::Bytewise
        };
        let duplicates = if o.is_some_and(|o| o.dup_cmp.is_some()) {
            OrderingSpec::Custom
        } else if settings.flags & 0x20 != 0 {
            let Some(order) = integer(settings.dup_width) else {
                return Ok(None);
            };
            order
        } else if settings.flags & 0x40 != 0 {
            OrderingSpec::ReverseBytes
        } else {
            OrderingSpec::Bytewise
        };
        Ok(Some(DatabaseOrdering { keys, duplicates }))
    }
    pub(super) fn actual_native_order(&self, h: &ResolvedAlias) -> FfiResult<DatabaseOrdering> {
        match &h.binding.view {
            DataView::Writable(v) => Ok(v.ordering()),
            DataView::Readonly(v) => Ok(v.ordering()),
            DataView::Direct => match self.state.as_ref() {
                Some(State::Read(r)) => r.database_ordering(h.actual().name()),
                Some(State::PrivateRead(r)) => r.database_ordering(h.actual().name()),
                Some(State::Write(w)) => w.database_ordering(h.actual().name()),
                Some(State::Child(w)) => w.database_ordering(h.actual().name()),
                _ => return Err(BAD_TXN),
            }
            .map_err(code),
        }
    }
    pub(super) fn native_order_matches(&self, h: &ResolvedAlias) -> FfiResult<bool> {
        Ok(self.desired_native_order(h)?.is_some_and(|desired| {
            self.actual_native_order(h)
                .is_ok_and(|actual| actual == desired)
        }))
    }
    /// Called after width learning and before a first actual mutation. Empty
    /// branch reconfiguration revokes its prior cap; nonempty historical trees
    /// retain their exact byte descriptor and the compatibility scan fallback.
    pub(super) fn configure_native_order(&mut self, h: &ResolvedAlias) -> FfiResult<ResolvedAlias> {
        let Some(desired) = self.desired_native_order(h)? else {
            return Ok(h.clone());
        };
        if self.actual_native_order(h)? == desired {
            return Ok(h.clone());
        }
        let callbacks = self.ordering_callbacks(h)?;
        let view = match &h.binding.view {
            DataView::Writable(v) => match self
                .writer()?
                .configure_empty_database_view(v, desired, &callbacks)
            {
                Ok(v) => DataView::Writable(v),
                Err(Error::InvalidInput(_)) => return Ok(h.clone()),
                Err(e) => return Err(code(e)),
            },
            DataView::Direct => {
                match self
                    .writer()?
                    .set_database_ordering(h.actual().name(), desired)
                {
                    Ok(()) => {}
                    Err(Error::InvalidInput(_)) => return Ok(h.clone()),
                    Err(e) => return Err(code(e)),
                };
                return Ok(h.clone());
            }
            DataView::Readonly(_) => return Err(self.shared.readonly_error()),
        };
        let binding = Arc::new(BoundAlias {
            slot: h.binding.slot,
            logical_life: h.binding.logical_life.clone(),
            actual: h.actual().clone(),
            view,
            seed_settings: h.binding.seed_settings,
        });
        self.dbis
            .lock()
            .map_err(|_| BAD_TXN)?
            .bindings
            .insert(h.id(), binding.clone());
        Ok(ResolvedAlias {
            alias: h.alias.clone(),
            binding,
        })
    }
}
pub(super) fn native_callback(
    callback: compat_features::Comparator,
) -> ulmdb::ordering::CustomComparator {
    Arc::new(move |a, b| compat_features::invoke(callback, a, b))
}

impl MDB_txn {
    pub(super) fn physical_pair_after(
        &self,
        h: &ResolvedAlias,
        after: Option<(&[u8], &[u8])>,
    ) -> ulmdb::Result<Option<(&[u8], &[u8])>> {
        if self
            .actual_native_order(h)
            .is_ok_and(|o| o.keys != OrderingSpec::Custom && o.duplicates != OrderingSpec::Custom)
        {
            return self.seek_database_pair(h, after.map(|(k, v)| (k, Some(v))), false, false);
        }
        match (&h.binding.view, self.state.as_ref()) {
            (DataView::Writable(v), Some(State::Write(w))) => {
                w.view_structural_resume_pair(v, after)
            }
            (DataView::Writable(v), Some(State::Child(w))) => {
                w.view_structural_resume_pair(v, after)
            }
            (DataView::Readonly(v), Some(State::PrivateRead(r))) => {
                r.read_scope().structural_resume_pair(v, after)
            }
            (DataView::Direct, _) => {
                self.check_native_actual(h.actual())?;
                match self.state.as_ref() {
                    Some(State::Read(r)) => r.structural_resume_pair(h.actual().name(), after),
                    Some(State::PrivateRead(r)) => {
                        r.structural_resume_pair(h.actual().name(), after)
                    }
                    Some(State::Write(w)) => w.structural_resume_pair(h.actual().name(), after),
                    Some(State::Child(w)) => w.structural_resume_pair(h.actual().name(), after),
                    _ => Err(Error::InvalidInput("inactive source")),
                }
            }
            _ => Err(Error::InvalidInput("physical alias source")),
        }
    }
}

pub(super) fn default_callback(flags: u32, duplicate: bool) -> ulmdb::ordering::CustomComparator {
    use std::sync::OnceLock;
    static BYTE: OnceLock<ulmdb::ordering::CustomComparator> = OnceLock::new();
    static REVERSE: OnceLock<ulmdb::ordering::CustomComparator> = OnceLock::new();
    static INTEGER: OnceLock<ulmdb::ordering::CustomComparator> = OnceLock::new();
    let reverse = if duplicate {
        flags & 0x40 != 0 && flags & 0x20 == 0
    } else {
        flags & 2 != 0
    };
    let integer = if duplicate {
        flags & 0x20 != 0
    } else {
        flags & 8 != 0 && flags & 2 == 0
    };
    if reverse {
        REVERSE
            .get_or_init(|| Arc::new(|a, b| a.iter().rev().cmp(b.iter().rev())))
            .clone()
    } else if integer {
        INTEGER
            .get_or_init(|| {
                Arc::new(|a, b| compat_features::integer(a).cmp(&compat_features::integer(b)))
            })
            .clone()
    } else {
        BYTE.get_or_init(|| Arc::new(|a, b| a.cmp(b))).clone()
    }
}
