#![cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::{fs, path::PathBuf};
use ulmdb::{
    DatabaseKind, Error,
    resident::{Environment, Options, ReadTxn, RedoFormat},
};
use ulmdb_platform::FileExt;
struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn options() -> Options {
    let mut o = Options::default();
    o.pool.kv_pages = 128;
    o.pool.tiny_pages = 32;
    o.pool.max_reserved_bytes = 16 << 20;
    o.wal_capacity = 2 << 20;
    o
}
#[test]
fn negotiated_capture_binds_actual_platform_identity_and_retains_old_bytes() {
    for format in [RedoFormat::LiteralV1, RedoFormat::CompactV2] {
        let d = Dir(std::env::temp_dir().join(format!(
            "resident-capture-identity-{}",
            ulmdb_storage::new_identity().unwrap()
        )));
        let e = Environment::create_with_redo(&d.0, options(), format).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"db", DatabaseKind::Kv).unwrap();
        w.put(b"db", b"key", b"old").unwrap();
        w.commit().unwrap();
        e.enable_readonly_capture().unwrap();
        let gate = ulmdb_platform::path_identity(&d.0.join("RESIDENT-CAPTURE")).unwrap();
        let mut h = [0; 256];
        fs::File::open(d.0.join("RESIDENT.wal"))
            .unwrap()
            .read_exact_at(&mut h, 0)
            .unwrap();
        assert_eq!(
            u64::from_le_bytes(h[192..200].try_into().unwrap()),
            gate.volume
        );
        if cfg!(windows) {
            assert_eq!(u64::from_le_bytes(h[184..192].try_into().unwrap()), 2);
            assert_eq!(
                u128::from_le_bytes(h[200..216].try_into().unwrap()),
                gate.file_id
            );
        } else {
            assert_eq!(u64::from_le_bytes(h[184..192].try_into().unwrap()), 1);
            assert_eq!(
                u128::from(u64::from_le_bytes(h[200..208].try_into().unwrap())),
                gate.file_id
            );
        }
        let lease = ulmdb_storage::resident::ReadCapture::open(&d.0).unwrap();
        let second = ulmdb_storage::resident::ReadCapture::open(&d.0).unwrap();
        // Header reads stay legal while two SH high-byte leases are active.
        fs::File::open(d.0.join("RESIDENT-CAPTURE"))
            .unwrap()
            .read_exact_at(&mut [0; 64], 0)
            .unwrap();
        assert!(matches!(e.checkpoint(), Err(Error::Busy)));
        drop((lease, second));
        let old = ReadTxn::open_coordinated_snapshot(&d.0, options()).unwrap();
        let borrowed = old.get(b"db", b"key").unwrap().unwrap();
        let mut w = e.begin_write().unwrap();
        w.put(b"db", b"key", b"new").unwrap();
        w.commit().unwrap();
        assert_eq!(borrowed, b"old");
        assert_eq!(
            ReadTxn::open_coordinated_snapshot(&d.0, options())
                .unwrap()
                .get(b"db", b"key")
                .unwrap(),
            Some(b"new".as_slice())
        );
        drop((old, e));
        let e = Environment::open_with_redo(&d.0, options(), format).unwrap();
        assert_eq!(
            e.begin_read().get(b"db", b"key").unwrap(),
            Some(b"new".as_slice())
        );
    }
}
