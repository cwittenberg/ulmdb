//! Cold immutable-prefix validation without retaining the writer's flock/OFD.
use super::{
    Journal,
    wire::{self, BLOCK, WITNESS_OFFSETS},
};
use crate::hybrid::{
    recovery::{self, CommittedPrefix},
    redo::RootState,
};
use crate::{Error, Result};
use std::{fs::File, path::Path};
use ulmdb_platform::FileExt;
pub struct CommittedAnchor {
    file: File,
    header: wire::Bootstrap,
    selected: wire::Witness,
    root: RootState,
    layout: wire::WalLayout,
}
impl CommittedAnchor {
    /// Reads only the immutable selected prefix. Mutable witness blocks may
    /// advance concurrently and are intentionally not its authority source.
    pub fn verify_seeded(
        &self,
        aliases: &crate::hybrid::retirement::CheckpointAliases,
    ) -> Result<crate::hybrid::retirement::SeededPrefix> {
        crate::hybrid::retirement::inspect_anchor(
            &self.file,
            self.header,
            self.layout,
            self.selected,
            self.root,
            aliases,
        )
    }
    pub(super) fn verify_compound(
        &self,
        aliases: &crate::hybrid::retirement::CheckpointAliases,
        compound: crate::hybrid::retirement::selection::Compound,
    ) -> Result<crate::hybrid::retirement::SeededPrefix> {
        crate::hybrid::retirement::inspect_compound_anchor(
            &self.file,
            self.header,
            self.layout,
            self.selected,
            self.root,
            aliases,
            compound,
        )
    }
    pub fn verify(&self) -> Result<CommittedPrefix> {
        recovery::inspect_prefix(
            &self.file,
            self.header,
            self.layout,
            self.selected,
            self.root,
        )
    }
}
impl Journal {
    pub fn try_retirement_lease(
        &mut self,
        path: &Path,
    ) -> Result<crate::hybrid::retirement::ExclusiveLease> {
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        if self.file.metadata()?.len() != self.layout.physical_end(self.selected.receipt.end()?) {
            self.poisoned = true;
            return Err(Error::Corrupt("retirement lease live WAL extent"));
        }
        let lease = crate::hybrid::retirement::ExclusiveLease::try_acquire(path, &self.file);
        if matches!(&lease, Err(Error::Corrupt(_)))
            || matches!(&lease, Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound)
        {
            self.poisoned = true;
        }
        lease
    }
    pub fn capture_committed(&mut self, path: &Path) -> Result<CommittedAnchor> {
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        for (expected, offset) in self.cached.iter().zip(WITNESS_OFFSETS) {
            let mut actual = [0; BLOCK];
            self.file.read_exact_at(&mut actual, offset)?;
            if *expected != actual {
                self.poisoned = true;
                return Err(Error::Corrupt("hybrid checkpoint live witness changed"));
            }
        }
        if self.file.metadata()?.len() != self.layout.physical_end(self.selected.receipt.end()?) {
            self.poisoned = true;
            return Err(Error::Corrupt("hybrid checkpoint WAL has private tail"));
        }
        // File::open creates a distinct OFD; try_clone/dup would retain flock.
        let file = File::open(path)?;
        let own = ulmdb_platform::file_identity(&self.file)?;
        let opened = ulmdb_platform::file_identity(&file)?;
        if !own.same_object(&opened) {
            self.poisoned = true;
            return Err(Error::Corrupt("hybrid checkpoint WAL path replaced"));
        }
        crate::hybrid::retirement::shared(&file)?;
        Ok(CommittedAnchor {
            file,
            header: self.header,
            selected: self.selected,
            root: self.root,
            layout: self.layout,
        })
    }
}
#[cfg(test)]
mod retirement_tests {
    use super::*;
    #[test]
    fn retirement_inode_drift_poisoning_is_distinct_from_busy() {
        let directory = std::env::temp_dir().join(format!(
            "ulmdb-lease-inode-{}",
            crate::new_identity().unwrap()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("wal");
        let header = wire::Bootstrap {
            uuid: [1; 16],
            epoch: 1,
            commit: 1,
        };
        let mut journal = Journal::create(&path, header).unwrap();
        let anchor = journal.capture_committed(&path).unwrap();
        assert!(matches!(
            journal.try_retirement_lease(&path),
            Err(Error::Busy)
        ));
        assert!(!journal.is_poisoned());
        drop(anchor);
        std::fs::rename(&path, directory.join("old")).unwrap();
        let replacement = Journal::create(&path, header).unwrap();
        assert!(matches!(
            journal.try_retirement_lease(&path),
            Err(Error::Corrupt(_))
        ));
        assert!(journal.is_poisoned());
        drop(replacement);
        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
