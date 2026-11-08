#![cfg(target_os = "linux")]
use std::{fs, path::PathBuf, sync::Mutex};
use ulmdb::{DatabaseKind, Environment, PublicationMode};
// These fixtures deliberately release and reacquire an exact address. Another
// fixture's automatic reservation may legitimately occupy that process range.
static ADDRESS_SPACE: Mutex<()> = Mutex::new(());
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "native-fixed-map-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
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
fn pointer(e: &Environment, key: &[u8]) -> usize {
    e.begin_read().get(b"data", key).unwrap().unwrap().as_ptr() as usize
}
#[test]
fn fixed_base_and_record_pointer_persist_across_reopen_without_clobbering_collision() {
    let _address_space = ADDRESS_SPACE.lock().unwrap();
    for mode in modes() {
        let d = Dir::new();
        let e = Environment::create_fixed_map(&d.0, mode, 16 << 20, None).unwrap();
        let base = e.fixed_map_info().unwrap().unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"data", DatabaseKind::Kv).unwrap();
        w.put(b"data", b"key", &vec![7; 12_000]).unwrap();
        w.commit().unwrap();
        assert_eq!(
            e.stats().segments,
            ulmdb_storage::read_current(&d.0).unwrap().segments.len()
        );
        let old = e.begin_read();
        let borrow = old.get(b"data", b"key").unwrap().unwrap();
        let ptr = borrow.as_ptr() as usize;
        assert!((base.address..base.address + base.capacity).contains(&ptr));
        assert!(
            ulmdb_storage::fixed_address::Reservation::reserve(Some(base.address), base.capacity)
                .is_err()
        );
        assert_eq!(borrow, vec![7; 12_000]);
        let ordinary = ulmdb::ReadTxn::open_snapshot(&d.0).unwrap();
        let normal = ordinary.get(b"data", b"key").unwrap().unwrap().as_ptr() as usize;
        assert!(!(base.address..base.address + base.capacity).contains(&normal));
        drop(ordinary);
        drop(old);
        drop(e);
        let reopened = Environment::open_fixed_map(&d.0).unwrap();
        assert_eq!(
            reopened.fixed_map_info().unwrap().unwrap().address,
            base.address
        );
        assert_eq!(pointer(&reopened, b"key"), ptr);
        let old = reopened.begin_read();
        let previous = old.get(b"data", b"key").unwrap().unwrap();
        let mut w = reopened.begin_write().unwrap();
        w.put(b"data", b"key", &vec![9; 14_000]).unwrap();
        w.commit().unwrap();
        let updated = pointer(&reopened, b"key");
        assert_ne!(updated, ptr);
        assert_eq!(previous, vec![7; 12_000]);
        assert!((base.address..base.address + base.capacity).contains(&updated));
        drop(old);
        drop(reopened);
        let reopened = Environment::open_fixed_map(&d.0).unwrap();
        assert_eq!(pointer(&reopened, b"key"), updated);
    }
}
#[test]
fn fixed_capacity_failure_aborts_before_selection_and_nonfixed_backup_contract_stays_ordinary() {
    let _address_space = ADDRESS_SPACE.lock().unwrap();
    let d = Dir::new();
    let copy = Dir::new();
    let e = Environment::create_fixed_map(&d.0, PublicationMode::Legacy, 32 << 10, None).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"data", DatabaseKind::Kv).unwrap();
    w.put(b"data", b"key", b"A").unwrap();
    w.commit().unwrap();
    let current = fs::read(d.0.join("CURRENT")).unwrap();
    let mut w = e.begin_write().unwrap();
    w.put(b"data", b"key", &vec![5; 48_000]).unwrap();
    assert!(w.prepare().is_err());
    assert_eq!(current, fs::read(d.0.join("CURRENT")).unwrap());
    assert_eq!(
        e.begin_read().get(b"data", b"key").unwrap(),
        Some(b"A".as_slice())
    );
    ulmdb::backup::export(&e, &copy.0).unwrap();
    let c = Environment::open(&copy.0).unwrap();
    assert_eq!(
        c.begin_read().get(b"data", b"key").unwrap(),
        Some(b"A".as_slice())
    );
    assert_eq!(c.fixed_map_info().unwrap(), None);
}
#[test]
fn fixed_writable_reserve_savepoint_and_old_graph_keep_real_disjoint_addresses() {
    let _address_space = ADDRESS_SPACE.lock().unwrap();
    for mode in modes() {
        let d = Dir::new();
        let e = Environment::create_fixed_map(&d.0, mode, 2 << 20, None).unwrap();
        let fixed = e.fixed_map_info().unwrap().unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"data", DatabaseKind::Kv).unwrap();
        w.put(b"data", b"k", b"A").unwrap();
        w.commit().unwrap();
        let old = e.begin_read();
        let a = old.get(b"data", b"k").unwrap().unwrap();
        let mut w = e.begin_write().unwrap();
        w.enable_write_map(2 << 20, ulmdb::MappedFlush::Synchronous)
            .unwrap();
        let map = w.mapped_allocation().unwrap().unwrap();
        assert!((fixed.address..fixed.address + fixed.capacity).contains(&map.address));
        let cap = w.reserve(b"data", b"mapped", 48_000).unwrap();
        let bytes = w.reservation_value_mut(&cap).unwrap();
        let value = bytes.as_ptr() as usize;
        bytes.fill(0x76);
        w.end_reservation(cap).unwrap();
        {
            let mut child = w.savepoint().unwrap();
            child.put(b"data", b"k", b"discard").unwrap();
            child.abort().unwrap();
        }
        assert_eq!(w.get(b"data", b"k").unwrap(), Some(b"A".as_slice()));
        w.put(b"data", b"k", b"B").unwrap();
        w.commit().unwrap();
        assert_eq!(a, b"A");
        let read = e.begin_read();
        let b = read.get(b"data", b"mapped").unwrap().unwrap();
        assert_eq!(b, vec![0x76; 48_000]);
        assert_eq!(b.as_ptr() as usize, value);
        let selected = fs::read(d.0.join("CURRENT")).unwrap();
        let mut w = e.begin_write().unwrap();
        w.enable_write_map(4096, ulmdb::MappedFlush::Synchronous)
            .unwrap();
        assert!(w.put(b"data", b"too-big", &vec![4; 16_000]).is_err());
        drop(w);
        assert_eq!(fs::read(d.0.join("CURRENT")).unwrap(), selected);
        assert!(
            fs::read_dir(d.0.join("transactions"))
                .unwrap()
                .next()
                .is_none()
        );
        drop(read);
        drop(old);
        drop(e);
        let reopened = Environment::open_fixed_map(&d.0).unwrap();
        assert_eq!(pointer(&reopened, b"mapped"), value);
    }
}
