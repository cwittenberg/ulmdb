//! C policy flags feed actual native data/metadata fences at their own phases.
use super::*;
use std::sync::atomic::Ordering;

pub(super) const NOSYNC: u32 = 0x10000;
pub(super) const NOMETASYNC: u32 = 0x40000;

fn policy(flags: u32) -> ulmdb::Durability {
    // Pinned Windows LMDB writes metadata through its write-through meta
    // handle even under WRITEMAP|MAPASYNC. The mapped arena independently
    // samples async data policy; preserve that data debt without weakening the
    // final metadata decision. Linux keeps its established MS_ASYNC behavior.
    if flags & NOSYNC != 0
        || (!cfg!(windows) && flags & (0x80000 | 0x100000) == (0x80000 | 0x100000))
    {
        ulmdb::Durability::NoSync
    } else if flags & NOMETASYNC != 0 {
        ulmdb::Durability::NoMetaSync
    } else {
        // MAPASYNC alone does not weaken the buffered default path. Only
        // genuine WRITEMAP requests mapped async data preparation.
        ulmdb::Durability::Full
    }
}

pub(super) fn local_policy(flags: u32, modern: bool) -> ulmdb::Durability {
    // Only the pinned modern header documents transaction-local weakening.
    // Legacy keeps its existing masked/ignored non-RDONLY begin behavior.
    if modern {
        policy(flags)
    } else {
        ulmdb::Durability::Full
    }
}

impl Shared {
    pub(super) fn current_durability(&self) -> ulmdb::Durability {
        policy(self.flags.load(Ordering::Acquire))
    }

    pub(super) fn sync_selected(&self, force: bool) -> FfiResult<()> {
        // Inherited owners cannot skip or mint fresh coordination authority.
        // Check before any poisoned mutex, retained lease, or NOSYNC fastpath.
        self.check_process()?;
        if self.poisoned.load(Ordering::Acquire) {
            return Err(-30795);
        }
        let flags = self.flags.load(Ordering::Acquire);
        if !force && flags & NOSYNC != 0 {
            return Ok(());
        }
        if !force && flags & (0x80000 | 0x100000) == (0x80000 | 0x100000) {
            if let Some(external) = &self.external {
                return external
                    .flush_async(self.transform.clone())
                    .map(|_| ())
                    .map_err(code);
            }
            if let Some(coordination) = &self.coordination {
                return coordination.flush_async().map(|_| ()).map_err(code);
            }
            return ulmdb_storage::flush_async_file_environment(
                &self.directory,
                self.transform.clone(),
            )
            .map(|_| ())
            .map_err(code);
        }
        if let Some(external) = &self.external {
            return external
                .sync(self.transform.clone())
                .map(|_| ())
                .map_err(code);
        }
        if let Some(coordination) = &self.coordination {
            coordination.sync().map(|_| ()).map_err(code)
        } else {
            // NOLOCK clients own external serialization. This selected-file
            // operation still avoids native LOCK and the retained writer mutex.
            ulmdb_storage::sync_file_environment(&self.directory)
                .map(|_| ())
                .map_err(|error| code(error.into()))
        }
    }
}

impl MDB_env {
    pub(super) fn flags(&self) -> u32 {
        self.flags.load(Ordering::Acquire)
    }
}
