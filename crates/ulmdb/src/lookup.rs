//! Explicit lookup strategies owned by one borrowed read transaction.
//!
//! Ordinary `kv`/`get` calls are unchanged. Preparing a hash index enumerates one
//! immutable bytewise KV database and borrows its keys and values. It adds no
//! global cache, writer hook, persistent state or independent snapshot owner.
//! Drop the prepared lookup and its caller-owned transaction to release the
//! snapshot. A new transaction requires a newly prepared lookup.
//!
//! Hash limits bound the index's heap buffer and logical referenced values, not
//! RSS, allocator bookkeeping, native cursor workspace or all pages retained by
//! the caller's snapshot. Transform-backed, externally synchronized, Range,
//! non-bytewise and non-KV sources are rejected for hashing. Ordered preparation
//! delegates to the existing prepared reader, retaining its admission rules.
mod cursor;
mod table;

pub use cursor::Cursor;
use std::ops::Bound;
use ulmdb_core::storage::ReadStore;
use ulmdb_storage::Reference;

use crate::{Error, Result, ordering::DatabaseOrdering};

/// The strategy chosen explicitly by the caller; existing native reads default
/// to their established ordered implementation and never build an accelerator.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Options {
    #[default]
    Ordered,
    Hash(HashLimits),
}

/// Admission budgets for a complete, immutable hash accelerator. Exceeding any
/// budget returns `LimitExceeded`; there is no partial index or silent fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HashLimits {
    pub max_entries: usize,
    /// Heap bytes in the actual slot-vector capacity. Excludes allocator
    /// bookkeeping and the inline PreparedLookup object. Keys/values are borrowed.
    pub max_index_bytes: usize,
    pub max_value_bytes: usize,
    /// Sum of logical value lengths, counted per key. This does not bound all
    /// pages retained by the already-owned read transaction.
    pub max_total_value_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Algorithm {
    Ordered,
    Hash,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Backend {
    File,
    Resident,
}

/// Descriptive identity, not permission to reuse/rebind an index. The private
/// native reader and Rust lifetime bind the actual root and borrowed storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity<'a> {
    pub backend: Backend,
    pub sequence: u64,
    pub commit: u128,
    pub database_id: u64,
    pub root: Reference,
    pub name: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LookupStats {
    pub algorithm: Algorithm,
    /// False for Ordered, which does not enumerate the database at preparation.
    pub enumerated: bool,
    pub entries: usize,
    pub capacity: usize,
    pub index_bytes: usize,
    pub referenced_key_bytes: usize,
    pub total_value_bytes: usize,
}

/// Heap slot bytes required for this cardinality on the current architecture.
/// This is not a persisted layout or RSS estimate; table occupancy is <=50%.
pub fn hash_index_bytes(entries: usize) -> Result<usize> {
    table::index_bytes(entries)
}

/// A strategy borrowing its caller-owned transaction; it cannot escape that
/// transaction or acquire an independent retention pin.
///
/// ```compile_fail
/// use ulmdb::{Environment, lookup::{Options, PreparedLookup}};
/// fn escape(env: &Environment) -> PreparedLookup<'_> {
///     let read = env.begin_read();
///     read.prepare_lookup(b"kv", Options::Ordered).unwrap()
/// }
/// ```
pub struct PreparedLookup<'a> {
    native: Native<'a>,
    identity: Identity<'a>,
    strategy: Strategy<'a>,
    max_key_bytes: usize,
}

pub(crate) enum Native<'a> {
    File(crate::KvRead<'a>),
    Resident(crate::resident::KvRead<'a>),
}
enum Strategy<'a> {
    Ordered,
    Hash(table::Table<'a>),
}

impl<'a> PreparedLookup<'a> {
    pub(crate) fn new(
        native: Native<'a>,
        identity: Identity<'a>,
        max_key_bytes: usize,
        options: Options,
    ) -> Result<Self> {
        let strategy = match options {
            Options::Ordered => Strategy::Ordered,
            Options::Hash(limits) => Strategy::Hash(table::Table::build(&native, limits)?),
        };
        Ok(Self {
            native,
            identity,
            strategy,
            max_key_bytes,
        })
    }

    /// Return the same borrowed value bytes as this transaction's native reader.
    /// Invalid key lengths retain the native prepared-reader error.
    #[inline]
    pub fn get(&self, key: &[u8]) -> Result<Option<&'a [u8]>> {
        if key.len() > self.max_key_bytes {
            return Err(Error::InvalidInput("key size limit exceeded"));
        }
        match &self.strategy {
            Strategy::Ordered => self.native.get(key),
            Strategy::Hash(table) => Ok(table.get(key)),
        }
    }

    /// Ordered ranges, including reverse traversal, retain the native cursor.
    pub fn cursor(
        &self,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<Cursor<'a>> {
        self.native.cursor(lower, upper, reverse)
    }

    pub fn identity(&self) -> Identity<'a> {
        self.identity
    }

    pub fn stats(&self) -> LookupStats {
        match &self.strategy {
            Strategy::Ordered => LookupStats {
                algorithm: Algorithm::Ordered,
                enumerated: false,
                entries: 0,
                capacity: 0,
                index_bytes: 0,
                referenced_key_bytes: 0,
                total_value_bytes: 0,
            },
            Strategy::Hash(table) => table.stats(),
        }
    }
}

impl<'a> Native<'a> {
    fn get(&self, key: &[u8]) -> Result<Option<&'a [u8]>> {
        match self {
            Self::File(v) => v.get(key),
            Self::Resident(v) => v.get(key),
        }
    }
    fn cursor(
        &self,
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<Cursor<'a>> {
        Ok(match self {
            Self::File(v) => Cursor::File(v.cursor(lower, upper, reverse)?),
            Self::Resident(v) => Cursor::Resident(v.cursor(lower, upper, reverse)?),
        })
    }
}

impl crate::ReadTxn {
    /// Prepare an optional strategy for this exact pinned database. The returned
    /// lookup cannot outlive either this transaction or the supplied name.
    pub fn prepare_lookup<'a>(
        &'a self,
        name: &'a [u8],
        options: Options,
    ) -> Result<PreparedLookup<'a>> {
        let database = self.database(name)?;
        if database.kind != crate::DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if matches!(options, Options::Hash(_)) {
            if database.ordering != DatabaseOrdering::default() {
                return Err(Error::InvalidInput(
                    "hash lookup requires bytewise ordering",
                ));
            }
            if self.snapshot.range_backed
                || self.snapshot.external.is_some()
                || self.transform_context().is_some()
            {
                return Err(Error::InvalidInput(
                    "hash lookup requires immutable untransformed managed storage",
                ));
            }
        }
        let identity = Identity {
            backend: Backend::File,
            sequence: self.sequence(),
            commit: self.commit_identity(),
            database_id: database.id,
            root: database.root,
            name,
        };
        PreparedLookup::new(
            Native::File(self.kv(name)?),
            identity,
            self.snapshot.segments.max_key_size(),
            options,
        )
    }
}
