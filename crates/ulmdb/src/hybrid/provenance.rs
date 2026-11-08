//! Writer-only active chunk plus immutable sealed provenance. No whole-history
//! clone on commit, rotation or capture; pending entries stay selected-sequence gated.
use super::Snapshot;
use crate::{Error, Result};
use std::cell::{Cell, RefCell};
use ulmdb_core::storage::{ReadStore, Reference};
pub(super) use ulmdb_storage::hybrid::provenance::Origin as Allocation;
use ulmdb_storage::hybrid::{
    ImageTable,
    provenance::{ActiveProvenance, AppendMark, ChunkIdentity, ProvenanceDirectory},
    redo::{Extent, SourceReceipt, SourceResolver},
};
#[derive(Default)]
pub(super) struct Ledger {
    active: RefCell<Option<ActiveProvenance>>,
    sealed: ProvenanceDirectory,
    poisoned: Cell<bool>,
}
impl Ledger {
    pub fn from_directory(directory: ProvenanceDirectory) -> Self {
        Self {
            sealed: directory,
            ..Self::default()
        }
    }
    pub fn new(identity: ChunkIdentity) -> Result<Self> {
        Ok(Self {
            active: RefCell::new(Some(ActiveProvenance::new(identity)?)),
            ..Self::default()
        })
    }
    pub fn healthy(&self) -> Result<()> {
        if self.poisoned.get() {
            Err(Error::OutcomeUnknown)
        } else {
            Ok(())
        }
    }
    pub fn directory(&self) -> &ProvenanceDirectory {
        &self.sealed
    }
    pub fn check_rotate(&self, end: u64, lsn: u64, sequence: u64) -> Result<()> {
        self.healthy()?;
        if let Some(active) = self.active.borrow().as_ref() {
            active.check_seal(end, lsn, sequence)?;
            if self.sealed.len() >= 1_048_575 {
                return Err(Error::LimitExceeded);
            }
            if self.sealed.get(active.identity().chunk).is_some() {
                return Err(Error::Corrupt("hybrid active chunk already sealed"));
            }
        }
        Ok(())
    }
    pub fn rotate(
        &mut self,
        identity: ChunkIdentity,
        end: u64,
        lsn: u64,
        sequence: u64,
    ) -> Result<()> {
        self.check_rotate(end, lsn, sequence)?;
        identity.validate()?;
        if let Some(old) = self.active.get_mut().as_ref()
            && (identity.uuid != old.identity().uuid
                || identity.epoch != old.identity().epoch
                || identity.chunk <= old.identity().chunk)
        {
            return Err(Error::InvalidInput(
                "hybrid chunk provenance rotation identity/order",
            ));
        }
        if self.sealed.get(identity.chunk).is_some() {
            return Err(Error::Corrupt("hybrid reused sealed chunk identity"));
        }
        let next = ActiveProvenance::new(identity)?;
        if let Some(old) = self.active.get_mut().take() {
            let frozen = old.seal(end, lsn, sequence)?;
            self.sealed = self.sealed.insert(frozen)?;
        }
        *self.active.get_mut() = Some(next);
        Ok(())
    }
    fn floor(&self, at: Reference) -> Option<Allocation> {
        if let Some(active) = self.active.borrow().as_ref()
            && active.identity().chunk == at.segment
        {
            return active.floor(at).copied();
        }
        self.sealed.get(at.segment)?.floor(at).copied()
    }
    #[cfg(test)]
    fn len(&self) -> usize {
        self.active
            .borrow()
            .as_ref()
            .map_or(0, ActiveProvenance::len)
            + self.sealed.iter().map(|c| c.len()).sum::<usize>()
    }
    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
pub(super) fn allocation(ledger: &Ledger, at: Reference) -> Option<Allocation> {
    if let Some(active) = ledger.active.borrow().as_ref()
        && active.identity().chunk == at.segment
    {
        return active.get(at).copied();
    }
    ledger.sealed.get(at.segment)?.get(at).copied()
}
pub(super) struct Pending<'a> {
    ledger: &'a Ledger,
    mark: Option<AppendMark>,
    maximum: usize,
    inserted: usize,
}
impl<'a> Pending<'a> {
    pub fn new(ledger: &'a Ledger, maximum: usize) -> Result<Self> {
        ledger.healthy()?;
        let mark = ledger
            .active
            .borrow()
            .as_ref()
            .ok_or(Error::Corrupt("hybrid active provenance absent"))?
            .append_mark();
        Ok(Self {
            ledger,
            mark: Some(mark),
            maximum,
            inserted: 0,
        })
    }
    pub fn insert(&mut self, a: Allocation) -> Result<()> {
        if self.inserted >= self.maximum {
            return Err(Error::LimitExceeded);
        }
        self.ledger
            .active
            .borrow_mut()
            .as_mut()
            .ok_or(Error::Corrupt("hybrid active provenance absent"))?
            .insert(a)?;
        self.inserted += 1;
        Ok(())
    }
    pub fn accept(mut self) {
        self.mark = None;
    }
}
impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if let Some(mark) = self.mark.take() {
            let result = self
                .ledger
                .active
                .borrow_mut()
                .as_mut()
                .ok_or(Error::Corrupt("hybrid active provenance absent"))
                .and_then(|a| a.rollback_append(mark));
            if result.is_err() {
                self.ledger.poisoned.set(true);
            }
        }
    }
}
pub(super) struct Sources<'a> {
    pub image: &'a ImageTable,
    pub ledger: &'a Ledger,
    pub sequence: u64,
}
impl SourceResolver for Sources<'_> {
    fn resolve_committed<'a>(
        &'a self,
        source: &Extent,
        receipt: &SourceReceipt,
    ) -> Result<&'a [u8]> {
        self.ledger.healthy()?;
        let known = allocation(self.ledger, source.at)
            .ok_or(Error::Corrupt("hybrid source allocation absent"))?;
        if known.extent != *source
            || known.receipt != *receipt
            || receipt.creation_sequence > self.sequence
        {
            return Err(Error::Corrupt(
                "hybrid source authority/allocation mismatch",
            ));
        }
        self.image.read(source.at, source.length as usize)
    }
}
impl<'a> Sources<'a> {
    pub fn for_snapshot(snapshot: &'a Snapshot, ledger: &'a Ledger) -> Self {
        Self {
            image: &snapshot.image,
            ledger,
            sequence: snapshot.receipt.sequence,
        }
    }
}
pub(super) struct CheckedImage<'a, S: ReadStore = ImageTable> {
    pub image: &'a S,
    pub ledger: &'a Ledger,
}
impl<S: ReadStore> ReadStore for CheckedImage<'_, S> {
    fn max_value_size(&self) -> usize {
        self.image.max_value_size()
    }
    fn page_size(&self) -> usize {
        self.image.page_size()
    }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        self.ledger.healthy()?;
        let known = self
            .ledger
            .floor(at)
            .ok_or(Error::Corrupt("hybrid reference is outside an allocation"))?;
        let end = at
            .offset
            .checked_add(length as u64)
            .ok_or(Error::Corrupt("hybrid reference extent overflow"))?;
        if known.extent.at.segment != at.segment || end > known.extent.end()? {
            return Err(Error::Corrupt("hybrid reference crosses allocation"));
        }
        self.image.read(at, length)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};
    use ulmdb_core::storage::WriteStore;
    use ulmdb_platform::FileExt;
    use ulmdb_storage::hybrid::{
        ChunkLimits, ChunkWriter,
        journal::{Bootstrap, Journal},
        redo::{Authority, ChunkDelta, ObjectRedo, Record, RootState},
    };
    struct Dir(PathBuf);
    impl Dir {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "ulmdb-ledger-{}",
                ulmdb_storage::new_identity().unwrap()
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn receipt(sequence: u64) -> SourceReceipt {
        SourceReceipt {
            uuid: [7; 16],
            epoch: 1,
            creation_lsn: sequence,
            creation_sequence: sequence,
            creation_commit: 100 + u128::from(sequence),
            provenance_digest: [sequence as u8; 32],
            authority: Authority::Wal {
                lsn: sequence,
                sequence,
                digest: [sequence as u8; 32],
            },
        }
    }
    fn limits() -> ChunkLimits {
        ChunkLimits {
            capacity: 65536,
            reserve_quantum: 65536,
            max_burned_bytes: 65536,
        }
    }
    #[test]
    fn pending_authority_is_gated_revoked_exactly_and_metadata_borrows_end() {
        let dir = Dir::new();
        let mut writer = ChunkWriter::create(dir.0.join("image"), 1, limits()).unwrap();
        let initial = ImageTable::default()
            .with_active(writer.read_view())
            .unwrap();
        let mut a = writer.begin(initial.clone()).unwrap();
        let at = a.put_payload(b"old").unwrap();
        let f = a.freeze().unwrap();
        let base = initial.with_active(f.view()).unwrap();
        f.accept();
        let old = Allocation {
            extent: Extent {
                at,
                logical_generation: 2,
                length: 19,
            },
            receipt: receipt(1),
        };
        let ledger = Ledger::new(ChunkIdentity {
            uuid: [7; 16],
            epoch: 1,
            chunk: 1,
            logical_generation: 2,
            capacity: 65536,
        })
        .unwrap();
        let mut first = Pending::new(&ledger, 1).unwrap();
        first.insert(old).unwrap();
        first.accept();
        let mut a = writer.begin(base.clone()).unwrap();
        let at = a.put_payload(b"new").unwrap();
        let f = a.freeze().unwrap();
        let candidate = base.with_active(f.view()).unwrap();
        let new = Allocation {
            extent: Extent {
                at,
                logical_generation: 2,
                length: 19,
            },
            receipt: receipt(2),
        };
        let sources = Sources {
            image: &candidate,
            ledger: &ledger,
            sequence: 1,
        };
        let held = sources
            .resolve_committed(&old.extent, &old.receipt)
            .unwrap();
        {
            let mut pending = Pending::new(&ledger, 2).unwrap();
            pending.insert(new).unwrap();
            assert_eq!(ledger.len(), 2);
            assert!(
                sources
                    .resolve_committed(&new.extent, &new.receipt)
                    .is_err()
            );
            // The old byte borrow must not retain a RefCell metadata guard.
            assert_eq!(&held[16..], b"old");
            assert!(pending.insert(old).is_err());
        }
        assert_eq!(ledger.len(), 1);
        assert!(allocation(&ledger, new.extent.at).is_none());
        assert_eq!(
            allocation(&ledger, old.extent.at).unwrap().receipt,
            old.receipt
        );
        let mut accepted = Pending::new(&ledger, 1).unwrap();
        accepted.insert(new).unwrap();
        accepted.accept();
        let selected = Sources {
            image: &candidate,
            ledger: &ledger,
            sequence: 2,
        };
        assert_eq!(
            &selected
                .resolve_committed(&new.extent, &new.receipt)
                .unwrap()[16..],
            b"new"
        );
        let mut wrong = new.receipt;
        wrong.creation_commit += 1;
        assert!(selected.resolve_committed(&new.extent, &wrong).is_err());
        let mut wrong_extent = new.extent;
        wrong_extent.logical_generation += 1;
        assert!(
            selected
                .resolve_committed(&wrong_extent, &new.receipt)
                .is_err()
        );
        let checked = CheckedImage {
            image: &candidate,
            ledger: &ledger,
        };
        assert!(
            checked
                .read(
                    Reference {
                        segment: 1,
                        offset: old.extent.end().unwrap()
                    },
                    1
                )
                .is_err()
        );
        assert_eq!(&held[16..], b"old");
        drop(f);
    }
    #[test]
    fn actual_receipt_callback_failure_or_late_preflight_revokes_future_entries() {
        for callback_refuses in [true, false] {
            let dir = Dir::new();
            let mut writer = ChunkWriter::create(dir.0.join("image"), 1, limits()).unwrap();
            let base = ImageTable::default()
                .with_active(writer.read_view())
                .unwrap();
            let mut arena = writer.begin(base.clone()).unwrap();
            let at = arena.put_payload(b"pending").unwrap();
            let frozen = arena.freeze().unwrap();
            let header = Bootstrap {
                uuid: [7; 16],
                epoch: 1,
                commit: 100,
            };
            let path = dir.0.join("wal");
            let mut journal = Journal::create(&path, header).unwrap();
            let prior = journal.selected();
            let dest = Extent {
                at,
                logical_generation: 2,
                length: 23,
            };
            let record = Record {
                uuid: header.uuid,
                epoch: 1,
                lsn: 1,
                sequence: 1,
                commit: 101,
                parent_commit: 100,
                parent: prior,
                root: RootState {
                    catalog: Reference::NULL,
                    next_chunk: 2,
                    next_database: 1,
                },
                chunks: vec![ChunkDelta {
                    chunk: 1,
                    logical_generation: 2,
                    before: 4096,
                    after: frozen.end() as u64,
                    capacity: 65536,
                }],
                objects: vec![ObjectRedo::Literal {
                    dest,
                    bytes: frozen.view().read(at.offset as usize, 23).unwrap().to_vec(),
                }],
            };
            if !callback_refuses {
                let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
                file.write_all_at(&[1], file.metadata().unwrap().len())
                    .unwrap();
            }
            let ledger = Ledger::new(ChunkIdentity {
                uuid: [7; 16],
                epoch: 1,
                chunk: 1,
                logical_generation: 2,
                capacity: 65536,
            })
            .unwrap();
            let sources = Sources {
                image: &base,
                ledger: &ledger,
                sequence: 0,
            };
            let mut called = false;
            {
                let mut pending = Pending::new(&ledger, 1).unwrap();
                let result = journal.commit_checked_with_receipt(&record, &sources, |_, future| {
                    called = true;
                    let mut authority = receipt(1);
                    authority.provenance_digest = future.digest;
                    authority.authority = Authority::Wal {
                        lsn: 1,
                        sequence: 1,
                        digest: future.digest,
                    };
                    pending.insert(Allocation {
                        extent: dest,
                        receipt: authority,
                    })?;
                    assert!(sources.resolve_committed(&dest, &authority).is_err());
                    if callback_refuses {
                        return Err(Error::InvalidInput("test callback refusal"));
                    }
                    Ok(())
                });
                assert!(result.is_err());
                assert!(called);
                assert_eq!(ledger.len(), 1);
            }
            assert!(ledger.is_empty());
            assert_eq!(journal.selected(), prior);
            assert_eq!(journal.is_poisoned(), !callback_refuses);
        }
    }
}
