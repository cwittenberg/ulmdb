use super::fault::point;
use super::{CheckpointCut, ChunkDescription, descriptor};
use crate::{Error, Result};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
};
const BUFFER: usize = 128 * 1024;
const IMAGE_DOMAIN: &[u8] = b"ULMDB/hybrid/checkpoint-image/v1\0";
#[derive(Clone, Copy, Debug)]
pub struct CheckpointLimits {
    pub max_chunks: usize,
    pub max_apparent_bytes: u64,
    pub max_allocated_bytes: u64,
    pub max_provenance_bytes: u64,
}
impl Default for CheckpointLimits {
    fn default() -> Self {
        Self {
            max_chunks: 4096,
            max_apparent_bytes: 1 << 40,
            max_allocated_bytes: 1 << 40,
            max_provenance_bytes: 160 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Default, Debug)]
pub struct SpaceReport {
    pub apparent_bytes: u64,
    pub allocated_bytes: u64,
    pub files: u64,
}
pub struct CheckpointArtifact {
    path: PathBuf,
    generation: u128,
    digest: [u8; 32],
    space: SpaceReport,
}
impl CheckpointArtifact {
    pub fn directory(&self) -> &Path {
        &self.path
    }
    pub fn generation(&self) -> u128 {
        self.generation
    }
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
    pub fn space(&self) -> SpaceReport {
        self.space
    }
}
struct OwnedDirectory {
    path: PathBuf,
    keep: bool,
}
impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
struct Budget {
    limits: CheckpointLimits,
    space: SpaceReport,
    held_apparent: u64,
    held_allocated: u64,
    held_directory_apparent: u64,
    held_directory_allocated: u64,
}
impl Budget {
    fn check(&self, apparent: u64, allocated: u64) -> Result<()> {
        if self
            .space
            .apparent_bytes
            .checked_add(self.held_apparent)
            .and_then(|n| n.checked_add(self.held_directory_apparent))
            .and_then(|n| n.checked_add(apparent))
            .ok_or(Error::LimitExceeded)?
            > self.limits.max_apparent_bytes
            || self
                .space
                .allocated_bytes
                .checked_add(self.held_allocated)
                .and_then(|n| n.checked_add(self.held_directory_allocated))
                .and_then(|n| n.checked_add(allocated))
                .ok_or(Error::LimitExceeded)?
                > self.limits.max_allocated_bytes
        {
            return Err(Error::LimitExceeded);
        }
        Ok(())
    }
    fn hold_directory(&mut self, path: &Path) -> Result<()> {
        let meta = ulmdb_platform::path_identity(&path.canonicalize()?)?;
        let allocated = meta.allocated_bytes;
        self.held_directory_apparent = 0;
        self.held_directory_allocated = 0;
        self.check(meta.len(), allocated)?;
        self.held_directory_apparent = meta.len();
        self.held_directory_allocated = allocated;
        Ok(())
    }
    fn hold_descriptor(&mut self, file: &File) -> Result<()> {
        let m = ulmdb_platform::file_identity(file)?;
        let allocated = m.allocated_bytes;
        self.held_apparent = 0;
        self.held_allocated = 0;
        self.check(m.len(), allocated)?;
        self.held_apparent = m.len();
        self.held_allocated = allocated;
        Ok(())
    }
    fn add(&mut self, meta: &ulmdb_platform::FileIdentity) -> Result<()> {
        let allocated = meta.allocated_bytes;
        self.check(meta.len(), allocated)?;
        self.space.apparent_bytes += meta.len();
        self.space.allocated_bytes += allocated;
        self.space.files += 1;
        Ok(())
    }
}
/// Checks apparent growth before each write and actual allocated blocks at
/// bounded 128KiB write intervals. Filesystem allocation granules may overshoot
/// a physical quota before the next check; errors remove the private artifact.
struct ChargedFile<'a> {
    file: File,
    budget: &'a mut Budget,
    length: u64,
    last_check: u64,
    max_length: u64,
}
impl<'a> ChargedFile<'a> {
    fn create(path: &Path, budget: &'a mut Budget, max_length: u64) -> Result<Self> {
        let file = ulmdb_platform::create_private_file(path, false)?;
        budget.hold_directory(
            path.parent()
                .ok_or(Error::InvalidInput("checkpoint asset parent"))?,
        )?;
        Ok(Self {
            file,
            budget,
            length: 0,
            last_check: 0,
            max_length,
        })
    }
    fn finish(self) -> Result<()> {
        ulmdb_platform::set_private_permissions(&self.file, false)?;
        ulmdb_platform::sync_file(&self.file)?;
        self.budget.add(&ulmdb_platform::file_identity(&self.file)?)
    }
}
impl Write for ChargedFile<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let n = bytes.len().min(BUFFER);
        let end = self.length.checked_add(n as u64).ok_or_else(limit_io)?;
        if end > self.max_length {
            return Err(limit_io());
        }
        self.budget.check(end, 0).map_err(to_io)?;
        let written = self.file.write(&bytes[..n])?;
        self.length += written as u64;
        if self.length - self.last_check >= BUFFER as u64 {
            let meta = ulmdb_platform::file_identity(&self.file)?;
            self.budget
                .check(meta.len(), meta.allocated_bytes)
                .map_err(to_io)?;
            self.last_check = self.length;
        }
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
fn write_descriptor_part(file: &mut File, budget: &mut Budget, bytes: &[u8]) -> Result<()> {
    budget.check(bytes.len() as u64, 0)?;
    file.write_all(bytes)?;
    budget.hold_descriptor(file)
}
fn limit_io() -> io::Error {
    io::Error::other("checkpoint space/length budget exceeded")
}
fn to_io(e: Error) -> io::Error {
    match e {
        Error::Io(e) => e,
        _ => limit_io(),
    }
}
fn name(id: u64, suffix: &str) -> String {
    format!("chunk-{id:016x}.{suffix}")
}
/// Build a durable, unselected artifact into an exclusive new directory.
/// Data/provenance files are synced before the final descriptor name; both
/// artifact and parent directories are synced. Existing WAL/pointers are never
/// changed, including on any partial write/quota/validation failure.
pub fn build(
    cut: &CheckpointCut,
    target: &Path,
    generation: u128,
    limits: CheckpointLimits,
) -> Result<CheckpointArtifact> {
    if generation == 0 || limits.max_chunks > 1_048_575 || cut.chunk_count() > limits.max_chunks {
        return Err(Error::LimitExceeded);
    }
    let expected = cut.image.sealed_chunks() + usize::from(cut.image.active().is_some());
    if expected != cut.chunk_count() {
        return Err(Error::Corrupt(
            "checkpoint provenance/image inventory mismatch",
        ));
    }
    let root = cut.selected.root.catalog;
    if !root.is_null() {
        let span = cut
            .image
            .chunk(root.segment)
            .ok_or(Error::Corrupt("checkpoint catalog chunk absent"))?;
        span.read(
            usize::try_from(root.offset).map_err(|_| Error::LimitExceeded)?,
            1,
        )?;
    }
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    ulmdb_platform::create_private_directory(target)?;
    let mut owned = OwnedDirectory {
        path: target.to_path_buf(),
        keep: false,
    };
    point("h5.directory_created")?;
    ulmdb_platform::sync_path(parent)?;
    point("h5.parent_synced")?;
    let mut budget = Budget {
        limits,
        space: SpaceReport::default(),
        held_apparent: 0,
        held_allocated: 0,
        held_directory_apparent: 0,
        held_directory_allocated: 0,
    };
    budget.hold_directory(target)?;
    let mut descriptor_file =
        ulmdb_platform::create_private_file(&target.join("DESCRIPTOR.tmp"), false)?;
    budget.hold_directory(target)?;
    let header = descriptor::header(
        generation,
        cut.uuid,
        cut.epoch,
        cut.selected,
        cut.chunk_count() as u64,
    );
    let mut hash = Sha256::new();
    hash.update(descriptor::DOMAIN);
    hash.update(header);
    write_descriptor_part(&mut descriptor_file, &mut budget, &header)?;

    for chunk in cut.directory.iter() {
        let id = chunk.identity();
        if id.uuid != cut.uuid
            || id.epoch != cut.epoch
            || id.chunk >= cut.selected.root.next_chunk
            || chunk.bounds().last_creation_lsn > cut.selected.receipt.lsn
            || chunk.bounds().last_creation_sequence > cut.selected.receipt.sequence
            || chunk.bounds().last_authority_sequence > cut.selected.receipt.sequence
        {
            return Err(Error::Corrupt("checkpoint future/foreign chunk"));
        }
        let span = cut
            .image
            .chunk(id.chunk)
            .ok_or(Error::Corrupt("checkpoint image missing"))?;
        if span.end() as u64 != chunk.end() || span.capacity() as u64 != id.capacity {
            return Err(Error::Corrupt("checkpoint frozen image bound mismatch"));
        }
        let mut image = ChargedFile::create(
            &target.join(name(id.chunk, "image")),
            &mut budget,
            chunk.end(),
        )?;
        let mut image_hash = Sha256::new();
        image_hash.update(IMAGE_DOMAIN);
        let mut offset = 0usize;
        while offset < span.end() {
            let bytes = span.read(offset, (span.end() - offset).min(BUFFER))?;
            image.write_all(bytes)?;
            image_hash.update(bytes);
            offset += bytes.len();
        }
        point("h5.image_written")?;
        image.finish()?;
        point("h5.image_synced")?;
        let output = ChargedFile::create(
            &target.join(name(id.chunk, "origins")),
            &mut budget,
            limits.max_provenance_bytes,
        )?;
        let mut buffered = BufWriter::with_capacity(BUFFER, output);
        let origins = super::super::provenance::encode(chunk, &mut buffered)?;
        buffered.flush()?;
        point("h5.origins_written")?;
        buffered
            .into_inner()
            .map_err(|e| Error::Io(e.into_error()))?
            .finish()?;
        point("h5.origins_synced")?;
        let description = ChunkDescription {
            identity: id,
            end: chunk.end(),
            image_digest: image_hash.finalize().into(),
            provenance_digest: origins.digest,
            provenance_bytes: origins.length,
            extent_count: chunk.len() as u64,
            bounds: chunk.bounds(),
        };
        let row = descriptor::entry(&description);
        budget.check(row.len() as u64 + 32, 0)?;
        write_descriptor_part(&mut descriptor_file, &mut budget, &row)?;
        hash.update(row);
    }
    ulmdb_platform::sync_path(target)?;
    point("h5.assets_directory_synced")?;
    let digest: [u8; 32] = hash.finalize().into();
    write_descriptor_part(&mut descriptor_file, &mut budget, &digest)?;
    point("h5.descriptor_written")?;
    ulmdb_platform::set_private_permissions(&descriptor_file, false)?;
    ulmdb_platform::sync_file(&descriptor_file)?;
    point("h5.descriptor_synced")?;
    budget.held_apparent = 0;
    budget.held_allocated = 0;
    budget.add(&ulmdb_platform::file_identity(&descriptor_file)?)?;
    ulmdb_platform::rename_replace(&target.join("DESCRIPTOR.tmp"), &target.join("CHECKPOINT"))
        .map_err(ulmdb_platform::rename_original_error)?;
    point("h5.descriptor_named")?;
    ulmdb_platform::sync_path(target)?;
    point("h5.final_directory_synced")?;
    ulmdb_platform::sync_path(parent)?;
    point("h5.final_parent_synced")?;
    budget.held_directory_apparent = 0;
    budget.held_directory_allocated = 0;
    budget.add(&ulmdb_platform::path_identity(&target.canonicalize()?)?)?;
    let space = budget.space;
    owned.keep = true;
    Ok(CheckpointArtifact {
        path: target.to_path_buf(),
        generation,
        digest,
        space,
    })
}
/// Checks the complete private artifact, including exact origin receipts and
/// all image bytes. This does not validate index graphs or issue a checkpoint
/// recovery authority; those belong to later H5 selection/recovery work.
pub fn verify_artifact(
    target: &Path,
    limits: CheckpointLimits,
) -> Result<super::CheckpointDescriptor> {
    let descriptor = super::read_descriptor(&target.join("CHECKPOINT"), limits.max_chunks)?;
    let mut budget = Budget {
        limits,
        space: SpaceReport::default(),
        held_apparent: 0,
        held_allocated: 0,
        held_directory_apparent: 0,
        held_directory_allocated: 0,
    };
    budget.add(&ulmdb_platform::path_identity(&target.canonicalize()?)?)?;
    let mut allowed = std::collections::BTreeSet::from(["CHECKPOINT".to_owned()]);
    budget.add(&ulmdb_platform::path_identity(
        &target.join("CHECKPOINT").canonicalize()?,
    )?)?;
    let mut buffer = vec![0u8; BUFFER];
    for c in &descriptor.chunks {
        let image_name = name(c.identity.chunk, "image");
        let provenance_name = name(c.identity.chunk, "origins");
        allowed.insert(image_name.clone());
        allowed.insert(provenance_name.clone());
        let mut image = open_regular(&target.join(image_name))?;
        let meta = ulmdb_platform::file_identity(&image)?;
        budget.add(&meta)?;
        if meta.len() != c.end {
            return Err(Error::Corrupt("checkpoint image length"));
        }
        let mut hash = Sha256::new();
        hash.update(IMAGE_DOMAIN);
        let mut remaining = c.end;
        while remaining > 0 {
            let n =
                usize::try_from(remaining.min(BUFFER as u64)).map_err(|_| Error::LimitExceeded)?;
            image.read_exact(&mut buffer[..n])?;
            hash.update(&buffer[..n]);
            remaining -= n as u64;
        }
        if image.metadata()?.len() != c.end {
            return Err(Error::Corrupt("checkpoint image changed length"));
        }
        if hash.finalize().as_slice() != c.image_digest {
            return Err(Error::Corrupt("checkpoint image digest"));
        }
        let mut origins = open_regular(&target.join(provenance_name))?;
        let meta = ulmdb_platform::file_identity(&origins)?;
        budget.add(&meta)?;
        if meta.len() != c.provenance_bytes || meta.len() > limits.max_provenance_bytes {
            return Err(Error::Corrupt("checkpoint origins length"));
        }
        let length = usize::try_from(meta.len()).map_err(|_| Error::LimitExceeded)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| Error::LimitExceeded)?;
        bytes.resize(length, 0);
        origins.read_exact(&mut bytes)?;
        let table = super::super::provenance::decode(&bytes)?;
        if table.identity() != c.identity
            || table.end() != c.end
            || table.len() as u64 != c.extent_count
            || table.bounds() != c.bounds
            || bytes[bytes.len() - 32..] != c.provenance_digest
        {
            return Err(Error::Corrupt("checkpoint origin descriptor mismatch"));
        }
    }
    for entry in fs::read_dir(target)? {
        let entry = entry?;
        let n = entry
            .file_name()
            .into_string()
            .map_err(|_| Error::Corrupt("checkpoint noncanonical name"))?;
        if !allowed.remove(&n) {
            return Err(Error::Corrupt("checkpoint unexpected file"));
        }
    }
    if !allowed.is_empty() {
        return Err(Error::Corrupt("checkpoint missing files"));
    }
    Ok(descriptor)
}
fn open_regular(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::Corrupt("checkpoint nonregular file"));
    }
    Ok(file)
}

