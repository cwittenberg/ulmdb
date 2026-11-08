//! Test-only inventory of canonical physical record paths. This can run while
//! a tested Capture/maintenance capability is deliberately held. It validates
//! namespace/type structure, not descriptor authority or record contents.
use std::{
    fs,
    path::{Path, PathBuf},
};

fn lower_hex(value: &str, width: usize) -> bool {
    value.len() == width
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn record(path: PathBuf, name: &str) -> PathBuf {
    let nonce = name
        .strip_prefix("pin-")
        .and_then(|v| v.strip_suffix(".lease"))
        .unwrap();
    assert!(
        lower_hex(nonce, 32),
        "noncanonical record: {}",
        path.display()
    );
    assert_ne!(u128::from_str_radix(nonce, 16).unwrap(), 0);
    let metadata = fs::symlink_metadata(&path).unwrap();
    assert!(metadata.is_file() && !metadata.file_type().is_symlink());
    path
}

pub fn paths(environment: &Path) -> Vec<PathBuf> {
    let root = environment.join(".ulmdb-coordination/pins");
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => panic!("physical record inventory: {error}"),
    };
    let metadata = fs::symlink_metadata(&root).unwrap();
    assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
    let root_identity = ulmdb_platform::path_identity(&root).unwrap();
    let mut records = Vec::new();
    for entry in entries {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_str().unwrap();
        if let Some(id) = name.strip_prefix("gen-") {
            assert!(lower_hex(id, 16));
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
            let identity = ulmdb_platform::path_identity(&path).unwrap();
            let mut scan_gate = false;
            for member in fs::read_dir(&path).unwrap() {
                let member = member.unwrap();
                let name = member.file_name();
                let name = name.to_str().unwrap();
                if name == "_scan" {
                    let metadata = fs::symlink_metadata(member.path()).unwrap();
                    assert!(metadata.is_file() && !metadata.file_type().is_symlink());
                    assert_eq!(metadata.len(), 0);
                    scan_gate = true;
                } else {
                    records.push(record(member.path(), name));
                }
            }
            assert!(scan_gate, "generation is missing its membership gate");
            assert!(identity.same_object(&ulmdb_platform::path_identity(&path).unwrap()));
        } else {
            records.push(record(entry.path(), name));
        }
    }
    assert!(root_identity.same_object(&ulmdb_platform::path_identity(&root).unwrap()));
    records.sort();
    records
}

#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub fn unique_live_ordinary(environment: &Path) -> PathBuf {
    let mut live = Vec::new();
    for path in paths(environment) {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        match file.try_lock() {
            Ok(()) => file.unlock().unwrap(),
            Err(fs::TryLockError::WouldBlock) => {
                let bytes = fs::read(&path).unwrap();
                if bytes.get(8..10) == Some(1u16.to_le_bytes().as_slice()) {
                    assert_eq!(bytes.len(), 160);
                    assert_eq!(&bytes[..8], b"ULMDBPIN");
                    live.push(path);
                }
            }
            Err(error) => panic!("physical record lease: {error}"),
        }
    }
    assert_eq!(
        live.len(),
        1,
        "expected one genuinely leased ordinary owner"
    );
    live.pop().unwrap()
}
