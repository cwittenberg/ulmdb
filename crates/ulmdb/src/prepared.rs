//! In-process file-commit preparation while retaining exclusive writer ownership.
use crate::{
    Environment, Error, Result,
    environment::{Snapshot, WriterGuard},
};
use std::sync::{Arc, atomic::Ordering};
use ulmdb_storage::ReadyPublication;

/// Whether finalization must publish even if native operations changed no bytes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CommitIntent {
    /// Preserve ordinary Rust commit's logical no-op optimization.
    #[default]
    IfChanged,
    /// Publish a real new generation/sequence even for an unchanged catalog.
    /// Compatibility facades can use this for an upstream dirty transaction.
    ForcePublication,
}

pub(crate) enum FilePublication {
    Ordinary(ReadyPublication),
    External(ulmdb_storage::ExternalReadyPublication),
    ExternalPrevious{publication:ulmdb_storage::ExternalReadyPublication,pending:Arc<std::sync::atomic::AtomicBool>},
    Previous {
        publication: ulmdb_storage::ReadyPreviousPublication,
        pending: Arc<std::sync::atomic::AtomicBool>,
    },
}
impl FilePublication {
    pub(crate) fn prepared(&self) -> &ulmdb_storage::PreparedPublication {
        match self {
            Self::Ordinary(publication) => publication.prepared(),
            Self::External(publication) => publication.prepared(),
            Self::ExternalPrevious{publication,..}=>publication.prepared(),
            Self::Previous { publication, .. } => publication.prepared(),
        }
    }
    fn is_previous(&self) -> bool {
        matches!(self, Self::Previous { .. } | Self::ExternalPrevious{..})
    }
    fn publish(
        self,
        metadata_policy: crate::Durability,
    ) -> Result<(
        Option<Arc<std::sync::atomic::AtomicBool>>,
        ulmdb_storage::FileDurabilityState,
    )> {
        match self {
            Self::Ordinary(publication) => Ok((None, publication.publish(metadata_policy)?)),
            Self::External(publication) => Ok((None, publication.publish(metadata_policy)?)),
            Self::ExternalPrevious{publication,pending}=>Ok((Some(pending),publication.publish(metadata_policy)?)),
            Self::Previous {
                publication,
                pending,
            } => Ok((Some(pending), publication.publish(metadata_policy)?)),
        }
    }
}

pub(crate) enum PreparedState {
    Noop(u64),
    Publish {
        snapshot: crate::physical::PreparedSnapshot,
        publication: FilePublication,
    },
}

/// An abandoned preparation can leave sealed immutable assets without an
/// activation/retirement event. Request discovery only; fresh root coverage at
/// a later ordinary maintenance step supplies all deletion authority.
pub(crate) struct PreparationCleanup<'env> {
    env: &'env Environment,
    armed: bool,
    creator: u32,
}
impl<'env> PreparationCleanup<'env> {
    pub(crate) fn new(env: &'env Environment, armed: bool) -> Self {
        Self {
            env,
            armed,
            creator: std::process::id(),
        }
    }
    fn disarm(&mut self) {
        self.armed = false;
    }
}
impl Drop for PreparationCleanup<'_> {
    fn drop(&mut self) {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if self.armed
            && self.creator == std::process::id()
            && !self.env.poisoned.load(Ordering::Acquire)
            && self.env.require_coordinated().is_ok()
        {
            if let Ok(mut state) = self.env.maintenance.lock() {
                state.request_orphan_discovery();
            }
        }
    }
}

