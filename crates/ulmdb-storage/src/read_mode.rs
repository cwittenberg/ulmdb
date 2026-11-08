//! Private read policy: ordinary readers retain their established open contract;
//! explicit synchronization rejects nonregular or replaced named metadata.
use crate::{Error, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::Path,
};
#[derive(Clone)]
pub(crate) enum ReadMode {
    Ordinary,
    Sync,
    Transformed {
        context: std::sync::Arc<crate::transform::TransformContext>,
        checked: bool,
    },
}
impl PartialEq for ReadMode {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Ordinary, Self::Ordinary) | (Self::Sync, Self::Sync) => true,
            (
                Self::Transformed {
                    context: a,
                    checked: x,
                },
                Self::Transformed {
                    context: b,
                    checked: y,
                },
            ) => x == y && std::sync::Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}
impl Eq for ReadMode {}
impl ReadMode {
    pub(crate) fn with_transform(
        self,
        context: Option<std::sync::Arc<crate::transform::TransformContext>>,
    ) -> Self {
        match context {
            Some(context) => Self::Transformed {
                checked: self.is_checked(),
                context,
            },
            None => self,
        }
    }
    pub(crate) fn context(&self) -> Option<&std::sync::Arc<crate::transform::TransformContext>> {
        match self {
            Self::Transformed { context, .. } => Some(context),
            _ => None,
        }
    }
    pub(crate) fn is_checked(&self) -> bool {
        matches!(self, Self::Sync | Self::Transformed { checked: true, .. })
    }
    pub(crate) fn open(&self, path: &Path) -> Result<File> {
        if !self.is_checked() {
            return Ok(File::open(path)?);
        }
        self.check_named(path)?;
        // Explicit opt-in rendezvous proves a regular-to-FIFO replacement
        // cannot block between lstat and open. Normal fault traces are unchanged.
        #[cfg(feature = "fault-injection")]
        crate::fault::history_point("sync.metadata.before_open")?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(ulmdb_platform::READ_NOFOLLOW_FLAGS);
        }
        let file = options.open(path)?;
        self.check(path, &file)?;
        Ok(file)
    }
    fn check_named(&self, path: &Path) -> Result<()> {
        let named = fs::symlink_metadata(path)?;
        if !named.is_file() || named.file_type().is_symlink() {
            return Err(Error::Corrupt("invalid durability file type"));
        }
        Ok(())
    }
    pub(crate) fn check(&self, path: &Path, file: &File) -> Result<()> {
        if !self.is_checked() {
            return Ok(());
        }
        self.check_named(path)?;
        let owned = file.metadata()?;
        if !owned.is_file() {
            return Err(Error::Corrupt("invalid durability file type"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let named = fs::symlink_metadata(path)?;
            if (owned.dev(), owned.ino()) != (named.dev(), named.ino()) {
                return Err(Error::Corrupt("durability inode changed"));
            }
        }
        #[cfg(windows)]
        {
            let owned = ulmdb_platform::file_identity(file)?;
            let named = ulmdb_platform::path_identity(path)?;
            if !owned.is_file() || !named.is_file() || !owned.same_object(&named) {
                return Err(Error::Corrupt("durability object changed"));
            }
        }
        Ok(())
    }
    /// The legacy ordinary path deliberately keeps its existing metadata/read
    /// order. Sync bounds the bytes on its checked owned descriptor instead.
    pub(crate) fn read_legacy(&self, path: &Path, maximum: u64) -> Result<Vec<u8>> {
        if let Some(context) = self.context() {
            let file = self.open(path)?;
            let (logical, encoded) = crate::transform::envelope_geometry(&file, 0)?;
            if logical > maximum || file.metadata()?.len() != encoded {
                return Err(Error::Corrupt("transformed descriptor extent"));
            }
            let id = u128::from_str_radix(
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or(Error::Corrupt("descriptor filename"))?,
                16,
            )
            .map_err(|_| Error::Corrupt("descriptor identity"))?;
            let (bytes, _lease) = context.decode_mapping(
                &file,
                crate::transform::ObjectKind::Descriptor,
                id,
                logical,
            )?;
            let output = bytes.to_vec();
            self.check(path, &file)?;
            return Ok(output);
        }
        if !self.is_checked() {
            if fs::metadata(path)?.len() > maximum {
                return Err(Error::LimitExceeded);
            }
            return Ok(fs::read(path)?);
        }
        let mut file = self.open(path)?;
        if file.metadata()?.len() > maximum {
            return Err(Error::LimitExceeded);
        }
        let mut bytes = Vec::new();
        (&mut file).take(maximum + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > maximum {
            return Err(Error::LimitExceeded);
        }
        self.check(path, &file)?;
        Ok(bytes)
    }
}
