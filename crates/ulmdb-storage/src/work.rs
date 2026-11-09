//! Cooperative checkpoints on cold file admission. Query byte access is unchanged.
use crate::Result;
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path, sync::Arc};
use ulmdb_core::work::{CHUNK_BYTES, WorkObserver, current_observer};

pub(crate) fn digest(bytes: &[u8]) -> Result<[u8; 32]> {
    let Some(observer) = current_observer() else {
        return Ok(Sha256::digest(bytes).into());
    };
    digest_observed(bytes, observer)
}

#[inline(never)]
fn digest_observed(bytes: &[u8], observer: Arc<dyn WorkObserver>) -> Result<[u8; 32]> {
    let mut digest = Sha256::new();
    for chunk in bytes.chunks(CHUNK_BYTES) {
        observer.checkpoint(1, chunk.len() as u64)?;
        digest.update(chunk);
    }
    observer.checkpoint(0, 0)?;
    Ok(digest.finalize().into())
}

pub(crate) fn equal(left: &[u8], right: &[u8]) -> Result<bool> {
    let Some(observer) = current_observer() else {
        return Ok(left == right);
    };
    equal_observed(left, right, observer)
}

#[inline(never)]
fn equal_observed(left: &[u8], right: &[u8], observer: Arc<dyn WorkObserver>) -> Result<bool> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left.chunks(CHUNK_BYTES).zip(right.chunks(CHUNK_BYTES)) {
        observer.checkpoint(1, left.len() as u64)?;
        if left != right {
            return Ok(false);
        }
    }
    observer.checkpoint(0, 0)?;
    Ok(true)
}

pub(crate) fn read_exact(input: &mut impl Read, bytes: &mut [u8]) -> Result<()> {
    let Some(observer) = current_observer() else {
        input.read_exact(bytes)?;
        return Ok(());
    };
    read_exact_observed(input, bytes, observer)
}

#[inline(never)]
fn read_exact_observed(
    input: &mut impl Read,
    bytes: &mut [u8],
    observer: Arc<dyn WorkObserver>,
) -> Result<()> {
    for chunk in bytes.chunks_mut(CHUNK_BYTES) {
        observer.checkpoint(1, chunk.len() as u64)?;
        input.read_exact(chunk)?;
    }
    observer.checkpoint(0, 0)
}

pub(crate) fn read_to_end_limited(
    input: &mut impl Read,
    bytes: &mut Vec<u8>,
    maximum: u64,
) -> Result<usize> {
    let Some(observer) = current_observer() else {
        return Ok(input.read_to_end(bytes)?);
    };
    read_to_end_observed(input, bytes, maximum, observer.as_ref())
}

// A large buffer in the dispatching function makes its stack reservation and
// page probes run even when observation is absent. Allocate it only after the
// caller has selected the observed path.
#[inline(never)]
fn read_to_end_observed(
    input: &mut impl Read,
    bytes: &mut Vec<u8>,
    mut maximum: u64,
    observer: &dyn WorkObserver,
) -> Result<usize> {
    let start = bytes.len();
    let mut buffer = [0; CHUNK_BYTES];
    loop {
        let request = maximum.min(buffer.len() as u64) as usize;
        if request == 0 {
            return Ok(bytes.len() - start);
        }
        observer.checkpoint(1, request as u64)?;
        let n = input.read(&mut buffer[..request])?;
        if n == 0 {
            return Ok(bytes.len() - start);
        }
        bytes.extend_from_slice(&buffer[..n]);
        maximum -= n as u64;
    }
}

pub(crate) fn read(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    if current_observer().is_none() {
        return Ok(std::fs::read(path)?);
    }
    ulmdb_core::work::checkpoint(1, 0)?;
    let mut file = File::open(path)?;
    let length =
        usize::try_from(file.metadata()?.len()).map_err(|_| crate::Error::LimitExceeded)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| crate::Error::LimitExceeded)?;
    read_to_end_limited(
        &mut file,
        &mut bytes,
        (length as u64)
            .checked_add(1)
            .ok_or(crate::Error::LimitExceeded)?,
    )?;
    if bytes.len() != length {
        return Err(crate::Error::Corrupt(
            "file changed during controlled admission",
        ));
    }
    Ok(bytes)
}
