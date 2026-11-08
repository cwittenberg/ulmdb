//! Explicit external-copy admission and unchanged strict archive framing.
use super::*;
use std::ffi::CString;
use ulmdb::PublicationMode;
const NOLOCK: u32 = 0x400000;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "external-c-copy-{}",
            ulmdb_storage::new_identity().unwrap()
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn modes() -> [PublicationMode; 3] {
    [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ]
}
fn seed(f: &Fixture, mode: PublicationMode) -> PathBuf {
    let source = f.0.join("source");
    let e = Environment::create_with_publication(&source, mode).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(MAIN, DatabaseKind::Kv).unwrap();
    w.put(MAIN, b"k", b"A").unwrap();
    w.commit().unwrap();
    let mut w = e.begin_write().unwrap();
    w.put(MAIN, b"k", b"B").unwrap();
    w.commit().unwrap();
    drop(e);
    fs::write(source.join("LMDB-ABI"), b"1.0\n").unwrap();
    source
}
fn metadata(path: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(root: &Path, path: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for e in fs::read_dir(path).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                walk(root, &p, out)
            } else {
                out.push((
                    p.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(p).unwrap(),
                ));
            }
        }
    }
    let root = path.join(".ulmdb-coordination");
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out.sort();
    out
}
fn records(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut at = 32;
    let mut out = Vec::new();
    for _ in 0..u64::from_le_bytes(bytes[16..24].try_into().unwrap()) {
        let n = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let size = u64::from_le_bytes(bytes[at + 4..at + 12].try_into().unwrap()) as usize;
        at += 12;
        let name = String::from_utf8(bytes[at..at + n].to_vec()).unwrap();
        at += n;
        out.push((name, bytes[at..at + size].to_vec()));
        at += size;
    }
    assert_eq!(at, bytes.len());
    out
}
fn frame(rows: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut b = ARCHIVE_MAGIC.to_vec();
    b.extend_from_slice(&2u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&(rows.len() as u64).to_le_bytes());
    b.extend_from_slice(
        &rows
            .iter()
            .map(|(_, v)| v.len() as u64)
            .sum::<u64>()
            .to_le_bytes(),
    );
    for (name, value) in rows {
        b.extend_from_slice(&(name.len() as u32).to_le_bytes());
        b.extend_from_slice(&(value.len() as u64).to_le_bytes());
        b.extend_from_slice(name.as_bytes());
        b.extend_from_slice(value);
    }
    b
}
#[test]
fn external_pair_archive_audits_both_graphs_without_touching_capture_or_history() {
    for mode in modes() {
        let f = Fixture::new();
        let source = seed(&f, mode);
        let manifest = ulmdb_storage::read_current(&source).unwrap();
        let c = ulmdb_storage::coordination::Coordination::open(
            &source,
            manifest.uuid,
            Default::default(),
        )
        .unwrap();
        let _held = c.try_maintenance().unwrap();
        let before = metadata(&source);
        let mut bytes = Vec::new();
        write_archive_v2_with_mode(&source, &mut bytes, None, true).unwrap();
        assert_eq!(&bytes[8..12], &2u32.to_le_bytes());
        assert_eq!(before, metadata(&source));
        let archive = f.0.join("pair.archive");
        fs::write(&archive, &bytes).unwrap();
        let target = f.0.join("target");
        restore_archive_externally_synchronized_with_transform(&archive, &target, None).unwrap();
        let e = Environment::open_externally_synchronized(&target).unwrap();
        assert_eq!(
            e.begin_read().get(MAIN, b"k").unwrap(),
            Some(b"B".as_slice())
        );
        let pair = e
            .begin_read()
            .external_synchronization()
            .unwrap()
            .capture_copy_pair(None)
            .unwrap()
            .admit(Default::default(), Default::default(), None)
            .unwrap();
        assert_eq!(
            pair.predecessor().unwrap().get(MAIN, b"k").unwrap(),
            Some(b"A".as_slice())
        );
        assert_eq!(pair.selected().stats().sequence, 2);
        assert_eq!(pair.predecessor().unwrap().stats().sequence, 1);
        assert!(!target.join(".ulmdb-coordination").exists());
        let mut bad = records(&bytes);
        let mut changed = bad.clone();
        changed.push(changed[0].clone());
        let duplicate = frame(&changed);
        bad.iter_mut().find(|(n, _)| n == "CURRENT").unwrap().1[0] ^= 1;
        let wrong = frame(&bad);
        let mut cases = vec![duplicate, wrong];
        let mut truncated = bytes.clone();
        truncated.pop();
        cases.push(truncated);
        let mut trailing = bytes.clone();
        trailing.push(0);
        cases.push(trailing);
        if mode == PublicationMode::Legacy {
            cases.push(frame(
                &records(&bytes)
                    .into_iter()
                    .filter(|(n, _)| n != "PREVIOUS")
                    .collect::<Vec<_>>(),
            ));
        }
        for (index, bad) in cases.into_iter().enumerate() {
            fs::write(&archive, bad).unwrap();
            let path = f.0.join(format!("bad-{index}"));
            assert!(
                restore_archive_externally_synchronized_with_transform(&archive, &path, None)
                    .is_err()
            );
            assert!(!path.exists());
        }
    }
}
struct CEnv(*mut MDB_env);
impl Drop for CEnv {
    fn drop(&mut self) {
        unsafe { environment::mdb_env_close(self.0) }
    }
}
#[test]
fn explicit_external_factory_routes_public_directory_and_fd_copy_and_declines_raw_older_view() {
    for mode in modes() {
        let f = Fixture::new();
        let source = seed(&f, mode);
        let name = CString::new(source.as_os_str().as_encoded_bytes()).unwrap();
        let mut raw = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut raw), 0);
            assert_eq!(
                environment::mdb_env_open(raw, name.as_ptr(), RDONLY | NOLOCK, 0o600),
                0
            );
        }
        let env = CEnv(raw);
        // The complete C switch is owned by the surrounding composition. Bind
        // this exact factory directly so its safety test does not fake a pin.
        let shared = Arc::get_mut(unsafe { (*env.0).shared.as_mut().unwrap() }).unwrap();
        shared.external =
            Some(ulmdb::ExternalSynchronization::open_namespace(&source, None).unwrap());
        let manifest = ulmdb_storage::read_current(&source).unwrap();
        let c = ulmdb_storage::coordination::Coordination::open(
            &source,
            manifest.uuid,
            Default::default(),
        )
        .unwrap();
        let _held = c.try_maintenance().unwrap();
        let before = metadata(&source);
        for compact in [0, 1] {
            let path = f.0.join(format!("copy-{compact}"));
            let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
            assert_eq!(unsafe { mdb_env_copy2(env.0, name.as_ptr(), compact) }, 0);
            let r = ReadTxn::open_externally_synchronized(&path).unwrap();
            assert_eq!(r.get(MAIN, b"k").unwrap(), Some(b"B".as_slice()));
            assert_eq!(r.stats().sequence, if compact == 0 { 2 } else { 1 });
            assert!(!path.join(".ulmdb-coordination").exists());
        }
        let archive = f.0.join("fd.archive");
        let file = File::create(&archive).unwrap();
        assert_eq!(
            unsafe { mdb_env_copyfd2(env.0, ulmdb_platform::raw_file_handle(&file), 0) },
            0
        );
        assert!(file.metadata().unwrap().len() > 32);
        drop(file);
        let restored = f.0.join("restored");
        restore_archive_externally_synchronized_with_transform(&archive, &restored, None).unwrap();
        assert_eq!(
            ReadTxn::open_externally_synchronized(&restored)
                .unwrap()
                .get(MAIN, b"k")
                .unwrap(),
            Some(b"B".as_slice())
        );
        assert_eq!(before, metadata(&source));
        let shared = Arc::get_mut(unsafe { (*env.0).shared.as_mut().unwrap() }).unwrap();
        shared.previous = previous::PreviousState::new(previous::READONLY, None);
        let raw_target = f.0.join("older-raw");
        let raw_name = CString::new(raw_target.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(
            unsafe { mdb_env_copy2(env.0, raw_name.as_ptr(), 0) },
            libc::ENOTSUP
        );
        assert!(!raw_target.exists());
        let compact = f.0.join("older-compact");
        let compact_name = CString::new(compact.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { mdb_env_copy2(env.0, compact_name.as_ptr(), 1) }, 0);
        assert_eq!(
            ReadTxn::open_externally_synchronized(&compact)
                .unwrap()
                .get(MAIN, b"k")
                .unwrap(),
            Some(b"A".as_slice())
        );
    }
}

