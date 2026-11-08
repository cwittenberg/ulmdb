//! Ready publications separate data preparation from fixed-size selection.
use super::*;
use crate::{Durability, SealedAssets};

pub(super) const WEAK_BYTES: usize = 176;
const WEAK_MAGIC: &[u8; 8] = b"ULMDBWK1";

/// Facts about an actual selector in one namespace. This is not a permanent
/// property of a descriptor, a logical graph certificate, or a mutable token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileDurabilityState {
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
    directory: PathBuf,
    identity: Option<crate::coordination::DirectoryIdentity>,
    creator: u32,
    checkpoint: [u8; 64],
    previous: Option<[u8; 64]>,
    weak: bool,
    data_debt: bool,
}
impl FileDurabilityState {
    pub(super) fn with_transform(
        mut self,
        transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
    ) -> Self {
        self.transform = transform;
        self
    }
    pub fn checkpoint(&self) -> &[u8; 64] {
        &self.checkpoint
    }
    pub fn matches(&self, checkpoint: &[u8; 64]) -> bool {
        self.creator == std::process::id() && self.checkpoint == *checkpoint
    }
    pub fn has_data_debt(&self) -> bool {
        self.data_debt
    }
    pub fn is_weak(&self) -> bool {
        self.weak
    }
    pub(super) fn captured(directory: &Path, state: &Current) -> Result<Self> {
        let metadata = fs::symlink_metadata(directory)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Corrupt("durability state directory"));
        }
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt;
            Some((metadata.dev(), metadata.ino()))
        };
        #[cfg(windows)]
        let identity = Some(super::coordination_directory_identity(directory)?);
        #[cfg(not(any(unix, windows)))]
        let identity = None;
        Ok(Self {
            transform: None,
            directory: directory.canonicalize()?,
            identity,
            creator: std::process::id(),
            checkpoint: state.current,
            previous: state.previous,
            weak: state.weak,
            data_debt: state.data_debt,
        })
    }
    fn check(&self, prepared: &PreparedPublication) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited durability state"));
        }
        if self.directory != prepared.directory
            && self.directory != prepared.directory.canonicalize()?
        {
            return Err(Error::InvalidInput("durability state namespace"));
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if self.identity != Some(prepared.directory_identity) {
            return Err(Error::InvalidInput("durability state directory identity"));
        }
        Ok(())
    }
    fn same_selection(&self, state: &Current) -> bool {
        self.checkpoint == state.current
            && self.previous == state.previous
            && self.weak == state.weak
            && self.data_debt == state.data_debt
    }
}

pub(super) fn decode_weak(bytes: &[u8]) -> Result<Current> {
    if bytes.len() != WEAK_BYTES
        || &bytes[..8] != WEAK_MAGIC
        || bytes[8..10] != 1u16.to_le_bytes()
        || bytes[10..12] != (WEAK_BYTES as u16).to_le_bytes()
    {
        return Err(Error::Corrupt("weak selector header/length"));
    }
    let checksum: [u8; 32] = Sha256::digest(&bytes[..144]).into();
    if bytes[144..] != checksum {
        return Err(Error::Corrupt("weak selector checksum"));
    }
    let flags = u32::from_le_bytes(number(bytes, 12)?);
    if flags & !3 != 0 {
        return Err(Error::Corrupt("weak selector flags"));
    }
    let current: [u8; 64] = number(bytes, 16)?;
    decode_pointer(&current)?;
    let previous: [u8; 64] = number(bytes, 80)?;
    let previous = match flags & 1 {
        0 if previous == [0; 64] => None,
        1 => {
            decode_pointer(&previous)?;
            Some(previous)
        }
        _ => return Err(Error::Corrupt("weak selector predecessor presence")),
    };
    Ok(Current {
        current,
        previous,
        bundled: true,
        weak: true,
        data_debt: flags & 2 != 0,
    })
}
fn weak_bundle(current: [u8; 64], previous: Option<[u8; 64]>, debt: bool) -> [u8; WEAK_BYTES] {
    let mut bytes = [0; WEAK_BYTES];
    bytes[..8].copy_from_slice(WEAK_MAGIC);
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&(WEAK_BYTES as u16).to_le_bytes());
    bytes[12..16]
        .copy_from_slice(&(u32::from(previous.is_some()) | (u32::from(debt) << 1)).to_le_bytes());
    bytes[16..80].copy_from_slice(&current);
    if let Some(previous) = previous {
        bytes[80..144].copy_from_slice(&previous);
    }
    let checksum: [u8; 32] = Sha256::digest(&bytes[..144]).into();
    bytes[144..].copy_from_slice(&checksum);
    bytes
}

