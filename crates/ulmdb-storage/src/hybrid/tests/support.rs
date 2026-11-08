use crate::hybrid::{
    journal::{Bootstrap, Journal},
    redo::*,
};
use crate::{Error, ReadStore, Reference, Result, WriteStore};
use std::collections::BTreeMap;

pub const HEADER: Bootstrap = Bootstrap {
    uuid: [7; 16],
    epoch: 3,
    commit: 17,
};
pub const GENERATION: u128 = 91;
#[derive(Clone)]
pub struct Store {
    pub bytes: Vec<u8>,
    pub allocations: BTreeMap<Reference, (usize, Option<Reference>)>,
    pub private_from: u64,
}
impl Store {
    pub fn new() -> Self {
        Self {
            bytes: vec![0; 4096],
            allocations: BTreeMap::new(),
            private_from: 4096,
        }
    }
    pub fn freeze(&mut self) {
        self.private_from = self.bytes.len() as u64;
    }
    pub fn extent(&self, at: Reference) -> Extent {
        Extent {
            at,
            logical_generation: GENERATION,
            length: self.allocations[&at].0 as u32,
        }
    }
    pub fn record(
        &self,
        journal: &Journal,
        catalog: Reference,
        sources: &Registry,
        compact: bool,
    ) -> Record {
        let mut objects = Vec::new();
        for (&at, &(len, base)) in &self.allocations {
            if at.offset < self.private_from {
                continue;
            }
            let dest = self.extent(at);
            let bytes = self.read(at, len).unwrap().to_vec();
            if compact
                && let Some(base) = base
                && let Some(origin) = sources.0.get(&base)
                && origin.extent.length == dest.length
            {
                objects.push(
                    ObjectRedo::from_committed_bytes(
                        dest,
                        origin.extent,
                        origin.receipt,
                        &origin.bytes,
                        &bytes,
                    )
                    .unwrap(),
                );
            } else {
                objects.push(ObjectRedo::Literal { dest, bytes });
            }
        }
        Record {
            uuid: HEADER.uuid,
            epoch: HEADER.epoch,
            lsn: journal.selected().lsn + 1,
            sequence: journal.selected().sequence + 1,
            commit: 100 + journal.selected().lsn as u128,
            parent_commit: journal.commit_id(),
            parent: journal.selected(),
            root: RootState {
                catalog,
                next_chunk: 2,
                next_database: 4,
            },
            chunks: vec![ChunkDelta {
                chunk: 1,
                logical_generation: GENERATION,
                before: self.private_from,
                after: self.bytes.len() as u64,
                capacity: MAX_CHUNK_BYTES,
            }],
            objects,
        }
    }
}
impl ReadStore for Store {
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("test chunk"));
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        let end = start.checked_add(len).ok_or(Error::LimitExceeded)?;
        self.bytes
            .get(start..end)
            .ok_or(Error::Corrupt("test read extent"))
    }
}
impl WriteStore for Store {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        let at = self.bytes.len().div_ceil(alignment) * alignment;
        self.bytes.resize(at, 0);
        self.bytes.extend_from_slice(bytes);
        let reference = Reference {
            segment: 1,
            offset: at as u64,
        };
        self.allocations.insert(reference, (bytes.len(), None));
        Ok(reference)
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        if old.offset >= self.private_from
            && self
                .allocations
                .get(&old)
                .is_some_and(|(n, _)| *n == bytes.len())
        {
            let at = old.offset as usize;
            self.bytes[at..at + bytes.len()].copy_from_slice(bytes);
            return Ok(old);
        }
        let at = self.allocate(bytes, alignment)?;
        if self
            .allocations
            .get(&old)
            .is_some_and(|(n, _)| *n == bytes.len())
        {
            self.allocations.get_mut(&at).unwrap().1 = Some(old);
        }
        Ok(at)
    }
}
impl DestinationResolver for Store {
    fn resolve_prepared(&self, d: &Extent) -> Result<&[u8]> {
        if *d != self.extent(d.at) || d.at.offset < self.private_from {
            return Err(Error::Corrupt("test unprepared object"));
        }
        self.read(d.at, d.length as usize)
    }
}
pub struct Source {
    pub extent: Extent,
    pub receipt: SourceReceipt,
    pub bytes: Vec<u8>,
}
#[derive(Default)]
pub struct Registry(pub BTreeMap<Reference, Source>);
impl Registry {
    pub fn register(
        &mut self,
        record: &Record,
        receipt: WalReceipt,
        objects: Vec<MaterializedObject>,
    ) {
        for object in objects {
            self.0.insert(
                object.extent.at,
                Source {
                    extent: object.extent,
                    bytes: object.bytes,
                    receipt: SourceReceipt {
                        uuid: record.uuid,
                        epoch: record.epoch,
                        creation_lsn: record.lsn,
                        creation_sequence: record.sequence,
                        creation_commit: record.commit,
                        provenance_digest: receipt.digest,
                        authority: Authority::Wal {
                            lsn: receipt.lsn,
                            sequence: receipt.sequence,
                            digest: receipt.digest,
                        },
                    },
                },
            );
        }
    }
}
impl SourceResolver for Registry {
    fn resolve_committed<'a>(
        &'a self,
        extent: &Extent,
        receipt: &SourceReceipt,
    ) -> Result<&'a [u8]> {
        let source = self
            .0
            .get(&extent.at)
            .ok_or(Error::Corrupt("unretained test source"))?;
        if source.extent != *extent || source.receipt != *receipt {
            return Err(Error::Corrupt("test source exact authority"));
        }
        Ok(&source.bytes)
    }
}
pub fn small_record(journal: &Journal) -> Record {
    Record {
        uuid: HEADER.uuid,
        epoch: HEADER.epoch,
        lsn: journal.selected().lsn + 1,
        sequence: journal.selected().sequence + 1,
        commit: 100 + journal.selected().lsn as u128,
        parent_commit: journal.commit_id(),
        parent: journal.selected(),
        root: RootState {
            catalog: Reference {
                segment: 1,
                offset: 4096 + journal.selected().sequence * 64,
            },
            next_chunk: 2,
            next_database: 1,
        },
        chunks: vec![ChunkDelta {
            chunk: 1,
            logical_generation: GENERATION,
            before: 4096 + journal.selected().sequence * 64,
            after: 4160 + journal.selected().sequence * 64,
            capacity: MAX_CHUNK_BYTES,
        }],
        objects: vec![ObjectRedo::Literal {
            dest: Extent {
                at: Reference {
                    segment: 1,
                    offset: 4096 + journal.selected().sequence * 64,
                },
                logical_generation: GENERATION,
                length: 64,
            },
            bytes: vec![journal.selected().sequence as u8; 64],
        }],
    }
}
pub struct Temp(pub std::path::PathBuf);
impl Temp {
    pub fn new(base: &std::path::Path, label: &str) -> Self {
        // The generic WAL cases retain their Unix path labels there. Windows
        // exercises those same assertions on its actual local temporary root;
        // this does not claim a Windows tmpfs/filesystem comparison.
        #[cfg(windows)]
        let local = [std::path::Path::new("/tmp"), std::path::Path::new("/dev/shm")]
            .contains(&base).then(std::env::temp_dir);
        #[cfg(windows)]
        let base = local.as_deref().unwrap_or(base);
        static ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = base.join(format!(
            "hybrid-{label}-{}-{}",
            std::process::id(),
            ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    pub fn wal(&self) -> std::path::PathBuf {
        self.0.join("WAL")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
