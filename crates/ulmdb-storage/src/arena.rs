use crate::{
    Error, ReadStore, Reference, Result, SegmentEntry, SegmentTable, WriteStore, mapping::hex,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Write,
    path::Path,
};
#[path = "savepoint.rs"]
mod savepoints;
pub use savepoints::ArenaCheckpoint;
use savepoints::Journal;
mod mapped;
mod reservation;
mod patch_many;
use mapped::ArenaBytes;
pub use mapped::{MappedAllocation, MappedFlush};
pub use reservation::ArenaReservation;

pub struct Arena {
    base: SegmentTable,
    value_limit: ulmdb_core::ValueLimit,
    id: u64,
    bytes: ArenaBytes,
    objects: std::collections::BTreeMap<u64, usize>,
    revision: u64,
    journal: Option<Journal>,
    reservation_owner: Option<std::sync::Arc<()>>,
    reservation_epoch: u64,
}
struct ArenaReadScope<'a> {
    arena: &'a Arena,
    base: Option<SegmentTable>,
}
impl ReadStore for ArenaReadScope<'_> {
    fn page_size(&self) -> usize { self.arena.page_size() }
    fn max_value_size(&self) -> usize { self.arena.max_value_size() }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment == self.arena.id { self.arena.read(at, length) }
        else { self.base.as_ref().unwrap_or(&self.arena.base).read(at, length) }
    }
    fn validate_extent(&self, at: Reference, length: usize) -> Result<()> {
        if at.segment == self.arena.id { self.arena.read(at, length).map(|_| ()) }
        else { self.base.as_ref().unwrap_or(&self.arena.base).validate_extent(at, length) }
    }
}
impl Arena {
    pub fn new(mut base: SegmentTable, id: u64) -> Result<Self> {
        base.check_insert_id(id)?;
        base.allow_fixed_registration(true);
        Ok(Self {
            value_limit: base.value_limit(),
            base,
            id,
            bytes: ArenaBytes::heap(),
            objects: std::collections::BTreeMap::new(),
            revision: 0,
            journal: None,
            reservation_owner: None,
            reservation_epoch: 0,
        })
    }
    /// Allocate directly in a bounded engine-owned writable mapped file.
    pub fn new_mapped(
        base: SegmentTable,
        id: u64,
        directory: &Path,
        capacity: usize,
        flush: MappedFlush,
    ) -> Result<Self> {
        let mut arena = Self::new(base, id)?;
        if arena.base.transform().is_some_and(|c| c.encrypted()) {
            return Err(Error::InvalidInput("encrypted mapped writer unsupported"));
        }
        arena.bytes = ArenaBytes::mapped(directory, capacity, flush)?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        if let Some(fixed) = arena.base.fixed_map() {
            arena
                .bytes
                .bind_fixed_writer(fixed, arena.base.fixed_external_authority())?;
        }
        Ok(arena)
    }
    pub fn is_write_mapped(&self) -> bool {
        self.bytes.is_mapped()
    }
    pub fn mapped_allocation(&self) -> Result<Option<MappedAllocation>> {
        self.bytes.mapping()
    }
    /// Skip redundant zero stores only in the never-exposed suffix of a fresh
    /// private mapped file. Heap storage and reused bytes stay initialized.
    pub fn set_mapped_no_mem_init(&mut self, enabled: bool) {
        self.bytes.set_no_mem_init(enabled);
    }
    pub fn set_mapped_flush(&mut self, flush: MappedFlush) {
        self.bytes.set_flush(flush);
    }
    pub fn mapped_data_policy(&self, policy: crate::Durability) -> crate::Durability {
        self.bytes.effective_policy(policy)
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    pub fn track_fixed_declaration(
        &mut self,
        declaration: std::sync::Arc<crate::fixed_map::FixedMapDeclaration>,
    ) {
        self.base = self.base.clone().with_fixed_declaration(declaration);
    }
    pub fn set_value_limit(&mut self, limit: ulmdb_core::ValueLimit) {
        self.value_limit = limit;
    }
    pub fn value_limit(&self) -> ulmdb_core::ValueLimit {
        self.value_limit
    }
    pub fn base(&self) -> &SegmentTable {
        &self.base
    }
    /// A temporary read view combining this writer's private objects with an
    /// independent bounded immutable-cache pin scope. Drop it before mutation;
    /// inspection then retains no borrowed windows in the long-lived base.
    pub fn fresh_read_scope(&self) -> Result<impl ReadStore + '_> {
        let base = if self.base.is_chunked() { Some(self.base.fresh_read_scope()?) } else { None };
        Ok(ArenaReadScope { arena: self, base })
    }
    /// Project a writer's immutable base after its caller has established that
    /// no surviving root/private object refers to a lower segment identity.
    /// This changes no bytes and confers no reachability certificate itself.
    pub fn retain_base_from(&mut self, first: u64) -> Result<()> {
        self.retain_base_matching(|entry| entry.id >= first)
    }
    /// Project only caller-certified live immutable entries. The predicate is
    /// metadata selection, not a graph/reclamation certificate.
    pub fn retain_base_matching(&mut self, keep: impl Fn(&SegmentEntry) -> bool) -> Result<()> {
        let ids = self.base.entries().iter().filter(|entry| keep(entry))
            .map(|entry| entry.id).collect();
        self.base = self.base.select(&ids)?;
        Ok(())
    }
    /// Pure allocation-capacity preflight. This performs the same alignment
    /// and extent arithmetic as allocate, without changing revision or bytes.
    fn capacity_limit(&self) -> usize {
        self.bytes
            .capacity_limit()
            .min(self.value_limit.segment_bytes())
    }
    pub fn can_allocate(&self, length: usize, alignment: usize) -> Result<bool> {
        if !alignment.is_power_of_two() || alignment > self.base.page_geometry().get().max(16384) {
            return Err(Error::InvalidInput("invalid allocation alignment"));
        }
        let start = self
            .bytes
            .len()
            .checked_add(alignment - 1)
            .map(|n| n & !(alignment - 1))
            .ok_or(Error::LimitExceeded)?;
        let end = start.checked_add(length).ok_or(Error::LimitExceeded)?;
        Ok(end <= self.capacity_limit())
    }
    pub fn dirty_bytes(&self) -> usize {
        self.bytes.len() - 4096
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    #[cfg(test)]
    pub(crate) fn set_revision_for_test(&mut self, revision: u64) {
        self.revision = revision;
    }
    pub const MAX_SAVEPOINT_DEPTH: usize = savepoints::MAX_DEPTH;
    pub const MAX_SAVEPOINT_UNDO_BYTES: usize = savepoints::MAX_UNDO_BYTES;

    pub fn savepoint_depth(&self) -> usize {
        self.journal.as_ref().map_or(0, Journal::depth)
    }
    /// Actual profile-bounded undo allowance (the legacy constant names the default).
    pub fn savepoint_undo_limit(&self) -> usize {
        self.value_limit.segment_bytes()
    }
    pub fn savepoint_undo_bytes(&self) -> usize {
        self.journal.as_ref().map_or(0, Journal::bytes)
    }
    pub fn checkpoint(&mut self) -> Result<ArenaCheckpoint> {
        self.invalidate_reservations()?;
        let boundary = self.objects.get(&(self.bytes.len() as u64)).copied();
        self.journal.get_or_insert_with(Journal::new).checkpoint(
            self.bytes.len(),
            self.revision,
            boundary,
        )
    }
    pub fn release_checkpoint(&mut self, checkpoint: &ArenaCheckpoint) -> Result<()> {
        self.invalidate_reservations()?;
        self.journal
            .as_mut()
            .ok_or(Error::InvalidInput("inactive checkpoint"))?
            .release(checkpoint)
    }
    pub fn rollback_checkpoint(&mut self, checkpoint: &ArenaCheckpoint) -> Result<()> {
        self.invalidate_reservations()?;
        let frame = self
            .journal
            .as_mut()
            .ok_or(Error::InvalidInput("inactive checkpoint"))?
            .rollback(checkpoint)?;
        for (start, before) in frame.before {
            let end = start
                .checked_add(before.len())
                .ok_or(Error::Corrupt("undo extent overflow"))?;
            self.bytes
                .get_mut(start..end)
                .ok_or(Error::Corrupt("undo extent outside arena"))?
                .copy_from_slice(&before);
        }
        self.bytes.truncate(frame.length);
        drop(self.objects.split_off(&(frame.length as u64)));
        if let Some(length) = frame.boundary_object {
            self.objects.insert(frame.length as u64, length);
        }
        self.revision = frame.revision;
        Ok(())
    }
    fn remember(&mut self, start: usize, length: usize) -> Result<()> {
        if let Some(journal) = &mut self.journal
            && journal.depth() != 0
        {
            journal.remember_with_limit(
                &self.bytes,
                start,
                length,
                self.value_limit.segment_bytes(),
            )?;
        }
        Ok(())
    }
    // One prefix encoder/hash for both publication protocols. Legacy seal's
    // write/sync/rename fault sequence remains unchanged after this pure step.
    fn prepare_segment(&mut self, cohort: u128) -> Result<Option<SegmentEntry>> {
        if self.savepoint_depth() != 0 {
            return Err(Error::InvalidInput(
                "active savepoints must release or rollback before seal",
            ));
        }
        if self.bytes.len() == 4096 {
            return Ok(None);
        }
        if self.base.value_limit() != self.value_limit {
            self.base = self.base.clone().with_value_limit(self.value_limit)?;
        }
        self.base.check_insert_id(self.id)?;
        self.bytes[..8].copy_from_slice(b"ULMDBSEG");
        let minor: u16 = if self.bytes.len() as u64 > 1_073_741_824 {
            4
        } else if self.bytes.len() > 268_435_456 {
            3
        } else {
            2
        };
        self.bytes[10..12].copy_from_slice(&minor.to_le_bytes());
        self.bytes[12..16].copy_from_slice(&128u32.to_le_bytes());
        self.bytes[16..24].copy_from_slice(&self.id.to_le_bytes());
        self.bytes[24..28].copy_from_slice(&4096u32.to_le_bytes());
        self.bytes[28] = 1;
        self.bytes[32..40].copy_from_slice(&4096u64.to_le_bytes());
        let length = self.bytes.len() as u64;
        self.bytes[40..48].copy_from_slice(&(length - 4096).to_le_bytes());
        self.bytes[48..56].copy_from_slice(&length.to_le_bytes());
        self.bytes[56..64].copy_from_slice(&((1u64 << minor) - 1).to_le_bytes());
        let hash: [u8; 32] = Sha256::digest(&self.bytes).into();
        Ok(Some(SegmentEntry {
            id: self.id,
            length,
            hash,
            cohort,
        }))
    }

    pub(crate) fn range_data(mut self) -> Result<(SegmentTable, Option<SegmentEntry>, Vec<u8>)> {
        let entry = self.prepare_segment(0)?;
        let bytes = if entry.is_some() {
            self.bytes.into_vec()
        } else {
            Vec::new()
        };
        Ok((self.base, entry, bytes))
    }
    pub(crate) fn range_parts(
        mut self,
        manifest: &mut crate::Manifest,
        base: &crate::Manifest,
    ) -> Result<(SegmentTable, Option<SegmentEntry>, Vec<u8>)> {
        let entry = self.prepare_segment(0)?;
        let fast = crate::metadata::prepare_range_append(
            manifest,
            base,
            self.base.residency_identity(),
            entry.as_ref(),
        )?;
        if !fast {
            manifest.segments = if entry.is_some() {
                self.base.entries_for_addition()?.into()
            } else {
                self.base.entries().into()
            };
            if let Some(entry) = &entry {
                manifest.next_segment = manifest
                    .next_segment
                    .max(entry.id.checked_add(1).ok_or(Error::LimitExceeded)?);
                let at = manifest.segments.partition_point(|e| e.id < entry.id);
                manifest.segments.insert(at, entry.clone());
            }
            manifest.prepare_indexed(Some(base))?;
        }
        let bytes = if entry.is_some() {
            self.bytes.into_vec()
        } else {
            Vec::new()
        };
        Ok((self.base, entry, bytes))
    }
    pub fn seal(self, directory: &Path) -> Result<(SegmentTable, Option<SegmentEntry>)> {
        self.seal_policy(directory, crate::Durability::Full)
    }
    pub fn seal_with_durability(
        self,
        directory: &Path,
        policy: crate::Durability,
    ) -> Result<(SegmentTable, Option<SegmentEntry>, crate::SealedAssets)> {
        let (table, entry) = self.seal_policy(directory, policy)?;
        let sealed = crate::SealedAssets::new(directory, entry.clone(), policy)?;
        Ok((table, entry, sealed))
    }
    fn seal_policy(
        mut self,
        directory: &Path,
        policy: crate::Durability,
    ) -> Result<(SegmentTable, Option<SegmentEntry>)> {
        crate::durability::check_policy(policy)?;
        self.base.fixed_registration_policy(policy);
        let Some(entry) = self.prepare_segment(0)? else {
            return Ok((self.base, None));
        };
        self.bytes.register_fixed_entry(Some(&entry), policy)?;
        let hash = entry.hash;
        let name = format!("{}.seg", hex(&hash));
        let final_path = directory.join("segments").join(&name);
        if self.base.transform().is_none() && self.bytes.is_mapped() {
            if final_path.exists() {
                // Verify an existing immutable hash identity before discarding
                // our private mapped attempt. Never replace a published inode.
                crate::mapping::Mapping::open_with_transform(
                    directory,
                    entry.clone(),
                    Some(&self.bytes),
                    self.base.read_ahead(),
                    None,
                )?;
                if policy.sync_data() {
                    ulmdb_platform::sync_file(&File::open(&final_path)?)?;
                }
            } else {
                self.bytes
                    .take_mapped()
                    .unwrap()
                    .publish(&final_path, policy)?;
            }
            drop(self.bytes.take_mapped());
            self.base.add(directory, entry.clone())?;
            return Ok((self.base, Some(entry)));
        }
        let (temporary, mut file) =
            crate::temporary::create(&directory.join("segments"), "segment")?;
        crate::fault_point!("segment.before_write");
        if let Some(context) = self.base.transform() {
            context.write_encoded(
                &mut file,
                crate::transform::ObjectKind::Segment,
                entry.id as u128,
                &self.bytes,
            )?;
        } else {
            file.write_all(&self.bytes)?;
        }
        crate::fault_point!("segment.after_write");
        #[cfg(not(windows))]
        {
            let mut perms = file.metadata()?.permissions();
            perms.set_readonly(true);
            file.set_permissions(perms)?;
        }
        #[cfg(windows)]
        ulmdb_platform::set_private_permissions(&file, false)?;
        if policy.sync_data() {
            crate::fault_point!("segment.before_file_sync");
            ulmdb_platform::sync_file(&file)?;
            crate::fault_point!("segment.after_file_sync");
        }
        if final_path.exists() {
            // A prior unpublished attempt may have sealed identical bytes.
            // Validate before reuse; never overwrite a committed immutable inode.
            crate::mapping::Mapping::open_with_transform(
                directory,
                entry.clone(),
                Some(&self.bytes),
                self.base.read_ahead(),
                self.base.transform(),
            )?;
            if policy.sync_data() {
                crate::fault_point!("segment.before_reused_file_sync");
                ulmdb_platform::sync_file(&crate::read_mode::ReadMode::Sync.open(&final_path)?)?;
                crate::fault_point!("segment.after_reused_file_sync");
            }
            fs::remove_file(&temporary)?;
        } else {
            crate::fault_point!("segment.before_rename");
            ulmdb_platform::rename_replace(&temporary, &final_path)
                .map_err(ulmdb_platform::rename_original_error)?;
            crate::fault_point!("segment.after_rename");
        }
        if policy.sync_data() {
            crate::fault_point!("segment.before_segments_dir_sync");
            ulmdb_platform::sync_directory(&directory.join("segments"))?;
            crate::fault_point!("segment.after_segments_dir_sync");
        }
        self.bytes.flush_encoded_if_async(&final_path)?;
        if self.bytes.is_mapped() {
            // The transform sealer has finished reading private bytes. End RW
            // fixed-slot authority before admitting authenticated decoded RO.
            drop(self.bytes.take_mapped());
            self.base.add(directory, entry.clone())?;
        } else {
            self.base
                .add_written(directory, entry.clone(), &self.bytes)?;
        }
        Ok((self.base, Some(entry)))
    }
    /// Prepare format0.4, sharing unchanged metadata nodes from the committed
    /// base. Data mappings and pointer publication have the same fences as0.3.
    pub fn seal_indexed(
        self,
        directory: &Path,
        manifest: &mut crate::Manifest,
        base: &crate::Manifest,
    ) -> Result<(SegmentTable, crate::PreparedPublication)> {
        self.seal_indexed_with_durability(directory, manifest, base, crate::Durability::Full)
    }
    pub fn seal_indexed_with_durability(
        mut self,
        directory: &Path,
        manifest: &mut crate::Manifest,
        base: &crate::Manifest,
        policy: crate::Durability,
    ) -> Result<(SegmentTable, crate::PreparedPublication)> {
        crate::durability::check_policy(policy)?;
        self.base.fixed_registration_policy(policy);
        if manifest.generation == 0 {
            return Err(Error::InvalidInput("zero indexed generation"));
        }
        let entry = self.prepare_segment(manifest.generation)?;
        self.bytes.register_fixed_entry(entry.as_ref(), policy)?;
        let fast = crate::metadata::prepare_append(
            manifest,
            base,
            self.base.residency_identity(),
            entry.as_ref(),
        )?;
        if !fast {
            manifest.segments = if entry.is_some() {
                self.base.entries_for_addition()?.into()
            } else {
                self.base.entries().into()
            };
            if let Some(entry) = &entry {
                let next = entry.id.checked_add(1).ok_or(Error::LimitExceeded)?;
                manifest.next_segment = manifest.next_segment.max(next);
                let position = manifest.segments.partition_point(|old| old.id < entry.id);
                manifest.segments.insert(position, entry.clone());
            }
            manifest.prepare_indexed(Some(base))?;
        }
        if self.base.transform().is_none() && self.bytes.is_mapped() {
            let payload = if entry.is_some() { self.bytes.len() } else { 0 };
            let hash = self
                .bytes
                .take_mapped()
                .unwrap()
                .publish_cohort(directory, manifest, payload, policy)?;
            if let Some(entry) = entry {
                self.base.add(directory, entry)?;
            }
            let prepared =
                crate::publication::prepare_promoted_cohort(directory, manifest, hash, policy)?;
            if let Some(index) = &mut manifest.index {
                index.certified_table = Some(self.base.residency_identity());
            }
            return Ok((self.base, prepared));
        }
        let (prepared, mapping) = crate::publication::prepare_mapped_cohort_with_transform(
            directory,
            manifest,
            entry.as_ref().map(|_| self.bytes.as_slice()),
            policy,
            self.base.read_ahead(),
            self.base.transform(),
        )?;
        self.bytes
            .flush_encoded_if_async(&crate::cohort::path(directory, manifest.generation))?;
        if let Some(mapping) = mapping {
            drop(self.bytes.take_mapped());
            self.base.insert_mapping(mapping)?;
        }
        // Both branches built this exact complete inventory from this table.
        manifest.index.as_mut().unwrap().certified_table = Some(self.base.residency_identity());
        Ok((self.base, prepared))
    }
    /// Prepare one immutable cohort and map only its segment prefix. Publication
    /// of CURRENT is exclusively the returned token's responsibility.
    pub fn seal_cohort(
        self,
        directory: &Path,
        manifest: &mut crate::Manifest,
    ) -> Result<(SegmentTable, crate::PreparedPublication)> {
        self.seal_cohort_with_durability(directory, manifest, crate::Durability::Full)
    }
    pub fn seal_cohort_with_durability(
        mut self,
        directory: &Path,
        manifest: &mut crate::Manifest,
        policy: crate::Durability,
    ) -> Result<(SegmentTable, crate::PreparedPublication)> {
        crate::durability::check_policy(policy)?;
        self.base.fixed_registration_policy(policy);
        if manifest.generation == 0 {
            return Err(Error::InvalidInput("zero cohort generation"));
        }
        let entry = self.prepare_segment(manifest.generation)?;
        self.bytes.register_fixed_entry(entry.as_ref(), policy)?;
        manifest.format_minor = 3;
        manifest.segments = if entry.is_some() {
            self.base.entries_for_addition()?.into()
        } else {
            self.base.entries().into()
        };
        if let Some(entry) = &entry {
            let next = entry.id.checked_add(1).ok_or(Error::LimitExceeded)?;
            manifest.next_segment = manifest.next_segment.max(next);
            let position = manifest
                .segments
                .partition_point(|existing| existing.id < entry.id);
            manifest.segments.insert(position, entry.clone());
        }
        if self.base.transform().is_none() && self.bytes.is_mapped() {
            let payload = if entry.is_some() { self.bytes.len() } else { 0 };
            let hash = self
                .bytes
                .take_mapped()
                .unwrap()
                .publish_cohort(directory, manifest, payload, policy)?;
            if let Some(entry) = entry {
                self.base.add(directory, entry)?;
            }
            let prepared =
                crate::publication::prepare_promoted_cohort(directory, manifest, hash, policy)?;
            if let Some(index) = &mut manifest.index {
                index.certified_table = Some(self.base.residency_identity());
            }
            return Ok((self.base, prepared));
        }
        let (prepared, mapping) = crate::publication::prepare_mapped_cohort_with_transform(
            directory,
            manifest,
            entry.as_ref().map(|_| self.bytes.as_slice()),
            policy,
            self.base.read_ahead(),
            self.base.transform(),
        )?;
        self.bytes
            .flush_encoded_if_async(&crate::cohort::path(directory, manifest.generation))?;
        if let Some(mapping) = mapping {
            drop(self.bytes.take_mapped());
            self.base.insert_mapping(mapping)?;
        }
        Ok((self.base, prepared))
    }
}
impl ReadStore for Arena {
    fn max_value_size(&self) -> usize {
        self.value_limit.get()
    }
    fn page_size(&self) -> usize {
        self.base.page_geometry().get()
    }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment != self.id {
            return self.base.read(at, length);
        }
        let offset = usize::try_from(at.offset).map_err(|_| Error::Corrupt("private offset"))?;
        let end = offset
            .checked_add(length)
            .ok_or(Error::Corrupt("private extent overflow"))?;
        if offset < 4096 {
            return Err(Error::Corrupt("private reference in header"));
        }
        self.bytes
            .get(offset..end)
            .ok_or(Error::Corrupt("private extent outside arena"))
    }
}
impl WriteStore for Arena {
    fn supports_patch_many(&self, object_len: usize, alignment: usize) -> bool {
        object_len == self.page_size() && alignment == self.page_size()
    }
    fn try_patch_many(
        &mut self,
        old: Reference,
        object_len: usize,
        patches: &[(usize, &[u8])],
        alignment: usize,
    ) -> Result<Option<Reference>> {
        if !self.supports_patch_many(object_len, alignment) {
            return Ok(None);
        }
        self.patch_many_page(old, patches).map(Some)
    }
    fn allocate_zeroed(&mut self, length: usize, alignment: usize) -> Result<Reference> {
        if !self.can_allocate(length, alignment)? {
            return Err(Error::LimitExceeded);
        }
        let start = (self.bytes.len() + alignment - 1) & !(alignment - 1);
        let end = start.checked_add(length).ok_or(Error::LimitExceeded)?;
        let revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
        self.bytes
            .try_reserve_profile(end - self.bytes.len(), self.value_limit)
            .map_err(|_| Error::LimitExceeded)?;
        self.bytes.resize(end, 0)?;
        self.objects.insert(start as u64, length);
        self.revision = revision;
        Ok(Reference {
            segment: self.id,
            offset: start as u64,
        })
    }

