//! Selected-asset failures fence writes only while this exact authority remains
//! selected. Caller input, draft publication, and operational resources are not
//! selected-asset validation failures.
use crate::hybrid::Environment;
use crate::{Error, Result};
use std::{io::ErrorKind, path::Path};
use ulmdb_storage::hybrid::checkpoint::{self, CheckpointLimits, selection::Selector};
pub(in crate::hybrid) fn integrity_failure(error: &Error) -> bool {
    match error {
        Error::LimitExceeded | Error::Busy => false,
        Error::Io(e) => {
            !matches!(
                e.kind(),
                ErrorKind::OutOfMemory | ErrorKind::WouldBlock | ErrorKind::Interrupted
            ) && !ulmdb_platform::is_resource_exhaustion(e)
        }
        // InvalidInput here can only describe invalid decoded persisted data
        // (e.g. ChunkIdentity). Caller/issuer/draft input is checked outside.
        _ => true,
    }
}
pub(in crate::hybrid) fn selected_result<T>(
    env: &Environment,
    expected: Selector,
    result: Result<T>,
) -> Result<T> {
    if let Err(error) = &result
        && integrity_failure(error)
        && let Ok(mut writer) = env.writer.lock()
        && writer.recovery_base == Some(expected)
    {
        writer.checkpoint_poisoned = true;
    }
    result
}
pub(in crate::hybrid) fn protocol(
    env: &Environment,
) -> Result<Option<ulmdb_storage::hybrid::retirement::selection::RecoveryBase>> {
    use ulmdb_storage::hybrid::retirement::selection::{RecoveryBase, read_typed};
    let expected = {
        let writer = env.writer.lock().map_err(|_| Error::Busy)?;
        match (&writer.seed, writer.recovery_base) {
            (Some(seed), Some(s)) if seed.compound.selector() == s => {
                Some(RecoveryBase::Compound(seed.compound))
            }
            (None, Some(s)) => Some(RecoveryBase::Legacy(s)),
            (None, None) => None,
            _ => return Err(Error::Corrupt("cached recovery-base context mismatch")),
        }
    };
    let checked = (|| {
        let required = super::super::retirement::marker::read(&env.directory)?;
        let actual = read_typed(&env.directory)?;
        if actual != expected {
            return Err(Error::Corrupt("selected recovery-base changed/missing"));
        }
        match (expected, required) {
            (Some(RecoveryBase::Compound(c)), Some(required)) => required.check(c)?,
            (Some(RecoveryBase::Compound(_)), None) => {
                return Err(Error::Corrupt("selected required-floor marker missing"));
            }
            (_, Some(_)) => {
                return Err(Error::Corrupt(
                    "required floor cannot use legacy/missing selector",
                ));
            }
            _ => {}
        }
        Ok(expected)
    })();
    if let Err(error) = &checked
        && integrity_failure(error)
        && let Ok(mut writer) = env.writer.lock()
    {
        let current = writer
            .seed
            .as_ref()
            .map(|s| RecoveryBase::Compound(s.compound))
            .or_else(|| writer.recovery_base.map(RecoveryBase::Legacy));
        if current == expected {
            writer.checkpoint_poisoned = true;
        }
    }
    checked
}
/// Some low-level readers historically report caller size limits as Corrupt.
/// Use trusted cut geometry and a bounded descriptor to classify those budgets
/// before the complete archive validator can encounter such branches.
pub(in crate::hybrid) fn budget(
    directory: &Path,
    expected: Selector,
    limits: CheckpointLimits,
) -> Result<()> {
    let count = expected
        .cut
        .root
        .next_chunk
        .checked_sub(1)
        .ok_or(Error::Corrupt("selected checkpoint chunk counter"))?;
    let count = usize::try_from(count).map_err(|_| Error::LimitExceeded)?;
    if count > limits.max_chunks {
        return Err(Error::LimitExceeded);
    }
    // This is budget preflight, not authority issuance. Exact cut/descriptor
    // correspondence remains in the existing staged/archive validators below.
    let d = checkpoint::read_descriptor(&directory.join("CHECKPOINT"), count)?;
    if d.chunks
        .iter()
        .any(|c| c.provenance_bytes > limits.max_provenance_bytes)
    {
        return Err(Error::LimitExceeded);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operational_resources_are_not_selected_integrity_failures() {
        for errno in [
            libc::ENOMEM,
            libc::EMFILE,
            libc::ENFILE,
            libc::ENOBUFS,
            libc::ENOSPC,
            #[cfg(unix)]
            libc::EDQUOT,
            libc::EAGAIN,
            libc::EINTR,
        ] {
            assert!(!integrity_failure(&Error::Io(
                std::io::Error::from_raw_os_error(errno)
            )));
        }
        for errno in [libc::ENOENT, libc::EIO, libc::ENOTDIR, libc::ELOOP] {
            assert!(integrity_failure(&Error::Io(
                std::io::Error::from_raw_os_error(errno)
            )));
        }
        assert!(!integrity_failure(&Error::LimitExceeded));
        assert!(!integrity_failure(&Error::Busy));
        assert!(integrity_failure(&Error::Corrupt("selected data")));
    }
    #[test]
    fn retryable_selected_checks_leave_the_actual_writer_ready() {
        use crate::hybrid::{ImageMode, Options};
        let path = std::env::temp_dir().join(format!(
            "ulmdb-selected-retry-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        let options = Options {
            image: ImageMode::Anonymous,
            chunk: ulmdb_storage::hybrid::ChunkLimits {
                capacity: 65536,
                reserve_quantum: 65536,
                max_burned_bytes: 32768,
            },
            max_image_bytes: 131072,
            ..Options::default()
        };
        let env = Environment::create(&path, options).unwrap();
        let mut prepared = env.prepare_checkpoint(CheckpointLimits::default()).unwrap();
        env.activate_checkpoint(&mut prepared, CheckpointLimits::default())
            .unwrap();
        let expected = env.writer.lock().unwrap().recovery_base.unwrap();
        for error in [
            Error::Busy,
            Error::LimitExceeded,
            Error::Io(std::io::Error::from_raw_os_error(libc::ENOMEM)),
            Error::Io(std::io::Error::from_raw_os_error(libc::EMFILE)),
        ] {
            assert!(selected_result::<()>(&env, expected, Err(error)).is_err());
            drop(env.begin_write().unwrap());
        }
        drop(prepared);
        drop(env);
        std::fs::remove_dir_all(path).unwrap();
    }
}
