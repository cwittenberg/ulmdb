//! LMDB ordering and buffer semantics layered on the unchanged native indexes.
use super::*;
use std::cmp::Ordering;
pub(super) const DB_FLAGS: u32 = 0x7e;
pub(super) const META: &[u8] = b"\0lmdb-meta";
pub(super) type Comparator = unsafe extern "C" fn(*const MDB_val, *const MDB_val) -> c_int;
// Callback authority belongs to a public logical alias. MAIN retains AliasId1
// across every native bootstrap; explicit MAIN open resets its comparators.
pub(super) const MAIN_CALLBACK_KEY: AliasId = AliasId::MAIN;
pub(super) fn callback_key(handle: &impl LogicalAlias) -> AliasId {
    handle.logical().id()
}
#[derive(Clone, Default)]
pub(super) struct DbOptions {
    pub flags: Option<u32>,
    // A rolled-back deletion keeps ULMDB's recovered integer handle usable.
    // A proper named open treats this alias as a fresh catalog registration.
    pub key_cmp: Option<Comparator>,
    pub dup_cmp: Option<Comparator>,
    pub native_key: Option<ulmdb::ordering::CustomComparator>,
    pub native_dup: Option<ulmdb::ordering::CustomComparator>,
    pub relfunc: usize,
    pub relctx: usize,
}
pub(super) struct Reserve {
    pub reservation: ulmdb::ValueReservation,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Settings {
    pub flags: u32,
    pub key_width: u32,
    pub dup_width: u32,
}
impl Settings {
    fn check_key_width(self, length: usize) -> FfiResult<()> {
        if self.flags & 8 != 0
            && (!matches!(length, 4 | 8)
                || (self.key_width != 0 && self.key_width as usize != length))
        {
            return Err(BAD_VALSIZE);
        }
        Ok(())
    }
    pub(super) fn for_kind(kind: DatabaseKind) -> Self {
        let mut settings = Self::default();
        if let DatabaseKind::Duplicates { width } = kind {
            settings.flags = DUPSORT;
            if let Some(width) = width {
                settings.flags |= 0x10;
                settings.dup_width = width;
            }
        }
        settings
    }
    pub(super) fn decode(bytes: &[u8]) -> FfiResult<Self> {
        if bytes.len() != 16 {
            return Err(-30796);
        }
        Ok(Self {
            flags: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
            key_width: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            dup_width: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
        })
    }
    pub(super) fn encode(self) -> [u8; 16] {
        let mut bytes = [0; 16];
        bytes[..4].copy_from_slice(&self.flags.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.key_width.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.dup_width.to_le_bytes());
        bytes
    }
}
pub(super) fn integer(bytes: &[u8]) -> u64 {
    let mut value = [0u8; 8];
    if cfg!(target_endian = "little") {
        value[..bytes.len().min(8)].copy_from_slice(&bytes[..bytes.len().min(8)]);
    } else {
        let n = bytes.len().min(8);
        value[8 - n..].copy_from_slice(&bytes[..n]);
    }
    u64::from_ne_bytes(value)
}
pub(super) fn invoke(callback: Comparator, a: &[u8], b: &[u8]) -> Ordering {
    let a = MDB_val {
        mv_size: a.len(),
        mv_data: a.as_ptr().cast_mut().cast(),
    };
    let b = MDB_val {
        mv_size: b.len(),
        mv_data: b.as_ptr().cast_mut().cast(),
    };
    // SAFETY: callbacks are registered by the application and receive valid
    // temporary views. As in LMDB, the application must supply a total order.
    unsafe { callback(&a, &b) }.cmp(&0)
}
impl MDB_txn {
    pub(super) fn raw_get(&self, name: &[u8], key: &[u8]) -> FfiResult<Option<&[u8]>> {
        let main_callbacks = if name == MAIN {
            let alias = self.main_alias()?;
            let mut callbacks = self.ordering_callbacks(&alias)?;
            let flags = self.db_flags(&alias)?;
            if callbacks.keys.is_none() {
                callbacks.keys = Some(native_order::default_callback(flags, false));
            }
            if callbacks.duplicates.is_none() {
                callbacks.duplicates = Some(native_order::default_callback(flags, true));
            }
            Some(callbacks)
        } else {
            None
        };
        let result = match self.state.as_ref() {
            Some(State::Read(r)) => match &main_callbacks {
                Some(c) => r.get_with_ordering(name, key, c),
                None => r.get(name, key),
            },
            Some(State::PrivateRead(r)) => match &main_callbacks {
                Some(c) => r.get_with_ordering(name, key, c),
                None => r.get(name, key),
            },
            Some(State::Write(w)) => match &main_callbacks {
                Some(c) => w.get_with_ordering(name, key, c),
                None => w.get(name, key),
            },
            Some(State::Child(w)) => match &main_callbacks {
                Some(c) => w.get_with_ordering(name, key, c),
                None => w.get(name, key),
            },
            Some(State::Prepared(_)) | None => return Err(BAD_TXN),
        };
        match result {
            Ok(v) => Ok(v),
            Err(Error::MissingDatabase) => Ok(None),
            Err(e) => Err(code(e)),
        }
    }
    pub(super) fn native_settings(&self, actual: &DatabaseHandle) -> FfiResult<Settings> {
        let mut result = Settings::for_kind(actual.kind());
        if let Some(bytes) = self.raw_get(META, &NativeId::of(actual).bytes())? {
            result = Settings::decode(bytes)?;
        } else if let Some(created) = self
            .dbis
            .lock()
            .map_err(|_| BAD_TXN)?
            .creation_settings
            .get(&NativeId::of(actual))
        {
            result = *created;
        }
        Ok(result)
    }
    pub(super) fn settings(&self, h: &impl LogicalAlias) -> FfiResult<Settings> {
        let alias = h.logical();
        {
            let dbis = self.dbis.lock().map_err(|_| BAD_TXN)?;
            if let Some(settings) = dbis.settings.get(&alias.id()) {
                return Ok(*settings);
            }
            if let Some(binding) = dbis.bindings.get(&alias.id()) {
                return Ok(binding.seed_settings);
            }
        }
        let settings = if alias.is_main() {
            match operations::current_handle(self, MAIN) {
                Ok(actual) => self.native_settings(&actual)?,
                Err(Error::MissingDatabase) => alias.exported_settings(),
                Err(error) => return Err(code(error)),
            }
        } else {
            alias.exported_settings()
        };
        self.cache_settings(alias, settings)?;
        Ok(settings)
    }
    pub(super) fn cache_settings(
        &self,
        h: &impl LogicalAlias,
        settings: Settings,
    ) -> FfiResult<()> {
        self.dbis
            .lock()
            .map_err(|_| BAD_TXN)?
            .settings
            .insert(h.logical().id(), settings);
        Ok(())
    }
    pub(super) fn db_flags(&self, h: &impl LogicalAlias) -> FfiResult<u32> {
        Ok(self.settings(h)?.flags)
    }
    pub(super) fn store_settings(
        &mut self,
        h: &impl LogicalAlias,
        settings: Settings,
    ) -> FfiResult<()> {
        // Alias widths are private until its winning root is selected. Writing
        // actual-ID META here would expose one alias's width to its siblings.
        self.finish_reserves()?;
        self.cache_settings(h, settings)?;
        self.dbis
            .lock()
            .map_err(|_| BAD_TXN)?
            .settings_dirty
            .insert(h.logical().id());
        Ok(())
    }
    pub(super) fn persist_native_settings(
        &mut self,
        actual: &DatabaseHandle,
        settings: Settings,
    ) -> FfiResult<()> {
        let writer = self.writer()?;
        if writer.open_database(META).is_err() {
            writer
                .create_database(META, DatabaseKind::Kv)
                .map_err(code)?;
        }
        writer
            .put(META, &NativeId::of(actual).bytes(), &settings.encode())
            .map_err(code)
    }
    pub(super) fn key_order(&self, h: &impl LogicalAlias, a: &[u8], b: &[u8]) -> Ordering {
        let authority = callback_key(h);
        let callback = self
            .shared
            .options
            .lock()
            .ok()
            .and_then(|options| options.get(&authority).and_then(|o| o.key_cmp));
        if let Some(callback) = callback {
            return invoke(callback, a, b);
        }
        let flags = self.db_flags(h).unwrap_or(0);
        if flags & 2 != 0 {
            a.iter().rev().cmp(b.iter().rev())
        } else if flags & 8 != 0 {
            integer(a).cmp(&integer(b))
        } else {
            a.cmp(b)
        }
    }
    pub(super) fn value_order(&self, h: &impl LogicalAlias, a: &[u8], b: &[u8]) -> Ordering {
        let authority = callback_key(h);
        let callback = self
            .shared
            .options
            .lock()
            .ok()
            .and_then(|options| options.get(&authority).and_then(|o| o.dup_cmp));
        if let Some(callback) = callback {
            return invoke(callback, a, b);
        }
        let flags = self.db_flags(h).unwrap_or(0);
        if flags & 0x20 != 0 {
            integer(a).cmp(&integer(b))
        } else if flags & 0x40 != 0 {
            a.iter().rev().cmp(b.iter().rev())
        } else {
            a.cmp(b)
        }
    }
    /// LMDB's comparison helpers use environment callbacks even for an alias
    /// exported after this transaction began. This grants no data validity and
    /// must not read a newer settings record through the old snapshot.
    pub(super) fn invalid_alias_order(
        &self,
        slot: u32,
        a: &[u8],
        b: &[u8],
        duplicate: bool,
    ) -> FfiResult<Option<Ordering>> {
        let handle = {
            let handles = self.shared.handles.lock().map_err(|_| BAD_TXN)?;
            let aliases = self.shared.aliases.lock().map_err(|_| BAD_TXN)?;
            let handle = handles
                .get(slot as usize)
                .and_then(Option::as_ref)
                .ok_or(BAD_DBI)?;
            if handle.is_main()
                || aliases.dormant.contains(&slot)
                || self.dbis.lock().map_err(|_| BAD_TXN)?.valid.get(&slot) == Some(&handle.id())
            {
                return Ok(None);
            }
            handle.clone()
        };
        let options = self
            .shared
            .options
            .lock()
            .map_err(|_| BAD_TXN)?
            .get(&handle.id())
            .cloned()
            .unwrap_or_default();
        let callback = if duplicate {
            options.dup_cmp
        } else {
            options.key_cmp
        };
        if let Some(callback) = callback {
            return Ok(Some(invoke(callback, a, b)));
        }
        let flags = options.flags.unwrap_or(0);
        Ok(Some(if !duplicate && flags & 2 != 0 {
            a.iter().rev().cmp(b.iter().rev())
        } else if flags & (if duplicate { 0x20 } else { 8 }) != 0 {
            integer(a).cmp(&integer(b))
        } else if flags & (if duplicate { 0x40 } else { 2 }) != 0 {
            a.iter().rev().cmp(b.iter().rev())
        } else {
            a.cmp(b)
        }))
    }
    pub(super) fn pair_order(&self, h: &impl LogicalAlias, a: &Pair, b: &Pair) -> Ordering {
        self.key_order(h, &a.0, &b.0)
            .then_with(|| self.value_order(h, &a.1, &b.1))
    }
    pub(super) fn compat_pairs(&self, h: &ResolvedAlias) -> FfiResult<Vec<Pair>> {
        self.settings(h)?;
        let mut pairs = self.pairs(h)?;
        for (key, value) in &mut pairs {
            if let Some(reserved) = self.reserve_value(h, key) {
                *value = reserved.to_vec();
            }
        }
        pairs.sort_by(|a, b| self.pair_order(h, a, b));
        Ok(pairs)
    }
    pub(super) fn custom_key(&self, h: &impl LogicalAlias) -> bool {
        let authority = callback_key(h);
        self.shared
            .options
            .lock()
            .is_ok_and(|options| options.get(&authority).is_some_and(|o| o.key_cmp.is_some()))
    }
    pub(super) fn custom_dup(&self, h: &impl LogicalAlias) -> bool {
        let authority = callback_key(h);
        self.shared
            .options
            .lock()
            .is_ok_and(|options| options.get(&authority).is_some_and(|o| o.dup_cmp.is_some()))
    }
    pub(super) fn canonical_key(
        &self,
        h: &ResolvedAlias,
        key: &[u8],
    ) -> FfiResult<Option<Vec<u8>>> {
        Ok(self
            .canonical_key_view(h, key)?
            .map(|canonical| canonical.into_owned()))
    }
    pub(super) fn canonical_key_view<'a>(
        &self,
        h: &ResolvedAlias,
        key: &'a [u8],
    ) -> FfiResult<Option<std::borrow::Cow<'a, [u8]>>> {
        if !self.custom_key(h) {
            return Ok(self
                .lookup(h, key)?
                .map(|_| std::borrow::Cow::Borrowed(key)));
        }
        if self.native_order_matches(h)? {
            return Ok(self
                .seek_database_pair(h, Some((key, None)), true, false)
                .map_err(code)?
                .filter(|(stored, _)| self.key_order(h, stored, key).is_eq())
                .map(|(stored, _)| std::borrow::Cow::Owned(stored.to_vec())));
        }
        Ok(self
            .pairs(h)?
            .into_iter()
            .find(|(stored, _)| self.key_order(h, stored, key).is_eq())
            .map(|(stored, _)| std::borrow::Cow::Owned(stored)))
    }
    pub(super) fn checked_key_settings(
        &self,
        h: &impl LogicalAlias,
        key: &[u8],
    ) -> FfiResult<Settings> {
        validate_key(key, self.shared.max_key_size())?;
        let settings = self.settings(h)?;
        settings.check_key_width(key.len())?;
        Ok(settings)
    }
    pub(super) fn check_key_settings(&self, key: &[u8], settings: Settings) -> FfiResult<()> {
        validate_key(key, self.shared.max_key_size())?;
        settings.check_key_width(key.len())
    }
    pub(super) fn check_sizes(
        &self,
        h: &impl LogicalAlias,
        key: &[u8],
        value: Option<&[u8]>,
        write: bool,
    ) -> FfiResult<()> {
        self.checked_sizes(h, key, value, write).map(|_| ())
    }
    pub(super) fn checked_sizes(
        &self,
        h: &impl LogicalAlias,
        key: &[u8],
        value: Option<&[u8]>,
        write: bool,
    ) -> FfiResult<Settings> {
        let settings = self.checked_key_settings(h, key)?;
        if let Some(value) = value
            && settings.flags & DUPSORT != 0
        {
            if value.is_empty() || value.len() > self.shared.max_key_size() {
                return Err(BAD_VALSIZE);
            }
            if settings.flags & 0x20 != 0 && !matches!(value.len(), 4 | 8) {
                return Err(BAD_VALSIZE);
            }
            if settings.flags & 0x10 != 0
                && settings.dup_width != 0
                && settings.dup_width as usize != value.len()
            {
                return Err(BAD_VALSIZE);
            }
        }
        let _ = write;
        Ok(settings)
    }
    pub(super) fn remember_sizes(
        &mut self,
        h: &impl LogicalAlias,
        key: &[u8],
        value: &[u8],
    ) -> FfiResult<()> {
        let mut settings = self.settings(h)?;
        let mut changed = false;
        if settings.flags & 8 != 0 && settings.key_width == 0 {
            settings.key_width = key.len() as u32;
            changed = true;
        }
        if settings.flags & 0x10 != 0 && settings.dup_width == 0 {
            settings.dup_width = value.len() as u32;
            changed = true;
        }
        if changed {
            self.store_settings(h, settings)?;
        }
        Ok(())
    }
    pub(super) fn retain_pair_output(
        &mut self,
        out: *mut MDB_val,
        value: Vec<u8>,
    ) -> FfiResult<()> {
        self.retired_rows.push(vec![(Vec::new(), value)]);
        set_value(out, &self.retired_rows.last().unwrap()[0].1)
    }
    pub(super) fn flush_reserves(&mut self) -> FfiResult<()> {
        // Reads must not rewrite private arena bytes: writable MDB_RESERVE
        // views and previous borrowed read views remain valid across reads.
        // Point and cursor lookups read the actual initialized private payload.
        self.live()
    }
    pub(super) fn reserve_value(&self, _h: &ResolvedAlias, _key: &[u8]) -> Option<&[u8]> {
        None
    }
    pub(super) fn finish_reserves(&mut self) -> FfiResult<()> {
        self.live()?;
        let reserves = std::mem::take(&mut self.pending_reserves);
        for reserve in reserves {
            self.writer()?
                .end_reservation(reserve.reservation)
                .map_err(code)?;
        }
        Ok(())
    }
    pub(super) fn handle_by_name(&self, name: &[u8]) -> FfiResult<DatabaseHandle> {
        match self.state.as_ref() {
            Some(State::Read(r)) => r.open_database(name),
            Some(State::PrivateRead(r)) => r.open_database(name),
            Some(State::Write(w)) => w.open_database(name),
            Some(State::Child(w)) => w.open_database(name),
            Some(State::Prepared(_)) | None => return Err(BAD_TXN),
        }
        .map_err(code)
    }
}
