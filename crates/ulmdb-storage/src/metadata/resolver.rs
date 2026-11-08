use super::NodeRef;
use crate::{Error, Result};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
pub(crate) trait NodeSource {
    fn read(&mut self, origin: u128, offset: u64, length: u32, bytes: &mut [u8]) -> Result<()>;
}
struct Origin {
    decoded: Option<(memmap2::Mmap, crate::transform::DecodedLease)>,
    file: File,
    checked_path: Option<PathBuf>,
    payload: u64,
    descriptor: u64,
}
/// At most16 cached open handles, leaving descriptor headroom for capture and
/// data admission under an ordinary64-descriptor process limit. Only bounded header/footer+requested node
/// bytes are read: historical root inventories are never recursively decoded.
pub(crate) struct Resolver {
    directory: PathBuf,
    fallback: Option<PathBuf>,
    origins: BTreeMap<u128, Origin>,
    order: VecDeque<u128>,
    source: Option<Box<dyn NodeSource>>,
    mode: crate::read_mode::ReadMode,
}
impl Resolver {
    pub fn new(directory: &Path, fallback: Option<&Path>) -> Self {
        Self::new_with_mode(directory, fallback, crate::read_mode::ReadMode::Ordinary)
    }
    pub(crate) fn new_with_mode(
        directory: &Path,
        fallback: Option<&Path>,
        mode: crate::read_mode::ReadMode,
    ) -> Self {
        Self {
            directory: directory.to_path_buf(),
            fallback: fallback.map(Path::to_path_buf),
            origins: BTreeMap::new(),
            order: VecDeque::new(),
            source: None,
            mode,
        }
    }
    pub(crate) fn from_source(source: Box<dyn NodeSource>) -> Self {
        let mut resolver = Self::new(Path::new(""), None);
        resolver.source = Some(source);
        resolver
    }
    fn open(&self, generation: u128) -> Result<Origin> {
        // Original files always take precedence. Missing metadata origins may
        // resolve from immutable history tails; present corruption never falls
        // through to a different source. Existing NodeRef hashes remain the
        // authority for each node, not the archive filename/header alone.
        let mut selected = None;
        for directory in std::iter::once(self.directory.as_path()).chain(self.fallback.as_deref()) {
            let path = crate::cohort::path(directory, generation);
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_file() => {
                    let mut file = self.mode.open(&path)?;
                    let (payload, descriptor, minor) = crate::cohort::geometry(&mut file)?;
                    selected = Some((path, file, payload, descriptor, minor, false));
                    break;
                }
                Ok(_) => return Err(Error::Corrupt("metadata origin is not a regular file")),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    match crate::cohort_history::open(directory, generation) {
                        Ok(archive) => {
                            if archive.transformed != self.mode.context().is_some() {
                                return Err(Error::CryptoMismatch);
                            }
                            selected = Some((crate::cohort_history::path(directory, generation),
                                archive.file, archive.body, archive.physical, archive.minor, true));
                            break;
                        }
                        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => (),
                        Err(e) => return Err(e),
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
        let (path, mut file, payload, mut descriptor, minor, archived) = selected
            .ok_or_else(|| Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "metadata origin unavailable")))?;
        let decoded = if let Some(context) = self.mode.context() {
            let (logical, encoded) = crate::transform::envelope_geometry(&file, payload)?;
            if encoded != descriptor {
                return Err(Error::Corrupt("metadata encoded extent"));
            }
            descriptor = logical;
            Some(context.decode_mapping_at(
                &file,
                payload,
                crate::transform::ObjectKind::Descriptor,
                generation,
                logical,
            )?)
        } else {
            None
        };
        if minor != 4 {
            return Err(Error::Corrupt("metadata node origin is not indexed"));
        }
        let mut header = [0; 256];
        if let Some((bytes, _)) = &decoded {
            header.copy_from_slice(
                bytes
                    .get(..256)
                    .ok_or(Error::Corrupt("metadata descriptor header"))?,
            );
        } else {
            file.seek(SeekFrom::Start(payload))?;
            file.read_exact(&mut header)?;
        }
        if &header[..8] != b"ULMDBROT"
            || header[8..10] != [0; 2]
            || header[10..12] != 4u16.to_le_bytes()
            || header[12..16] != 256u32.to_le_bytes()
            || header[32..48] != generation.to_le_bytes()
            || ![
                15u64.to_le_bytes(),
                31u64.to_le_bytes(),
                47u64.to_le_bytes(),
                63u64.to_le_bytes(),
            ]
            .contains(&header[120..128].try_into().unwrap())
        {
            return Err(Error::Corrupt("metadata origin descriptor identity/header"));
        }
        crate::Manifest::page_size_from_header(&header)?;
        crate::Manifest::value_limit_from_header(&header)?;
        self.mode.check(&path, &file)?;
        Ok(Origin {
            decoded,
            file,
            checked_path: (archived || self.mode.is_checked()).then_some(path),
            payload,
            descriptor,
        })
    }
    #[cfg(test)]
    pub(super) fn open_handles(&self) -> usize {
        self.origins.len()
    }
    pub(super) fn read(&mut self, r: NodeRef, bytes: &mut [u8; 160]) -> Result<()> {
        if let Some(source) = &mut self.source {
            return source.read(
                r.origin,
                r.offset,
                r.length,
                &mut bytes[..r.length as usize],
            );
        }
        if !self.origins.contains_key(&r.origin) {
            if self.origins.len() == 16 {
                let oldest = self
                    .order
                    .pop_front()
                    .ok_or(Error::Corrupt("metadata handle cache"))?;
                self.origins.remove(&oldest);
            }
            let origin = self.open(r.origin)?;
            self.origins.insert(r.origin, origin);
            self.order.push_back(r.origin);
        }
        let origin = self.origins.get_mut(&r.origin).unwrap();
        if r.offset
            .checked_add(u64::from(r.length))
            .is_none_or(|end| end > origin.descriptor)
        {
            return Err(Error::Corrupt("metadata origin node extent"));
        }
        let offset = origin
            .payload
            .checked_add(r.offset)
            .ok_or(Error::Corrupt("metadata origin offset overflow"))?;
        if let Some((decoded, _)) = &origin.decoded {
            let start = usize::try_from(r.offset).map_err(|_| Error::LimitExceeded)?;
            bytes[..r.length as usize].copy_from_slice(
                decoded
                    .get(start..start + r.length as usize)
                    .ok_or(Error::Corrupt("decoded metadata node bounds"))?,
            );
        } else {
            origin.file.seek(SeekFrom::Start(offset))?;
            origin.file.read_exact(&mut bytes[..r.length as usize])?;
        }
        if let Some(path) = &origin.checked_path {
            crate::read_mode::ReadMode::Sync.check(path, &origin.file)?;
        }
        Ok(())
    }
}
