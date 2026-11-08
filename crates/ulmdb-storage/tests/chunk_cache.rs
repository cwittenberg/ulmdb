use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    sync::{
        Arc, Barrier, Mutex, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use ulmdb_storage::{
    Error,
    chunk_cache::{AuthenticatedFile, ChunkCache, Limits, ObjectIdentity},
    transform::{
        Checksum, ChecksumConfig, Direction, Encryption, EncryptionConfig, ObjectKind,
        TransformContext,
    },
};
const CHUNK: usize = 65536;
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "chunk-cache-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn limits(bytes: usize) -> Limits {
    Limits {
        mapped_bytes: bytes as u64,
        windows: 4,
        chunk_bytes: CHUNK,
        max_window_bytes: 2 * CHUNK,
    }
}
fn data() -> Vec<u8> {
    (0..3 * CHUNK + 19)
        .map(|i| (i.wrapping_mul(7) >> 9) as u8)
        .collect()
}
fn source(
    dir: &Dir,
    bytes: &[u8],
    context: Option<Arc<TransformContext>>,
) -> Arc<AuthenticatedFile> {
    let path = dir.0.join("object");
    let mut file = File::create(&path).unwrap();
    if let Some(c) = &context {
        c.write_encoded(&mut file, ObjectKind::Segment, 17, bytes)
            .unwrap();
    } else {
        file.write_all(bytes).unwrap();
    }
    file.sync_all().unwrap();
    drop(file);
    AuthenticatedFile::open(
        File::open(path).unwrap(),
        ObjectIdentity {
            kind: ObjectKind::Segment,
            id: 17,
            length: bytes.len() as u64,
            hash: Sha256::digest(bytes).into(),
            file_offset: 0,
        },
        context,
    )
    .unwrap()
}