    fn patch(
        &mut self,
        old: Reference,
        object_len: usize,
        offset: usize,
        replacement: &[u8],
        alignment: usize,
    ) -> Result<Reference> {
        if !alignment.is_power_of_two()
            || alignment > self.base.page_geometry().get().max(16384)
            || old.segment == 0
            || !old.offset.is_multiple_of(alignment as u64)
        {
            return Err(Error::InvalidInput("invalid patch reference/alignment"));
        }
        let relative_end = offset
            .checked_add(replacement.len())
            .ok_or(Error::LimitExceeded)?;
        if relative_end > object_len {
            return Err(Error::InvalidInput("patch range exceeds object"));
        }
        if old.segment == self.id {
            if old.offset < 4096 || self.objects.get(&old.offset) != Some(&object_len) {
                return Err(Error::InvalidInput(
                    "patch must target complete private object",
                ));
            }
            let start = usize::try_from(old.offset).map_err(|_| Error::LimitExceeded)?;
            let object_end = start.checked_add(object_len).ok_or(Error::LimitExceeded)?;
            let target = self
                .bytes
                .get(start..object_end)
                .ok_or(Error::Corrupt("private patch extent"))?;
            if &target[offset..relative_end] == replacement {
                return Ok(old);
            }
            let revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
            // Whole-object before-images are shared with ordinary rewrites:
            // subsequent range changes in the same frame never recapture it.
            self.remember(start, object_len)?;
            self.bytes[start + offset..start + relative_end].copy_from_slice(replacement);
            self.revision = revision;
            return Ok(old);
        }
        let source = self.base.read(old, object_len)?;
        if source.len() != object_len {
            return Err(Error::Corrupt("short base patch object"));
        }
        if &source[offset..relative_end] == replacement {
            return Ok(old);
        }
        let start = self
            .bytes
            .len()
            .checked_add(alignment - 1)
            .map(|n| n & !(alignment - 1))
            .ok_or(Error::LimitExceeded)?;
        let end = start.checked_add(object_len).ok_or(Error::LimitExceeded)?;
        if end > self.capacity_limit() {
            return Err(Error::LimitExceeded);
        }
        let revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
        self.bytes
            .try_reserve_profile(end - self.bytes.len(), self.value_limit)
            .map_err(|_| Error::LimitExceeded)?;
        // Distinct fields permit a direct immutable mapping -> arena copy.
        // No temporary page Vec and no before-image for this new tail object.
        self.bytes.resize(start, 0)?;
        self.bytes.extend_from_slice(source)?;
        self.bytes[start + offset..start + relative_end].copy_from_slice(replacement);
        self.objects.insert(start as u64, object_len);
        self.revision = revision;
        Ok(Reference {
            segment: self.id,
            offset: start as u64,
        })
    }
    fn put_payload(&mut self, value: &[u8]) -> Result<Reference> {
        if value.len() > self.max_value_size() {
            return Err(Error::InvalidInput("payload size limit exceeded"));
        }
        let size = value.len().checked_add(16).ok_or(Error::LimitExceeded)?;
        if !self.can_allocate(size, 16)? {
            return Err(Error::LimitExceeded);
        }
        let start = self
            .bytes
            .len()
            .checked_add(15)
            .ok_or(Error::LimitExceeded)?
            & !15;
        let end = start.checked_add(size).ok_or(Error::LimitExceeded)?;
        let revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
        self.bytes
            .try_reserve_profile(end - self.bytes.len(), self.value_limit)
            .map_err(|_| Error::LimitExceeded)?;
        let mut header = [0; 16];
        header[..2].copy_from_slice(&1u16.to_le_bytes());
        header[4..8].copy_from_slice(&16u32.to_le_bytes());
        header[8..16].copy_from_slice(&(value.len() as u64).to_le_bytes());
        self.bytes.resize(start, 0)?;
        self.bytes.extend_from_slice(&header)?;
        self.bytes.extend_from_slice(value)?;
        self.objects.insert(start as u64, size);
        self.revision = revision;
        Ok(Reference {
            segment: self.id,
            offset: start as u64,
        })
    }
    fn replace_payload(&mut self, old: Reference, value: &[u8]) -> Result<Reference> {
        if value.len() > self.max_value_size() {
            return Err(Error::InvalidInput("payload size limit exceeded"));
        }
        if old.is_null() {
            return self.put_payload(value);
        }
        let prior = self.payload(old)?;
        if prior == value {
            return Ok(old);
        }
        if old.segment != self.id || prior.len() != value.len() {
            return self.put_payload(value);
        }
        let start = usize::try_from(old.offset).map_err(|_| Error::Corrupt("payload offset"))?;
        let size = 16usize
            .checked_add(value.len())
            .ok_or(Error::LimitExceeded)?;
        if self.objects.get(&old.offset) != Some(&size) {
            return Err(Error::Corrupt("invalid private payload allocation"));
        }
        let revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
        self.remember(start, size)?;
        self.revision = revision;
        self.bytes[start + 16..start + size].copy_from_slice(value);
        Ok(old)
    }
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        if !alignment.is_power_of_two() || alignment > self.base.page_geometry().get().max(16384) {
            return Err(Error::InvalidInput("invalid allocation alignment"));
        }
        let start = self
            .bytes
            .len()
            .checked_add(alignment - 1)
            .map(|n| n & !(alignment - 1))
            .ok_or(Error::LimitExceeded)?;
        let end = start.checked_add(bytes.len()).ok_or(Error::LimitExceeded)?;
        if end > self.capacity_limit() {
            return Err(Error::LimitExceeded);
        }
        self.revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
        if self.value_limit > ulmdb_core::ValueLimit::EXTENDED {
            self.bytes
                .try_reserve_profile(end - self.bytes.len(), self.value_limit)?;
        }
        self.bytes.resize(start, 0)?;
        self.bytes.extend_from_slice(bytes)?;
        self.objects.insert(start as u64, bytes.len());
        Ok(Reference {
            segment: self.id,
            offset: start as u64,
        })
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        if !alignment.is_power_of_two() || alignment > self.base.page_geometry().get().max(16384) {
            return Err(Error::InvalidInput("invalid rewrite alignment"));
        }
        if old.segment == self.id
            && old.offset >= 4096
            && old.offset.is_multiple_of(alignment as u64)
        {
            if self.objects.get(&old.offset) != Some(&bytes.len()) {
                return Err(Error::InvalidInput(
                    "rewrite must preserve private object size",
                ));
            }
            let start =
                usize::try_from(old.offset).map_err(|_| Error::Corrupt("rewrite offset"))?;
            let end = start.checked_add(bytes.len()).ok_or(Error::LimitExceeded)?;
            let target = self
                .bytes
                .get(start..end)
                .ok_or(Error::Corrupt("private rewrite outside arena"))?;
            if target != bytes {
                let revision = self.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
                self.remember(start, bytes.len())?;
                self.revision = revision;
                self.bytes[start..end].copy_from_slice(bytes);
            }
            Ok(old)
        } else {
            if !old.is_null() && self.base.read(old, bytes.len())? == bytes {
                Ok(old)
            } else {
                self.allocate(bytes, alignment)
            }
        }
    }
}

#[cfg(test)]
mod patch_tests;

#[cfg(test)]
mod cohort_tests;
