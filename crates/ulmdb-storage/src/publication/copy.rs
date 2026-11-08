//! Strong initial pair selection for independently copied, fully fenced assets.
use super::*;

/// A freshly created unpublished namespace and its exclusive writer ownership.
/// Does not certify copied data or the reachable logical graph.
pub struct BootstrapPairTarget {
    directory: PathBuf,
    identity: crate::coordination::DirectoryIdentity,
    creator: u32,
    _writer: Option<crate::WriterFence>,
}
impl BootstrapPairTarget {
    pub fn create(target: impl AsRef<Path>) -> Result<Self> {
        Self::create_with_access(target, crate::access::FileAccess::PRIVATE)
    }
    pub fn create_with_access(
        target: impl AsRef<Path>,
        access: crate::access::FileAccess,
    ) -> Result<Self> {
        Self::create_inner(target, access, false)
    }
    pub fn create_externally_synchronized(
        target: impl AsRef<Path>,
        access: crate::access::FileAccess,
    ) -> Result<Self> {
        Self::create_inner(target, access, true)
    }
    fn create_inner(
        target: impl AsRef<Path>,
        access: crate::access::FileAccess,
        external: bool,
    ) -> Result<Self> {
        if !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
            return Err(Error::InvalidInput(
                "copy bootstrap requires Linux, macOS or Windows",
            ));
        }
        crate::access::create_layout(target.as_ref(), access)?;
        let directory = target.as_ref().canonicalize()?;
        let writer = if external {
            None
        } else {
            let file = crate::access::options_for_entry(&directory.join("LOCK"))?
                .create(false)
                .read(true)
                .write(true)
                .open(directory.join("LOCK"))?;
            Some(
                crate::WriterFence::try_lock(file).map_err(|error| match error {
                    std::fs::TryLockError::WouldBlock => Error::Busy,
                    std::fs::TryLockError::Error(error) => Error::Io(error),
                })?,
            )
        };
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt;
            let metadata = fs::symlink_metadata(&directory)?;
            (metadata.dev(), metadata.ino())
        };
        #[cfg(windows)]
        let identity = super::coordination_directory_identity(&directory)?;
        #[cfg(not(any(unix, windows)))]
        let identity = (0, 0);
        Ok(Self {
            directory,
            identity,
            creator: std::process::id(),
            _writer: writer,
        })
    }
    fn check(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited copy bootstrap"));
        }
        let metadata = fs::symlink_metadata(&self.directory)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Corrupt("copy bootstrap directory"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if (metadata.dev(), metadata.ino()) != self.identity {
                return Err(Error::Corrupt("copy bootstrap namespace changed"));
            }
        }
        #[cfg(windows)]
        if super::coordination_directory_identity(&self.directory)? != self.identity {
            return Err(Error::Corrupt("copy bootstrap namespace changed"));
        }
        Ok(())
    }
    fn unselected(&self) -> Result<()> {
        self.check()?;
        for name in ["CURRENT", "PREVIOUS"] {
            match fs::symlink_metadata(self.directory.join(name)) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
                Ok(_) => return Err(Error::InvalidInput("copy target already selected")),
            }
        }
        Ok(())
    }
    pub fn directory(&self) -> Result<&Path> {
        self.check()?;
        Ok(&self.directory)
    }
    /// Rebind copied v2 placement metadata only in this genuinely fresh target.
    /// Original bytes/realm/inventory are authenticated first. V1 is unchanged;
    /// external targets cannot claim the v2 kernel-lease writer protocol.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub fn rebind_imported_fixed_layout(&self, roots: &[&Manifest]) -> Result<()> {
        self.check()?;
        crate::fixed_map::FixedMap::rebind_imported_layout(
            &self.directory,
            roots,
            self._writer.as_deref(),
            || self.check(),
        )?;
        self.check()
    }
    /// Caller has fenced every referenced target asset and audited both graphs.
    /// Preparation authenticates descriptors; publication performs only selection.
    pub fn prepare(
        self,
        selected: PreparedPublication,
        previous: Option<PreparedPublication>,
    ) -> Result<PreparedBootstrapPair> {
        self.unselected()?;
        if selected.data_policy != crate::Durability::Full
            || !selected.matches_directory(&self.directory)?
        {
            return Err(Error::InvalidInput("copy bootstrap selected token"));
        }
        let mode = crate::read_mode::ReadMode::Sync;
        let current = read_checkpoint_inner(
            &self.directory,
            None,
            &selected.pointer,
            mode.clone().with_transform(selected.transform.clone()),
        )?;
        if !(2..=4).contains(&current.format_minor) {
            return Err(Error::InvalidInput("unsupported copy bootstrap format"));
        }
        let previous_pointer = if let Some(previous) = previous {
            if previous.data_policy != crate::Durability::Full
                || !previous.matches_directory(&self.directory)?
            {
                return Err(Error::InvalidInput("copy bootstrap previous token"));
            }
            let predecessor = read_checkpoint_inner(
                &self.directory,
                None,
                &previous.pointer,
                mode.with_transform(previous.transform.clone()),
            )?;
            validate_copy_pair(&current, &predecessor)?;
            Some(previous.pointer)
        } else {
            None
        };
        let result = FileDurabilityState::captured(
            &self.directory,
            &Current {
                current: selected.pointer,
                bundled: is_cohort(&selected.pointer),
                previous: if is_cohort(&selected.pointer) {
                    previous_pointer
                } else {
                    None
                },
                weak: false,
                data_debt: false,
            },
        )?;
        Ok(PreparedBootstrapPair {
            target: self,
            current: selected.pointer,
            previous: previous_pointer,
            result,
        })
    }
}
pub struct PreparedBootstrapPair {
    target: BootstrapPairTarget,
    current: [u8; 64],
    previous: Option<[u8; 64]>,
    result: FileDurabilityState,
}
impl PreparedBootstrapPair {
    /// Full strong selection; no graph, registry, Capture, or data-file IO.
    /// Before CURRENT installation a failure leaves no readable selected root.
    pub fn publish(self) -> Result<FileDurabilityState> {
        self.target.unselected()?;
        let history = if self.target._writer.is_none() {
            None
        } else {
            Some(crate::history::HistoryGate::exclusive(
                &self.target.directory,
            )?)
        };
        self.target.unselected()?;
        if let Some(history) = &history {
            history.check(&self.target.directory)?;
        }
        crate::fault_point!("copy.pair.before_previous");
        self.target.check()?;
        if let Some(history) = &history {
            history.check(&self.target.directory)?;
        }
        if !is_cohort(&self.current)
            && let Some(previous) = self.previous
        {
            replace_policy(
                &self.target.directory,
                "PREVIOUS",
                &previous,
                crate::Durability::Full,
                false,
            )?;
            ulmdb_platform::sync_directory(&self.target.directory)?;
        }
        crate::fault_point!("copy.pair.after_previous");
        crate::fault_point!("copy.pair.before_current");
        // A wait/rendezvous must not turn our owned old directory into
        // authority over a different namespace now at the same pathname.
        // This remains a known pre-CURRENT failure, outside uncertainty.
        self.target.check()?;
        if let Some(history) = &history {
            history.check(&self.target.directory)?;
        }
        let bytes = if is_cohort(&self.current) {
            bundle(self.current, self.previous).to_vec()
        } else {
            self.current.to_vec()
        };
        // From this call onwards a pointer operation can have taken effect.
        let result = (|| -> Result<()> {
            replace_policy(
                &self.target.directory,
                "CURRENT",
                &bytes,
                crate::Durability::Full,
                true,
            )?;
            crate::fault_point!("copy.pair.after_current");
            // A terminal namespace/gate change cannot certify success at the
            // requested path. CURRENT may already be selected in the old inode.
            self.target.check()?;
            if let Some(history) = &history {
                history.check(&self.target.directory)?;
            }
            Ok(())
        })();
        result.map_err(|_| Error::OutcomeUnknown)?;
        Ok(self.result)
    }
}
fn validate_copy_pair(current: &Manifest, previous: &Manifest) -> Result<()> {
    if current.format_minor != previous.format_minor {
        return Err(Error::InvalidInput("mixed copy pair format unsupported"));
    }
    if current.uuid != previous.uuid
        || current.epoch != previous.epoch
        || current.previous != previous.generation
        || current.generation == previous.generation
    {
        return Err(Error::Corrupt("copy predecessor binding"));
    }
    let business = previous.sequence.checked_add(1) == Some(current.sequence)
        && current.parent == previous.commit
        && current.commit != previous.commit;
    let maintenance = current.sequence == previous.sequence
        && current.commit == previous.commit
        && current.parent == previous.parent;
    if !(business || maintenance) {
        return Err(Error::Corrupt("copy predecessor logical lineage"));
    }
    Ok(())
}
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn read_copy_current_checkpoint(
    directory: &Path,
    history: &crate::history::HistoryGate,
) -> Result<(Manifest, [u8; 64])> {
    read_copy_current_checkpoint_with_transform(directory, history, None)
}
pub(crate) fn read_copy_current_checkpoint_with_transform(
    directory: &Path,
    history: &crate::history::HistoryGate,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<(Manifest, [u8; 64])> {
    history.check(directory)?;
    read_copy_current_external(directory, transform)
}
pub(crate) fn read_copy_current_external(
    directory: &Path,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<(Manifest, [u8; 64])> {
    let state = sync_current(directory)?;
    let manifest = read_checkpoint_inner(
        directory,
        None,
        &state.current,
        crate::read_mode::ReadMode::Sync.with_transform(transform),
    )?;
    check_predecessor(&state, &manifest)?;
    Ok((manifest, state.current))
}
/// One exact eligible pair under History; no logical graph IO occurs here.
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn capture_copy_roots_expected(
    directory: &Path,
    expected: &[u8; 64],
    history: &crate::history::HistoryGate,
) -> Result<(Manifest, Option<Manifest>)> {
    capture_copy_roots_expected_with_transform(directory, expected, history, None)
}
pub(crate) fn capture_copy_roots_expected_with_transform(
    directory: &Path,
    expected: &[u8; 64],
    history: &crate::history::HistoryGate,
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
) -> Result<(Manifest, Option<Manifest>)> {
    capture_copy_roots_external_inner(directory, expected, transform, || history.check(directory))
}
pub(crate) fn capture_copy_roots_external_inner(
    directory: &Path,
    expected: &[u8; 64],
    transform: Option<std::sync::Arc<crate::transform::TransformContext>>,
    check: impl Fn() -> Result<()>,
) -> Result<(Manifest, Option<Manifest>)> {
    check()?;
    let state = sync_current(directory)?;
    if &state.current != expected {
        return Err(Error::Busy);
    }
    let mode = crate::read_mode::ReadMode::Sync.with_transform(transform);
    let selected = read_checkpoint_inner(directory, None, &state.current, mode.clone())?;
    check_predecessor(&state, &selected)?;
    if !(2..=4).contains(&selected.format_minor) {
        return Err(Error::InvalidInput("unsupported copy capture format"));
    }
    let previous = if state.bundled {
        state
            .previous
            .map(|p| read_checkpoint_inner(directory, None, &p, mode))
            .transpose()?
    } else if selected.previous == 0 {
        None
    } else {
        match fs::symlink_metadata(directory.join("PREVIOUS")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
            Ok(_) => Some(read_checkpoint_inner(
                directory,
                None,
                &read_sync_pointer(&directory.join("PREVIOUS"), 64)?,
                mode,
            )?),
        }
    };
    if let Some(previous) = &previous {
        validate_copy_pair(&selected, previous)?;
    }
    check()?;
    Ok((selected, previous))
}