impl super::CheckpointCut {
    /// Byte-for-byte correspondence to this immutable cut. Self-consistent
    /// untrusted archives alone do not prove that they contain selected data.
    pub fn verify_staged(
        &self,
        path: &Path,
        limits: CheckpointLimits,
    ) -> Result<super::CheckpointDescriptor> {
        let archive = super::CheckpointArchive::open(path, limits)?;
        let d = archive.descriptor();
        if d.uuid != self.uuid
            || d.epoch != self.epoch
            || d.selected != self.selected
            || d.chunks.len() != self.directory.len()
        {
            return Err(Error::Corrupt("checkpoint cut descriptor mismatch"));
        }
        for (&id, chunk) in archive.chunks() {
            let known = self
                .directory
                .get(id)
                .ok_or(Error::Corrupt("checkpoint cut origin chunk absent"))?;
            if chunk.table().identity() != known.identity()
                || chunk.table().end() != known.end()
                || !chunk.table().origins().eq(known.origins())
            {
                return Err(Error::Corrupt("checkpoint cut exact origins mismatch"));
            }
            let span = self
                .image
                .chunk(id)
                .ok_or(Error::Corrupt("checkpoint cut image chunk absent"))?;
            let mut offset = 0;
            while offset < span.end() {
                let n = (span.end() - offset).min(BUFFER);
                if span.read(offset, n)? != chunk.read(offset, n)? {
                    return Err(Error::Corrupt("checkpoint cut image bytes differ"));
                }
                offset += n;
            }
        }
        Ok(d.clone())
    }
}

