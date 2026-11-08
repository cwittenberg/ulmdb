//! Callback-free logical tuple counts with short metadata-only read scopes.
use crate::{
    DatabaseKind, Error, ReadTxn, Result, WriteTxn,
    ordering::{DatabaseOrdering, OrderingSpec, Width},
};
use ulmdb_core::storage::{ReadStore, Reference};
use ulmdb_kv::object_graph::{LogicalPage, SmallValue, inspect_logical_page};
use ulmdb_storage::{Arena, SegmentTable};
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LogicalDatabaseSize {
    pub entries: u64,
    pub key_bytes: u64,
    pub value_bytes: u64,
}
impl LogicalDatabaseSize {
    /// Adapter's defined logical tuple overhead, distinct from physical pages.
    pub fn tuple_bytes(self) -> Result<u64> {
        self.entries
            .checked_mul(16)
            .and_then(|n| n.checked_add(self.key_bytes))
            .and_then(|n| n.checked_add(self.value_bytes))
            .ok_or(Error::LimitExceeded)
    }
    fn add(&mut self, other: Self) -> Result<()> {
        self.entries = self
            .entries
            .checked_add(other.entries)
            .ok_or(Error::LimitExceeded)?;
        self.key_bytes = self
            .key_bytes
            .checked_add(other.key_bytes)
            .ok_or(Error::LimitExceeded)?;
        self.value_bytes = self
            .value_bytes
            .checked_add(other.value_bytes)
            .ok_or(Error::LimitExceeded)?;
        Ok(())
    }
}
#[derive(Clone, Copy)]
enum Source<'a> {
    Table(&'a SegmentTable),
    Arena(&'a Arena),
}
// A page owns only its own short scope. Overflow length queries get an even
// shorter independent scope, so many distant payload headers cannot pin their
// windows until the containing page finishes.
struct Headers<'a> {
    inner: &'a dyn ReadStore,
    source: Source<'a>,
}
impl ReadStore for Headers<'_> {
    fn page_size(&self) -> usize {
        self.inner.page_size()
    }
    fn max_value_size(&self) -> usize {
        self.inner.max_value_size()
    }
    fn read(&self, at: Reference, n: usize) -> Result<&[u8]> {
        self.inner.read(at, n)
    }
    fn validate_extent(&self, at: Reference, n: usize) -> Result<()> {
        self.inner.validate_extent(at, n)
    }
    fn payload_length(&self, at: Reference) -> Result<usize> {
        self.source.length(at)
    }
}
impl Source<'_> {
    fn page(self, at: Reference, buckets: bool) -> Result<LogicalPage> {
        match self {
            Self::Table(table) => {
                let scope = table.fresh_read_scope()?;
                inspect_logical_page(
                    &Headers {
                        inner: &scope,
                        source: self,
                    },
                    at,
                    buckets.then_some(crate::duplicates::bucket::INLINE_BYTES),
                )
            }
            Self::Arena(arena) => {
                let scope = arena.fresh_read_scope()?;
                inspect_logical_page(
                    &Headers {
                        inner: &scope,
                        source: self,
                    },
                    at,
                    buckets.then_some(crate::duplicates::bucket::INLINE_BYTES),
                )
            }
        }
    }
    fn length(self, at: Reference) -> Result<usize> {
        match self {
            Self::Table(table) => table.fresh_read_scope()?.payload_length(at),
            Self::Arena(arena) => arena.fresh_read_scope()?.payload_length(at),
        }
    }
    fn bucket(self, at: Reference) -> Result<Vec<u8>> {
        fn copy<S: ReadStore>(store: &S, at: Reference) -> Result<Vec<u8>> {
            if store.payload_length(at)? > crate::duplicates::bucket::INLINE_BYTES {
                return Err(Error::Corrupt("logical bucket limit"));
            }
            Ok(store.payload(at)?.to_vec())
        }
        match self {
            Self::Table(table) => copy(&table.fresh_read_scope()?, at),
            Self::Arena(arena) => copy(&arena.fresh_read_scope()?, at),
        }
    }
}
#[derive(Clone, Copy)]
enum Kind {
    Kv,
    Outer {
        width: Option<u32>,
        values: OrderingSpec,
    },
    Values {
        width: Option<u32>,
    },
}
struct Walker<'a> {
    source: Source<'a>,
    left: u64,
    max_key: usize,
    max_value: usize,
}
fn check_width(page: &LogicalPage, order: OrderingSpec) -> Result<()> {
    if let OrderingSpec::UnsignedInteger { width, .. } = order {
        let n = match width {
            Width::U32 => 4,
            Width::U64 => 8,
        };
        if page.minimum_key.is_some_and(|v| v != n)
            || (page.minimum_key.is_some() && page.maximum_key != n)
        {
            return Err(Error::Corrupt("logical integer key width"));
        }
    }
    Ok(())
}
impl Walker<'_> {
    fn walk(
        &mut self,
        root: Reference,
        kind: Kind,
        order: OrderingSpec,
        level: Option<u16>,
        depth: usize,
    ) -> Result<LogicalDatabaseSize> {
        if root.is_null() {
            return Ok(Default::default());
        }
        if depth >= 128 {
            return Err(Error::Corrupt("logical tree depth"));
        }
        self.left = self.left.checked_sub(1).ok_or(Error::LimitExceeded)?;
        let page = self.source.page(root, matches!(kind, Kind::Outer { .. }))?;
        if level.is_some_and(|expected| expected != page.level) {
            return Err(Error::Corrupt("logical child level"));
        }
        check_width(&page, order)?;
        let max_key = if matches!(kind, Kind::Values { .. }) {
            65536
        } else {
            self.max_key
        };
        if page.maximum_key > max_key {
            return Err(Error::Corrupt("logical key limit"));
        }
        if let Kind::Values { width: Some(width) } = kind {
            if page.minimum_key.is_some_and(|v| v != width as usize)
                || (page.minimum_key.is_some() && page.maximum_key != width as usize)
            {
                return Err(Error::Corrupt("logical duplicate width"));
            }
        }
        if page.level != 0 {
            let next = page
                .level
                .checked_sub(1)
                .ok_or(Error::Corrupt("logical branch level"))?;
            let mut result = LogicalDatabaseSize::default();
            for child in page.children {
                result.add(self.walk(child, kind, order, Some(next), depth + 1)?)?;
            }
            return Ok(result);
        }
        match kind {
            Kind::Kv => {
                if page.maximum_value > self.max_value {
                    return Err(Error::Corrupt("logical value limit"));
                }
                Ok(LogicalDatabaseSize {
                    entries: page.entries,
                    key_bytes: page.key_bytes,
                    value_bytes: page.value_bytes,
                })
            }
            Kind::Values { .. } => {
                if page.maximum_value != 0 {
                    return Err(Error::Corrupt("logical duplicate child data"));
                }
                Ok(LogicalDatabaseSize {
                    entries: page.entries,
                    key_bytes: page.key_bytes,
                    value_bytes: 0,
                })
            }
            Kind::Outer { width, values } => {
                let mut result = LogicalDatabaseSize::default();
                for row in page.small_values {
                    let bytes = match row.value {
                        SmallValue::Inline(b) => b,
                        SmallValue::Payload(at) => self.source.bucket(at)?,
                    };
                    use crate::duplicates::bucket::Bucket;
                    let bucket = Bucket::decode_with(&bytes, width, None)?;
                    let (count, value_bytes) = match bucket {
                        Bucket::Inline(inline) => {
                            let mut sum = 0u64;
                            for i in 0..inline.count {
                                let len = inline.value(i).len();
                                if let OrderingSpec::UnsignedInteger { width, .. } = values {
                                    let n = match width {
                                        Width::U32 => 4,
                                        Width::U64 => 8,
                                    };
                                    if len != n {
                                        return Err(Error::Corrupt(
                                            "logical integer duplicate width",
                                        ));
                                    }
                                }
                                sum = sum.checked_add(len as u64).ok_or(Error::LimitExceeded)?;
                            }
                            (inline.count as u64, sum)
                        }
                        Bucket::Tree { root, count } => {
                            let actual =
                                self.walk(root, Kind::Values { width }, values, None, depth + 1)?;
                            if actual.entries != count {
                                return Err(Error::Corrupt("logical duplicate count"));
                            }
                            (count, actual.key_bytes)
                        }
                    };
                    result.add(LogicalDatabaseSize {
                        entries: count,
                        key_bytes: (row.key_bytes as u64)
                            .checked_mul(count)
                            .ok_or(Error::LimitExceeded)?,
                        value_bytes,
                    })?;
                }
                Ok(result)
            }
        }
    }
}
fn measure(
    source: Source<'_>,
    root: Reference,
    kind: DatabaseKind,
    ordering: DatabaseOrdering,
    bytes: u64,
    page_size: usize,
    max_key: usize,
    max_value: usize,
) -> Result<LogicalDatabaseSize> {
    let kind = match kind {
        DatabaseKind::Kv => Kind::Kv,
        DatabaseKind::Duplicates { width } => Kind::Outer {
            width,
            values: ordering.duplicates,
        },
        _ => return Err(Error::WrongKind),
    };
    let visits = bytes
        .checked_div(page_size as u64)
        .and_then(|n| n.checked_add(1))
        .and_then(|n| n.checked_mul(64))
        .ok_or(Error::LimitExceeded)?;
    Walker {
        source,
        left: visits,
        max_key,
        max_value,
    }
    .walk(root, kind, ordering.keys, None, 0)
}
impl ReadTxn {
    /// Scalar logical KV/duplicate tuple lengths. Does not borrow value bodies,
    /// retain cache windows, or invoke key/duplicate comparison callbacks.
    pub fn logical_database_size(&self, name: &[u8]) -> Result<LogicalDatabaseSize> {
        let db = self.database(name)?;
        let m = &self.snapshot.manifest;
        let bytes = m.segments.iter().try_fold(0u64, |n, e| {
            n.checked_add(e.length).ok_or(Error::LimitExceeded)
        })?;
        measure(
            Source::Table(&self.snapshot.segments),
            db.root,
            db.kind,
            db.ordering,
            bytes,
            m.page_size.get(),
            m.page_size.native_key_limit(),
            m.value_limit.get(),
        )
    }
}
impl WriteTxn<'_> {
    /// As ReadTxn::logical_database_size, including genuine private/savepoint roots.
    pub fn logical_database_size(&self, name: &[u8]) -> Result<LogicalDatabaseSize> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        let m = &self.base.manifest;
        let bytes = m
            .segments
            .iter()
            .try_fold(self.arena.dirty_bytes() as u64, |n, e| {
                n.checked_add(e.length).ok_or(Error::LimitExceeded)
            })?;
        measure(
            Source::Arena(&self.arena),
            db.root,
            db.kind,
            db.ordering,
            bytes,
            m.page_size.get(),
            m.page_size.native_key_limit(),
            self.arena.max_value_size(),
        )
    }
}
