//! Shared storage contract. Indexes are monomorphized over these checked views.
use std::fmt;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Reference {
    pub segment: u64,
    pub offset: u64,
}
impl Reference {
    pub const NULL: Self = Self {
        segment: 0,
        offset: 0,
    };
    pub const fn is_null(self) -> bool {
        self.segment == 0 && self.offset == 0
    }
    pub fn encode(self) -> [u8; 16] {
        crate::endian::encode_reference(self.segment, self.offset)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let bytes: [u8; 16] = bytes
            .get(..16)
            .ok_or(Error::Corrupt("short object reference"))?
            .try_into()
            .map_err(|_| Error::Corrupt("short object reference"))?;
        let (segment, offset) = crate::endian::decode_reference(bytes)
            .ok_or(Error::Corrupt("malformed null reference"))?;
        Ok(Self { segment, offset })
    }
}

#[derive(Debug)]
pub enum Error {
    Corrupt(&'static str),
    InvalidInput(&'static str),
    MissingDatabase,
    StaleHandle,
    WrongKind,
    Busy,
    ReadOnly,
    LimitExceeded,
    Io(std::io::Error),
    OutcomeUnknown,
    BadChecksum,
    CryptoFailed,
    CryptoMismatch,
}
pub type Result<T> = std::result::Result<T, Error>;
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupt(s) => write!(f, "database integrity error: {s}"),
            Self::InvalidInput(s) => write!(f, "invalid input: {s}"),
            Self::MissingDatabase => write!(f, "database does not exist"),
            Self::StaleHandle => write!(f, "foreign or stale database handle"),
            Self::WrongKind => write!(f, "database kind mismatch"),
            Self::Busy => write!(f, "database writer is busy"),
            Self::ReadOnly => write!(f, "replica is read-only"),
            Self::LimitExceeded => write!(f, "database resource limit exceeded"),
            Self::Io(e) => e.fmt(f),
            Self::BadChecksum => write!(f, "application checksum mismatch"),
            Self::CryptoFailed => write!(f, "encryption or authentication failed"),
            Self::CryptoMismatch => write!(f, "persisted transform configuration mismatch"),
            Self::OutcomeUnknown => write!(f, "commit outcome unknown; reopen required"),
        }
    }
}
impl std::error::Error for Error {}

pub trait ReadStore {
    /// Parent keys and duplicate-value child keys can have different bounds.
    fn max_duplicate_key_size(&self) -> usize {
        self.max_key_size()
    }
    /// Duplicate values become keys in per-key trees. Their limit is independent
    /// of the ordinary KV payload profile; old stores retain the64KiB codec.
    fn max_duplicate_value_size(&self) -> usize {
        65_536
    }
    fn max_value_size(&self) -> usize {
        crate::ValueLimit::DEFAULT.get()
    }
    /// Persisted logical tree-page size; old/custom stores retain4KiB by default.
    fn page_size(&self) -> usize {
        4096
    }
    fn max_key_size(&self) -> usize {
        crate::PageSize::new(self.page_size()).map_or(4096, crate::PageSize::native_key_limit)
    }
    /// Borrow exact bounded bytes from an immutable snapshot or writer-private arena.
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]>;
    /// Verify a complete immutable extent without retaining its bytes. Stores
    /// with independently authenticated source objects can check their bounds;
    /// ordinary stores retain the established full read validation by default.
    fn validate_extent(&self, at: Reference, length: usize) -> Result<()> {
        self.read(at, length).map(|_| ())
    }
    /// Validate a payload header and its entire extent without borrowing its
    /// body. Bounded cold graph walkers need no user-value materialization.
    fn payload_length(&self, at: Reference) -> Result<usize> {
        let h = self.read(at, 16)?;
        if h[..2] != 1u16.to_le_bytes()
            || h[2..4] != 0u16.to_le_bytes()
            || h[4..8] != 16u32.to_le_bytes()
        {
            return Err(Error::Corrupt("invalid payload header"));
        }
        let length = usize::try_from(u64::from_le_bytes(h[8..16].try_into().unwrap()))
            .map_err(|_| Error::LimitExceeded)?;
        let offset = at
            .offset
            .checked_add(16)
            .ok_or(Error::Corrupt("payload offset overflow"))?;
        self.validate_extent(
            Reference {
                segment: at.segment,
                offset,
            },
            length,
        )?;
        Ok(length)
    }
    fn payload(&self, at: Reference) -> Result<&[u8]> {
        let h = self.read(at, 16)?;
        if h[..2] != 1u16.to_le_bytes()
            || h[2..4] != 0u16.to_le_bytes()
            || h[4..8] != 16u32.to_le_bytes()
        {
            return Err(Error::Corrupt("invalid payload header"));
        }
        let length = u64::from_le_bytes(
            h[8..16]
                .try_into()
                .map_err(|_| Error::Corrupt("payload length"))?,
        );
        let offset = at
            .offset
            .checked_add(16)
            .ok_or(Error::Corrupt("payload offset overflow"))?;
        self.read(
            Reference {
                segment: at.segment,
                offset,
            },
            usize::try_from(length).map_err(|_| Error::LimitExceeded)?,
        )
    }
}

