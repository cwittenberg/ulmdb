//! Dense two-fence authoritative WAL. No image or serving ownership is exposed.
use crate::WriterFence;
mod anchor;
mod seeded;
pub use seeded::SeededAnchor;
mod fault;
pub use anchor::CommittedAnchor;
pub(crate) mod tail;
pub(crate) mod wire;
use crate::hybrid::{
    codec,
    recovery::{self, RecoveryPlan},
    redo::{DestinationResolver, Record, RootState, SourceResolver, WalReceipt},
};
use crate::{Error, Result};
pub(crate) use fault::point;
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
use ulmdb_platform::FileExt;
use wire::{BLOCK, DATA_START, WITNESS_OFFSETS};
pub use wire::{Bootstrap, WalLayout, Witness, genesis};
#[derive(Clone, Copy)]
struct Publication {
    lsn: u64,
    sequence: u64,
    commit: u128,
    root: RootState,
}
impl From<&Record> for Publication {
    fn from(r: &Record) -> Self {
        Self {
            lsn: r.lsn,
            sequence: r.sequence,
            commit: r.commit,
            root: r.root,
        }
    }
}
pub struct Journal {
    file: WriterFence,
    header: Bootstrap,
    layout: WalLayout,
    selected: Witness,
    root: RootState,
    slot: usize,
    poisoned: bool,
    cached: [[u8; BLOCK]; 2],
    encode_workspace: Vec<u8>,
}
impl Journal {
    pub fn create(path: &Path, header: Bootstrap) -> Result<Self> {
        Self::create_with_layout(path, header, WalLayout::Dense)
    }
    /// Experimental fixed-capacity mode pays full zero-fill before durable creation.
    pub fn create_with_layout(path: &Path, header: Bootstrap, layout: WalLayout) -> Result<Self> {
        let bytes = wire::encode_layout_header(header, layout)?;
        let witness = wire::genesis_layout(header, layout)?;
        let encoded = wire::encode_layout_witness(header, witness, layout);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        let file = lock(file)?;
        match layout {
            WalLayout::Dense => file.set_len(DATA_START)?,
            WalLayout::PreSized { capacity } => {
                point("hybrid.create.before_zero_fill")?;
                tail::clear(&file, 0, capacity)?;
                point("hybrid.create.after_zero_fill")?;
            }
        }
        if matches!(layout, WalLayout::PreSized { .. }) {
            point("hybrid.create.before_header")?;
        }
        file.write_all_at(&bytes, 0)?;
        if matches!(layout, WalLayout::PreSized { .. }) {
            point("hybrid.create.after_header")?;
        }
        for offset in WITNESS_OFFSETS {
            file.write_all_at(&encoded, offset)?;
        }
        if matches!(layout, WalLayout::PreSized { .. }) {
            point("hybrid.create.after_witnesses")?;
        }
        ulmdb_platform::sync_file(&file)?;
        if matches!(layout, WalLayout::PreSized { .. }) {
            point("hybrid.create.after_file_sync")?;
        }
        sync_parent(path)?;
        if matches!(layout, WalLayout::PreSized { .. }) {
            point("hybrid.create.after_parent_sync")?;
        }
        Ok(Self {
            file,
            header,
            layout,
            selected: witness,
            root: wire::empty_root(),
            slot: 0,
            poisoned: false,
            cached: [encoded; 2],
            encode_workspace: Vec::new(),
        })
    }
    /// Callback must reconstruct/validate the changed graph and admission in a fresh
    /// or verified-no-write image. It executes before ANY heal/trim/ready mutation.
    pub fn open_checked<T>(
        path: &Path,
        validate: impl FnOnce(&RecoveryPlan) -> Result<T>,
    ) -> Result<(Self, T)> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let file = lock(file)?;
        let plan = recovery::inspect(&file)?;
        let state = validate(&plan)?;
        if plan.heal {
            point("hybrid.recovery.before_heal")?;
            file.write_all_at(
                &wire::encode_layout_witness(plan.header, plan.selected, plan.layout),
                WITNESS_OFFSETS[plan.slot],
            )?;
            point("hybrid.recovery.after_heal")?;
            ulmdb_platform::sync_file(&file)?;
            point("hybrid.recovery.after_heal_sync")?;
        }
        let end = plan.selected.receipt.end()?;
        match plan.layout {
            WalLayout::Dense if file.metadata()?.len() != end => {
                point("hybrid.recovery.before_trim")?;
                file.set_len(end)?;
                point("hybrid.recovery.after_trim")?;
                ulmdb_platform::sync_file(&file)?;
                point("hybrid.recovery.after_trim_sync")?;
            }
            WalLayout::PreSized { capacity } => {
                // All graph/source checks ran before ANY mutation. Both-valid
                // witnesses permit private tail discard; one-invalid requires
                // exact successor+zero suffix in inspect before reaching here.
                point("hybrid.recovery.before_clear")?;
                tail::clear(&file, end, capacity)?;
                point("hybrid.recovery.after_clear")?;
                ulmdb_platform::sync_data(&file)?;
                point("hybrid.recovery.after_clear_sync")?;
            }
            _ => {}
        }
        // Accepted late creation residue/unsynced visible witness needs cold fences.
        point("hybrid.recovery.before_ready_file_sync")?;
        ulmdb_platform::sync_file(&file)?;
        point("hybrid.recovery.after_ready_file_sync")?;
        sync_parent(path)?;
        point("hybrid.recovery.after_ready_parent_sync")?;
        let mut cached = [[0; BLOCK]; 2];
        for (b, offset) in cached.iter_mut().zip(WITNESS_OFFSETS) {
            file.read_exact_at(b, offset)?;
        }
        let root = plan.root();
        let journal = Self {
            file,
            header: plan.header,
            layout: plan.layout,
            selected: plan.selected,
            root,
            slot: plan.slot,
            poisoned: false,
            cached,
            encode_workspace: Vec::new(),
        };
        Ok((journal, state))
    }
    pub fn layout(&self) -> WalLayout {
        self.layout
    }
    /// Physical capacity is distinct from selected logical end in PreSized mode.
    pub fn physical_bytes(&self) -> Result<u64> {
        Ok(self.file.metadata()?.len())
    }
    pub fn selected(&self) -> WalReceipt {
        self.selected.receipt
    }
    pub fn commit_id(&self) -> u128 {
        self.selected.commit
    }
    pub fn root(&self) -> RootState {
        self.root
    }
    pub fn bootstrap(&self) -> Bootstrap {
        self.header
    }
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }
    /// Source authority, materialization, graph and admission must all be ready
    /// before append. The callback must not publish a serving snapshot itself.
    pub fn commit_checked(
        &mut self,
        record: &Record,
        sources: &impl SourceResolver,
        validate_ready: impl FnOnce(&Record) -> Result<()>,
    ) -> Result<WalReceipt> {
        self.commit_checked_with_receipt(record, sources, |record, _| validate_ready(record))
    }
    /// Exposes the exact future canonical receipt before durable IO so H3 can
    /// preallocate private source/provenance entries and a complete snapshot.
    /// The receipt is not an authority until successful publication. The callback
    /// must not expose it or its objects as committed, including on failure.
    /// Canonical encoding and hashing happen once, before this callback.
    pub fn commit_checked_with_receipt(
        &mut self,
        record: &Record,
        sources: &impl SourceResolver,
        validate_ready: impl FnOnce(&Record, WalReceipt) -> Result<()>,
    ) -> Result<WalReceipt> {
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        record.validate_sources(sources)?;
        self.check_base(record)?;
        let encoded = codec::encode(record)?;
        self.publish_encoded(
            Publication::from(record),
            &encoded.bytes,
            encoded.digest,
            |receipt| validate_ready(record, receipt),
        )
    }
    /// Single-commit live path: structure, exact source authority and complete
    /// prepared-byte correspondence are checked once. Nothing is cached or
    /// returned as a validation capability. The callback remains before all IO.
    /// Source/destination traits retain their trusted immutable-view contracts.
    pub fn commit_materialized_checked_with_receipt(
        &mut self,
        record: &Record,
        sources: &impl SourceResolver,
        destinations: &impl DestinationResolver,
        validate_ready: impl FnOnce(&Record, WalReceipt) -> Result<()>,
    ) -> Result<WalReceipt> {
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        let checked = crate::hybrid::checked::CheckedRecord::new(record)?;
        checked.verify_materialized(sources, destinations)?;
        self.check_base(record)?;
        let encoded = codec::encode_checked(&checked)?;
        self.publish_encoded(
            Publication::from(record),
            &encoded.bytes,
            encoded.digest,
            |receipt| validate_ready(record, receipt),
        )
    }
    /// Borrowed frozen-byte recipe; reusable encoding memory stays owned by the
    /// writer, bounded by the caller WAL policy and this journal's layout. No
    /// validated token, encoded bytes or future receipt escapes as authority.
    pub fn commit_borrowed_checked_with_receipt(
        &mut self,
        record: &crate::hybrid::borrowed::BorrowedRecord<'_, '_>,
        sources: &impl SourceResolver,
        destinations: &impl DestinationResolver,
        max_workspace_bytes: usize,
        validate_ready: impl FnOnce(
            &crate::hybrid::borrowed::BorrowedRecord<'_, '_>,
            WalReceipt,
        ) -> Result<()>,
    ) -> Result<WalReceipt> {
        if self.poisoned {
            return Err(Error::OutcomeUnknown);
        }
        let checked = crate::hybrid::checked::CheckedRecord::new(record)?;
        checked.verify_materialized(sources, destinations)?;
        self.check_base(record.metadata())?;
        let bound = max_workspace_bytes
            .min(codec::MAX_RECORD_BYTES)
            .min(usize::try_from(self.layout.limit()).map_err(|_| Error::LimitExceeded)?);
        if self.encode_workspace.capacity() > bound {
            self.encode_workspace = Vec::new();
        }
        let mut bytes = std::mem::take(&mut self.encode_workspace);
        let result = (|| {
            let digest = codec::encode_checked_into(&checked, &mut bytes, bound)?;
            self.publish_encoded(
                Publication::from(record.metadata()),
                &bytes,
                digest,
                |receipt| validate_ready(record, receipt),
            )
        })();
        // Vec ownership restoration is an infallible scalar move, including
        // after the durable decision and every returned error.
        self.encode_workspace = bytes;
        result
    }
    pub fn encoding_workspace_bytes(&self) -> usize {
        self.encode_workspace.capacity()
    }
    fn check_base(&self, record: &Record) -> Result<()> {
        if record.uuid != self.header.uuid
            || record.epoch != self.header.epoch
            || record.parent != self.selected.receipt
            || record.parent_commit != self.selected.commit
            || record.root.next_chunk < self.root.next_chunk
            || record.root.next_database < self.root.next_database
        {
            return Err(Error::Corrupt("hybrid commit base identity/root counters"));
        }
        Ok(())
    }
    // Wire publication consumes only scalar metadata and canonical encoded bytes.
    // A future borrowed recipe/reusable encoder can use this same IO protocol.
    fn publish_encoded(
        &mut self,
        meta: Publication,
        encoded: &[u8],
        digest: [u8; 32],
        validate_ready: impl FnOnce(WalReceipt) -> Result<()>,
    ) -> Result<WalReceipt> {
        let start = self.selected.receipt.end()?;
        let end = start
            .checked_add(encoded.len() as u64)
            .filter(|n| *n <= self.layout.limit())
            .ok_or(Error::LimitExceeded)?;
        let receipt = WalReceipt {
            lsn: meta.lsn,
            sequence: meta.sequence,
            offset: start,
            length: end - start,
            digest,
        };
        validate_ready(receipt)?;
        let next = Witness {
            receipt,
            commit: meta.commit,
        };
        let witness = wire::encode_layout_witness(self.header, next, self.layout);
        let inactive = 1 - self.slot;
        for (expected, offset) in self.cached.iter().zip(WITNESS_OFFSETS) {
            let mut actual = [0; BLOCK];
            self.file.read_exact_at(&mut actual, offset)?;
            if actual != *expected {
                self.poisoned = true;
                return Err(Error::Corrupt("hybrid active selected witness changed"));
            }
        }
        if self.file.metadata()?.len() != self.layout.physical_end(start) {
            self.poisoned = true;
            return Err(Error::Corrupt("hybrid active WAL has private tail"));
        }
        let appended = (|| {
            point("hybrid.commit.before_append")?;
            write_with_cut(&self.file, encoded, start, "hybrid.commit.half_record")?;
            point("hybrid.commit.after_append")?;
            point("hybrid.commit.before_wal_sync")?;
            self.sync_commit()?;
            point("hybrid.commit.after_wal_sync")?;
            Ok::<_, Error>(())
        })();
        if let Err(error) = appended {
            // Only private attempted bytes may reset; selected prefix is immutable.
            let cleanup = match self.layout {
                WalLayout::Dense => self.file.set_len(start).map_err(Error::from),
                WalLayout::PreSized { .. } => (|| {
                    point("hybrid.commit.before_clear_failed_append")?;
                    tail::clear(&self.file, start, end)?;
                    point("hybrid.commit.after_clear_failed_append")?;
                    Ok(())
                })(),
            }
            .and_then(|()| self.sync_commit());
            if cleanup.is_err() {
                self.poisoned = true;
                return Err(Error::OutcomeUnknown);
            }
            return Err(error);
        }
        // From this attempt onward a partial witness can select the durable record.
        let published = (|| {
            point("hybrid.commit.before_witness")?;
            write_with_cut(
                &self.file,
                &witness,
                WITNESS_OFFSETS[inactive],
                "hybrid.commit.half_witness",
            )?;
            point("hybrid.commit.after_witness")?;
            point("hybrid.commit.before_witness_sync")?;
            self.sync_commit()?;
            point("hybrid.commit.after_witness_sync")?;
            point("hybrid.commit.before_ack")?;
            Ok::<_, Error>(())
        })();
        if published.is_err() {
            self.poisoned = true;
            return Err(Error::OutcomeUnknown);
        }
        // Scalar/array moves only after the durable decision; no allocation here.
        self.cached[inactive] = witness;
        self.selected = next;
        self.root = meta.root;
        self.slot = inactive;
        Ok(receipt)
    }
    fn sync_commit(&self) -> Result<()> {
        match self.layout {
            WalLayout::Dense => ulmdb_platform::sync_file(&self.file)?,
            WalLayout::PreSized { .. } => ulmdb_platform::sync_data(&self.file)?,
        }
        Ok(())
    }
}
pub(crate) fn write_with_cut(file: &File, bytes: &[u8], offset: u64, half: &str) -> Result<()> {
    #[cfg(feature = "fault-injection")]
    {
        let n = bytes.len() / 2;
        file.write_all_at(&bytes[..n], offset)?;
        point(half)?;
        file.write_all_at(&bytes[n..], offset + n as u64)?;
    }
    #[cfg(not(feature = "fault-injection"))]
    {
        let _ = half;
        file.write_all_at(bytes, offset)?;
    }
    Ok(())
}
fn lock(file: File) -> Result<WriterFence> {
    WriterFence::try_lock(file).map_err(|e| match e {
        std::fs::TryLockError::WouldBlock => Error::Busy,
        std::fs::TryLockError::Error(e) => Error::Io(e),
    })
}
fn sync_parent(path: &Path) -> Result<()> {
    let resolved = std::fs::canonicalize(path)?;
    ulmdb_platform::sync_path(
        resolved
            .parent()
            .ok_or(Error::InvalidInput("hybrid WAL parent"))?,
    )?;
    Ok(())
}

