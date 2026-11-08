use super::*;
use std::{fs, io::Write};
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn temp(label: &str) -> Temp {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "owned-export-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&path).unwrap();
    Temp(path)
}
#[test]
fn publication_is_exclusive_and_parent_rebinding_never_redirects_output_or_cleanup() {
    let root = temp("publish");
    let target = root.0.join("target");
    let mut output = OwnedExport::create(&target, 1).unwrap();
    output
        .create_file(OsStr::new("payload"))
        .unwrap()
        .write_all(b"owned")
        .unwrap();
    output.publish().unwrap();
    assert_eq!(fs::read(target.join("payload")).unwrap(), b"owned");
    assert!(OwnedExport::create(&target, 2).is_err());
    let late = root.0.join("late");
    let mut output = OwnedExport::create(&late, 3).unwrap();
    output
        .create_file(OsStr::new("payload"))
        .unwrap()
        .write_all(b"discard")
        .unwrap();
    fs::create_dir(&late).unwrap();
    fs::write(late.join("sentinel"), b"keep").unwrap();
    assert!(output.publish().is_err());
    assert_eq!(fs::read(late.join("sentinel")).unwrap(), b"keep");
    assert!(
        !root
            .0
            .join(".resident-export-00000000000000000000000000000003")
            .exists()
    );
    let original = root.0.join("parent");
    let moved = root.0.join("moved");
    fs::create_dir(&original).unwrap();
    let mut output = OwnedExport::create(&original.join("target"), 4).unwrap();
    output
        .create_file(OsStr::new("payload"))
        .unwrap()
        .write_all(b"owned")
        .unwrap();
    fs::rename(&original, &moved).unwrap();
    fs::create_dir(&original).unwrap();
    fs::write(original.join("sentinel"), b"replacement").unwrap();
    assert!(output.publish().is_err());
    assert_eq!(fs::read(original.join("sentinel")).unwrap(), b"replacement");
    assert!(!original.join("target").exists());
    assert_eq!(fs::read_dir(&moved).unwrap().count(), 0);
}
