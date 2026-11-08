#![cfg(target_os = "linux")]
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{FileExt, PermissionsExt},
    path::PathBuf,
    sync::Arc,
};
use ulmdb::{DatabaseKind, Environment, PublicationMode, ReadAhead, ResidencyPolicy};
use ulmdb_storage::transform::{Checksum, ChecksumConfig, TransformContext};
struct Sum;
impl Checksum for Sum {
    fn compute(&self, source: &[u8], out: &mut [u8], _: Option<&[u8]>) -> ulmdb::Result<()> {
        out.copy_from_slice(&Sha256::digest(source));
        Ok(())
    }
}
fn context(limit: u64) -> Arc<TransformContext> {
    Arc::new(
        TransformContext::new(
            Some(ChecksumConfig {
                callback: Arc::new(Sum),
                bytes: 32,
            }),
            None,
            limit,
        )
        .unwrap(),
    )
}
struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn checksum_fixed_bytes_are_sealed_quota_owned_shared_and_cold_source_still_authenticates() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir(std::env::temp_dir().join(format!(
            "fixed-sum-{}",
            ulmdb_storage::new_identity().unwrap()
        )));
        let ctx = context(16 << 20);
        let mut e = Environment::create_with_transform(&d.0, mode, ctx.clone()).unwrap();
        let window = e.enable_fixed_map(8 << 20, None).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"data", DatabaseKind::Kv).unwrap();
        w.put(b"data", b"k", &vec![7; 48_000]).unwrap();
        w.commit().unwrap();
        let old = e.begin_read();
        let a = old.get(b"data", b"k").unwrap().unwrap();
        let ap = a.as_ptr() as usize;
        assert!((window.address..window.address + window.capacity).contains(&ap));
        let maps = fs::read_to_string("/proc/self/maps").unwrap();
        assert!(
            maps.lines()
                .any(|line| line.contains("ulmdb-fixed-decoded") && {
                    let range = line.split_whitespace().next().unwrap();
                    let (a, b) = range.split_once('-').unwrap();
                    (usize::from_str_radix(a, 16).unwrap()..usize::from_str_radix(b, 16).unwrap())
                        .contains(&ap)
                })
        );
        // Reopening the same authenticated graph shares the actual sealed backing,
        // with every extra reader's context allowance kept alive until its map ends.
        let again = ulmdb::ReadTxn::open_snapshot_with_transform_options(
            &d.0,
            ReadAhead::Normal,
            Some(ctx.clone()),
        )
        .unwrap()
        .with_fixed_map(&d.0, ulmdb::FixedMap::open_namespace(&d.0).unwrap())
        .unwrap();
        assert_eq!(
            again.get(b"data", b"k").unwrap().unwrap().as_ptr() as usize,
            ap
        );
        drop(again);
        let mut found = false;
        for fd in fs::read_dir("/proc/self/fd").unwrap().flatten() {
            if fs::read_link(fd.path())
                .is_ok_and(|p| p.to_string_lossy().contains("memfd:ulmdb-fixed-decoded"))
            {
                let file = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(fd.path())
                    .unwrap();
                assert_eq!(
                    file.write_all_at(b"x", 0).unwrap_err().raw_os_error(),
                    Some(libc::EPERM)
                );
                assert_eq!(
                    file.set_len(1).unwrap_err().raw_os_error(),
                    Some(libc::EPERM)
                );
                found = true;
            }
        }
        assert!(found);
        let mut w = e.begin_write().unwrap();
        w.put(b"data", b"k", &vec![9; 48_000]).unwrap();
        w.commit().unwrap();
        assert_eq!(a, vec![7; 48_000]);
        let read = e.begin_read();
        let b = read.get(b"data", b"k").unwrap().unwrap();
        let bp = b.as_ptr() as usize;
        assert_ne!(ap, bp);
        assert!((window.address..window.address + window.capacity).contains(&bp));
        drop(e);
        // Keep an existing fixed reader and its backing alive. A new open must still
        // reject actual encoded-source corruption instead of trusting that cache.
        let m = read.file_manifest().unwrap();
        let entry = m.segments.last().unwrap();
        let path = ulmdb_storage::segment_path(&d.0, entry);
        let encoded = fs::read(&path).unwrap();
        let perms = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.write_all_at(&[encoded[200] ^ 1], 200).unwrap();
        drop(file);
        assert!(
            Environment::open_fixed_map_with_transform_options(
                &d.0,
                ResidencyPolicy::Demand,
                ReadAhead::Normal,
                Some(ctx.clone())
            )
            .is_err()
        );
        assert_eq!(b, vec![9; 48_000]);
        fs::write(&path, &encoded).unwrap();
        fs::set_permissions(&path, perms).unwrap();
        let reopened = Environment::open_fixed_map_with_transform_options(
            &d.0,
            ResidencyPolicy::Demand,
            ReadAhead::Normal,
            Some(ctx.clone()),
        )
        .unwrap();
        assert_eq!(
            reopened
                .begin_read()
                .get(b"data", b"k")
                .unwrap()
                .unwrap()
                .as_ptr() as usize,
            bp
        );
        drop(reopened);
        let tight = context(4096);
        assert!(
            Environment::open_fixed_map_with_transform_options(
                &d.0,
                ResidencyPolicy::Demand,
                ReadAhead::Normal,
                Some(tight.clone())
            )
            .is_err()
        );
        assert_eq!(tight.decoded_bytes(), 0);
        drop(read);
        drop(old);
        assert_eq!(ctx.decoded_bytes(), 0);
    }
}