#[derive(Clone, Copy)]
enum Target {
    Ordinary,
    Previous {
        expected: [u8; 64],
        previous: [u8; 64],
    },
    Rollback {
        expected: [u8; 64],
    },
}
struct ReadyCore {
    prepared: PreparedPublication,
    before: Option<FileDurabilityState>,
    target: Target,
    data_debt: bool,
    result: FileDurabilityState,
}
pub struct ReadyPublication {
    core: ReadyCore,
}
pub struct ExternalReadyPublication {
    core: ReadyCore,
    authority: crate::external::ExternalSynchronization,
}
impl ExternalReadyPublication {
    pub fn prepared(&self) -> &PreparedPublication {
        &self.core.prepared
    }
    pub fn publish(self, policy: Durability) -> Result<FileDurabilityState> {
        self.authority.check_prepared(&self.core.prepared)?;
        self.core
            .publish_inner(policy, false, Some(&self.authority))
    }
    pub fn publish_bootstrap(self, policy: Durability) -> Result<FileDurabilityState> {
        self.authority.check_prepared(&self.core.prepared)?;
        self.core.publish_inner(policy, true, Some(&self.authority))
    }
}
pub struct ReadyPreviousPublication {
    core: ReadyCore,
}
pub struct ReadyRollback {
    core: ReadyCore,
}