/// A payload created by a store's initialized reservation allocator. Fields are
/// private: index callers cannot manufacture an unchecked reference/length pair.
#[derive(Debug)]
pub struct ReservedPayload {
    at: Reference,
    length: usize,
}
impl ReservedPayload {
    pub fn reference(&self) -> Reference {
        self.at
    }
    pub fn len(&self) -> usize {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
    pub fn check<S: ReadStore>(&self, store: &S) -> Result<()> {
        if store.payload(self.at)?.len() != self.length {
            return Err(Error::Corrupt("reserved payload extent"));
        }
        Ok(())
    }
}

pub trait WriteStore: ReadStore {
    /// Assert that this EXACT already-loaded4KiB V1 leaf belongs to this
    /// source/publication's admitted graph or its closed checked-builder
    /// successor, and that all raw-cell flags/references/extents/padding,
    /// nonoverlap and dense coverage were validated or preserved by induction.
    /// This is physical-layout authority only, never comparator/key ordering.
    /// A truthful implementation must revoke it after unsupported/raw edits and
    /// must not infer it from a header, allocation, or patch capability alone.
    /// Default stores retain strict validation and perform no additional read.
    fn admitted_dense_leaf(&self, _at: Reference, _page_bytes: &[u8]) -> bool { false }
    /// Assert that this exact current 4 KiB V2 leaf image has valid geometry,
    /// strict local bytewise key ordering, and an all-zero unused tail.
    /// Authority must be bound to this store's exact current image and revoked
    /// after unsupported edits; a header, allocation, or equal byte copy alone
    /// is insufficient. This is not graph, routing, or application-comparator
    /// authority. Default stores perform no extra read and retain the ordinary
    /// KV validator, which remains the authority for corruption errors.
    fn admitted_compact_leaf(&self, _at: Reference, _page_bytes: &[u8]) -> bool {
        false
    }
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference>;
    /// Optional receipt that a successful tree edit removed one incoming page
    /// edge (including the tree's root position). This is not payload removal,
    /// proof that the page has no other owner, or authority to free its bytes.
    /// The builder omits equal-reference/no-op replacements; receipts can name
    /// private intermediate pages as well as immutable base pages. Consumers
    /// must bind/filter them to the final selected transaction and preserve
    /// aliases, snapshots, savepoints and aborts themselves. Bulk construction
    /// of a new tree does not report removal of a separately owned old tree.
    ///
    /// The hook runs after preparing the edit and may fail. On any hook or
    /// subsequent builder error, abandon the complete private candidate and
    /// its receipts; these calls do not roll back prior private mutations.
    fn retire_page_edge(&mut self, _old: Reference) -> Result<()> {
        Ok(())
    }
    /// Initialized allocation; specialized arenas zero their actual private
    /// storage directly. Custom stores retain the ordinary copying fallback.
    fn allocate_zeroed(&mut self, length: usize, alignment: usize) -> Result<Reference> {
        self.allocate(&vec![0; length], alignment)
    }
    fn reserve_payload(&mut self, length: usize) -> Result<ReservedPayload> {
        if length > self.max_value_size() {
            return Err(Error::InvalidInput("payload size limit exceeded"));
        }
        let size = length.checked_add(16).ok_or(Error::LimitExceeded)?;
        let at = self.allocate_zeroed(size, 16)?;
        let mut header = [0; 16];
        header[..2].copy_from_slice(&1u16.to_le_bytes());
        header[4..8].copy_from_slice(&16u32.to_le_bytes());
        header[8..16].copy_from_slice(&(length as u64).to_le_bytes());
        let at = self.patch(at, size, 0, &header, 16)?;
        Ok(ReservedPayload { at, length })
    }