/// A file transaction whose immutable data, descriptor, residency and physical
/// pin are prepared, while CURRENT and in-process readers still select its base.
///
/// The owner retains the writer guard until consuming commit, abort or drop.
/// It has no data-read or mutation interface. Dropping it releases candidate
/// mappings/pins; unpublished immutable files are reclaimed by normal verified
/// maintenance. This is not a restart-recoverable distributed transaction.
pub struct PreparedWriteTxn<'env> {
    pub(crate) env: &'env Environment,
    // Drop candidate mappings/pin and base before releasing the writer fence.
    pub(crate) state: PreparedState,
    pub(crate) local_durability: crate::Durability,
    pub(crate) prepared_data_policy: crate::Durability,
    pub(crate) maintenance_rebuilt: bool,
    pub(crate) maintenance_rewrite: Option<crate::maintenance::RewriteProgress>,
    pub(crate) cleanup: PreparationCleanup<'env>,
    pub(crate) _base: Arc<Snapshot>,
    pub(crate) _guard: WriterGuard<'env>,
}

impl PreparedWriteTxn<'_> {
    /// The sequence returned by a successful final commit.
    pub fn sequence(&self) -> u64 {
        match &self.state {
            PreparedState::Noop(sequence) => *sequence,
            PreparedState::Publish { snapshot, .. } => snapshot.sequence(),
        }
    }
    pub fn will_publish(&self) -> bool {
        matches!(self.state, PreparedState::Publish { .. })
    }
    /// Policy applied to data preparation. A later metadata-policy change does
    /// not repeat these fences. A guarded no-op owner performs no data IO.
    pub fn data_policy(&self) -> crate::Durability {
        self.prepared_data_policy
    }
    /// Select and activate the prepared candidate exactly once. Any failure
    /// after invoking selection is uncertain and poisons this environment.
    pub fn commit(self) -> Result<u64> {
        let environment_policy = self.env.durability();
        self.commit_with_metadata_policy(environment_policy)
    }
    /// A compatibility owner can supply its current environment metadata
    /// policy at final selection. The retained transaction-local weakening is
    /// still applied. This does not upgrade or repeat data preparation.
    pub fn commit_with_metadata_policy(mut self, environment_policy: crate::Durability) -> Result<u64> {
        self.env.check_external_head()?;
        let metadata_policy = environment_policy.weaken(self.local_durability);
        let PreparedState::Publish {
            snapshot,
            publication,
        } = self.state
        else {
            return Ok(self.sequence());
        };
        let sequence = snapshot.sequence();
        let previous = publication.is_previous();
        let selected: Result<_> = (|| {
            if previous {
                previous_point("previous.before_publish")?;
            }
            let selected = publication.publish(metadata_policy)?;
            if previous {
                previous_point("previous.after_publish")?;
            }
            Ok(selected)
        })();
        let (pending, durability) = match selected {
            Ok(pending) => pending,
            Err(_) => {
                self.env.poisoned.store(true, Ordering::Release);
                return Err(Error::OutcomeUnknown);
            }
        };
        let activated: Result<()> = (|| {
            if previous {
                previous_point("previous.before_activate")?;
            }
            if previous { self.env.activate_prepared(snapshot)?; }
            else { self.env.activate_prepared_business(snapshot, self.maintenance_rebuilt)?; }
            self.env.remember_file_durability(durability)?;
            if previous {
                previous_point("previous.after_activate")?;
            }
            Ok(())
        })();
        if activated.is_err() {
            self.env.poisoned.store(true, Ordering::Release);
            return Err(Error::OutcomeUnknown);
        }
        if let Some(pending) = pending {
            pending.store(false, Ordering::Release);
        }
        if let Ok(mut state) = self.env.maintenance.lock() {
            state.install_rewrite(self.maintenance_rewrite);
        }
        self.cleanup.disarm();
        // Selection is already acknowledged. Verified cleanup is best effort;
        // it never turns a definite commit into an ambiguous caller error.
        self.env.automatic_reclaim();
        Ok(sequence)
    }
    /// Abandon selection and release ownership. Existing readers remain valid.
    pub fn abort(self) {}
}

fn previous_point(name: &str) -> Result<()> {
    #[cfg(feature = "fault-injection")]
    ulmdb_storage::injected_fault(name)?;
    #[cfg(not(feature = "fault-injection"))]
    let _ = name;
    Ok(())
}
