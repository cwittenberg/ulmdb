#![cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};
use ulmdb::{DatabaseKind, Environment, MappedFlush, PublicationMode};
struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn fixed_portable_worker() {
    if std::env::var_os("ULMDB_FIXED_PORTABLE_WORKER").is_none() {
        return;
    }
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let d = Dir(std::env::temp_dir().join(format!(
            "fixed-portable-{}",
            ulmdb_storage::new_identity().unwrap()
        )));
        let e = Environment::create_fixed_map(&d.0, mode, 16 << 20, None).unwrap();
        let fixed = e.fixed_map_info().unwrap().unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"data", DatabaseKind::Kv).unwrap();
        w.put(b"data", b"k", &[1; 12000]).unwrap();
        w.commit().unwrap();
        let old = e.begin_read();
        let borrow = old.get(b"data", b"k").unwrap().unwrap();
        assert!(
            (fixed.address..fixed.address + fixed.capacity).contains(&(borrow.as_ptr() as usize))
        );
        let mut w = e.begin_write().unwrap();
        w.enable_write_map(4 << 20, MappedFlush::Synchronous)
            .unwrap();
        let actual = w.mapped_allocation().unwrap().unwrap();
        assert!(actual.file_inode != 0);
        assert!((fixed.address..fixed.address + fixed.capacity).contains(&actual.address));
        let cap = w.reserve(b"data", b"mapped", 48000).unwrap();
        let v = w.reservation_value_mut(&cap).unwrap();
        let pointer = v.as_ptr() as usize;
        v.fill(0x6d);
        w.end_reservation(cap).unwrap();
        w.put(b"data", b"k", b"new").unwrap();
        w.commit().unwrap();
        assert_eq!(borrow, &[1; 12000]);
        {
            let r = e.begin_read();
            let value = r.get(b"data", b"mapped").unwrap().unwrap();
            assert_eq!(value, &[0x6d; 48000]);
            assert_eq!(value.as_ptr() as usize, pointer);
        }
        drop((old, e));
        let e = Environment::open_fixed_map(&d.0).unwrap();
        assert_eq!(e.fixed_map_info().unwrap().unwrap().address, fixed.address);
        let read = e.begin_read();
        let value = read.get(b"data", b"mapped").unwrap().unwrap();
        assert_eq!(value.as_ptr() as usize, pointer);
        assert_eq!(value, &[0x6d; 48000]);
        assert_eq!(read.get(b"data", b"k").unwrap(), Some(b"new".as_slice()));
    }
}
#[test]
fn portable_fixed_plain_mapped_old_borrow_and_reopen() {
    // Releasing/reopening a fixed base must not race another libtest thread's
    // unrelated VMA allocation. The real smoke body has its own exec image.
    struct Owned(std::process::Child);
    impl Drop for Owned {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }
    let mut child = Owned(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "fixed_portable_worker",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("ULMDB_FIXED_PORTABLE_WORKER", "1")
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(s) = child.0.try_wait().unwrap() {
            assert!(s.success(), "fixed smoke {s}");
            break;
        }
        assert!(Instant::now() < deadline, "fixed smoke deadline");
        std::thread::sleep(Duration::from_millis(5));
    }
}