#[cfg(test)]
mod quota_tests {
    use super::*;
    #[test]
    fn descriptor_header_and_empty_footer_are_preflighted_before_file_growth() {
        let directory =
            std::env::temp_dir().join(format!("ulmdb-h5-quota-{}", crate::new_identity().unwrap()));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("descriptor");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let mut budget = Budget {
            limits: CheckpointLimits {
                max_apparent_bytes: 1,
                ..CheckpointLimits::default()
            },
            space: SpaceReport::default(),
            held_apparent: 0,
            held_allocated: 0,
            held_directory_apparent: 0,
            held_directory_allocated: 0,
        };
        assert!(matches!(
            write_descriptor_part(&mut file, &mut budget, &[0; 256]),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(file.metadata().unwrap().len(), 0);
        budget.limits.max_apparent_bytes = 256;
        write_descriptor_part(&mut file, &mut budget, &[0; 256]).unwrap();
        assert!(matches!(
            write_descriptor_part(&mut file, &mut budget, &[0; 32]),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(file.metadata().unwrap().len(), 256);
        budget.limits.max_apparent_bytes = 288;
        write_descriptor_part(&mut file, &mut budget, &[0; 32]).unwrap();
        assert_eq!(file.metadata().unwrap().len(), 288);
        drop(file);
        fs::remove_dir_all(directory).unwrap();
    }
}