#[test]
fn empty_external_compact_copy_uses_genuine_genesis_without_kernel_metadata() {
    for mode in modes() {
        let f = Fixture::new();
        let source = f.0.join("empty");
        drop(Environment::create_with_publication(&source, mode).unwrap());
        let name = CString::new(source.as_os_str().as_encoded_bytes()).unwrap();
        let mut raw = ptr::null_mut();
        unsafe {
            assert_eq!(environment::mdb_env_create(&mut raw), 0);
            assert_eq!(
                environment::mdb_env_open(raw, name.as_ptr(), RDONLY | NOLOCK, 0o600),
                0
            );
        }
        let env = CEnv(raw);
        let shared = Arc::get_mut(unsafe { (*env.0).shared.as_mut().unwrap() }).unwrap();
        shared.external =
            Some(ulmdb::ExternalSynchronization::open_namespace(&source, None).unwrap());
        let target = f.0.join("compact");
        let target_name = CString::new(target.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { mdb_env_copy2(env.0, target_name.as_ptr(), 1) }, 0);
        let read = ReadTxn::open_externally_synchronized(&target).unwrap();
        let m = read.file_manifest().unwrap();
        assert_eq!(m.sequence, 0);
        assert_eq!(m.previous, 0);
        assert!(m.catalog.is_null());
        assert_eq!(m.next_database, 1);
        assert!(m.segments.is_empty());
        assert!(!target.join(".ulmdb-coordination").exists());
        let archive = f.0.join("compact.archive");
        let file = File::create(&archive).unwrap();
        assert_eq!(
            unsafe { mdb_env_copyfd2(env.0, ulmdb_platform::raw_file_handle(&file), 1) },
            0
        );
        assert!(file.metadata().unwrap().len() > 32);
        drop(file);
        let restored = f.0.join("compact-restored");
        restore_archive_externally_synchronized_with_transform(&archive, &restored, None).unwrap();
        assert_eq!(
            ReadTxn::open_externally_synchronized(&restored)
                .unwrap()
                .stats()
                .sequence,
            0
        );
        assert!(!restored.join(".ulmdb-coordination").exists());
    }
}