    /// Reuse an equal-size private object; copy an immutable base object once.
    fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference>;
    /// Advertise a bounded multi-span patch shape without reading or mutating.
    /// For an advertised shape, `try_patch_many` returns Some or an error; it
    /// never declines. This lets callers finish an immutable preparation borrow
    /// before mutation without rereading an object after a declined capability.
    fn supports_patch_many(&self, _object_len: usize, _alignment: usize) -> bool {
        false
    }
    /// Optionally patch at most four sorted, nonoverlapping spans atomically.
    /// Each offset is relative to the checked complete object. Empty spans are
    /// allowed. Preflight every span, exact allocation, generation and revision
    /// before changing any bytes. A no-op keeps the reference and revision.
    /// Unsupported stores return None without reads, allocation or mutation;
    /// callers retain their original rewrite fallback. An advertised supported
    /// shape must return Some or an error, with one logical mutation for all
    /// spans and the store's normal CoW/snapshot/abort policy.
    fn try_patch_many(
        &mut self,
        _old: Reference,
        _object_len: usize,
        _patches: &[(usize, &[u8])],
        _alignment: usize,
    ) -> Result<Option<Reference>> {
        Ok(None)
    }
    /// Patch one bounded span of a complete object. Callers must supply its
    /// checked complete length: immutable stores can verify the extent but do
    /// not necessarily carry object-allocation metadata. Bytes outside the
    /// span/length stay unchanged. The default follows the store's CoW rewrite
    /// policy; alignment must be a power of two no greater than 16384.
    fn patch(
        &mut self,
        old: Reference,
        object_len: usize,
        offset: usize,
        replacement: &[u8],
        alignment: usize,
    ) -> Result<Reference> {
        if !alignment.is_power_of_two()
            || alignment > self.page_size().max(16384)
            || old.segment == 0
            || !old.offset.is_multiple_of(alignment as u64)
        {
            return Err(Error::InvalidInput("invalid patch reference/alignment"));
        }
        let end = offset
            .checked_add(replacement.len())
            .ok_or(Error::LimitExceeded)?;
        if end > object_len {
            return Err(Error::InvalidInput("patch range exceeds object"));
        }
        let source = self.read(old, object_len)?;
        if source.len() != object_len {
            return Err(Error::Corrupt("short patch object"));
        }
        if &source[offset..end] == replacement {
            return Ok(old);
        }
        let mut bytes = source.to_vec();
        bytes[offset..end].copy_from_slice(replacement);
        self.rewrite(old, &bytes, alignment)
    }
    fn put_payload(&mut self, value: &[u8]) -> Result<Reference> {
        let mut bytes = Vec::with_capacity(
            16usize
                .checked_add(value.len())
                .ok_or(Error::LimitExceeded)?,
        );
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
        bytes.extend_from_slice(value);
        self.allocate(&bytes, 16)
    }
    /// Replace an equal-sized writer-owned payload; immutable/base data is copied.
    fn replace_payload(&mut self, old: Reference, value: &[u8]) -> Result<Reference> {
        if old.is_null() || self.payload(old)?.len() != value.len() {
            return self.put_payload(value);
        }
        let mut bytes = Vec::with_capacity(
            16usize
                .checked_add(value.len())
                .ok_or(Error::LimitExceeded)?,
        );
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
        bytes.extend_from_slice(value);
        self.rewrite(old, &bytes, 16)
    }
}

#[cfg(test)]
mod patch_tests;

// Keep cooperative stop classification out of the common query Error enum.
// The private payload distinguishes explicit stops from ordinary IO failures
// with the same kind, errno, or display text. Allocation occurs only on stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BackupStopMarker {
    Cancelled,
    WorkLimitExceeded,
}
impl fmt::Display for BackupStopMarker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cancelled => "operation cancelled",
            Self::WorkLimitExceeded => "operation work or deadline limit exceeded",
        })
    }
}
impl std::error::Error for BackupStopMarker {}

