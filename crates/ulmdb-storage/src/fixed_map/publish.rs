//! Numeric placement metadata follows its requested policy. Prepare bytes and
//! file fences before taking the same short History gate as force-sync. External
//! callers supply genuine namespace authority and use no kernel coordination.
use super::*;
#[derive(Clone, Default)]
pub(crate) struct Options {
    pub policy: crate::Durability,
    pub external: Option<crate::external::ExternalSynchronization>,
}
impl Options {
    pub(crate) fn check(&self, directory: &Path, uuid: [u8; 16], epoch: u64) -> Result<()> {
        if let Some(external) = &self.external {
            external.check(directory, uuid, epoch)?;
        }
        Ok(())
    }
}
struct Gate {
    history: Option<crate::history::HistoryGate>,
    _capture: Option<crate::coordination::CaptureLease>,
}
impl Gate {
    fn acquire(directory: &Path, uuid: [u8; 16], epoch: u64, options: &Options) -> Result<Self> {
        options.check(directory, uuid, epoch)?;
        if options.external.is_some() {
            return Ok(Self {
                _capture: None,
                history: None,
            });
        }
        let (_, capture) =
            crate::coordination::Coordination::capture_directory(directory, Default::default())?
                .bind(uuid)?;
        let history = crate::history::HistoryGate::exclusive(directory)?;
        capture.sync_namespace()?;
        history.check(directory)?;
        Ok(Self {
            _capture: Some(capture),
            history: Some(history),
        })
    }
    fn check(&self, directory: &Path, uuid: [u8; 16], epoch: u64, options: &Options) -> Result<()> {
        options.check(directory, uuid, epoch)?;
        if let Some(capture) = &self._capture {
            capture.sync_namespace()?;
        }
        if let Some(history) = &self.history {
            history.check(directory)?;
        }
        Ok(())
    }
}
pub(crate) fn replace(
    directory: &Path,
    uuid: [u8; 16],
    epoch: u64,
    bytes: &[u8],
    expected: host::FileToken,
    options: &Options,
    check: impl Fn() -> Result<()>,
) -> Result<()> {
    replace_inner(
        directory, uuid, epoch, bytes, expected, options, None, check,
    )
}
pub(crate) fn replace_under_maintenance(
    directory: &Path,
    uuid: [u8; 16],
    epoch: u64,
    bytes: &[u8],
    expected: host::FileToken,
    maintenance: &crate::coordination::MaintenanceLease,
    check: impl Fn() -> Result<()>,
) -> Result<()> {
    replace_inner(
        directory,
        uuid,
        epoch,
        bytes,
        expected,
        &Options::default(),
        Some(maintenance),
        check,
    )
}
fn replace_inner(
    directory: &Path,
    uuid: [u8; 16],
    epoch: u64,
    bytes: &[u8],
    expected: host::FileToken,
    options: &Options,
    maintenance: Option<&crate::coordination::MaintenanceLease>,
    check: impl Fn() -> Result<()>,
) -> Result<()> {
    crate::durability::check_policy(options.policy)?;
    options.check(directory, uuid, epoch)?;
    if let Some(maintenance) = maintenance {
        maintenance.fixed_reuse_check(directory, uuid)?;
    }
    check()?;
    crate::fault_point!("fixed.layout.before_file_open");
    let target = directory.join("FIXEDMAP");
    let before = host::readonly(&target)?;
    if host::token(&before)? != expected {
        return Err(Error::Busy);
    }
    let (temporary, mut file) = crate::temporary::create(directory, "fixed-layout")?;
    let result = (|| {
        file.write_all(bytes)?;
        if options.policy.sync_pointer_file() {
            crate::fault_point!("fixed.layout.before_file_sync");
            ulmdb_platform::sync_file(&file)?;
        }
        crate::fault_point!("fixed.layout.before_gate");
        let gate = if let Some(maintenance) = maintenance {
            maintenance.fixed_reuse_check(directory, uuid)?;
            // Caller already retains Capture EX. Reacquiring Capture SH on a
            // fresh description would self-deadlock; only History is nested.
            let history = crate::history::HistoryGate::exclusive(directory)?;
            maintenance.fixed_reuse_check(directory, uuid)?;
            Gate {
                history: Some(history),
                _capture: None,
            }
        } else {
            Gate::acquire(directory, uuid, epoch, options)?
        };
        check()?;
        gate.check(directory, uuid, epoch, options)?;
        host::check_regular(&target, &before)?;
        if let Some(maintenance) = maintenance {
            maintenance.fixed_reuse_check(directory, uuid)?;
        }
        crate::fault_point!("fixed.layout.before_rename");
        ulmdb_platform::rename_replace(&temporary, &target).map_err(|error| {
            if ulmdb_platform::rename_outcome_unknown(&error) {
                Error::OutcomeUnknown
            } else {
                error.into()
            }
        })?;
        host::check_regular(&target, &file)?;
        if options.policy.sync_pointer_directory() {
            crate::fault_point!("fixed.layout.before_directory_sync");
            ulmdb_platform::sync_directory(directory)?;
        }
        gate.check(directory, uuid, epoch, options)?;
        if let Some(maintenance) = maintenance {
            maintenance.fixed_reuse_check(directory, uuid)?;
        }
        check()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
