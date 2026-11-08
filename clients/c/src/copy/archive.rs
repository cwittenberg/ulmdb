//! Versioned, bounded-memory container for fd copies of native snapshots.
use super::*;
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::Component,
};
const MAX_FILES: u64 = 1_048_582;
const MAX_TOTAL: u64 = 1 << 49;
const MAX_FILE: u64 = ulmdb_storage::MAX_COHORT_BYTES;
#[derive(Clone, Copy, Eq, PartialEq)]
enum Version {
    Single = 1,
    Pair = 2,
}
fn valid_path(version: Version, text: &str) -> bool {
    let path = Path::new(text);
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
        || text.contains('\\')
    {
        return false;
    }
    if matches!(text, "CURRENT" | "REPLICA" | "LMDB-ABI" | "LMDB-MAPSIZE") {
        return true;
    }
    if version == Version::Pair && text == "PREVIOUS" {
        return true;
    }
    if version == Version::Pair && text == "FIXEDMAP" {
        return true;
    }
    if version == Version::Pair && text == "TRANSFORM" {
        return true;
    }
    let Some((directory, name)) = text.split_once('/') else {
        return false;
    };
    let (suffix, digits) = match directory {
        "segments" => (".seg", 64),
        "commits" => (".root", 32),
        "cohorts" => (".cohort", 32),
        _ => return false,
    };
    let Some(hex) = name.strip_suffix(suffix) else {
        return false;
    };
    hex.len() == digits
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn valid_size(version: Version, name: &str, size: u64) -> bool {
    match name {
        "CURRENT" => matches!(size, 64 | 144),
        "PREVIOUS" => version == Version::Pair && size == 64,
        "REPLICA" => size == 64,
        "LMDB-MAPSIZE" => size == 8,
        "LMDB-ABI" => size <= 64,
        "TRANSFORM" => version == Version::Pair && (128..=8192).contains(&size),
        "FIXEDMAP" => version == Version::Pair && (128..=160 + 80 * 1_048_576).contains(&size),
        _ => size <= MAX_FILE,
    }
}
fn visit(
    version: Version,
    directory: &Path,
    mut callback: impl FnMut(&str, &Path, u64) -> FfiResult<()>,
) -> FfiResult<()> {
    for name in [
        "CURRENT",
        "PREVIOUS",
        "REPLICA",
        "LMDB-ABI",
        "LMDB-MAPSIZE",
        "TRANSFORM",
        "FIXEDMAP",
    ] {
        if matches!(name, "PREVIOUS" | "TRANSFORM" | "FIXEDMAP") && version == Version::Single {
            continue;
        }
        let path = directory.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && valid_size(version, name, metadata.len()) => {
                callback(name, &path, metadata.len())?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && name != "CURRENT" => {}
            Err(e) => return Err(code(e.into())),
            _ => return Err(-30796),
        }
    }
    for subdir in ["segments", "commits", "cohorts"] {
        if version == Version::Pair
            && !fs::symlink_metadata(directory.join(subdir))
                .map_err(|e| code(e.into()))?
                .is_dir()
        {
            return Err(-30796);
        }
        for entry in fs::read_dir(directory.join(subdir)).map_err(|e| code(e.into()))? {
            let entry = entry.map_err(|e| code(e.into()))?;
            let text = format!("{subdir}/{}", entry.file_name().to_str().ok_or(-30796)?);
            let metadata = fs::symlink_metadata(entry.path()).map_err(|e| code(e.into()))?;
            if !metadata.is_file() || !valid_path(version, &text) || metadata.len() > MAX_FILE {
                return Err(-30796);
            }
            callback(&text, &entry.path(), metadata.len())?;
        }
    }
    Ok(())
}
pub(super) fn write_archive(directory: &Path, output: &mut impl Write) -> FfiResult<()> {
    write_version(Version::Single, directory, output)
}
/// Explicit pair container. The original v1 writer and accepted grammar stay
/// single-root; a v1 stream can never smuggle an ancillary PREVIOUS record.
pub(super) fn write_archive_v2(directory: &Path, output: &mut impl Write) -> FfiResult<()> {
    write_archive_v2_with_transform(directory, output, None)
}
pub(super) fn write_archive_v2_with_transform(
    directory: &Path,
    output: &mut impl Write,
    transform: Transform,
) -> FfiResult<()> {
    write_archive_v2_with_mode(directory, output, transform, false)
}
pub(super) fn write_archive_v2_with_mode(
    directory: &Path,
    output: &mut impl Write,
    transform: Transform,
    external: bool,
) -> FfiResult<()> {
    write_archive_v2_with_cache_options(directory, output, transform, external, None)
}
pub(super) fn write_archive_v2_with_cache_options(
    directory: &Path,
    output: &mut impl Write,
    transform: Transform,
    external: bool,
    cache: chunks::Options,
) -> FfiResult<()> {
    validate_pair_with_cache_options(directory, transform, external, cache)?;
    write_version(Version::Pair, directory, output)
}
fn write_version(version: Version, directory: &Path, output: &mut impl Write) -> FfiResult<()> {
    let (mut files, mut total) = (0u64, 0u64);
    visit(version, directory, |_, _, size| {
        files = files.checked_add(1).ok_or(-30792)?;
        total = total.checked_add(size).ok_or(-30792)?;
        Ok(())
    })?;
    if files > MAX_FILES || total > MAX_TOTAL {
        return Err(-30792);
    }
    let write =
        |output: &mut dyn Write, bytes: &[u8]| output.write_all(bytes).map_err(|e| code(e.into()));
    write(output, ARCHIVE_MAGIC)?;
    write(output, &(version as u32).to_le_bytes())?;
    write(output, &0u32.to_le_bytes())?;
    write(output, &files.to_le_bytes())?;
    write(output, &total.to_le_bytes())?;
    let mut buffer = [0u8; 65536];
    visit(version, directory, |name, path, size| {
        write(output, &(name.len() as u32).to_le_bytes())?;
        write(output, &size.to_le_bytes())?;
        write(output, name.as_bytes())?;
        let mut input = File::open(path).map_err(|e| code(e.into()))?;
        let mut remaining = size;
        while remaining != 0 {
            let length = remaining.min(buffer.len() as u64) as usize;
            input
                .read_exact(&mut buffer[..length])
                .map_err(|e| code(e.into()))?;
            write(output, &buffer[..length])?;
            remaining -= length as u64;
        }
        Ok(())
    })
}
pub(crate) fn restore_archive(source: &Path, target: &Path) -> FfiResult<()> {
    restore_archive_with_transform(source, target, None)
}
pub(crate) fn restore_archive_with_transform(
    source: &Path,
    target: &Path,
    transform: Transform,
) -> FfiResult<()> {
    restore_archive_with_mode(source, target, transform, false)
}
pub(crate) fn restore_archive_externally_synchronized_with_transform(
    source: &Path,
    target: &Path,
    transform: Transform,
) -> FfiResult<()> {
    restore_archive_with_mode(source, target, transform, true)
}
fn restore_archive_with_mode(
    source: &Path,
    target: &Path,
    transform: Transform,
    external: bool,
) -> FfiResult<()> {
    restore_archive_with_cache_options(source, target, transform, external, None)
}
pub(crate) fn restore_archive_with_cache_options(
    source: &Path,
    target: &Path,
    transform: Transform,
    external: bool,
    cache: chunks::Options,
) -> FfiResult<()> {
    check_target(target)?;
    let mut input = File::open(source).map_err(|e| code(e.into()))?;
    let mut header = [0u8; 32];
    input.read_exact(&mut header).map_err(|_| -30793)?;
    if &header[..8] != ARCHIVE_MAGIC || header[12..16] != [0; 4] {
        return Err(-30793);
    }
    let version = match u32::from_le_bytes(header[8..12].try_into().unwrap()) {
        1 => Version::Single,
        2 => Version::Pair,
        _ => return Err(-30793),
    };
    let files = u64::from_le_bytes(header[16..24].try_into().unwrap());
    let expected = u64::from_le_bytes(header[24..32].try_into().unwrap());
    if files == 0 || files > MAX_FILES || expected > MAX_TOTAL {
        return Err(-30793);
    }
    let staging = Staging::absent(parent(target))?;
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    let bootstrap = if external {
        ulmdb_storage::BootstrapPairTarget::create_externally_synchronized(
            &staging.path,
            Default::default(),
        )
    } else {
        ulmdb_storage::BootstrapPairTarget::create(&staging.path)
    }
    .map_err(code)?;
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    ulmdb_storage::access::create_layout(&staging.path, Default::default()).map_err(code)?;
    let (mut total, mut current) = (0u64, false);
    let mut buffer = [0u8; 65536];
    for _ in 0..files {
        let mut record = [0u8; 12];
        input.read_exact(&mut record).map_err(|_| -30793)?;
        let length = u32::from_le_bytes(record[..4].try_into().unwrap()) as usize;
        let size = u64::from_le_bytes(record[4..].try_into().unwrap());
        if length == 0 || length > 128 || size > MAX_FILE {
            return Err(-30793);
        }
        total = total.checked_add(size).ok_or(-30793)?;
        if total > expected {
            return Err(-30793);
        }
        let mut name = vec![0u8; length];
        input.read_exact(&mut name).map_err(|_| -30793)?;
        let name = std::str::from_utf8(&name).map_err(|_| -30793)?;
        if !valid_path(version, name) || !valid_size(version, name, size) {
            return Err(-30793);
        }
        if name == "CURRENT" {
            current = true;
        }
        let mut output = ulmdb_storage::access::options_for_entry(&staging.path.join(name))
            .map_err(code)?
            .write(true)
            .create_new(true)
            .open(staging.path.join(name))
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    -30793
                } else {
                    code(e.into())
                }
            })?;
        let mut remaining = size;
        while remaining != 0 {
            let length = remaining.min(buffer.len() as u64) as usize;
            input
                .read_exact(&mut buffer[..length])
                .map_err(|_| -30793)?;
            output
                .write_all(&buffer[..length])
                .map_err(|e| code(e.into()))?;
            remaining -= length as u64;
        }
        ulmdb_platform::sync_file(&output).map_err(|e| code(e.into()))?;
    }
    let mut trailing = [0u8; 1];
    if !current || total != expected || input.read(&mut trailing).map_err(|e| code(e.into()))? != 0
    {
        return Err(-30793);
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        if version == Version::Pair {
            let (selected, previous) =
                ulmdb_storage::read_roots_with_transform(&staging.path, transform.clone())
                    .map_err(code)?;
            let mut roots = vec![&selected];
            if let Some(p) = &previous {
                roots.push(p);
            }
            bootstrap
                .rebind_imported_fixed_layout(&roots)
                .map_err(code)?;
        }
        // Normal validation opens its own native writer. This fresh-target
        // guard spans import/rebinding, never that second ownership acquisition.
        drop(bootstrap);
    }
    if version == Version::Pair {
        validate_pair_with_cache_options(&staging.path, transform, external, cache)?;
    } else if external {
        // v1 has no predecessor grammar; retain its existing complete selected
        // graph admission, while avoiding Native locks in caller-owned staging.
        let _read = ReadTxn::open_externally_synchronized_with_cache_options(
            &staging.path,
            Default::default(),
            transform,
            cache,
        )
        .map_err(code)?;
    } else {
        let environment = Environment::open_with_cache_options(
            &staging.path,
            Default::default(),
            Default::default(),
            transform,
            cache,
        )
        .map_err(code)?;
        ulmdb::backup::verify(&environment).map_err(code)?;
    }
    for directory in ["segments", "commits", "cohorts", "transactions", ""] {
        ulmdb_platform::sync_path(staging.path.join(directory)).map_err(|e| code(e.into()))?;
    }
    staging.publish(target)
}

