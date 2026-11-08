//! Runtime vectors for an actual Windows host; cross-compilation is not runtime
//! evidence. Every replacement keeps a genuine owned placeholder reservation.
#![cfg(windows)]
#![allow(unsafe_code)]
use std::{fs, io::Write, sync::Arc};
use ulmdb_storage::{Error, fixed_address::Reservation};
#[test]
fn placeholder_views_keep_exact_borrows_and_reuse_writable_addresses() {
    let directory = std::env::temp_dir().join(format!(
        "ulmdb-win-fixed-{:032x}",
        ulmdb_storage::new_identity().unwrap()
    ));
    fs::create_dir(&directory).unwrap();
    //4KiB is the supported file/placeholder page geometry. Larger unsupported
    //host geometry must fail rather than fabricate adjacent byte references.
    let p = 4096usize;
    let source = directory.join("source");
    let mut file = ulmdb_platform::create_private_file(&source, true).unwrap();
    file.write_all(&vec![0x41; 2 * p + 17]).unwrap();
    ulmdb_platform::sync_file(&file).unwrap();
    ulmdb_platform::set_private_permissions(&file, false).unwrap();
    drop(file);
    let file = Arc::new(ulmdb_platform::open_readonly(&source).unwrap());
    let window = Reservation::reserve(None, 8 * p).unwrap();
    let base = window.address();
    assert!(matches!(
        Reservation::reserve(Some(base), 8 * p),
        Err(Error::Busy)
    ));
    // Test owns the sealed immutable file and never writes/truncates it while
    //these leases exist. The source's last partial page is never exposed past EOF.
    let read = unsafe { window.map_file(file.clone(), 0, p, 2 * p + 17) }.unwrap();
    assert_eq!(read.as_ptr() as usize, base + p);
    assert_eq!(read.bytes().unwrap(), vec![0x41; 2 * p + 17]);
    let clone = read.clone();
    drop(read);
    drop(file);
    let mutable = directory.join("mutable");
    let file = ulmdb_platform::create_private_file(&mutable, true).unwrap();
    file.set_len((2 * p) as u64).unwrap();
    // This private staging inode has no other writer or mutable mapping.
    let mut write = unsafe { window.map_file_mut(&file, 4 * p, 2 * p) }.unwrap();
    let pointer = write.as_ptr() as usize;
    write.bytes_mut().unwrap().fill(0x76);
    write.flush_range(2 * p, false).unwrap();
    drop(write);
    ulmdb_platform::set_private_permissions(&file, false).unwrap();
    drop(file);
    let file = Arc::new(ulmdb_platform::open_readonly(&mutable).unwrap());
    let sealed = unsafe { window.map_file(file, 0, 4 * p, 2 * p) }.unwrap();
    assert_eq!(sealed.as_ptr() as usize, pointer);
    assert_eq!(sealed.bytes().unwrap(), vec![0x76; 2 * p]);
    assert_eq!(clone.bytes().unwrap(), vec![0x41; 2 * p + 17]);
    drop(window);
    assert_eq!(sealed.bytes().unwrap()[0], 0x76);
    assert_eq!(clone.bytes().unwrap()[0], 0x41);
    drop(sealed);
    drop(clone);
    fs::remove_dir_all(directory).unwrap();
}