#[cfg(test)]
mod reset_tests {
    use super::*;
    use crate::hybrid::tests::support::{HEADER, Registry, Temp, small_record};
    #[test]
    fn failed_private_reset_poison_preserves_selected_bytes_and_writer_fence() {
        let t = Temp::new(Path::new("/tmp"), "failed-reset");
        let mut j = Journal::create_with_layout(
            &t.wal(),
            HEADER,
            WalLayout::PreSized {
                capacity: 1024 * 1024,
            },
        )
        .unwrap();
        j.commit_checked(&small_record(&j), &Registry::default(), |_| Ok(()))
            .unwrap();
        let selected = j.selected();
        let before = std::fs::read(t.wal()).unwrap();
        // Keep the original locked OFD alive while a test-only read-only FD forces
        // both append and private-zero-reset to fail. No production injection.
        let readonly = File::open(t.wal()).unwrap();
        let locked = std::mem::replace(&mut j.file, WriterFence::unlocked_for_fault(readonly));
        let r = small_record(&j);
        assert!(matches!(
            j.commit_checked(&r, &Registry::default(), |_| Ok(())),
            Err(Error::OutcomeUnknown)
        ));
        assert!(j.is_poisoned());
        assert_eq!(j.selected(), selected);
        assert_eq!(std::fs::read(t.wal()).unwrap(), before);
        assert!(matches!(
            File::open(t.wal()).unwrap().try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(locked);
    }
}