/// Validate the selector relation and both complete user graphs without ever
/// temporarily selecting the predecessor. The directory is the private owned
/// staging namespace, so no publisher can change the pair during admission.
fn validate_pair(directory: &Path) -> FfiResult<()> {
    validate_pair_with_transform(directory, None)
}
fn validate_pair_with_transform(directory: &Path, transform: Transform) -> FfiResult<()> {
    validate_pair_with_mode(directory, transform, false)
}
fn validate_pair_with_mode(
    directory: &Path,
    transform: Transform,
    external: bool,
) -> FfiResult<()> {
    validate_pair_with_cache_options(directory, transform, external, None)
}
fn validate_pair_with_cache_options(
    directory: &Path,
    transform: Transform,
    external: bool,
    cache: chunks::Options,
) -> FfiResult<()> {
    // Reject nonregular metadata/assets and linked directories before a native
    // decoder opens any referenced file. This also keeps FIFO-shaped hostile
    // source files from blocking the writer's preflight.
    let (mut files, mut total) = (0u64, 0u64);
    visit(Version::Pair, directory, |_, _, size| {
        files = files.checked_add(1).ok_or(-30792)?;
        total = total.checked_add(size).ok_or(-30792)?;
        if files > MAX_FILES || total > MAX_TOTAL {
            return Err(-30792);
        }
        Ok(())
    })?;
    let mut current_bytes = Vec::with_capacity(145);
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let current = options
        .open(directory.join("CURRENT"))
        .map_err(|e| code(e.into()))?;
    if !current.metadata().map_err(|e| code(e.into()))?.is_file() {
        return Err(-30793);
    }
    current
        .take(145)
        .read_to_end(&mut current_bytes)
        .map_err(|e| code(e.into()))?;
    if !matches!(current_bytes.len(), 64 | 144) {
        return Err(-30793);
    }
    let ancillary = match fs::symlink_metadata(directory.join("PREVIOUS")) {
        Ok(m) if m.is_file() && m.len() == 64 => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(code(e.into())),
        _ => return Err(-30793),
    };
    let (selected, previous) =
        ulmdb_storage::read_roots_with_transform(directory, transform.clone()).map_err(code)?;
    if (current_bytes.len() == 144 && ancillary)
        || (current_bytes.len() == 64 && ancillary != previous.is_some())
        || previous.as_ref().map_or(selected.previous != 0, |p| {
            p.generation != selected.previous
                || p.uuid != selected.uuid
                || p.epoch != selected.epoch
                || p.format_minor != selected.format_minor
        })
    {
        return Err(-30793);
    }
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        let mut roots = vec![&selected];
        if let Some(p) = &previous {
            roots.push(p);
        }
        ulmdb::FixedMap::validate_layout(directory, &roots).map_err(code)?;
    }
    if external {
        let authority =
            ulmdb::ExternalSynchronization::open_namespace(directory, transform.clone())
                .map_err(code)?;
        let pair = authority
            .capture_copy_pair(transform.clone())
            .map_err(code)?
            .admit_with_cache_options(
                ulmdb::ResidencyPolicy::Demand,
                Default::default(),
                transform,
                cache,
            )
            .map_err(code)?;
        if pair.selected().file_checkpoint().map_err(code)?
            != ulmdb_storage::checkpoint_bytes(&selected).map_err(code)?
            || pair
                .predecessor()
                .map(ReadTxn::file_checkpoint)
                .transpose()
                .map_err(code)?
                != previous
                    .as_ref()
                    .map(ulmdb_storage::checkpoint_bytes)
                    .transpose()
                    .map_err(code)?
        {
            return Err(-30793);
        }
    } else {
        let environment = Environment::open_with_cache_options(
            directory,
            Default::default(),
            Default::default(),
            transform.clone(),
            cache.clone(),
        )
        .map_err(code)?;
        ulmdb::backup::verify(&environment).map_err(code)?;
        if let Some(expected) = previous {
            // This native admission checks exact pair lineage and the complete
            // predecessor graph. It never borrows the writer or rewrites CURRENT.
            let previous = ReadTxn::open_previous_coordinated_snapshot_with_cache_options(
                directory,
                ulmdb::ResidencyPolicy::Demand,
                Default::default(),
                transform,
                cache,
            )
            .map_err(|error| match error {
                ulmdb::PreviousError::NoPrevious => -30793,
                ulmdb::PreviousError::Engine(error) => code(error),
            })?;
            if previous.file_checkpoint().map_err(code)?
                != ulmdb_storage::checkpoint_bytes(&expected).map_err(code)?
            {
                return Err(-30793);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "archive_pair_tests.rs"]
mod pair_tests;

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            let path = std::env::temp_dir().join(format!(
                "ulmdb-copy-archive-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn exported(fixture: &Fixture) -> Vec<u8> {
        let source = Environment::create(fixture.0.join("source")).unwrap();
        let mut writer = source.begin_write().unwrap();
        writer.create_database(b"data", DatabaseKind::Kv).unwrap();
        writer.put(b"data", b"key", b"value").unwrap();
        writer.commit().unwrap();
        fs::write(fixture.0.join("source/LMDB-ABI"), b"1.0\n").unwrap();
        let snapshot = fixture.0.join("snapshot");
        ulmdb::backup::export(&source, &snapshot).unwrap();
        adapter_metadata(&fixture.0.join("source"), &snapshot).unwrap();
        let mut archive = Vec::new();
        write_archive(&snapshot, &mut archive).unwrap();
        archive
    }
    fn handcrafted(records: &[(&str, &[u8])]) -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(ARCHIVE_MAGIC);
        output.extend_from_slice(&1u32.to_le_bytes());
        output.extend_from_slice(&0u32.to_le_bytes());
        output.extend_from_slice(&(records.len() as u64).to_le_bytes());
        output.extend_from_slice(
            &records
                .iter()
                .map(|(_, bytes)| bytes.len() as u64)
                .sum::<u64>()
                .to_le_bytes(),
        );
        for (name, bytes) in records {
            output.extend_from_slice(&(name.len() as u32).to_le_bytes());
            output.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            output.extend_from_slice(name.as_bytes());
            output.extend_from_slice(bytes);
        }
        output
    }
    #[test]
    fn archive_restores_verified_independent_native_snapshot() {
        let fixture = Fixture::new();
        let bytes = exported(&fixture);
        let archive = fixture.0.join("copy.archive");
        fs::write(&archive, bytes).unwrap();
        let destination = fixture.0.join("restored");
        restore_archive(&archive, &destination).unwrap();
        assert_eq!(fs::read(destination.join("LMDB-ABI")).unwrap(), b"1.0\n");
        let restored = Environment::open(&destination).unwrap();
        assert_eq!(
            restored.begin_read().get(b"data", b"key").unwrap(),
            Some(b"value".as_slice())
        );
        let mut writer = restored.begin_write().unwrap();
        writer.put(b"data", b"key", b"changed").unwrap();
        writer.commit().unwrap();
        drop(restored);
        let original = Environment::open(fixture.0.join("snapshot")).unwrap();
        assert_eq!(
            original.begin_read().get(b"data", b"key").unwrap(),
            Some(b"value".as_slice())
        );
    }
    #[test]
    fn invalid_archives_never_publish_and_existing_targets_remain_untouched() {
        let fixture = Fixture::new();
        let valid = exported(&fixture);
        let archive = fixture.0.join("copy.archive");
        let destination = fixture.0.join("restored");
        let mut cases = vec![
            handcrafted(&[("../escape", b"invalid")]),
            handcrafted(&[("CURRENT", b"one"), ("CURRENT", b"two")]),
            handcrafted(&[("/absolute", b"invalid")]),
        ];
        let mut wrong_magic = valid.clone();
        wrong_magic[0] ^= 1;
        cases.push(wrong_magic);
        let mut trailing = valid.clone();
        trailing.push(1);
        cases.push(trailing);
        let mut truncated = valid.clone();
        truncated.pop();
        cases.push(truncated);
        let mut wrong_total = valid.clone();
        wrong_total[24..32].copy_from_slice(&0u64.to_le_bytes());
        cases.push(wrong_total);
        let mut corrupted = valid.clone();
        let last = corrupted.len() - 1;
        corrupted[last] ^= 1;
        cases.push(corrupted);
        for bytes in cases {
            fs::write(&archive, bytes).unwrap();
            assert!(restore_archive(&archive, &destination).is_err());
            assert!(!destination.exists());
            assert!(!fixture.0.join("escape").exists());
        }
        fs::write(&archive, valid).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("keep"), b"untouched").unwrap();
        assert!(restore_archive(&archive, &destination).is_err());
        assert_eq!(fs::read(destination.join("keep")).unwrap(), b"untouched");
    }
}

