//! Shared frame decoder for bounded cache windows and whole-object mappings.
use super::*;
pub(crate) struct FrameScratch {
    cipher: Vec<u8>,
    plain: Vec<u8>,
    authentication: Vec<u8>,
    expected: Vec<u8>,
}
impl FrameScratch {
    pub(crate) fn new(context: &TransformContext) -> Self {
        let size = BIND + PAGE + context.checksum_bytes();
        Self {
            cipher: vec![0; size],
            plain: vec![0; size],
            authentication: vec![0; context.authentication_bytes()],
            expected: vec![0; context.checksum_bytes()],
        }
    }
}
/// Decode one frame into its exact logical destination. No bytes escape before
/// binding/checksum/authentication/padding checks have all succeeded.
pub(crate) fn decode_frame(
    context: &TransformContext,
    file: &File,
    base: u64,
    header: &[u8; HEADER],
    index: u64,
    destination: &mut [u8],
    scratch: &mut FrameScratch,
) -> Result<()> {
    ulmdb_core::work::checkpoint(1, destination.len() as u64)?;
    if destination.is_empty() || destination.len() > PAGE {
        return Err(Error::InvalidInput("decoded frame length"));
    }
    let offset = index
        .checked_mul(context.frame_bytes() as u64)
        .and_then(|n| n.checked_add(HEADER as u64))
        .and_then(|n| n.checked_add(base))
        .ok_or(Error::LimitExceeded)?;
    file.read_exact_at(&mut scratch.cipher, offset)?;
    file.read_exact_at(
        &mut scratch.authentication,
        offset + scratch.cipher.len() as u64,
    )?;
    if let Some(encryption) = &context.encryption {
        encryption
            .callback
            .transform(
                &scratch.cipher,
                &mut scratch.plain,
                &encryption.key.0,
                &TransformContext::iv(header, index),
                &mut scratch.authentication,
                Direction::Decrypt,
            )
            .map_err(|_| Error::CryptoFailed)?;
    } else {
        scratch.plain.copy_from_slice(&scratch.cipher);
    }
    if scratch.plain[..BIND] != TransformContext::binding(header, index) {
        return Err(if context.encrypted() {
            Error::CryptoFailed
        } else {
            Error::BadChecksum
        });
    }
    if let Some(sum) = &context.checksum {
        sum.callback.compute(
            &scratch.plain[..BIND + PAGE],
            &mut scratch.expected,
            context.key(),
        )?;
        let difference = scratch
            .expected
            .iter()
            .zip(&scratch.plain[BIND + PAGE..])
            .fold(0u8, |a, (b, c)| a | (b ^ c));
        if difference != 0 {
            return Err(Error::BadChecksum);
        }
    }
    let count = destination.len();
    if scratch.plain[BIND + count..BIND + PAGE]
        .iter()
        .any(|b| *b != 0)
    {
        return Err(Error::Corrupt("transform padding"));
    }
    destination.copy_from_slice(&scratch.plain[BIND..BIND + count]);
    Ok(())
}

impl TransformContext {
    pub(crate) fn checked_object_header(
        &self,
        file: &File,
        base: u64,
        kind: ObjectKind,
        id: u128,
        length: u64,
    ) -> Result<[u8; HEADER]> {
        let mut header = [0; HEADER];
        file.read_exact_at(&mut header, base)?;
        if header != self.header(kind, id, length, header[48..64].try_into().unwrap())? {
            return Err(Error::CryptoMismatch);
        }
        let end = base
            .checked_add(self.encoded_length(length)?)
            .ok_or(Error::LimitExceeded)?;
        if file.metadata()?.len() < end {
            return Err(Error::Corrupt("truncated transformed object"));
        }
        Ok(header)
    }
    pub(crate) fn reserve_cache_mapping(self: &Arc<Self>, length: u64) -> Result<DecodedLease> {
        self.reserve(Self::mapped_charge(length)?)
    }
}
