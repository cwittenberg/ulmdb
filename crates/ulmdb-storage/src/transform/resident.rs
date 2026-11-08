//! Append-only authenticated Resident packets. An append never rewrites a
//! selected ciphertext byte. The two head slots are independent repair units.
use super::*;
use crate::WriterFence;
use std::{
    ops::Deref,
    sync::{Mutex, TryLockError},
};
const DATA: u64 = 196608;
const BLOCK: u64 = 256;
const MAX_RECORD: u64 = 32 * 1024 * 1024;
fn unknown_lock<T>(result: std::result::Result<T, TryLockError<T>>) -> Result<T> {
    result.map_err(|_| Error::Busy)
}
fn packet_length(context: &TransformContext, length: u64) -> Result<u64> {
    let pages = length.div_ceil(PAGE as u64);
    let padded = length.checked_add(15).ok_or(Error::LimitExceeded)? / 16 * 16;
    padded
        .checked_add(
            pages * (BIND + context.checksum_bytes() + context.authentication_bytes()) as u64,
        )
        .and_then(|n| n.checked_add(HEADER as u64))
        .ok_or(Error::LimitExceeded)
}
fn packet_header(
    context: &TransformContext,
    id: u64,
    length: u64,
    nonce: [u8; 16],
) -> Result<[u8; HEADER]> {
    let mut h = context.header(ObjectKind::ResidentWal, id as u128, length, nonce)?;
    h[45] = 1;
    h[64..72].copy_from_slice(&packet_length(context, length)?.to_le_bytes());
    let hash: [u8; 32] = Sha256::digest(&h[..96]).into();
    h[96..].copy_from_slice(&hash);
    Ok(h)
}
fn packet_geometry(
    context: &TransformContext,
    file: &File,
    physical: u64,
    logical: u64,
) -> Result<(u64, u64, [u8; HEADER])> {
    let mut h = [0; HEADER];
    file.read_exact_at(&mut h, physical)?;
    let length = u64::from_le_bytes(h[32..40].try_into().unwrap());
    if length == 0
        || length > MAX_RECORD
        || h != packet_header(context, logical, length, h[48..64].try_into().unwrap())?
    {
        return Err(Error::Corrupt("Resident transformed packet header"));
    }
    Ok((length, packet_length(context, length)?, h))
}
fn encode(context: &TransformContext, logical: u64, source: &[u8]) -> Result<Vec<u8>> {
    let h = packet_header(
        context,
        logical,
        source.len() as u64,
        crate::new_identity()?.to_le_bytes(),
    )?;
    let length = packet_length(context, source.len() as u64)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(length as usize)
        .map_err(|_| Error::LimitExceeded)?;
    output.extend_from_slice(&h);
    for (index, data) in source.chunks(PAGE).enumerate() {
        let padded = data.len().div_ceil(16) * 16;
        let mut plain = vec![0; BIND + padded + context.checksum_bytes()];
        plain[..BIND].copy_from_slice(&TransformContext::binding(&h, index as u64));
        plain[BIND..BIND + data.len()].copy_from_slice(data);
        if let Some(sum) = &context.checksum {
            let (data, out) = plain.split_at_mut(BIND + padded);
            sum.callback.compute(data, out, context.key())?;
        }
        let mut tag = vec![0; context.authentication_bytes()];
        if let Some(enc) = &context.encryption {
            let mut cipher = vec![0; plain.len()];
            enc.callback
                .transform(
                    &plain,
                    &mut cipher,
                    &enc.key.0,
                    &TransformContext::iv(&h, index as u64),
                    &mut tag,
                    Direction::Encrypt,
                )
                .map_err(|_| Error::CryptoFailed)?;
            output.extend_from_slice(&cipher);
        } else {
            output.extend_from_slice(&plain);
        }
        output.extend_from_slice(&tag);
    }
    if output.len() as u64 != length {
        return Err(Error::Corrupt("Resident transformed packet length"));
    }
    Ok(output)
}
struct Cache {
    logical: u64,
    bytes: Mmap,
    _lease: DecodedLease,
}
fn decode(
    context: &Arc<TransformContext>,
    file: &File,
    physical: u64,
    logical: u64,
) -> Result<Cache> {
    let (length, encoded, h) = packet_geometry(context, file, physical, logical)?;
    if (logical < DATA && length != BLOCK)
        || (logical >= DATA && (length < 288 || !length.is_multiple_of(8)))
    {
        return Err(Error::Corrupt("Resident packet role/length"));
    }
    if physical
        .checked_add(encoded)
        .is_none_or(|end| end > file.metadata().map(|m| m.len()).unwrap_or(0))
    {
        return Err(Error::Corrupt("Resident packet crosses capacity"));
    }
    let lease = context.reserve(TransformContext::mapped_charge(length)?)?;
    let mut bytes = MmapMut::map_anon(length as usize)?;
    let mut position = physical + HEADER as u64;
    for index in 0..length.div_ceil(PAGE as u64) {
        let start = index as usize * PAGE;
        let count = (length as usize - start).min(PAGE);
        let padded = count.div_ceil(16) * 16;
        let size = BIND + padded + context.checksum_bytes();
        let mut cipher = vec![0; size];
        let mut plain = vec![0; size];
        let mut auth = vec![0; context.authentication_bytes()];
        file.read_exact_at(&mut cipher, position)?;
        file.read_exact_at(&mut auth, position + size as u64)?;
        position += size as u64 + auth.len() as u64;
        if let Some(enc) = &context.encryption {
            enc.callback
                .transform(
                    &cipher,
                    &mut plain,
                    &enc.key.0,
                    &TransformContext::iv(&h, index),
                    &mut auth,
                    Direction::Decrypt,
                )
                .map_err(|_| Error::CryptoFailed)?;
        } else {
            plain.copy_from_slice(&cipher);
        }
        if plain[..BIND] != TransformContext::binding(&h, index) {
            return Err(if context.encrypted() {
                Error::CryptoFailed
            } else {
                Error::BadChecksum
            });
        }
        if let Some(sum) = &context.checksum {
            let mut expected = vec![0; sum.bytes];
            sum.callback
                .compute(&plain[..BIND + padded], &mut expected, context.key())?;
            if expected
                .iter()
                .zip(&plain[BIND + padded..])
                .fold(0u8, |a, (b, c)| a | (b ^ c))
                != 0
            {
                return Err(Error::BadChecksum);
            }
        }
        if plain[BIND + count..BIND + padded].iter().any(|b| *b != 0) {
            return Err(Error::Corrupt("Resident packet padding"));
        }
        bytes[start..start + count].copy_from_slice(&plain[BIND..BIND + count]);
    }
    Ok(Cache {
        logical,
        bytes: bytes.make_read_only()?,
        _lease: lease,
    })
}
struct Cursor {
    logical: u64,
    physical: u64,
}
/// Only journal code owns this adapter. No FileExt/raw mutable file escapes.
enum ResidentHandle {
    Writer(WriterFence),
    Reader(File),
}
impl Deref for ResidentHandle {
    type Target = File;
    fn deref(&self) -> &File {
        match self {
            Self::Writer(f) => f,
            Self::Reader(f) => f,
        }
    }
}
pub(crate) struct ResidentFile {
    file: ResidentHandle,
    context: Option<Arc<TransformContext>>,
    creator: u32,
    cursor: Mutex<Cursor>,
    cache: Mutex<Option<Cache>>,
}
impl Deref for ResidentFile {
    type Target = File;
    fn deref(&self) -> &File {
        &self.file
    }
}
impl ResidentFile {
    pub(crate) fn new(file: WriterFence, context: Option<Arc<TransformContext>>) -> Self {
        Self {
            file: ResidentHandle::Writer(file),
            context,
            creator: std::process::id(),
            cursor: Mutex::new(Cursor {
                logical: DATA,
                physical: DATA,
            }),
            cache: Mutex::new(None),
        }
    }
    pub(crate) fn readonly(file: File, context: Option<Arc<TransformContext>>) -> Self {
        Self {
            file: ResidentHandle::Reader(file),
            context,
            creator: std::process::id(),
            cursor: Mutex::new(Cursor {
                logical: DATA,
                physical: DATA,
            }),
            cache: Mutex::new(None),
        }
    }
    pub(crate) fn context(&self) -> Option<&Arc<TransformContext>> {
        self.context.as_ref()
    }
    fn check(&self) -> Result<()> {
        if self.creator != std::process::id() {
            return Err(Error::InvalidInput("inherited Resident journal"));
        }
        Ok(())
    }
    fn boundary(&self, logical: u64) -> Result<u64> {
        let Some(context) = &self.context else {
            return Ok(logical);
        };
        self.check()?;
        if logical < DATA {
            return Err(Error::Corrupt("Resident data boundary"));
        }
        let mut cursor = unknown_lock(self.cursor.try_lock())?;
        if cursor.logical > logical {
            cursor.logical = DATA;
            cursor.physical = DATA;
        }
        let capacity = self.file.metadata()?.len();
        while cursor.logical < logical {
            let (length, encoded, _) =
                packet_geometry(context, &self.file, cursor.physical, cursor.logical)?;
            if length < 288 || !length.is_multiple_of(8) {
                return Err(Error::Corrupt("Resident record packet geometry"));
            }
            let next = cursor
                .logical
                .checked_add(length)
                .ok_or(Error::LimitExceeded)?;
            let physical = cursor
                .physical
                .checked_add(encoded)
                .ok_or(Error::LimitExceeded)?;
            if next > logical || physical > capacity {
                return Err(Error::Corrupt("Resident record packet boundary"));
            }
            cursor.logical = next;
            cursor.physical = physical;
        }
        Ok(cursor.physical)
    }
    pub(crate) fn physical_end(&self, logical: u64) -> Result<u64> {
        self.boundary(logical)
    }
    pub(crate) fn read_exact_at(&self, out: &mut [u8], logical: u64) -> Result<()> {
        self.check()?;
        let Some(context) = &self.context else {
            self.file.read_exact_at(out, logical)?;
            return Ok(());
        };
        let mut cache = unknown_lock(self.cache.try_lock())?;
        if let Some(c) = cache
            .as_ref()
            .filter(|c| c.logical == logical && c.bytes.len() >= out.len())
        {
            out.copy_from_slice(&c.bytes[..out.len()]);
            return Ok(());
        }
        *cache = None;
        let physical = if logical < DATA {
            if ![0, 65536, 131072].contains(&logical) {
                return Err(Error::Corrupt("Resident fixed packet offset"));
            }
            logical
        } else {
            self.boundary(logical)?
        };
        match decode(context, &self.file, physical, logical) {
            Ok(value) => {
                if value.bytes.len() < out.len() {
                    return Err(Error::Corrupt("short Resident decoded packet"));
                }
                out.copy_from_slice(&value.bytes[..out.len()]);
                *cache = Some(value);
                Ok(())
            }
            Err(
                Error::CryptoFailed
                | Error::BadChecksum
                | Error::CryptoMismatch
                | Error::Corrupt(_),
            ) if logical == 65536 || logical == 131072 => {
                out.fill(0xff);
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
    fn location(&self, logical: u64) -> Result<u64> {
        if logical < DATA {
            if ![0, 65536, 131072].contains(&logical) {
                return Err(Error::InvalidInput("Resident fixed packet write"));
            }
            Ok(logical)
        } else {
            self.boundary(logical)
        }
    }
    pub(crate) fn can_write_extent(&self, logical: u64, length: u64) -> Result<bool> {
        self.check()?;
        let capacity = self.file.metadata()?.len();
        if logical.checked_add(length).is_none_or(|end| end > capacity) {
            return Ok(false);
        }
        let physical = self.location(logical)?;
        let length = match &self.context {
            Some(c) => packet_length(c, length)?,
            None => length,
        };
        Ok(physical
            .checked_add(length)
            .is_some_and(|end| end <= capacity))
    }
    pub(crate) fn write_all_at(&self, source: &[u8], logical: u64) -> Result<()> {
        self.check()?;
        if self.context.is_none() {
            self.file.write_all_at(source, logical)?;
            return Ok(());
        }
        self.write_cut(source, logical, || Ok(()))
    }
    pub(crate) fn write_cut(
        &self,
        source: &[u8],
        logical: u64,
        midpoint: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.check()?;
        if self.context.is_none() {
            let split = source.len() / 2;
            self.file.write_all_at(&source[..split], logical)?;
            midpoint()?;
            self.file
                .write_all_at(&source[split..], logical + split as u64)?;
            return Ok(());
        }
        *unknown_lock(self.cache.try_lock())? = None;
        let encoded = encode(self.context.as_ref().unwrap(), logical, source)?;
        let at = self.location(logical)?;
        let capacity = self.file.metadata()?.len();
        if at
            .checked_add(encoded.len() as u64)
            .is_none_or(|end| end > capacity)
        {
            return Err(Error::LimitExceeded);
        }
        if logical < DATA && source.len() != BLOCK as usize {
            return Err(Error::InvalidInput("Resident fixed packet size"));
        }
        let split = encoded.len() / 2;
        self.file.write_all_at(&encoded[..split], at)?;
        midpoint()?;
        self.file
            .write_all_at(&encoded[split..], at + split as u64)?;
        if logical >= DATA {
            let mut cursor = unknown_lock(self.cursor.try_lock())?;
            cursor.logical = logical + source.len() as u64;
            cursor.physical = at + encoded.len() as u64;
        }
        Ok(())
    }
    fn zero_bounds(&self, start: u64, end: u64) -> Result<(u64, u64)> {
        if self.context.is_none() {
            return Ok((start, end));
        }
        let capacity = self.file.metadata()?.len();
        if start == 0 && (capacity == 0 || end == capacity) {
            return Ok((0, end));
        }
        if start < DATA {
            let base = start
                .checked_sub(BLOCK)
                .ok_or(Error::Corrupt("Resident reserved packet range"))?;
            if ![0, 65536, 131072].contains(&base) {
                return Err(Error::Corrupt("Resident reserved packet range"));
            }
            return Ok((
                base + packet_length(self.context.as_ref().unwrap(), BLOCK)?,
                end,
            ));
        }
        let physical = self.boundary(start)?;
        let physical_end = if end == capacity {
            capacity
        } else {
            physical
                .checked_add(packet_length(self.context.as_ref().unwrap(), end - start)?)
                .ok_or(Error::LimitExceeded)?
        };
        if physical_end > capacity {
            return Err(Error::Corrupt("Resident clear capacity"));
        }
        Ok((physical, physical_end))
    }
    pub(crate) fn clear(&self, start: u64, end: u64) -> Result<()> {
        self.check()?;
        *unknown_lock(self.cache.try_lock())? = None;
        let (start, end) = self.zero_bounds(start, end)?;
        let zeros = [0; 65536];
        let mut at = start;
        while at < end {
            let n = (end - at).min(zeros.len() as u64) as usize;
            self.file.write_all_at(&zeros[..n], at)?;
            at += n as u64;
        }
        Ok(())
    }
    pub(crate) fn verify_zero(&self, start: u64, end: u64) -> Result<()> {
        self.check()?;
        let (start, end) = self.zero_bounds(start, end)?;
        let mut buffer = [0; 65536];
        let mut at = start;
        while at < end {
            let n = (end - at).min(buffer.len() as u64) as usize;
            self.file.read_exact_at(&mut buffer[..n], at)?;
            if buffer[..n].iter().any(|b| *b != 0) {
                return Err(Error::Corrupt("Resident nonzero physical suffix"));
            }
            at += n as u64;
        }
        Ok(())
    }
    pub(crate) fn sync_all(&self) -> Result<()> {
        self.check()?;
        ulmdb_platform::sync_file(&self.file)?;
        Ok(())
    }
    pub(crate) fn sync_data(&self) -> Result<()> {
        self.check()?;
        ulmdb_platform::sync_data(&self.file)?;
        Ok(())
    }
}
pub(crate) fn read_magic(
    path: &std::path::Path,
    context: Option<&Arc<TransformContext>>,
) -> Result<[u8; 8]> {
    let file = File::open(path)?;
    let mut out = [0; 8];
    if let Some(context) = context {
        let cache = decode(context, &file, 0, 0)?;
        if cache.bytes.len() != 256 {
            return Err(Error::Corrupt("Resident transformed header width"));
        }
        out.copy_from_slice(&cache.bytes[..8]);
    } else {
        file.read_exact_at(&mut out, 0)?;
    }
    Ok(out)
}