#[cfg(test)]
#[path = "external_tests.rs"]
mod external_tests;

#[cfg(all(test, target_os = "linux"))]
mod fixed_v2_tests {
    use super::*;
    use std::{ffi::CString, os::fd::AsRawFd};
    struct Dir(PathBuf);
    impl Dir {
        fn new() -> Self {
            let d = std::env::temp_dir().join(format!(
                "c-fixed-archive-{:032x}",
                ulmdb_storage::new_identity().unwrap()
            ));
            fs::create_dir(&d).unwrap();
            Self(d)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    struct Env(*mut MDB_env);
    impl Drop for Env {
        fn drop(&mut self) {
            unsafe { environment::mdb_env_close(self.0) }
        }
    }
    fn open(path: &Path, flags: u32) -> Env {
        let path = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let mut env = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut env), 0);
            assert_eq!(environment::mdb_env_set_maxdbs(env, 16), 0);
            assert_eq!(
                environment::mdb_env_open(env, path.as_ptr(), flags, 0o600),
                0
            );
        }
        Env(env)
    }
    fn isolated(name: &str) -> bool {
        if std::env::var("ULMDB_FIXED_ARCHIVE_WORKER").is_ok_and(|v| v == name) {
            return false;
        }
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("ULMDB_FIXED_ARCHIVE_WORKER", name)
            .spawn()
            .unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "{name}: {status}");
                return true;
            }
            if std::time::Instant::now() > end {
                let _ = child.kill();
                let _ = child.wait();
                panic!("owned fixed test worker exceeded deadline: {name}");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    #[test]
    fn genuinely_fresh_c_fixed_map_uses_reusable_default_and_keeps_nolock_v1() {
        if isolated(
            "copy::archive::fixed_v2_tests::genuinely_fresh_c_fixed_map_uses_reusable_default_and_keeps_nolock_v1",
        ) {
            return;
        }
        fn fresh(path: &Path, flags: u32) -> Env {
            let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
            let mut raw = ptr::null_mut();
            unsafe {
                assert_eq!(environment::mdb_env_create(&mut raw), 0);
                assert_eq!(environment::mdb_env_set_maxdbs(raw, 16), 0);
                assert_eq!(environment::mdb_env_set_mapsize(raw, 4 << 20), 0);
                assert_eq!(
                    environment::mdb_env_open(raw, name.as_ptr(), flags | 0x200000 | 1, 0o600),
                    0
                );
            }
            Env(raw)
        }
        let d = Dir::new();
        let source = d.0.join("fresh");
        let env = fresh(&source, 0);
        let raw = env.0;
        assert_eq!(
            &fs::read(source.join("FIXEDMAP")).unwrap()[..8],
            b"ULMDBFM2"
        );
        let mut txn = ptr::null_mut();
        let mut db = 0;
        let data = vec![0u8; 512 << 10];
        let mut key = MDB_val {
            mv_size: 1,
            mv_data: b"k".as_ptr().cast_mut().cast(),
        };
        let mut value = MDB_val {
            mv_size: data.len(),
            mv_data: data.as_ptr().cast_mut().cast(),
        };
        unsafe {
            assert_eq!(
                transaction::mdb_txn_begin(raw, ptr::null_mut(), 0, &mut txn),
                0
            );
            assert_eq!(operations::mdb_dbi_open(txn, ptr::null(), 0, &mut db), 0);
            assert_eq!(operations::mdb_put(txn, db, &mut key, &mut value, 0), 0);
            assert_eq!(transaction::mdb_txn_commit(txn), 0);
        }
        let mut old = ptr::null_mut();
        let mut borrowed = MDB_val {
            mv_size: 0,
            mv_data: ptr::null_mut(),
        };
        unsafe {
            assert_eq!(
                transaction::mdb_txn_begin(raw, ptr::null_mut(), RDONLY, &mut old),
                0
            );
            assert_eq!(operations::mdb_get(old, db, &mut key, &mut borrowed), 0);
        }
        for n in 1..=24u8 {
            let bytes = vec![n; 512 << 10];
            let mut value = MDB_val {
                mv_size: bytes.len(),
                mv_data: bytes.as_ptr().cast_mut().cast(),
            };
            unsafe {
                assert_eq!(
                    transaction::mdb_txn_begin(raw, ptr::null_mut(), 0, &mut txn),
                    0
                );
                assert_eq!(
                    operations::mdb_put(txn, db, &mut key, &mut value, 0),
                    0,
                    "fresh put n={n}"
                );
                assert_eq!(transaction::mdb_txn_commit(txn), 0, "fresh commit n={n}");
                assert_eq!(
                    std::slice::from_raw_parts(borrowed.mv_data.cast::<u8>(), borrowed.mv_size),
                    vec![0; 512 << 10]
                );
            }
        }
        unsafe {
            transaction::mdb_txn_abort(old);
        }
        drop(env);
        let native = Environment::open_fixed_map(&source).unwrap();
        assert_eq!(
            native.begin_read().get(MAIN, b"k").unwrap().unwrap(),
            vec![24; 512 << 10]
        );
        drop(native);
        // Reusable slots require genuine cross-owner leases. Reject an existing
        // v2 layout at open, including an ordinary non-fixed NOLOCK open.
        for placement in [0, 1] {
            let name = CString::new(source.as_os_str().as_encoded_bytes()).unwrap();
            let mut raw = ptr::null_mut();
            unsafe {
                assert_eq!(environment::mdb_env_create(&mut raw), 0);
                assert_eq!(environment::mdb_env_set_mapsize(raw, 4 << 20), 0);
                assert_eq!(environment::mdb_env_open(raw, name.as_ptr(), 0x200000 | 0x400000 | placement, 0o600), libc::EINVAL);
                environment::mdb_env_close(raw);
            }
        }
        let external = d.0.join("external");
        drop(fresh(&external, 0x400000));
        assert_eq!(
            &fs::read(external.join("FIXEDMAP")).unwrap()[..8],
            b"ULMDBFM1"
        );
        drop(fresh(&external, 0));
        assert_eq!(
            &fs::read(external.join("FIXEDMAP")).unwrap()[..8],
            b"ULMDBFM1"
        );
    }
    #[test]
    fn default_c_lazy_admission_honors_reusable_window_pressure() {
        if isolated(
            "copy::archive::fixed_v2_tests::default_c_lazy_admission_honors_reusable_window_pressure",
        ) {
            return;
        }

        for mode in [
            ulmdb::PublicationMode::Legacy,
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            let d = Dir::new();
            let source = d.0.join("source");
            let native =
                Environment::create_reusable_fixed_map(&source, mode, 4 << 20, None).unwrap();
            let mut w = native.begin_write().unwrap();
            w.create_database(b"db", ulmdb::DatabaseKind::Kv).unwrap();
            w.put(b"db", b"k", &vec![0; 512 << 10]).unwrap();
            w.commit().unwrap();
            drop(native);
            let name = CString::new(source.as_os_str().as_encoded_bytes()).unwrap();
            let mut raw = ptr::null_mut();
            unsafe {
                assert_eq!(environment::mdb_env_create(&mut raw), 0);
                assert_eq!(environment::mdb_env_set_maxdbs(raw, 16), 0);
                assert_eq!(environment::mdb_env_set_mapsize(raw, 4 << 20), 0);
                assert_eq!(
                    environment::mdb_env_open(raw, name.as_ptr(), 0x200000 | 1, 0o600),
                    0
                );
            }
            let env = Env(raw);
            let mut txn = ptr::null_mut();
            let mut db = 0;
            let named = CString::new("db").unwrap();
            unsafe {
                assert_eq!(
                    transaction::mdb_txn_begin(raw, ptr::null_mut(), 0, &mut txn),
                    0
                );
                assert_eq!(operations::mdb_dbi_open(txn, named.as_ptr(), 0, &mut db), 0);
                assert_eq!(transaction::mdb_txn_commit(txn), 0);
            }
            let mut old = ptr::null_mut();
            let mut key = MDB_val {
                mv_size: 1,
                mv_data: b"k".as_ptr().cast_mut().cast(),
            };
            let mut old_value = MDB_val {
                mv_size: 0,
                mv_data: ptr::null_mut(),
            };
            unsafe {
                assert_eq!(
                    transaction::mdb_txn_begin(raw, ptr::null_mut(), RDONLY, &mut old),
                    0
                );
                assert_eq!(operations::mdb_get(old, db, &mut key, &mut old_value), 0);
            }
            for n in 1..=24u8 {
                let data = vec![n; 512 << 10];
                let mut value = MDB_val {
                    mv_size: data.len(),
                    mv_data: data.as_ptr().cast_mut().cast(),
                };
                unsafe {
                    assert_eq!(
                        transaction::mdb_txn_begin(raw, ptr::null_mut(), 0, &mut txn),
                        0
                    );
                    assert_eq!(
                        operations::mdb_put(txn, db, &mut key, &mut value, 0),
                        0,
                        "put {mode:?} n={n}"
                    );
                    assert_eq!(transaction::mdb_txn_commit(txn), 0, "commit {mode:?} n={n}");
                    assert_eq!(
                        std::slice::from_raw_parts(
                            old_value.mv_data.cast::<u8>(),
                            old_value.mv_size
                        ),
                        vec![0; 512 << 10]
                    );
                }
            }
            unsafe {
                transaction::mdb_txn_abort(old);
            }
            drop(env);
            let native = Environment::open_fixed_map(&source).unwrap();
            assert_eq!(
                native.begin_read().get(b"db", b"k").unwrap().unwrap(),
                vec![24; 512 << 10]
            );
        }
    }
    #[test]
    fn public_fd_archive_rebinds_reusable_layout_before_complete_target_admission() {
        if isolated(
            "copy::archive::fixed_v2_tests::public_fd_archive_rebinds_reusable_layout_before_complete_target_admission",
        ) {
            return;
        }

        for mode in [
            ulmdb::PublicationMode::Legacy,
            ulmdb::PublicationMode::Cohort,
            ulmdb::PublicationMode::Indexed,
        ] {
            let d = Dir::new();
            let source = d.0.join("source");
            let archive = d.0.join("copy.mdb");
            let env = Environment::create_reusable_fixed_map(&source, mode, 4 << 20, None).unwrap();
            let mut w = env.begin_write().unwrap();
            w.create_database(b"db", ulmdb::DatabaseKind::Kv).unwrap();
            w.put(b"db", b"k", b"original").unwrap();
            w.commit().unwrap();
            drop(env);
            let source_env = open(&source, 0x200000);
            let output = File::create(&archive).unwrap();
            unsafe {
                assert_eq!(
                    crate::copy::mdb_env_copyfd2(source_env.0, output.as_raw_fd(), 0),
                    0
                );
            }
            drop(output);
            drop(source_env);
            let restored = open(&archive, 0x200000 | 0x4000);
            drop(restored);
            let target = std::path::PathBuf::from(format!("{}.ulmdb", archive.display()));
            let env = Environment::open_fixed_map(&target).unwrap();
            assert_eq!(
                env.begin_read().get(b"db", b"k").unwrap(),
                Some(b"original".as_slice())
            );
            let mut w = env.begin_write().unwrap();
            w.put(b"db", b"k", b"next").unwrap();
            w.commit().unwrap();
            assert_eq!(
                env.begin_read().get(b"db", b"k").unwrap(),
                Some(b"next".as_slice())
            );
        }
    }
}
