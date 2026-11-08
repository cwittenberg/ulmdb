//! Bounded immutable-object streaming for Resident checkpoint images.
use super::*;
use std::io::SeekFrom;
pub(crate) struct Writer {
    file: File,
    context: Option<Arc<TransformContext>>,
    header: [u8; HEADER],
    buffer: Vec<u8>,
    written: u64,
    length: u64,
    index: u64,
    finished: bool,
}
impl Writer {
    pub(crate) fn new(
        mut file: File,
        context: Option<Arc<TransformContext>>,
        kind: ObjectKind,
        id: u128,
        length: u64,
    ) -> Result<Self> {
        let header = match &context {
            Some(c) => {
                let h = c.header(kind, id, length, crate::new_identity()?.to_le_bytes())?;
                file.write_all(&h)?;
                h
            }
            None => [0; HEADER],
        };
        Ok(Self {
            file,
            context,
            header,
            buffer: Vec::new(),
            written: 0,
            length,
            index: 0,
            finished: false,
        })
    }
    fn flush_frame(&mut self) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let Some(context) = &self.context else {
            return Err(Error::Corrupt("plain checkpoint staging"));
        };
        let mut plain = vec![0; BIND + PAGE + context.checksum_bytes()];
        plain[..BIND].copy_from_slice(&TransformContext::binding(&self.header, self.index));
        plain[BIND..BIND + self.buffer.len()].copy_from_slice(&self.buffer);
        if let Some(sum) = &context.checksum {
            let (source, dest) = plain.split_at_mut(BIND + PAGE);
            sum.callback.compute(source, dest, context.key())?;
        }
        let mut auth = vec![0; context.authentication_bytes()];
        if let Some(enc) = &context.encryption {
            let mut cipher = vec![0; plain.len()];
            enc.callback
                .transform(
                    &plain,
                    &mut cipher,
                    &enc.key.0,
                    &TransformContext::iv(&self.header, self.index),
                    &mut auth,
                    Direction::Encrypt,
                )
                .map_err(|_| Error::CryptoFailed)?;
            self.file.write_all(&cipher)?;
        } else {
            self.file.write_all(&plain)?;
        }
        self.file.write_all(&auth)?;
        self.buffer.clear();
        self.index += 1;
        Ok(())
    }
    pub(crate) fn write_all(&mut self, mut bytes: &[u8]) -> Result<()> {
        if self.finished
            || self
                .written
                .checked_add(bytes.len() as u64)
                .is_none_or(|n| n > self.length)
        {
            return Err(Error::InvalidInput("checkpoint stream length"));
        }
        self.written += bytes.len() as u64;
        if self.context.is_none() {
            self.file.write_all(bytes)?;
            return Ok(());
        }
        while !bytes.is_empty() {
            let n = (PAGE - self.buffer.len()).min(bytes.len());
            self.buffer.extend_from_slice(&bytes[..n]);
            bytes = &bytes[n..];
            if self.buffer.len() == PAGE {
                self.flush_frame()?;
            }
        }
        Ok(())
    }
    pub(crate) fn sync_all(&mut self) -> Result<()> {
        if self.written != self.length {
            return Err(Error::Corrupt("checkpoint short stream"));
        }
        if !self.finished {
            self.flush_frame()?;
            self.finished = true;
        }
        ulmdb_platform::sync_file(&self.file)?;
        Ok(())
    }
}
pub(crate) struct Reader {
    file: File,
    context: Option<Arc<TransformContext>>,
    header: [u8; HEADER],
    length: u64,
    physical: u64,
    position: u64,
    page: u64,
    cache: Vec<u8>,
    _lease: Option<DecodedLease>,
}
impl Reader {
    pub(crate) fn new(
        file: File,
        context: Option<Arc<TransformContext>>,
        kind: ObjectKind,
        id: u128,
    ) -> Result<Self> {
        let physical = file.metadata()?.len();
        let mut h = [0; HEADER];
        let (length, lease) = match &context {
            Some(c) => {
                file.read_exact_at(&mut h, 0)?;
                let length = u64::from_le_bytes(h[32..40].try_into().unwrap());
                if length == 0
                    || length > ulmdb_core::ValueLimit::MAXIMUM.segment_limit()
                    || h != c.header(kind, id, length, h[48..64].try_into().unwrap())?
                    || physical != c.encoded_length(length)?
                {
                    return Err(Error::CryptoMismatch);
                }
                (length, Some(c.reserve(PAGE as u64)?))
            }
            None => (physical, None),
        };
        Ok(Self {
            file,
            context,
            header: h,
            length,
            physical,
            position: 0,
            page: u64::MAX,
            cache: Vec::new(),
            _lease: lease,
        })
    }
    pub(crate) fn len(&self) -> u64 {
        self.length
    }
    pub(crate) fn seek(&mut self, to: SeekFrom) -> Result<u64> {
        let next = match to {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(n) => self
                .position
                .checked_add_signed(n)
                .ok_or(Error::LimitExceeded)?,
            SeekFrom::End(n) => self
                .length
                .checked_add_signed(n)
                .ok_or(Error::LimitExceeded)?,
        };
        if next > self.length {
            return Err(Error::Corrupt("checkpoint seek bounds"));
        }
        self.position = next;
        Ok(next)
    }
    fn page(&mut self, index: u64) -> Result<()> {
        let Some(context) = &self.context else {
            return Err(Error::Corrupt("plain checkpoint cache"));
        };
        let size = BIND + PAGE + context.checksum_bytes();
        let offset = HEADER as u64 + index * context.frame_bytes() as u64;
        let mut cipher = vec![0; size];
        let mut plain = vec![0; size];
        let mut auth = vec![0; context.authentication_bytes()];
        self.file.read_exact_at(&mut cipher, offset)?;
        self.file.read_exact_at(&mut auth, offset + size as u64)?;
        if let Some(enc) = &context.encryption {
            enc.callback
                .transform(
                    &cipher,
                    &mut plain,
                    &enc.key.0,
                    &TransformContext::iv(&self.header, index),
                    &mut auth,
                    Direction::Decrypt,
                )
                .map_err(|_| Error::CryptoFailed)?;
        } else {
            plain.copy_from_slice(&cipher);
        }
        if plain[..BIND] != TransformContext::binding(&self.header, index) {
            return Err(if context.encrypted() {
                Error::CryptoFailed
            } else {
                Error::BadChecksum
            });
        }
        if let Some(sum) = &context.checksum {
            let mut expected = vec![0; sum.bytes];
            sum.callback
                .compute(&plain[..BIND + PAGE], &mut expected, context.key())?;
            if expected
                .iter()
                .zip(&plain[BIND + PAGE..])
                .fold(0u8, |a, (b, c)| a | (b ^ c))
                != 0
            {
                return Err(Error::BadChecksum);
            }
        }
        let count = (self.length - index * PAGE as u64).min(PAGE as u64) as usize;
        if plain[BIND + count..BIND + PAGE].iter().any(|b| *b != 0) {
            return Err(Error::Corrupt("checkpoint transform padding"));
        }
        self.cache.clear();
        self.cache.extend_from_slice(&plain[BIND..BIND + PAGE]);
        self.page = index;
        Ok(())
    }
    pub(crate) fn read_exact(&mut self, mut out: &mut [u8]) -> Result<()> {
        if self
            .position
            .checked_add(out.len() as u64)
            .is_none_or(|n| n > self.length)
        {
            return Err(Error::Corrupt("checkpoint stream bounds"));
        }
        if self.context.is_none() {
            self.file.read_exact_at(out, self.position)?;
            self.position += out.len() as u64;
            return Ok(());
        }
        while !out.is_empty() {
            let index = self.position / PAGE as u64;
            if self.page != index {
                self.page(index)?;
            }
            let start = (self.position % PAGE as u64) as usize;
            let n = (PAGE - start).min(out.len());
            out[..n].copy_from_slice(&self.cache[start..start + n]);
            out = &mut out[n..];
            self.position += n as u64;
        }
        Ok(())
    }
    pub(crate) fn check(&self) -> Result<()> {
        if self.file.metadata()?.len() != self.physical {
            return Err(Error::Corrupt("checkpoint physical extent changed"));
        }
        if self.context.is_some() {
            let mut h = [0; HEADER];
            self.file.read_exact_at(&mut h, 0)?;
            if h != self.header {
                return Err(Error::Corrupt("checkpoint transform header changed"));
            }
        }
        Ok(())
    }
}
