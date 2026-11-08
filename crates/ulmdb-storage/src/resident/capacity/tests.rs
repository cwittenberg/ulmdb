use super::file_length;
use std::{fs::OpenOptions, path::PathBuf};

struct TemporaryFile {
    file: std::fs::File,
    path: PathBuf,
}
impl TemporaryFile {
    fn new() -> Self {
        Self::new_at(&std::env::temp_dir())
    }
    fn new_at(base: &std::path::Path) -> Self {
        let path = base.join(format!(
            "ulmdb-capacity-stat-{:032x}",
            crate::new_identity().unwrap()
        ));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        Self { file, path }
    }
}
impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn regular_file_length_is_fresh_after_external_growth_and_shrink() {
    let temporary = TemporaryFile::new();
    let external = OpenOptions::new()
        .write(true)
        .open(&temporary.path)
        .unwrap();
    for length in [0, 1, 255, 4095, 4096, 1 << 20, 8192, 4096, 0, 16384] {
        external.set_len(length).unwrap();
        assert_eq!(file_length(&temporary.file).unwrap(), length);
        assert_eq!(
            file_length(&temporary.file).unwrap(),
            temporary.file.metadata().unwrap().len()
        );
    }
}

#[cfg(unix)]
#[test]
fn length_uses_the_pinned_descriptor_after_path_unlink_and_replacement() {
    let temporary = TemporaryFile::new();
    temporary.file.set_len(8192).unwrap();
    std::fs::remove_file(&temporary.path).unwrap();
    let replacement = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&temporary.path)
        .unwrap();
    replacement.set_len(17).unwrap();
    assert_eq!(file_length(&temporary.file).unwrap(), 8192);
    assert_eq!(file_length(&replacement).unwrap(), 17);
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
#[test]
fn ram_kind_selection_accepts_only_the_two_linux_uapi_magic_values() {
    for (kind, expected) in [
        (0, false),
        (0x0102_1993, false),
        (0x0102_1994, true),
        (0x0102_1995, false),
        (0x8584_58f5, false),
        (0x8584_58f6, true),
        (0x8584_58f7, false),
        (0xef53, false),        // ext filesystem
        (0x794c_7630, false),   // overlayfs
        (0x1_0102_1994, false), // Do not truncate filesystem identifiers.
        (u64::MAX, false),
    ] {
        assert_eq!(super::ram_filesystem_kind(kind), expected);
    }
}

#[cfg(unix)]
#[test]
fn one_probe_keeps_fresh_length_and_pinned_identity_on_each_filesystem() {
    for base in [std::env::temp_dir(), PathBuf::from("/dev/shm")]
        .into_iter()
        .filter(|base| base.is_dir())
    {
        let temporary = TemporaryFile::new_at(&base);
        let probe = super::CapacityProbe::for_file(&temporary.file);
        let external = OpenOptions::new()
            .write(true)
            .open(&temporary.path)
            .unwrap();
        for length in [0, 1, 255, 4096, 1 << 20, 8192, 0, 16384] {
            external.set_len(length).unwrap();
            assert_eq!(probe.length(&temporary.file).unwrap(), length);
            assert_eq!(
                probe.length(&temporary.file).unwrap(),
                temporary.file.metadata().unwrap().len()
            );
        }
        std::fs::remove_file(&temporary.path).unwrap();
        let replacement = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&temporary.path)
            .unwrap();
        replacement.set_len(777).unwrap();
        assert_eq!(probe.length(&temporary.file).unwrap(), 16384);
        assert_eq!(
            super::CapacityProbe::for_file(&replacement)
                .length(&replacement)
                .unwrap(),
            777
        );
        external.set_len(321).unwrap();
        assert_eq!(probe.length(&temporary.file).unwrap(), 321);
        assert_eq!(replacement.metadata().unwrap().len(), 777);
    }
}