impl PreparedPublication {
    pub(super) fn prepare_import_ready_external(
        self,
        expected: [u8; 64],
        previous: Option<[u8; 64]>,
        authority: &crate::external::ExternalSynchronization,
    ) -> Result<ExternalReadyPublication> {
        authority.check_prepared(&self)?;
        let target = match previous {
            Some(previous) => Target::Previous { expected, previous },
            None => Target::Rollback { expected },
        };
        Ok(ExternalReadyPublication {
            core: ReadyCore::prepare_inner(self, None, target, Some(authority))?,
            authority: authority.clone(),
        })
    }
    pub(super) fn prepare_import_ready(
        self,
        expected: [u8; 64],
        previous: Option<[u8; 64]>,
    ) -> Result<ReadyPublication> {
        let target = match previous {
            Some(previous) => Target::Previous { expected, previous },
            None => Target::Rollback { expected },
        };
        Ok(ReadyPublication {
            core: ReadyCore::prepare(self, None, target)?,
        })
    }
    pub fn checkpoint(&self) -> &[u8; 64] {
        &self.pointer
    }
    pub fn prepare_ready_externally_synchronized(
        self,
        state: Option<&FileDurabilityState>,
        authority: &crate::external::ExternalSynchronization,
    ) -> Result<ExternalReadyPublication> {
        authority.check_prepared(&self)?;
        let core = ReadyCore::prepare_inner(self, state, Target::Ordinary, Some(authority))?;
        Ok(ExternalReadyPublication {
            core,
            authority: authority.clone(),
        })
    }
    pub fn prepare_ready(self, state: Option<&FileDurabilityState>) -> Result<ReadyPublication> {
        Ok(ReadyPublication {
            core: ReadyCore::prepare(self, state, Target::Ordinary)?,
        })
    }
}
impl PreparedPreviousPublication {
    pub fn prepare_ready_externally_synchronized(
        self,
        state: Option<&FileDurabilityState>,
        authority: &crate::external::ExternalSynchronization,
    ) -> Result<ExternalReadyPublication> {
        authority.check_prepared(&self.prepared)?;
        Ok(ExternalReadyPublication {
            core: ReadyCore::prepare_inner(
                self.prepared,
                state,
                Target::Previous {
                    expected: self.expected_current,
                    previous: self.previous_checkpoint,
                },
                Some(authority),
            )?,
            authority: authority.clone(),
        })
    }
    pub fn prepare_ready(
        self,
        state: Option<&FileDurabilityState>,
    ) -> Result<ReadyPreviousPublication> {
        Ok(ReadyPreviousPublication {
            core: ReadyCore::prepare(
                self.prepared,
                state,
                Target::Previous {
                    expected: self.expected_current,
                    previous: self.previous_checkpoint,
                },
            )?,
        })
    }
}
impl PreparedRollback {
    pub fn prepare_ready_externally_synchronized(
        self,
        state: Option<&FileDurabilityState>,
        authority: &crate::external::ExternalSynchronization,
    ) -> Result<ExternalReadyPublication> {
        authority.check_prepared(&self.prepared)?;
        Ok(ExternalReadyPublication {
            core: ReadyCore::prepare_inner(
                self.prepared,
                state,
                Target::Rollback {
                    expected: self.expected_current,
                },
                Some(authority),
            )?,
            authority: authority.clone(),
        })
    }
    pub fn prepare_ready(self, state: Option<&FileDurabilityState>) -> Result<ReadyRollback> {
        Ok(ReadyRollback {
            core: ReadyCore::prepare(
                self.prepared,
                state,
                Target::Rollback {
                    expected: self.expected_current,
                },
            )?,
        })
    }
}
impl ReadyPublication {
    pub fn prepared(&self) -> &PreparedPublication {
        &self.core.prepared
    }
    pub fn publish(self, policy: Durability) -> Result<FileDurabilityState> {
        self.core.publish(policy, false)
    }
    pub fn publish_bootstrap(self, policy: Durability) -> Result<FileDurabilityState> {
        self.core.publish(policy, true)
    }
}
impl ReadyPreviousPublication {
    pub fn prepared(&self) -> &PreparedPublication {
        &self.core.prepared
    }
    pub fn publish(self, policy: Durability) -> Result<FileDurabilityState> {
        self.core.publish(policy, false)
    }
}
impl ReadyRollback {
    pub fn prepared(&self) -> &PreparedPublication {
        &self.core.prepared
    }
    pub fn publish(self, policy: Durability) -> Result<FileDurabilityState> {
        self.core.publish(policy, false)
    }
}
impl ReadyCore {
    fn prepare(
        prepared: PreparedPublication,
        cached: Option<&FileDurabilityState>,
        target: Target,
    ) -> Result<Self> {
        Self::prepare_inner(prepared, cached, target, None)
    }
    fn prepare_inner(
        prepared: PreparedPublication,
        cached: Option<&FileDurabilityState>,
        target: Target,
        external: Option<&crate::external::ExternalSynchronization>,
    ) -> Result<Self> {
        if let Some(authority) = external {
            authority.check_prepared(&prepared)?;
        }

        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        prepared.check_directory_identity(prepared.directory_identity)?;
        let before = if let Some(state) = cached {
            state.check(&prepared)?;
            Some(state.clone())
        } else {
            match current(&prepared.directory) {
                Ok(state) => Some(FileDurabilityState::captured(&prepared.directory, &state)?),
                Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            }
        };
        if let Some(state) = &before {
            let expected = match target {
                Target::Ordinary => decode_pointer(&state.checkpoint)?.0 == prepared.previous,
                Target::Previous { expected, .. } | Target::Rollback { expected } => {
                    state.checkpoint == expected
                }
            };
            if !expected {
                return Err(Error::Corrupt("ready publication base mismatch"));
            }
        } else if !matches!(target, Target::Ordinary) {
            return Err(Error::Corrupt("ready publication missing base"));
        }
        let inherited = before.as_ref().is_some_and(|s| s.data_debt);
        let data_debt = if matches!(target, Target::Rollback { .. }) {
            inherited || !prepared.data_policy.sync_data()
        } else if !prepared.data_policy.sync_data() {
            true
        } else {
            if inherited {
                Self::repair(&prepared, before.as_ref().unwrap(), target, external)?;
            }
            false
        };
        let result = if let Some(state) = &before {
            state.clone()
        } else {
            FileDurabilityState::captured(
                &prepared.directory,
                &Current {
                    current: prepared.pointer,
                    previous: None,
                    bundled: false,
                    weak: false,
                    data_debt: false,
                },
            )?
        };
        Ok(Self {
            prepared,
            before,
            target,
            data_debt,
            result,
        })
    }
    fn repair(
        prepared: &PreparedPublication,
        state: &FileDurabilityState,
        target: Target,
        external: Option<&crate::external::ExternalSynchronization>,
    ) -> Result<()> {
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = (prepared, state, target);
            return Err(Error::InvalidInput(
                "weak durability repair is unavailable on this platform",
            ));
        }
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            let _capture = if let Some(authority) = external {
                authority.check_prepared(prepared)?;
                None
            } else {
                let coordination = crate::coordination::Coordination::open(
                    &prepared.directory,
                    prepared.uuid,
                    Default::default(),
                )?;
                Some(coordination.try_capture()?)
            };
            prepared.check_directory_identity(prepared.directory_identity)?;
            let actual = current(&prepared.directory)?;
            if !state.same_selection(&actual) {
                return Err(Error::Corrupt("durability repair head changed"));
            }
            let mut pointers = std::collections::BTreeSet::new();
            pointers.insert(prepared.pointer);
            pointers.insert(state.checkpoint);
            if let Some(previous) = state.previous {
                pointers.insert(previous);
            }
            if let Target::Previous { previous, .. } = target {
                pointers.insert(previous);
            }
            let mut manifests = Vec::new();
            for pointer in pointers {
                let manifest = read_checkpoint_inner(
                    &prepared.directory,
                    None,
                    &pointer,
                    crate::read_mode::ReadMode::Sync.with_transform(prepared.transform.clone()),
                )?;
                if manifest.uuid != prepared.uuid || manifest.epoch != prepared.epoch {
                    return Err(Error::Corrupt("durability repair realm mismatch"));
                }
                manifests.push(manifest);
            }
            let roots = manifests.iter().collect::<Vec<_>>();
            crate::durability::sync_manifests(&prepared.directory, &roots)?;
            prepared.check_directory_identity(prepared.directory_identity)?;
            Ok(())
        }
    }
    fn publish(self, policy: Durability, bootstrap: bool) -> Result<FileDurabilityState> {
        self.publish_inner(policy, bootstrap, None)
    }
    fn publish_inner(
        mut self,
        policy: Durability,
        bootstrap: bool,
        external: Option<&crate::external::ExternalSynchronization>,
    ) -> Result<FileDurabilityState> {
        crate::durability::check_policy(policy)?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        self.prepared
            .check_directory_identity(self.prepared.directory_identity)?;
        let _history = if let Some(authority) = external {
            authority.check_prepared(&self.prepared)?;
            None
        } else {
            Some(crate::history::HistoryGate::exclusive(
                &self.prepared.directory,
            )?)
        };
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        self.prepared
            .check_directory_identity(self.prepared.directory_identity)?;
        let old = match current(&self.prepared.directory) {
            Ok(old) => Some(old),
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        if bootstrap && old.is_some() {
            return Err(Error::InvalidInput("snapshot already initialized"));
        }
        match (&self.before, &old) {
            (Some(before), Some(old)) if before.same_selection(old) => {}
            (None, None) => {}
            _ => return Err(Error::Corrupt("ready publication selection changed")),
        }
        if old.is_none() && (policy != Durability::Full || self.data_debt) {
            return Err(Error::InvalidInput(
                "weak bootstrap publication unsupported",
            ));
        }
        let previous = match self.target {
            Target::Ordinary => {
                if let Some(old) = &old {
                    if decode_pointer(&old.current)?.0 != self.prepared.previous {
                        return Err(Error::Corrupt("publication base changed"));
                    }
                } else if !bootstrap && (self.prepared.sequence != 0 || self.prepared.previous != 0)
                {
                    return Err(Error::Corrupt("missing initialized CURRENT"));
                }
                old.as_ref().map(|old| old.current)
            }
            Target::Previous { expected, previous } => {
                if old.as_ref().map(|s| s.current) != Some(expected) {
                    return Err(Error::Corrupt("previous publication base changed"));
                }
                Some(previous)
            }
            Target::Rollback { expected } => {
                if old.as_ref().map(|s| s.current) != Some(expected) {
                    return Err(Error::Corrupt("rollback publication base changed"));
                }
                None
            }
        };
        let weak = policy != Durability::Full || self.data_debt;
        if weak {
            replace_policy(
                &self.prepared.directory,
                "CURRENT",
                &weak_bundle(self.prepared.pointer, previous, self.data_debt),
                policy,
                true,
            )?;
        } else if is_cohort(&self.prepared.pointer) {
            replace_policy(
                &self.prepared.directory,
                "CURRENT",
                &bundle(self.prepared.pointer, previous),
                policy,
                true,
            )?;
        } else {
            if let Some(previous) = previous {
                replace_policy(
                    &self.prepared.directory,
                    "PREVIOUS",
                    &previous,
                    policy,
                    false,
                )?;
                if old.as_ref().is_some_and(|s| s.weak) {
                    crate::fault_point!("pointer.PREVIOUS.before_transition_directory_sync");
                    ulmdb_platform::sync_directory(&self.prepared.directory)?;
                    crate::fault_point!("pointer.PREVIOUS.after_transition_directory_sync");
                }
            }
            replace_policy(
                &self.prepared.directory,
                "CURRENT",
                &self.prepared.pointer,
                policy,
                true,
            )?;
        }
        self.result.checkpoint = self.prepared.pointer;
        self.result.previous = if weak || is_cohort(&self.prepared.pointer) {
            previous
        } else {
            None
        };
        self.result.weak = weak;
        self.result.data_debt = self.data_debt;
        if let Some(authority) = external {
            authority.check_prepared(&self.prepared)?;
        }
        Ok(self.result)
    }
}

