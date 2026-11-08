//! Public LMDB alias authority is distinct from a native database identity.
//!
//! The original native handle is never retargeted. A transaction binds each
//! logical alias independently to a genuine actual handle and data-view cap.
use super::*;
use std::sync::{
    Weak,
    atomic::{AtomicU64, Ordering},
};

/// Selected once at EnvOpen. A patch role changes refresh errors, never the
/// persisted major catalog-key layout or a live environment's interpretation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Profile {
    Legacy31,
    Legacy36,
    Modern,
}
impl Profile {
    pub(super) fn select(marker: Option<&str>, requested: Option<&str>) -> Self {
        let requested = match requested.map(str::trim) {
            Some("0.9.36") => Self::Legacy36,
            Some("1.0" | "1.0.2") => Self::Modern,
            _ => Self::Legacy31,
        };
        match marker.map(str::trim) {
            Some("1.0" | "1.0.2") => Self::Modern,
            Some("0.9.36") => Self::Legacy36,
            Some("0.9.31") => Self::Legacy31,
            Some("0.9") if requested == Self::Legacy36 => Self::Legacy36,
            Some(_) => Self::Legacy31,
            None => requested,
        }
    }
    pub(super) fn catalog_terminator(self) -> bool {
        self == Self::Modern
    }
    pub(super) fn missing_stale(self, main_empty: bool) -> c_int {
        if main_empty || self == Self::Legacy31 {
            NOTFOUND
        } else {
            BAD_DBI
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct AliasId(u64);
impl AliasId {
    pub(super) const MAIN: Self = Self(1);
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct NativeId(u64);
impl NativeId {
    pub(super) fn of(handle: &DatabaseHandle) -> Self {
        Self(handle.id())
    }
    pub(super) fn bytes(self) -> [u8; 8] {
        self.0.to_be_bytes()
    }
    #[cfg(test)]
    pub(super) fn value(self) -> u64 {
        self.0
    }
}
pub(super) enum VisibleName {
    Absent,
    Ordinary,
    Named(DatabaseHandle),
}

/// Issuance is monotonic even if a transaction aborts or a slot is reused.
pub(super) struct AliasIds(AtomicU64);
impl Default for AliasIds {
    fn default() -> Self {
        Self(AtomicU64::new(2))
    }
}
impl AliasIds {
    pub(super) fn preflight(&self) -> FfiResult<()> {
        if self.0.load(Ordering::Relaxed) == u64::MAX {
            Err(-30792)
        } else {
            Ok(())
        }
    }
    pub(super) fn issue(&self) -> FfiResult<AliasId> {
        self.0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .map(AliasId)
            .map_err(|_| -30792)
    }
}

#[derive(Debug)]
struct AliasRecord {
    identity: AliasId,
    // Exact input bytes, not the comparator-equal physical catalog name.
    requested: Box<[u8]>,
    original: DatabaseHandle,
    exported: compat_features::Settings,
    strict: bool,
}

#[derive(Clone, Debug)]
pub(super) struct AliasHandle(Arc<AliasRecord>);
#[derive(Clone)]
pub(super) struct AliasLife(Weak<AliasRecord>);
impl AliasLife {
    pub(super) fn alive(&self) -> bool {
        self.0.strong_count() != 0
    }
}
impl AliasHandle {
    pub(super) fn new(
        identity: AliasId,
        requested: &[u8],
        original: DatabaseHandle,
        exported: compat_features::Settings,
        strict: bool,
    ) -> Self {
        debug_assert_eq!(identity == AliasId::MAIN, requested == MAIN);
        debug_assert!(requested == MAIN || !requested.contains(&0));
        // Named issuance follows the selected bound; modern empty is distinct
        // from the private MAIN sentinel and uses a checked physical alias.
        Self(Arc::new(AliasRecord {
            identity,
            requested: requested.into(),
            original,
            exported,
            strict,
        }))
    }
    pub(super) fn id(&self) -> AliasId {
        self.0.identity
    }
    pub(super) fn name(&self) -> &[u8] {
        &self.0.requested
    }
    pub(super) fn is_main(&self) -> bool {
        self.id() == AliasId::MAIN
    }
    pub(super) fn native(&self) -> &DatabaseHandle {
        &self.0.original
    }
    pub(super) fn native_id(&self) -> NativeId {
        NativeId::of(self.native())
    }
    pub(super) fn kind(&self) -> DatabaseKind {
        self.native().kind()
    }
    pub(super) fn exported_settings(&self) -> compat_features::Settings {
        self.0.exported
    }
    pub(super) fn strict(&self) -> bool {
        self.0.strict
    }
    pub(super) fn life(&self) -> AliasLife {
        AliasLife(Arc::downgrade(&self.0))
    }
}

#[derive(Clone)]
pub(super) enum DataView {
    // ReadTxn snapshots and MAIN/native typed capabilities use checked handles.
    Direct,
    Writable(ulmdb::DatabaseView),
    Readonly(ulmdb::DatabaseReadView),
}

/// Immutable per-alias binding. Learned widths remain transaction-local and
/// are not shared through another alias's current private META.
pub(super) struct BoundAlias {
    pub slot: u32,
    pub logical_life: AliasLife,
    pub actual: DatabaseHandle,
    pub view: DataView,
    pub seed_settings: compat_features::Settings,
}
impl BoundAlias {
    pub(super) fn native_id(&self) -> NativeId {
        NativeId::of(&self.actual)
    }
}

/// Explicit public authority plus a snapshot-local checked native capability.
/// No Deref to DatabaseHandle: physical operations must name their boundary.
#[derive(Clone)]
pub(super) struct ResolvedAlias {
    pub alias: AliasHandle,
    pub binding: Arc<BoundAlias>,
}
pub(super) trait LogicalAlias {
    fn logical(&self) -> &AliasHandle;
}
impl LogicalAlias for AliasHandle {
    fn logical(&self) -> &AliasHandle {
        self
    }
}
impl LogicalAlias for ResolvedAlias {
    fn logical(&self) -> &AliasHandle {
        &self.alias
    }
}
impl ResolvedAlias {
    pub(super) fn id(&self) -> AliasId {
        self.alias.id()
    }
    pub(super) fn name(&self) -> &[u8] {
        self.alias.name()
    }
    pub(super) fn is_main(&self) -> bool {
        self.alias.is_main()
    }
    pub(super) fn kind(&self) -> DatabaseKind {
        self.binding.actual.kind()
    }
    pub(super) fn actual(&self) -> &DatabaseHandle {
        &self.binding.actual
    }
    pub(super) fn native_id(&self) -> NativeId {
        self.binding.native_id()
    }
}
