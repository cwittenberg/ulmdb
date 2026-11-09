//! Transactional sorted loading and bounded updates of named indexes.
use crate::{DatabaseKind, Error, Prefix, Result, WriteTxn, read::check_family};

impl WriteTxn<'_> {
    /// Insert or replace sorted records in an empty or existing KV index.
    ///
    /// Keys must increase strictly under the database's ordering; duplicate
    /// input keys, including comparator-equivalent keys, reject the batch.
    /// Existing keys are replaced and absent keys inserted. Untouched subtrees
    /// are reused, and each bounded chunk shares leaf and ancestor work.
    /// Returns the input count, including unchanged values. Empty input is a
    /// no-op. Ordinary limits apply. Iterator/build errors poison the writer
    /// until abort or rollback to a preceding savepoint, as `load_sorted_kv`.
    pub fn upsert_sorted_kv<I, K, V>(&mut self, name: &[u8], records: I) -> Result<u64>
    where
        I: IntoIterator<Item = Result<(K, V)>>,
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.upsert_sorted_kv_with_ordering(name, records, &Default::default())
    }

    /// Sorted KV upsert with registered comparison authority. Replacements
    /// retain the existing key bytes for a comparator equality class.
    pub fn upsert_sorted_kv_with_ordering<I, K, V>(
        &mut self,
        name: &[u8],
        records: I,
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<u64>
    where
        I: IntoIterator<Item = Result<(K, V)>>,
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.live()?;
        let database = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        if database.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        let prior_root = database.root;
        let order = crate::ordering::BoundComparator::bind(
            database.ordering.keys,
            callbacks.keys.as_ref(),
        )?;
        let prior_revision = self.arena.revision();
        let large = std::cell::Cell::new(false);
        if self.env.require_file_backend().is_ok() {
            self.arena
                .set_value_limit(self.value_limit.max(crate::ValueLimit::EXTENDED));
        }
        let checked = records.into_iter().map(|record| {
            let (key, value) = record?;
            if value.as_ref().len() > crate::ValueLimit::DEFAULT.get() {
                large.set(true);
            }
            Ok((key, value))
        });
        self.failed = true;
        let updated = ulmdb_kv::upsert_sorted_with(&mut self.arena, prior_root, checked, &order)?;
        if large.get() {
            self.enable_value_limit(crate::ValueLimit::EXTENDED.get())?;
        } else {
            self.arena.set_value_limit(self.value_limit);
        }
        self.catalog
            .get_mut(name)
            .ok_or(Error::MissingDatabase)?
            .root = updated.root;
        if updated.root != prior_root || self.arena.revision() != prior_revision {
            self.mark_changed(name);
        }
        self.failed = false;
        Ok(updated.records)
    }

    /// Insert or replace strictly sorted exact IPv4/IPv6 prefixes, preserving
    /// descendants and unrelated prefixes in the existing index. Ordering,
    /// family checks and transactional failure semantics match sorted loading.
    /// Shared ancestors are updated once per bounded chunk without scanning
    /// untouched subtrees. Returns the input count, including unchanged values.
    pub fn upsert_sorted_prefixes<I, V>(&mut self, name: &[u8], records: I) -> Result<u64>
    where
        I: IntoIterator<Item = Result<(Prefix, V)>>,
        V: AsRef<[u8]>,
    {
        self.live()?;
        let database = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        let kind = database.kind;
        if !matches!(kind, DatabaseKind::Ipv4 | DatabaseKind::Ipv6) {
            return Err(Error::WrongKind);
        }
        let prior_root = database.root;
        let prior_revision = self.arena.revision();
        let large = std::cell::Cell::new(false);
        if self.env.require_file_backend().is_ok() {
            self.arena
                .set_value_limit(self.value_limit.max(crate::ValueLimit::EXTENDED));
        }
        let checked = records.into_iter().map(|record| {
            let (prefix, value) = record?;
            check_family(kind, prefix.network())?;
            if value.as_ref().len() > crate::ValueLimit::DEFAULT.get() {
                large.set(true);
            }
            Ok((prefix, value))
        });
        self.failed = true;
        let updated = ulmdb_prefix::upsert_sorted(&mut self.arena, prior_root, checked)?;
        if large.get() {
            self.enable_value_limit(crate::ValueLimit::EXTENDED.get())?;
        } else {
            self.arena.set_value_limit(self.value_limit);
        }
        self.catalog
            .get_mut(name)
            .ok_or(Error::MissingDatabase)?
            .root = updated.root;
        if updated.root != prior_root || self.arena.revision() != prior_revision {
            self.mark_changed(name);
        }
        self.failed = false;
        Ok(updated.records)
    }

    /// Load an empty KV database by constructing sorted pages directly.
    ///
    /// Keys must increase strictly; ordinary 4096-byte key/64-MiB value limits
    /// apply. The complete load shares this transaction's atomic commit and
    /// private arena limit. Existing nonempty databases reject before allocation.
    /// Any iterator/build error poisons the transaction until abort or rollback
    /// to an earlier savepoint. An empty input leaves an empty database unchanged.
    pub fn load_sorted_kv<I, K, V>(&mut self, name: &[u8], records: I) -> Result<u64>
    where
        I: IntoIterator<Item = Result<(K, V)>>,
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.load_sorted_kv_with_ordering(name, records, &Default::default())
    }
    pub fn load_sorted_kv_with_ordering<I, K, V>(
        &mut self,
        name: &[u8],
        records: I,
        callbacks: &crate::ordering::DatabaseComparators,
    ) -> Result<u64>
    where
        I: IntoIterator<Item = Result<(K, V)>>,
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.live()?;
        let database = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        if database.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if !database.root.is_null() {
            return Err(Error::InvalidInput(
                "sorted load requires an empty database",
            ));
        }
        let order = crate::ordering::BoundComparator::bind(
            database.ordering.keys,
            callbacks.keys.as_ref(),
        )?;
        let large = std::cell::Cell::new(false);
        let file = self.env.require_file_backend().is_ok();
        if file {
            self.arena
                .set_value_limit(self.value_limit.max(crate::ValueLimit::EXTENDED));
        }
        let checked = records.into_iter().map(|record| {
            let (key, value) = record?;
            if value.as_ref().len() > crate::ValueLimit::DEFAULT.get() {
                large.set(true);
            }
            Ok((key, value))
        });
        self.failed = true;
        let built = ulmdb_kv::build_sorted_with(&mut self.arena, checked, order)?;
        if large.get() {
            self.enable_value_limit(crate::ValueLimit::EXTENDED.get())?;
        } else {
            self.arena.set_value_limit(self.value_limit);
        }
        self.catalog
            .get_mut(name)
            .ok_or(Error::MissingDatabase)?
            .root = built.root;
        if built.records != 0 {
            self.mark_changed(name);
        }
        self.failed = false;
        Ok(built.records)
    }

    /// Load an empty IPv4/IPv6 database using direct radix construction.
    ///
    /// Prefixes must match the named database's family and increase in unsigned
    /// network-address/length order, without duplicates. Values obey the ordinary
    /// 64-MiB limit. Arena limits, atomicity, failure and savepoint semantics are
    /// the same as `load_sorted_kv`; no existing nonempty index is replaced.
    pub fn load_sorted_prefixes<I, V>(&mut self, name: &[u8], records: I) -> Result<u64>
    where
        I: IntoIterator<Item = Result<(Prefix, V)>>,
        V: AsRef<[u8]>,
    {
        self.live()?;
        let database = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        let kind = database.kind;
        if !matches!(kind, DatabaseKind::Ipv4 | DatabaseKind::Ipv6) {
            return Err(Error::WrongKind);
        }
        if !database.root.is_null() {
            return Err(Error::InvalidInput(
                "sorted load requires an empty database",
            ));
        }
        let large = std::cell::Cell::new(false);
        if self.env.require_file_backend().is_ok() {
            self.arena
                .set_value_limit(self.value_limit.max(crate::ValueLimit::EXTENDED));
        }
        self.failed = true;
        let checked = records.into_iter().map(|record| {
            let (prefix, value) = record?;
            check_family(kind, prefix.network())?;
            if value.as_ref().len() > crate::ValueLimit::DEFAULT.get() {
                large.set(true);
            }
            Ok((prefix, value))
        });
        let built = ulmdb_prefix::build_sorted(&mut self.arena, checked)?;
        if large.get() {
            self.enable_value_limit(crate::ValueLimit::EXTENDED.get())?;
        } else {
            self.arena.set_value_limit(self.value_limit);
        }
        self.catalog
            .get_mut(name)
            .ok_or(Error::MissingDatabase)?
            .root = built.root;
        if built.records != 0 {
            self.mark_changed(name);
        }
        self.failed = false;
        Ok(built.records)
    }
}
