use super::types::{AssetDescription, AssetId, ReplicationError as Error, Result};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use ulmdb_storage::Manifest;
pub(super) const MAX_ASSETS: u32 = 1_048_576;
fn hex(hash: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(68);
    for b in hash {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out.push_str(".seg");
    out
}
impl AssetId {
    pub fn path(self, directory: &Path) -> PathBuf {
        match self {
            Self::Segment(hash) => directory.join("segments").join(hex(&hash)),
            Self::Descriptor(generation) => directory
                .join("commits")
                .join(format!("{generation:032x}.root")),
            Self::Cohort(generation) => directory
                .join("cohorts")
                .join(format!("{generation:032x}.cohort")),
        }
    }
    pub(super) fn bounds(self) -> (u64, u64) {
        match self {
            Self::Segment(_) => (4096, ulmdb_storage::MAX_COHORT_BYTES),
            Self::Descriptor(_) => (192, 67_109_056),
            Self::Cohort(_) => (224, ulmdb_storage::MAX_COHORT_BYTES),
        }
    }
    pub(crate) fn check_length(self, length: u64) -> Result<()> {
        let (min, max) = self.bounds();
        if !(min..=max).contains(&length) {
            return Err(Error::Corrupt(
                "asset physical length outside format bounds",
            ));
        }
        Ok(())
    }
}
pub(crate) fn ids(manifest: &Manifest) -> BTreeSet<AssetId> {
    let mut assets = BTreeSet::new();
    assets.insert(if manifest.format_minor >= 3 {
        AssetId::Cohort(manifest.generation)
    } else {
        AssetId::Descriptor(manifest.generation)
    });
    for segment in &manifest.segments {
        assets.insert(if segment.cohort == 0 {
            AssetId::Segment(segment.hash)
        } else {
            AssetId::Cohort(segment.cohort)
        });
    }
    assets.extend(manifest.metadata_origins().into_iter().map(AssetId::Cohort));
    assets
}
pub(super) fn describe_manifest(
    directory: &Path,
    manifest: &Manifest,
) -> Result<Vec<AssetDescription>> {
    describe_ids(directory, ids(manifest))
}
pub(crate) fn describe_ids(
    directory: &Path,
    ids: impl IntoIterator<Item = AssetId>,
) -> Result<Vec<AssetDescription>> {
    let mut assets = Vec::new();
    for id in ids {
        if assets.len() >= MAX_ASSETS as usize {
            return Err(Error::LimitExceeded);
        }
        let metadata = fs::symlink_metadata(id.path(directory))?;
        if !metadata.file_type().is_file() {
            return Err(Error::Corrupt(
                "replication source asset is not a regular file",
            ));
        }
        let length = metadata.len();
        id.check_length(length)?;
        assets.push(AssetDescription { id, length });
    }
    Ok(assets)
}
