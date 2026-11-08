use super::LockOps;
use crate::mapping::Mapping;
use std::{io, sync::OnceLock};
pub(crate) fn page_size() -> io::Result<usize> {
    static PAGE: OnceLock<usize> = OnceLock::new();
    let size = *PAGE.get_or_init(system_page_size);
    if size == 0 {
        Err(io::Error::other("system page size unavailable"))
    } else {
        Ok(size)
    }
}
fn system_page_size() -> usize {
    // The platform boundary queries sysconf or GetSystemInfo. A failed query
    // retains the explicit unavailable-page-geometry error above.
    ulmdb_platform::page_size().unwrap_or(0)
}
pub(super) struct OsLocks;
impl LockOps for OsLocks {
    fn lock(&self, mapping: &Mapping) -> io::Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let Some(fixed) = mapping.fixed_lease() {
            return fixed.lock_required();
        }
        #[cfg(unix)]
        {
            mapping.bytes.lock()
        }
        #[cfg(windows)]
        {
            // SAFETY: this Mapping owns the complete immutable view throughout
            // the call. The OS rounds the same pages used by quota accounting.
            #[allow(unsafe_code)]
            unsafe {
                ulmdb_platform::lock_memory(mapping.bytes.as_ptr(), mapping.bytes.len())
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = mapping;
            Err(io::ErrorKind::Unsupported.into())
        }
    }
    fn unlock(&self, mapping: &Mapping) -> io::Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let Some(fixed) = mapping.fixed_lease() {
            return fixed.unlock();
        }
        #[cfg(unix)]
        {
            mapping.bytes.unlock()
        }
        #[cfg(windows)]
        {
            // SAFETY: the admitted Mapping still owns these mapped bytes.
            #[allow(unsafe_code)]
            unsafe {
                ulmdb_platform::unlock_memory(mapping.bytes.as_ptr(), mapping.bytes.len())
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = mapping;
            Err(io::ErrorKind::Unsupported.into())
        }
    }
}