#[test]
fn real_window_pins_survive_eviction_controller_drop_and_source_removal() {
    let dir = Dir::new();
    let bytes = data();
    let source = source(&dir, &bytes, None);
    let cache = ChunkCache::new(limits(2 * CHUNK)).unwrap();
    let first = cache.pin(&source, 7, 300).unwrap();
    let address = first.as_ptr();
    let second = cache.pin(&source, CHUNK as u64, 300).unwrap();
    assert!(matches!(
        cache.pin(&source, (2 * CHUNK) as u64, 300),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(cache.stats().unwrap().mapped_bytes, (2 * CHUNK) as u64);
    assert_eq!(&*first, &bytes[7..307]);
    drop(second);
    let third = cache.pin(&source, (2 * CHUNK) as u64, 300).unwrap();
    assert_eq!(first.as_ptr(), address);
    assert_eq!(&*first, &bytes[7..307]);
    assert_eq!(cache.stats().unwrap().evictions, 1);
    let same = cache.pin(&source, 7, 300).unwrap();
    assert_eq!(same.as_ptr(), address);
    assert_eq!(cache.trim_unpinned().unwrap(), 0);
    drop(source);
    fs::remove_file(dir.0.join("object")).unwrap();
    drop(cache);
    assert_eq!(&*third, &bytes[2 * CHUNK..2 * CHUNK + 300]);
    assert_eq!(&*same, &bytes[7..307]);
}

#[test]
fn cross_chunk_contiguity_exact_tail_and_preflight_errors_preserve_pins() {
    let dir = Dir::new();
    let bytes = data();
    let source = source(&dir, &bytes, None);
    let cache = ChunkCache::new(limits(2 * CHUNK)).unwrap();
    let span = cache.pin(&source, (CHUNK - 10) as u64, 40).unwrap();
    assert_eq!(&*span, &bytes[CHUNK - 10..CHUNK + 30]);
    assert_eq!(
        cache
            .pin(&source, (CHUNK - 10) as u64, 40)
            .unwrap()
            .as_ptr(),
        span.as_ptr()
    );
    let before = cache.stats().unwrap();
    assert!(cache.pin(&source, 0, 2 * CHUNK + 1).is_err());
    assert!(cache.pin(&source, source.length() + 1, 0).is_err());
    assert!(cache.pin(&source, u64::MAX, 99).is_err());
    assert!(cache.pin(&source, source.length(), 0).unwrap().is_empty());
    assert_eq!(cache.stats().unwrap(), before);
    drop(span);
    cache.trim_unpinned().unwrap();
    let tail = cache.pin(&source, (3 * CHUNK) as u64, 19).unwrap();
    assert_eq!(&*tail, &bytes[3 * CHUNK..]);
    let mapped = cache.stats().unwrap().mapped_bytes;
    assert!(mapped.is_power_of_two() && (4096..=CHUNK as u64).contains(&mapped));
}

struct Crypto {
    calls: AtomicUsize,
    fail: AtomicBool,
    cache: Mutex<Weak<ChunkCache>>,
    concurrent: AtomicBool,
    arrivals: AtomicUsize,
    barrier: Barrier,
}
impl Crypto {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            fail: AtomicBool::new(false),
            cache: Mutex::new(Weak::new()),
            concurrent: AtomicBool::new(false),
            arrivals: AtomicUsize::new(0),
            barrier: Barrier::new(2),
        })
    }
}
impl Checksum for Crypto {
    fn compute(
        &self,
        input: &[u8],
        output: &mut [u8],
        _key: Option<&[u8]>,
    ) -> ulmdb_storage::Result<()> {
        if self.fail.load(Ordering::Acquire) {
            return Err(Error::BadChecksum);
        }
        output.copy_from_slice(&Sha256::digest(input)[..output.len()]);
        Ok(())
    }
}
impl Encryption for Crypto {
    fn transform(
        &self,
        input: &[u8],
        output: &mut [u8],
        key: &[u8],
        iv: &[u8],
        authentication: &mut [u8],
        direction: Direction,
    ) -> ulmdb_storage::Result<()> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.fail.load(Ordering::Acquire) {
            return Err(Error::CryptoFailed);
        }
        if direction == Direction::Decrypt {
            if let Some(cache) = self.cache.lock().unwrap().upgrade() {
                let stats = cache.stats().unwrap();
                assert!(stats.mapped_bytes <= (2 * CHUNK) as u64);
            }
            if self.concurrent.load(Ordering::Acquire)
                && self.arrivals.fetch_add(1, Ordering::AcqRel) < 2
            {
                self.barrier.wait();
            }
            let mut hash = Sha256::new();
            hash.update(input);
            hash.update(key);
            hash.update(iv);
            if authentication != &hash.finalize()[..authentication.len()] {
                return Err(Error::CryptoFailed);
            }
        }
        for (i, (from, to)) in input.iter().zip(output.iter_mut()).enumerate() {
            *to = *from ^ key[i % key.len()] ^ iv[i % iv.len()];
        }
        if direction == Direction::Encrypt {
            let mut hash = Sha256::new();
            hash.update(output);
            hash.update(key);
            hash.update(iv);
            let length = authentication.len();
            authentication.copy_from_slice(&hash.finalize()[..length]);
        }
        Ok(())
    }
}
fn context(
    crypto: &Arc<Crypto>,
    checksum: bool,
    encrypt: bool,
    limit: u64,
) -> Arc<TransformContext> {
    Arc::new(
        TransformContext::new(
            checksum.then(|| ChecksumConfig {
                callback: crypto.clone(),
                bytes: 8,
            }),
            encrypt.then(|| EncryptionConfig {
                callback: crypto.clone(),
                key: vec![7, 19, 233],
                authentication_bytes: 8,
            }),
            limit,
        )
        .unwrap(),
    )
}