impl Error {
    /// A caller explicitly stopped a cooperative background operation.
    #[cold]
    pub fn cancelled() -> Self {
        Self::Io(std::io::Error::other(BackupStopMarker::Cancelled))
    }
    /// A caller work or deadline allowance was exhausted.
    #[cold]
    pub fn work_limit_exceeded() -> Self {
        Self::Io(std::io::Error::other(BackupStopMarker::WorkLimitExceeded))
    }
    /// Whether this is an engine-created cooperative cancellation marker.
    pub fn is_cancelled(&self) -> bool {
        self.stop_marker() == Some(BackupStopMarker::Cancelled)
    }
    /// Whether this is an engine-created work or deadline limit marker.
    pub fn is_work_limit_exceeded(&self) -> bool {
        self.stop_marker() == Some(BackupStopMarker::WorkLimitExceeded)
    }
    fn stop_marker(&self) -> Option<BackupStopMarker> {
        let Self::Io(error) = self else {
            return None;
        };
        error.get_ref()?.downcast_ref::<BackupStopMarker>().copied()
    }
}

#[cfg(test)]
mod stop_marker_tests {
    use super::{BackupStopMarker, Error};
    use std::{error::Error as _, io};

    #[test]
    fn cooperative_stops_preserve_display_and_typed_io_payloads() {
        for (error, marker, text) in [
            (
                Error::cancelled(),
                BackupStopMarker::Cancelled,
                "operation cancelled",
            ),
            (
                Error::work_limit_exceeded(),
                BackupStopMarker::WorkLimitExceeded,
                "operation work or deadline limit exceeded",
            ),
        ] {
            assert_eq!(error.to_string(), text);
            assert_eq!(error.is_cancelled(), marker == BackupStopMarker::Cancelled);
            assert_eq!(
                error.is_work_limit_exceeded(),
                marker == BackupStopMarker::WorkLimitExceeded
            );
            // Preserve the existing public Error::source behavior. The IO
            // envelope still owns its concrete, inspectable error payload.
            assert!(error.source().is_none());
            let Error::Io(inner) = error else {
                panic!("cooperative stop changed the ordinary Error enum");
            };
            assert_eq!(inner.kind(), io::ErrorKind::Other);
            assert_eq!(inner.raw_os_error(), None);
            let payload = inner.get_ref().unwrap();
            assert_eq!(payload.downcast_ref::<BackupStopMarker>(), Some(&marker));
            assert!(payload.source().is_none());
            let restored = Error::from(inner);
            assert_eq!(
                restored.is_cancelled(),
                marker == BackupStopMarker::Cancelled
            );
            assert_eq!(
                restored.is_work_limit_exceeded(),
                marker == BackupStopMarker::WorkLimitExceeded
            );
        }
    }

    #[test]
    fn ordinary_io_kinds_codes_and_messages_are_not_cooperative_stops() {
        for error in [
            Error::from(io::Error::new(
                io::ErrorKind::Interrupted,
                "operation cancelled",
            )),
            Error::from(io::Error::other("operation cancelled")),
            Error::from(io::Error::other(
                "operation work or deadline limit exceeded",
            )),
            // POSIX E2BIG and Linux ECANCELED values must not be interpreted
            // as typed markers; classification never consults raw errno.
            Error::from(io::Error::from_raw_os_error(7)),
            Error::from(io::Error::from_raw_os_error(125)),
            Error::from(io::Error::other(Error::cancelled())),
            Error::LimitExceeded,
            Error::Busy,
        ] {
            assert!(!error.is_cancelled(), "{error:?}");
            assert!(!error.is_work_limit_exceeded(), "{error:?}");
        }
    }
}
