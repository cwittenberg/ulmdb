//! Bounded positional chunk sources. These are NOT selected-root authorities.
use super::{
    geometry::{self, ALIGN_BYTES, DATA_BYTES, HEADER_BYTES},
    header::{self, ChunkBinding, ChunkHeader, StreamId},
};
use crate::{Error, Result};
use std::{fs::File, io, path::Path, sync::Arc};
#[derive(Clone)]
pub struct ChunkLease {
    file: Arc<File>,
    header: Arc<ChunkHeader>,
    visible_end: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtentMode {
    SealedExact,
    AppendSnapshot,
}
impl ChunkLease {
    /// The caller obtains binding/end from a verified publication or predecessor
    /// chain. Opening validates this exact envelope and physical bound, and owns
    /// an independent read-only FD. It grants no mutation/head capability.
    pub fn open(
        directory: &Path,
        binding: ChunkBinding,
        visible_end: u64,
        mode: ExtentMode,
    ) -> Result<Self> {
        let file = File::open(directory.join(header::filename(binding.stream, binding.index)))?;
        let mut bytes = [0; HEADER_BYTES as usize];
        super::super::pack::io::read_exact_at(&file, &mut bytes, 0)?;
        let header = ChunkHeader::decode(&bytes)?;
        if header.binding() != binding {
            return Err(Error::Corrupt("chunk physical envelope binding"));
        }
        if visible_end <= header.base()
            || visible_end % ALIGN_BYTES != 0
            || visible_end - header.base() > DATA_BYTES
        {
            return Err(Error::Corrupt("chunk visible boundary"));
        }
        let physical = HEADER_BYTES
            .checked_add(visible_end - header.base())
            .ok_or(Error::LimitExceeded)?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.len() < physical
            || metadata.len() > HEADER_BYTES + DATA_BYTES
            || (mode == ExtentMode::SealedExact && metadata.len() != physical)
        {
            return Err(Error::Corrupt("chunk physical EOF"));
        }
        Ok(Self {
            file: Arc::new(file),
            header: Arc::new(header),
            visible_end,
        })
    }
    pub fn header(&self) -> &ChunkHeader {
        &self.header
    }
    pub fn visible_end(&self) -> u64 {
        self.visible_end
    }
    pub fn physical_span(&self, start: u64, length: u64) -> Result<(u64, u64)> {
        let end = start.checked_add(length).ok_or(Error::LimitExceeded)?;
        if start < self.header.base() || end > self.visible_end {
            return Err(Error::Corrupt("chunk span crosses bound/header"));
        }
        // Empty endpoints retain this exact chunk owner. Logical base of the
        // next chunk and one-past this file have distinct physical provenance.
        if length == 0
            && start > self.header.base()
            && geometry::locate(start).0 != self.header.index()
        {
            return Ok((
                geometry::locate(start - 1)
                    .1
                    .checked_add(1)
                    .ok_or(Error::LimitExceeded)?,
                0,
            ));
        }
        if geometry::locate(start).0 != self.header.index()
            || (length != 0 && geometry::locate(end - 1).0 != self.header.index())
        {
            return Err(Error::Corrupt("chunk span crosses bound/header"));
        }
        Ok((geometry::locate(start).1, length))
    }
    pub fn read_at(&self, bytes: &mut [u8], offset: u64) -> io::Result<usize> {
        if offset < self.header.base() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        if offset >= self.visible_end {
            return Ok(0);
        }
        let count = (self.visible_end - offset).min(bytes.len() as u64) as usize;
        let physical = geometry::locate(offset).1;
        super::super::pack::io::ReadAt::read_at(&*self.file, &mut bytes[..count], physical)
    }
}
/// A resolver must supply leases whose envelope/bounds are bound to the
/// selected v7 head's authenticated chunk chain. The interface deliberately
/// imposes no whole-map clone or segment-table serving-path change.
pub trait ChunkResolve {
    fn lease(&self, index: u64) -> Result<ChunkLease>;
}
impl ChunkResolve for ChunkLease {
    fn lease(&self, index: u64) -> Result<ChunkLease> {
        if self.header.index() != index {
            return Err(Error::Corrupt("wrong chunk lease index"));
        }
        Ok(self.clone())
    }
}
pub struct PackRead<R> {
    source: R,
    stream: StreamId,
    selected_end: u64,
}
impl<R: ChunkResolve> PackRead<R> {
    pub fn new(source: R, stream: StreamId, selected_end: u64) -> Result<Self> {
        if selected_end == 0 || selected_end % ALIGN_BYTES != 0 {
            return Err(Error::InvalidInput("chunk selected logical end"));
        }
        Ok(Self {
            source,
            stream,
            selected_end,
        })
    }
    pub fn read_at(&self, bytes: &mut [u8], offset: u64) -> io::Result<usize> {
        if offset >= self.selected_end {
            return Ok(0);
        }
        let index = geometry::locate(offset).0;
        let lease = self
            .source
            .lease(index)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if lease.header.stream() != self.stream
            || lease.header.index() != index
            || offset < lease.header.base()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "foreign chunk resolver lease",
            ));
        }
        if offset >= lease.visible_end {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "read targets unallocated rollover tail",
            ));
        }
        let count = (self.selected_end - offset).min(bytes.len() as u64) as usize;
        lease.read_at(&mut bytes[..count], offset)
    }
    pub fn selected_end(&self) -> u64 {
        self.selected_end
    }
}
impl<R: ChunkResolve> super::super::pack::io::ReadAt for PackRead<R> {
    fn read_at(&self, bytes: &mut [u8], offset: u64) -> io::Result<usize> {
        self.read_at(bytes, offset)
    }
}
