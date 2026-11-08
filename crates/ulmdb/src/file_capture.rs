//! Owned descriptor/pin authority with graph admission deliberately deferred.
use crate::{Error, PreviousError, ReadTxn, ResidencyPolicy, Result};
use std::path::{Path, PathBuf};
use ulmdb_storage::{
    Manifest, PreviousSelection,
    coordination::{CaptureLease, DirectoryCapture, PhysicalPin},
};
mod chunked;
mod copy;
pub use copy::{ExternalFileCopyPairCapture, FileCopyPair, FileCopyView, OwnedFileCopyPairCapture};

/// One captured file namespace and exact D/V descriptor/pin authority. A graph
/// is not trusted until consuming `admit` succeeds. Constructors consume one
/// checked directory capture; no API accepts separately mixable tuple components or callbacks.
/// No History lease escapes, and callers may release their own short metadata
/// lock before full graph/residency admission while this retains Capture/pin.
pub struct OwnedFileCapture {
    directory: PathBuf,
    identity: ulmdb_storage::coordination::DirectoryIdentity,
    selected: Manifest,
    selected_checkpoint: [u8; 64],
    selection: Option<PreviousSelection>,
    pin: PhysicalPin,
    _capture: CaptureLease,
}
fn namespace(
    directory: &Path,
    capture: &DirectoryCapture,
) -> Result<(PathBuf, ulmdb_storage::coordination::DirectoryIdentity)> {
    let directory = directory.canonicalize()?;
    let (captured_directory, identity) = capture.directory_binding()?;
    if captured_directory != directory {
        return Err(Error::InvalidInput("file capture directory namespace"));
    }
    if ulmdb_storage::range::is_selected(&directory)? {
        return Err(Error::InvalidInput("file capture does not support Range"));
    }
    Ok((directory, identity))
}
impl OwnedFileCapture {
    pub(crate) fn admit_with_coordinator(
        self, owner:&crate::CoordinatedEnvironment, advice:crate::ReadAhead,
    ) -> Result<(ReadTxn,Option<PreviousSelection>)> {
        advice.validate()?;
        self.check()?;
        let Self { directory,pin,selection,_capture,.. }=self;
        let read=owner.admit_pin(&directory,pin,advice)?;
        drop(_capture);
        Ok((read,selection))
    }
    /// Validate only the capture's directory namespace, including canonical
    /// spelling and file-backend selection. No head, pin or graph is admitted.
    pub fn check_directory(directory: impl AsRef<Path>, capture: &DirectoryCapture) -> Result<()> {
        capture.directory()?; // Creator validation precedes caller-path IO.
        namespace(directory.as_ref(), capture).map(|_| ())
    }
    pub fn current(directory: impl AsRef<Path>, capture: DirectoryCapture) -> Result<Self> {
        let (directory, identity) = namespace(directory.as_ref(), &capture)?;
        let (_, capture, pin) = capture.capture_current()?;
        if !pin.matches_directory(&directory)? {
            return Err(Error::InvalidInput("file capture pin namespace"));
        }
        let selected = pin.root().manifest().clone();
        let selected_checkpoint = *pin.root().checkpoint();
        Ok(Self {
            directory,
            identity,
            selected,
            selected_checkpoint,
            selection: None,
            pin,
            _capture: capture,
        })
    }
    pub fn previous(
        directory: impl AsRef<Path>,
        capture: DirectoryCapture,
    ) -> std::result::Result<Self, PreviousError> {
        let (directory, identity) = namespace(directory.as_ref(), &capture)?;
        if directory
            .join("REPLICA")
            .try_exists()
            .map_err(Error::from)?
        {
            return Err(Error::ReadOnly.into());
        }
        let (_, capture, selection, pin) = capture
            .capture_previous()?
            .ok_or(PreviousError::NoPrevious)?;
        if !pin.matches_directory(&directory)?
            || !selection.matches_directory(&directory)?
            || pin.root().checkpoint() != selection.previous_checkpoint()
        {
            return Err(Error::InvalidInput("previous file capture binding").into());
        }
        let selected = selection.selected().clone();
        let selected_checkpoint = *selection.selected_checkpoint();
        Ok(Self {
            directory,
            identity,
            selected,
            selected_checkpoint,
            selection: Some(selection),
            pin,
            _capture: capture,
        })
    }
    /// Turn an already captured canonical D into its coherent A choice under
    /// the same Capture. CURRENT is checked against the authenticated D pin
    /// before predecessor IO. Busy means an actual D change, never stable damage.
    pub fn select_previous(self) -> std::result::Result<Self, PreviousError> {
        self.check()?;
        if self
            .directory
            .join("REPLICA")
            .try_exists()
            .map_err(Error::from)?
        {
            return Err(Error::ReadOnly.into());
        }
        let Self {
            directory,
            identity,
            selected,
            selected_checkpoint,
            selection,
            pin,
            _capture,
        } = self;
        let (selection, pin) = if let Some(selection) = selection {
            if pin.root().checkpoint() == selection.previous_checkpoint() {
                (selection, pin)
            } else {
                drop(pin);
                let pin = _capture.pin_checkpoint(selection.previous_checkpoint())?;
                (selection, pin)
            }
        } else {
            _capture
                .capture_previous_for(pin)?
                .ok_or(PreviousError::NoPrevious)?
        };
        if selection.selected_checkpoint() != &selected_checkpoint
            || !selection.matches_directory(&directory)?
            || pin.root().checkpoint() != selection.previous_checkpoint()
        {
            return Err(Error::Corrupt("previous capture exact selection binding").into());
        }
        Ok(Self {
            directory,
            identity,
            selected,
            selected_checkpoint,
            selection: Some(selection),
            pin,
            _capture,
        })
    }
    /// Choose real selected D from this same captured pair, without consulting
    /// CURRENT. This changes the actual pinned data view, not only its ID.
    pub fn select_selected(self) -> Result<Self> {
        self.check()?;
        if self.pin.root().checkpoint() == &self.selected_checkpoint {
            return Ok(self);
        }
        let Self {
            directory,
            identity,
            selected,
            selected_checkpoint,
            selection,
            pin,
            _capture,
        } = self;
        drop(pin);
        let pin = _capture.pin_checkpoint(&selected_checkpoint)?;
        if !pin.matches_directory(&directory)? || pin.root().checkpoint() != &selected_checkpoint {
            return Err(Error::Corrupt("selected capture pin binding"));
        }
        Ok(Self {
            directory,
            identity,
            selected,
            selected_checkpoint,
            selection,
            pin,
            _capture,
        })
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn directory_identity(&self) -> ulmdb_storage::coordination::DirectoryIdentity {
        self.identity
    }
    pub fn selected_checkpoint(&self) -> &[u8; 64] {
        &self.selected_checkpoint
    }
    pub fn view_checkpoint(&self) -> &[u8; 64] {
        self.pin.root().checkpoint()
    }
    pub fn previous_checkpoint(&self) -> Option<&[u8; 64]> {
        self.selection
            .as_ref()
            .map(PreviousSelection::previous_checkpoint)
    }
    pub fn selected_sequence(&self) -> u64 {
        self.selected.sequence
    }
    pub fn view_sequence(&self) -> u64 {
        self.pin.root().manifest().sequence
    }
    /// Immutable authenticated descriptor metadata; graph admission is pending.
    pub fn selected_manifest(&self) -> &Manifest {
        &self.selected
    }
    pub fn view_manifest(&self) -> &Manifest {
        self.pin.root().manifest()
    }
    fn check(&self) -> Result<()> {
        if !self.pin.matches_directory(&self.directory)? {
            return Err(Error::Corrupt("file capture directory changed"));
        }
        if let Some(selection) = &self.selection {
            if !selection.matches_directory(&self.directory)? {
                return Err(Error::Corrupt("previous capture directory changed"));
            }
        }
        Ok(())
    }
    /// Full graph audit and residency admission use only the chosen pin's
    /// manifest, retaining Capture until the fully owned reader is ready.
    pub fn admit(self, policy: ResidencyPolicy) -> Result<(ReadTxn, Option<PreviousSelection>)> {
        self.admit_with_read_ahead(policy, crate::ReadAhead::Normal)
    }
    pub fn admit_with_read_ahead(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
    ) -> Result<(ReadTxn, Option<PreviousSelection>)> {
        self.admit_with_transform(policy, read_ahead, None)
    }
    pub fn admit_with_transform(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
    ) -> Result<(ReadTxn, Option<PreviousSelection>)> {
        self.admit_with_cache_options(policy, read_ahead, transform, None)
    }
    pub fn admit_with_cache_options(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<(ReadTxn, Option<PreviousSelection>)> {
        if self.selection.is_none() && transform.is_none() && cache.is_none() {
            let Self {
                directory,
                selected,
                selection,
                pin,
                _capture,
                ..
            } = self;
            // Keep the complete selected manifest alive through admission. In
            // particular, no discarded transform Arc can run user Drop between
            // the namespace check and its immediate typed-pin consumption.
            let read = ReadTxn::from_plain_file_capture(&directory, policy, pin, read_ahead)?;
            drop(selected);
            drop(_capture);
            return Ok((read, selection));
        }
        self.admit_full_with_cache_options(policy, read_ahead, transform, cache)
    }
    // Copy callers retain the original full path even for a plain current view.
    fn admit_full_with_cache_options(
        self,
        policy: ResidencyPolicy,
        read_ahead: crate::ReadAhead,
        transform: Option<std::sync::Arc<ulmdb_storage::transform::TransformContext>>,
        cache: Option<crate::chunked::CacheOptions>,
    ) -> Result<(ReadTxn, Option<PreviousSelection>)> {
        read_ahead.validate()?;
        self.check()?;
        let Self {
            directory,
            pin,
            selection,
            _capture,
            ..
        } = self;
        let read = ReadTxn::from_physical_pin_with_cache_options(
            &directory, policy, pin, read_ahead, transform, cache,
        )?;
        drop(_capture);
        Ok((read, selection))
    }
}

impl ReadTxn {
    /// Bind this genuine physical read to a captured namespace without another
    /// selector read or physical pin. The returned lease grants only namespace
    /// exclusion; callers must separately check currentness and their role.
    pub fn bind_capture_namespace(
        &self,
        directory: impl AsRef<Path>,
        capture: DirectoryCapture,
    ) -> Result<CaptureLease> {
        capture.directory()?; // Reject inherited capture authority first.
        if self.snapshot.range_backed || self.snapshot.external.is_some() {
            return Err(Error::InvalidInput(
                "namespace binding requires coordinated file read",
            ));
        }
        let pin = self
            .snapshot
            .physical_pin
            .as_ref()
            .ok_or(Error::InvalidInput(
                "namespace binding requires a physical file pin",
            ))?;
        // Pin creator, exact manifest and full physical directory authority are
        // checked before realm binding. No numeric sequence certifies a read.
        if !pin.matches_environment(directory.as_ref(), &self.snapshot.manifest)? {
            return Err(Error::Corrupt("snapshot physical pin mismatch"));
        }
        namespace(directory.as_ref(), &capture)?;
        let (_, capture) = capture.bind(self.snapshot.manifest.uuid)?;
        Ok(capture)
    }
}
