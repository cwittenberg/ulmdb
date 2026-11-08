//! Private bounded metadata publication. No public bytes-to-authority adapter.
use crate::Result;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use ulmdb_storage::hybrid::retirement::selection::{Compound, decode_compound};
#[derive(Clone, Copy)]
enum Kind {
    Floor,
    Required,
}
pub(in crate::hybrid) struct Publication {
    temporary: PathBuf,
    target: PathBuf,
    directory: PathBuf,
    created: bool,
    published: bool,
    kind: Kind,
}
fn point(kind: Kind, phase: usize) -> Result<()> {
    let names = match kind {
        Kind::Floor => [
            "h5.retire.before_selector_write",
            "h5.retire.after_selector_write",
            "h5.retire.after_selector_file_sync",
            "h5.retire.before_selector_rename",
            "h5.retire.after_selector_rename",
            "h5.retire.after_selector_directory_sync",
        ],
        Kind::Required => [
            "h5.retire.before_required_write",
            "h5.retire.after_required_write",
            "h5.retire.after_required_file_sync",
            "h5.retire.before_required_rename",
            "h5.retire.after_required_rename",
            "h5.retire.after_required_directory_sync",
        ],
    };
    super::super::checkpoint::selector::point(names[phase])
}
impl Publication {
    pub fn floor(engine: &Path, bytes: &[u8; 512]) -> Result<Self> {
        decode_compound(bytes)?;
        Self::prepare(engine, "HYBRID.base", bytes, Kind::Floor)
    }
    pub fn required(engine: &Path, c: Compound) -> Result<Self> {
        Self::prepare(
            engine,
            super::marker::NAME,
            &super::marker::encode(c),
            Kind::Required,
        )
    }
    fn prepare(engine: &Path, name: &str, bytes: &[u8], kind: Kind) -> Result<Self> {
        let mut token = Self {
            temporary: engine.join(format!(
                ".{name}-{:032x}.tmp",
                ulmdb_storage::new_identity()?
            )),
            target: engine.join(name),
            directory: engine.to_path_buf(),
            created: false,
            published: false,
            kind,
        };
        point(kind, 0)?;
        let mut f = ulmdb_platform::create_private_file(&token.temporary, false)?;
        token.created = true;
        f.write_all(bytes)?;
        point(kind, 1)?;
        ulmdb_platform::set_private_permissions(&f, false)?;
        ulmdb_platform::sync_file(&f)?;
        point(kind, 2)?;
        Ok(token)
    }
    pub fn publish(mut self) -> Result<()> {
        point(self.kind, 3)?;
        ulmdb_platform::rename_replace(&self.temporary, &self.target)?;
        self.published = true;
        point(self.kind, 4)?;
        ulmdb_platform::sync_path(&self.directory)?;
        point(self.kind, 5)?;
        Ok(())
    }
}
impl Drop for Publication {
    fn drop(&mut self) {
        if self.created && !self.published {
            let _ = fs::remove_file(&self.temporary);
        }
    }
}
