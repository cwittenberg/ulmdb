//! Private recovery-base encoding/publication: only PreparedCheckpoint reaches
//! this module. The engine directory is trusted and exclusively writer-owned.
use crate::Result;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use ulmdb_storage::hybrid::checkpoint::CheckpointDescriptor;
pub(in crate::hybrid) use ulmdb_storage::hybrid::checkpoint::selection::Selector;
pub(in crate::hybrid) use ulmdb_storage::hybrid::checkpoint::selection::read;
const NAME: &str = "HYBRID.base";
const DOMAIN: &[u8] = b"ULMDB/hybrid/recovery-base/v1\0";
fn p64(b: &mut [u8], at: usize, n: u64) {
    b[at..at + 8].copy_from_slice(&n.to_le_bytes());
}
pub(in crate::hybrid) fn from_descriptor(d: &CheckpointDescriptor) -> Selector {
    Selector {
        uuid: d.uuid,
        epoch: d.epoch,
        generation: d.generation,
        cut: d.selected,
        digest: d.digest,
    }
}
fn encode(selector: Selector) -> [u8; 256] {
    let mut b = [0; 256];
    b[..8].copy_from_slice(b"ULMDBHB1");
    b[8..12].copy_from_slice(&1u32.to_le_bytes());
    b[12..16].copy_from_slice(&256u32.to_le_bytes());
    b[16..32].copy_from_slice(&selector.uuid);
    p64(&mut b, 32, selector.epoch);
    b[40..56].copy_from_slice(&selector.generation.to_le_bytes());
    p64(&mut b, 56, selector.cut.receipt.lsn);
    p64(&mut b, 64, selector.cut.receipt.sequence);
    p64(&mut b, 72, selector.cut.receipt.offset);
    p64(&mut b, 80, selector.cut.receipt.length);
    b[88..120].copy_from_slice(&selector.cut.receipt.digest);
    b[120..136].copy_from_slice(&selector.cut.commit.to_le_bytes());
    b[136..152].copy_from_slice(&selector.cut.root.catalog.encode());
    p64(&mut b, 152, selector.cut.root.next_chunk);
    p64(&mut b, 160, selector.cut.root.next_database);
    b[168..200].copy_from_slice(&selector.digest);
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(&b[..224]);
    b[224..].copy_from_slice(&h.finalize());
    b
}
pub(super) struct Publication {
    temporary: PathBuf,
    target: PathBuf,
    directory: PathBuf,
    published: bool,
    created: bool,
}
impl Publication {
    pub fn prepare(engine: &Path, selector: Selector) -> Result<Self> {
        ulmdb_storage::hybrid::checkpoint::selection::decode_bytes(&encode(selector))?;
        let temporary = engine.join(format!(
            ".HYBRID-base-{:032x}.tmp",
            ulmdb_storage::new_identity()?
        ));
        let mut token = Self {
            temporary,
            target: engine.join(NAME),
            directory: engine.to_path_buf(),
            published: false,
            created: false,
        };
        point("h5.activate.before_selector_write")?;
        let mut f = ulmdb_platform::create_private_file(&token.temporary, false)?;
        token.created = true;
        f.write_all(&encode(selector))?;
        point("h5.activate.after_selector_write")?;
        ulmdb_platform::set_private_permissions(&f, false)?;
        ulmdb_platform::sync_file(&f)?;
        point("h5.activate.after_selector_file_sync")?;
        Ok(token)
    }
    /// Caller marks the attempt uncertain BEFORE entering this rename/fence.
    pub fn publish(mut self) -> Result<()> {
        point("h5.activate.before_selector_rename")?;
        ulmdb_platform::rename_replace(&self.temporary, &self.target)?;
        self.published = true;
        point("h5.activate.after_selector_rename")?;
        ulmdb_platform::sync_path(&self.directory)?;
        point("h5.activate.after_selector_directory_sync")?;
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
pub(in crate::hybrid) fn point(name: &str) -> Result<()> {
    #[cfg(feature = "fault-injection")]
    {
        if let Some(path) = std::env::var_os("ULMDB_H5_ACT_TRACE") {
            writeln!(
                OpenOptions::new().create(true).append(true).open(path)?,
                "{name}"
            )?;
        }
        let exit = std::env::var("ULMDB_H5_ACT_EXIT_AT").is_ok_and(|p| p == name);
        let fail = std::env::var("ULMDB_H5_ACT_FAIL_AT").is_ok_and(|p| p == name);
        if exit || fail {
            static MATCHES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let skip = std::env::var("ULMDB_H5_ACT_SKIP")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            if MATCHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == skip {
                if exit {
                    std::process::exit(86);
                }
                return Err(std::io::Error::other("injected H5 activation IO failure").into());
            }
        }
    }
    #[cfg(not(feature = "fault-injection"))]
    let _ = name;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_exclusive_create_does_not_authorize_temporary_file_cleanup() {
        let dir = std::env::temp_dir().join(format!(
            "ulmdb-cp-selection-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("unowned");
        fs::write(&path, b"keep").unwrap();
        let token = Publication {
            temporary: path.clone(),
            target: dir.join(NAME),
            directory: dir.clone(),
            published: false,
            created: false,
        };
        drop(token);
        assert_eq!(fs::read(path).unwrap(), b"keep");
        fs::remove_dir_all(dir).unwrap();
    }
}
