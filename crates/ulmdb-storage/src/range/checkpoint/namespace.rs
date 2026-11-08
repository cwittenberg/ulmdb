//! Runtime directory/creator binding, separate from portable frame identities.
use super::*;
use std::path::PathBuf;
pub(in crate::range) struct Namespace {
    path: PathBuf,
    directory: File,
    creator: u32,
}
impl Namespace {
    pub(in crate::range) fn capture(path: &Path) -> Result<Arc<Self>> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Corrupt("range namespace is not a real directory"));
        }
        let binding = Arc::new(Self {
            path: fs::canonicalize(path)?,
            directory: File::open(path)?,
            creator: std::process::id(),
        });
        if !binding.matches(path)? {
            return Err(Error::Busy);
        }
        Ok(binding)
    }
    pub(in crate::range) fn check_creator(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput(
                "range lease belongs to another process",
            ));
        }
        Ok(())
    }
    pub(in crate::range) fn matches(&self, path: &Path) -> Result<bool> {
        self.check_creator()?;
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || fs::canonicalize(path)? != self.path
        {
            return Ok(false);
        }
        let actual = File::open(path)?;
        Ok(ulmdb_platform::file_identity(&actual)?
            .same_object(&ulmdb_platform::file_identity(&self.directory)?))
    }
    pub(super) fn same_binding(&self, other: &Self) -> Result<bool> {
        self.check_creator()?;
        other.check_creator()?;
        Ok(self.path == other.path
            && ulmdb_platform::file_identity(&self.directory)?
                .same_object(&ulmdb_platform::file_identity(&other.directory)?))
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(in crate::range) fn reader(&self, path: &Path) -> Result<File> {
        self.reader_asset(path, super::super::rotation::selection(path)?.file)
    }
    pub(in crate::range) fn reader_asset(&self, path: &Path, id: [u8; 16]) -> Result<File> {
        self.check_creator()?;
        if !self.matches(path)? {
            return Err(Error::Corrupt("range namespace replaced"));
        }
        let name = super::super::rotation::asset(path, id)?;
        let mode = crate::read_mode::ReadMode::Sync;
        let file = mode.open(&name)?;
        // Independent from the writer's flock/OFD. Clones retain this same
        // shared physical-asset lease; Drop only closes, never explicitly unlocks.
        crate::hybrid::retirement::shared(&file)?;
        mode.check(&name, &file)?;
        if !self.matches(path)? {
            return Err(Error::Corrupt("range namespace replaced while capturing"));
        }
        Ok(file)
    }
}
