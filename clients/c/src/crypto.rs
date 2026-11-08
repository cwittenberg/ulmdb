//! C callback configuration, owned keys and the native transform bridge.
use super::*;
use crypto_module::{Checksum, Encrypt};
use ulmdb_storage::transform::{self, TransformContext};

#[derive(Default)]
pub(super) struct Configuration {
    checksum: Option<(Checksum, usize)>,
    encryption: Option<(Encrypt, Secret, usize)>,
    failure: Option<c_int>,
}
struct Secret(Vec<u8>);
impl Drop for Secret {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // SAFETY: each pointer refers to an exclusive live byte.
            unsafe { ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}
fn value(bytes: &[u8]) -> MDB_val {
    MDB_val {
        mv_size: bytes.len(),
        mv_data: bytes.as_ptr().cast_mut().cast(),
    }
}
fn output(bytes: &mut [u8]) -> MDB_val {
    MDB_val {
        mv_size: bytes.len(),
        mv_data: bytes.as_mut_ptr().cast(),
    }
}
struct ChecksumBridge(Checksum);
impl transform::Checksum for ChecksumBridge {
    fn compute(
        &self,
        source: &[u8],
        destination: &mut [u8],
        key: Option<&[u8]>,
    ) -> ulmdb::Result<()> {
        let source = value(source);
        let mut destination = output(destination);
        let expected = destination;
        let key = key.map(value);
        // SAFETY: buffers remain alive for this synchronous callback. The
        // callback's lifetime/thread contract is the same as LMDB's C API.
        unsafe {
            (self.0)(
                &source,
                &mut destination,
                key.as_ref().map_or(ptr::null(), |k| k),
            )
        };
        if destination.mv_size != expected.mv_size || destination.mv_data != expected.mv_data {
            return Err(Error::BadChecksum);
        }
        Ok(())
    }
}
struct EncryptionBridge(Encrypt);
impl transform::Encryption for EncryptionBridge {
    fn transform(
        &self,
        source: &[u8],
        destination: &mut [u8],
        key: &[u8],
        iv: &[u8],
        authentication: &mut [u8],
        direction: transform::Direction,
    ) -> ulmdb::Result<()> {
        let source = value(source);
        let mut destination = output(destination);
        let expected = destination;
        let keys = [value(key), value(iv), output(authentication)];
        // SAFETY: the three key entries and their buffers remain alive through
        // the call; MAC storage is writable for authenticated encryption.
        let result = unsafe {
            (self.0)(
                &source,
                &mut destination,
                keys.as_ptr(),
                i32::from(direction == transform::Direction::Encrypt),
            )
        };
        if result != 0
            || destination.mv_size != expected.mv_size
            || destination.mv_data != expected.mv_data
        {
            return Err(Error::CryptoFailed);
        }
        Ok(())
    }
}
impl Configuration {
    pub(super) fn context(&self, mapsize: usize) -> FfiResult<Option<Arc<TransformContext>>> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        if self.checksum.is_none() && self.encryption.is_none() {
            return Ok(None);
        }
        let checksum = self
            .checksum
            .map(|(callback, bytes)| transform::ChecksumConfig {
                callback: Arc::new(ChecksumBridge(callback)),
                bytes,
            });
        let encryption =
            self.encryption
                .as_ref()
                .map(|(callback, key, bytes)| transform::EncryptionConfig {
                    callback: Arc::new(EncryptionBridge(*callback)),
                    key: key.0.clone(),
                    authentication_bytes: *bytes,
                });
        // Charged by native mapping owners, including retained snapshots.
        let decoded_limit = decoded_limit(mapsize);
        TransformContext::new(checksum, encryption, decoded_limit)
            .map(|context| Some(Arc::new(context)))
            .map_err(code)
    }
}
pub(super) fn decoded_limit(mapsize: usize) -> u64 {
    (mapsize as u64).saturating_mul(4).max(64 << 20)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_set_checksum(
    env: *mut MDB_env,
    callback: Option<Checksum>,
    size: c_uint,
) -> c_int {
    boundary(|| {
        let env = unsafe { get_mut(env)? };
        let callback = callback.ok_or(libc::EINVAL)?;
        if env.shared.is_some() || size == 0 || size > 256 {
            return Err(libc::EINVAL);
        }
        env.crypto.checksum = Some((callback, size as usize));
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_env_set_encrypt(
    env: *mut MDB_env,
    callback: Option<Encrypt>,
    key: *const MDB_val,
    size: c_uint,
) -> c_int {
    boundary(|| {
        let env = unsafe { get_mut(env)? };
        let callback = callback.ok_or(libc::EINVAL)?;
        if env.shared.is_some() || size > 256 {
            return Err(libc::EINVAL);
        }
        let key = unsafe { bytes(key)? };
        if key.len() > 1 << 20 {
            return Err(libc::EINVAL);
        }
        let mut copied = Vec::new();
        copied
            .try_reserve_exact(key.len())
            .map_err(|_| libc::ENOMEM)?;
        copied.extend_from_slice(key);
        // As upstream, a zero size keeps a previously selected auth width.
        let authentication = if size == 0 {
            env.crypto.encryption.as_ref().map_or(0, |(_, _, n)| *n)
        } else {
            size as usize
        };
        env.crypto.encryption = Some((callback, Secret(copied), authentication));
        env.crypto.failure = None;
        env.flags
            .fetch_or(0x4002000, std::sync::atomic::Ordering::AcqRel);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mdb_modsetup(
    env: *mut MDB_env,
    functions: *mut crypto_module::MDB_crypto_funcs,
    passphrase: *const c_char,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let fail = |error| {
            if let Some(env) = unsafe { env.as_mut() } {
                env.crypto.failure = Some(error);
            }
        };
        let Some(functions) = (unsafe { functions.as_ref() }) else {
            return;
        };
        if functions.mcf_sumfunc.is_some() {
            let result = unsafe {
                mdb_env_set_checksum(env, functions.mcf_sumfunc, functions.mcf_sumsize as c_uint)
            };
            if result != 0 {
                fail(result);
                return;
            }
        }
        let Some(callback) = functions.mcf_encfunc else {
            return;
        };
        if passphrase.is_null() {
            return;
        }
        if !(0..=1 << 20).contains(&functions.mcf_keysize) {
            fail(libc::EINVAL);
            return;
        }
        let mut secret = Secret(vec![0; functions.mcf_keysize as usize]);
        let mut key = output(&mut secret.0);
        if let Some(derive) = functions.mcf_str2key {
            let original = key;
            if unsafe { derive(passphrase, &mut key) } != 0 {
                fail(-30777);
                return;
            }
            if key.mv_size > original.mv_size || key.mv_data != original.mv_data {
                fail(libc::EINVAL);
                return;
            }
        } else {
            let password = unsafe { CStr::from_ptr(passphrase) }.to_bytes();
            let length = password.len().min(secret.0.len());
            secret.0[..length].copy_from_slice(&password[..length]);
        }
        let result = unsafe {
            mdb_env_set_encrypt(env, Some(callback), &key, functions.mcf_esumsize as c_uint)
        };
        if result != 0 {
            fail(result);
        }
    }));
}