pub fn prepare_publication_with_sealed_data(
    directory: &Path,
    manifest: &Manifest,
    sealed: SealedAssets,
) -> Result<PreparedPublication> {
    sealed.check(directory, manifest)?;
    prepare_publication_with_durability(directory, manifest, sealed.durability())
}
pub fn prepare_publication_with_durability(
    directory: &Path,
    manifest: &Manifest,
    policy: Durability,
) -> Result<PreparedPublication> {
    crate::durability::check_policy(policy)?;
    let mut publication = if manifest.format_minor >= 3 {
        prepare_cohort_with_durability(directory, manifest, None, policy)?
    } else {
        prepared(
            directory,
            manifest,
            write_descriptor_policy(directory, manifest, policy)?,
        )?
    };
    publication.data_policy = policy;
    Ok(publication)
}
pub fn prepare_rollback_with_durability(
    directory: &Path,
    manifest: &Manifest,
    expected_current: &[u8; 64],
    policy: Durability,
) -> Result<PreparedRollback> {
    if manifest.previous != 0 || manifest.sequence == 0 {
        return Err(Error::InvalidInput(
            "rollback requires nonzero root without predecessor",
        ));
    }
    let state = current(directory)?;
    let selected = read_checkpoint_with_transform(
        directory,
        &state.current,
        manifest.transform_context().cloned(),
    )?;
    check_predecessor(&state, &selected)?;
    if state.current != *expected_current
        || manifest.uuid != selected.uuid
        || manifest.epoch != selected.epoch
        || manifest.format_minor != selected.format_minor
        || manifest.generation == selected.generation
    {
        return Err(Error::Corrupt("rollback preparation base mismatch"));
    }
    Ok(PreparedRollback {
        prepared: prepare_publication_with_durability(directory, manifest, policy)?,
        expected_current: *expected_current,
    })
}