#[test]
fn transformed_cache_reuses_verified_windows_retains_context_and_releases_failed_charges() {
    for (checksum, encrypt) in [(true, false), (false, true), (true, true)] {
        let dir = Dir::new();
        let bytes = data();
        let crypto = Crypto::new();
        let context = context(&crypto, checksum, encrypt, (2 * CHUNK) as u64);
        let source = source(&dir, &bytes, Some(context.clone()));
        assert_eq!(context.decoded_bytes(), 0); // streaming verification, no full map
        let cache = ChunkCache::new(limits(2 * CHUNK)).unwrap();
        let first = cache.pin(&source, 0, 500).unwrap();
        let calls = crypto.calls.load(Ordering::Acquire);
        assert_eq!(cache.pin(&source, 0, 500).unwrap().as_ptr(), first.as_ptr());
        assert_eq!(crypto.calls.load(Ordering::Acquire), calls);
        crypto.fail.store(true, Ordering::Release);
        assert!(cache.pin(&source, CHUNK as u64, 500).is_err());
        assert_eq!(cache.stats().unwrap().live_windows, 1);
        assert_eq!(context.decoded_bytes(), CHUNK as u64);
        crypto.fail.store(false, Ordering::Release);
        let second = cache.pin(&source, CHUNK as u64, 500).unwrap();
        let weak = Arc::downgrade(&context);
        drop(source);
        drop(context);
        drop(cache);
        assert_eq!(&*first, &bytes[..500]);
        assert!(weak.upgrade().is_some());
        drop(first);
        assert!(weak.upgrade().is_some());
        drop(second);
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn context_quota_pressure_evicts_unpinned_windows_without_releasing_live_borrows() {
    let dir = Dir::new();
    let bytes = data();
    let crypto = Crypto::new();
    let context = context(&crypto, true, true, CHUNK as u64);
    let source = source(&dir, &bytes, Some(context.clone()));
    let cache = ChunkCache::new(limits(2 * CHUNK)).unwrap();
    let first = cache.pin(&source, 0, 20).unwrap();
    assert!(matches!(
        cache.pin(&source, CHUNK as u64, 20),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(&*first, &bytes[..20]);
    drop(first);
    let second = cache.pin(&source, CHUNK as u64, 20).unwrap();
    assert_eq!(&*second, &bytes[CHUNK..CHUNK + 20]);
    assert_eq!(context.decoded_bytes(), CHUNK as u64);
    assert_eq!(cache.stats().unwrap().evictions, 1);
}

#[test]
fn concurrent_duplicate_loaders_are_charged_and_callbacks_can_inspect_cache() {
    let dir = Dir::new();
    let bytes = data();
    let crypto = Crypto::new();
    let context = context(&crypto, true, true, (2 * CHUNK) as u64);
    let source = source(&dir, &bytes, Some(context.clone()));
    let cache = ChunkCache::new(limits(2 * CHUNK)).unwrap();
    *crypto.cache.lock().unwrap() = Arc::downgrade(&cache);
    crypto.concurrent.store(true, Ordering::Release);
    let pins = std::thread::scope(|scope| {
        let a = scope.spawn(|| cache.pin(&source, 0, 100).unwrap());
        let b = scope.spawn(|| cache.pin(&source, 0, 100).unwrap());
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(pins.0.as_ptr(), pins.1.as_ptr());
    assert_eq!(cache.stats().unwrap().mapped_bytes, CHUNK as u64);
    assert_eq!(context.decoded_bytes(), CHUNK as u64);
}

#[test]
fn tampering_wrong_context_identity_and_live_source_change_fail_closed() {
    let dir = Dir::new();
    let bytes = data();
    let crypto = Crypto::new();
    let context = context(&crypto, true, true, (2 * CHUNK) as u64);
    let source = source(&dir, &bytes, Some(context.clone()));
    let cache = ChunkCache::new(limits(2 * CHUNK)).unwrap();
    let pin = cache.pin(&source, 0, 20).unwrap();
    let path = dir.0.join("object");
    let identity = ObjectIdentity {
        kind: ObjectKind::Segment,
        id: 18,
        length: bytes.len() as u64,
        hash: Sha256::digest(&bytes).into(),
        file_offset: 0,
    };
    assert!(
        AuthenticatedFile::open(File::open(&path).unwrap(), identity, Some(context.clone()))
            .is_err()
    );
    let writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    writer.set_len(4).unwrap();
    assert!(cache.pin(&source, 0, 20).is_err());
    assert_eq!(&*pin, &bytes[..20]);
    assert!(
        AuthenticatedFile::open(
            File::open(path).unwrap(),
            ObjectIdentity { id: 17, ..identity },
            Some(context)
        )
        .is_err()
    );
}

#[cfg(target_os = "linux")]
fn vma(address: usize) -> (usize, String) {
    let maps = fs::read_to_string("/proc/self/smaps").unwrap();
    let mut matched = false;
    let mut length = 0;
    for line in maps.lines() {
        if let Some((start, rest)) = line.split_once('-') {
            if let Some(end) = rest.split_whitespace().next() {
                if let (Ok(start), Ok(end)) = (
                    usize::from_str_radix(start, 16),
                    usize::from_str_radix(end, 16),
                ) {
                    matched = (start..end).contains(&address);
                    if matched {
                        length = end - start;
                    }
                }
            }
        }
        if matched && line.starts_with("VmFlags:") {
            return (length, line.to_owned());
        }
    }
    panic!("mapped window missing from smaps");
}

#[cfg(target_os = "linux")]
#[test]
fn unaligned_plain_window_charge_matches_real_kernel_mapping() {
    let dir = Dir::new();
    let bytes = data();
    let path = dir.0.join("object");
    let mut file = File::create(&path).unwrap();
    file.write_all(&[0; 23]).unwrap();
    file.write_all(&bytes).unwrap();
    drop(file);
    let source = AuthenticatedFile::open(
        File::open(path).unwrap(),
        ObjectIdentity {
            kind: ObjectKind::Segment,
            id: 17,
            length: bytes.len() as u64,
            hash: Sha256::digest(&bytes).into(),
            file_offset: 23,
        },
        None,
    )
    .unwrap();
    let cache = ChunkCache::new(limits(2 * CHUNK)).unwrap();
    let pin = cache.pin(&source, 0, CHUNK).unwrap();
    assert_eq!(&*pin, &bytes[..CHUNK]);
    let (mapped, _) = vma(pin.as_ptr() as usize);
    assert_eq!(cache.stats().unwrap().mapped_bytes, mapped as u64);
    assert!(mapped > CHUNK);
}

#[cfg(target_os = "linux")]
#[test]
fn immutable_mapping_advice_is_part_of_cache_identity() {
    let dir = Dir::new();
    let bytes = data();
    let normal = source(&dir, &bytes, None);
    let random = AuthenticatedFile::open_with_read_ahead(
        File::open(dir.0.join("object")).unwrap(),
        ObjectIdentity {
            kind: ObjectKind::Segment,
            id: 17,
            length: bytes.len() as u64,
            hash: Sha256::digest(&bytes).into(),
            file_offset: 0,
        },
        None,
        ulmdb_storage::ReadAhead::Random,
    )
    .unwrap();
    let cache = ChunkCache::new(limits(2 * CHUNK)).unwrap();
    let before = cache.pin(&normal, 0, 50).unwrap();
    let advised = cache.pin(&random, 0, 50).unwrap();
    assert_ne!(before.as_ptr(), advised.as_ptr());
    assert!(
        !vma(before.as_ptr() as usize)
            .1
            .split_whitespace()
            .any(|f| f == "rr")
    );
    assert!(
        vma(advised.as_ptr() as usize)
            .1
            .split_whitespace()
            .any(|f| f == "rr")
    );
    assert_eq!(&*before, &*advised);
}
