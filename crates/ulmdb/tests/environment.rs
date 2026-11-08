use std::{
    ops::Bound,
    path::{Path, PathBuf},
};
use ulmdb::{Address, DatabaseKind, Environment, Prefix};

struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        Self(parent.join(format!(
            "ulmdb-engine-{}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn p(address: u32, length: u8) -> Prefix {
    Prefix::new(Address::V4(address), length).unwrap()
}

fn exercise(parent: &Path) {
    let path = Directory::new(parent);
    let env = Environment::create(&path.0).unwrap();
    assert!(Environment::open(&path.0).is_err());
    let old = env.begin_read();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    write.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
    write.create_database(b"v6", DatabaseKind::Ipv6).unwrap();
    for n in 0u64..1200 {
        write
            .put(b"kv", &n.to_be_bytes(), &n.wrapping_mul(17).to_be_bytes())
            .unwrap();
    }
    write.put(b"kv", b"", b"").unwrap();
    write.put(b"kv", &vec![9; 4096], &vec![7; 120000]).unwrap();
    write.insert_prefix(b"v4", p(0x0a000000, 8), b"A").unwrap();
    write.insert_prefix(b"v4", p(0x0a010000, 16), b"B").unwrap();
    write.insert_prefix(b"v4", p(0x0a010200, 24), b"C").unwrap();
    write.insert_prefix(b"v4", p(0x0a010200, 32), b"D").unwrap();
    let v6 = Prefix::new(Address::V6(0x20010db8000000000000000000000000), 32).unwrap();
    write.insert_prefix(b"v6", v6, b"six").unwrap();
    assert_eq!(write.get(b"kv", b"").unwrap(), Some(&b""[..]));
    assert_eq!(write.commit().unwrap(), 1);
    assert_eq!(old.sequence(), 0);
    assert!(old.get(b"kv", b"").is_err());
    let stable = env.begin_read();
    let borrowed = stable.get(b"kv", &10u64.to_be_bytes()).unwrap().unwrap();
    let mut write = env.begin_write().unwrap();
    write.put(b"kv", &10u64.to_be_bytes(), b"changed").unwrap();
    write.delete_prefix(b"v4", p(0x0a010200, 24)).unwrap();
    assert_eq!(write.commit().unwrap(), 2);
    assert_eq!(borrowed, &170u64.to_be_bytes());
    assert_eq!(
        stable
            .longest_match(b"v4", Address::V4(0x0a010201))
            .unwrap()
            .unwrap()
            .1,
        b"C"
    );
    let now = env.begin_read();
    assert_eq!(
        now.longest_match(b"v4", Address::V4(0x0a010201))
            .unwrap()
            .unwrap()
            .1,
        b"B"
    );
    assert_eq!(
        now.covering_prefix(b"v4", p(0x0a010200, 24))
            .unwrap()
            .unwrap()
            .1,
        b"B"
    );
    assert_eq!(
        now.longest_match(b"v4", Address::V4(0x0a010200))
            .unwrap()
            .unwrap()
            .1,
        b"D"
    );
    assert_eq!(
        now.longest_match(b"v6", Address::V6(0x20010db8000000000000000000000001))
            .unwrap()
            .unwrap()
            .1,
        b"six"
    );
    let mut abort = env.begin_write().unwrap();
    abort.delete(b"kv", &11u64.to_be_bytes()).unwrap();
    abort.abort();
    assert!(
        env.begin_read()
            .get(b"kv", &11u64.to_be_bytes())
            .unwrap()
            .is_some()
    );
    assert_eq!(
        now.range(b"kv", Bound::Unbounded, Bound::Unbounded, false)
            .unwrap()
            .collect::<ulmdb::Result<Vec<_>>>()
            .unwrap()
            .len(),
        1202
    );
    drop(env); // Old mmap borrows survive environment destruction and a later reopen.
    assert_eq!(borrowed, &170u64.to_be_bytes());
    let reopened = Environment::open(&path.0).unwrap();
    let read = reopened.begin_read();
    assert_eq!(read.sequence(), 2);
    assert_eq!(
        read.get(b"kv", &10u64.to_be_bytes()).unwrap(),
        Some(&b"changed"[..])
    );
    assert_eq!(
        read.get(b"kv", &vec![9; 4096]).unwrap().unwrap().len(),
        120000
    );
}

#[test]
fn mapped_transactions_reopen_and_old_borrows_on_disk() {
    exercise(&std::env::temp_dir());
}
#[test]
fn same_engine_on_mounted_ramdrive() {
    assert!(Path::new("/dev/shm").is_dir());
    exercise(Path::new("/dev/shm"));
}

#[test]
fn corrupted_current_and_data_fail_closed() {
    let path = Directory::new(&std::env::temp_dir());
    let env = Environment::create(&path.0).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"k", DatabaseKind::Kv).unwrap();
    w.put(b"k", b"a", b"b").unwrap();
    w.commit().unwrap();
    drop(env);
    let current = path.0.join("CURRENT");
    let mut bytes = std::fs::read(&current).unwrap();
    bytes[20] ^= 1;
    std::fs::write(&current, bytes).unwrap();
    assert!(Environment::open(&path.0).is_err());
}

#[test]
fn no_op_commits_preserve_current_and_skip_segment_publication() {
    let path = Directory::new(&std::env::temp_dir());
    let env = Environment::create(&path.0).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"k", DatabaseKind::Kv).unwrap();
    w.create_database(b"p", DatabaseKind::Ipv4).unwrap();
    w.put(b"k", b"a", b"b").unwrap();
    w.insert_prefix(b"p", p(0x0a000000, 8), b"net").unwrap();
    assert_eq!(w.commit().unwrap(), 1);
    let current = std::fs::read(path.0.join("CURRENT")).unwrap();
    let files = std::fs::read_dir(path.0.join("segments")).unwrap().count();
    let mut w = env.begin_write().unwrap();
    w.delete(b"k", b"missing").unwrap();
    w.delete_prefix(b"p", p(0x0b000000, 8)).unwrap();
    w.put(b"k", b"a", b"b").unwrap();
    w.insert_prefix(b"p", p(0x0a000000, 8), b"net").unwrap();
    assert_eq!(w.commit().unwrap(), 1);
    assert_eq!(env.begin_write().unwrap().commit().unwrap(), 1);
    assert_eq!(std::fs::read(path.0.join("CURRENT")).unwrap(), current);
    assert_eq!(
        std::fs::read_dir(path.0.join("segments")).unwrap().count(),
        files
    );
    let read = env.begin_read();
    assert_eq!(read.kv(b"k").unwrap().get(b"a").unwrap(), Some(&b"b"[..]));
    assert_eq!(
        read.prefix_index(b"p")
            .unwrap()
            .longest_match(Address::V4(0x0a000001))
            .unwrap()
            .unwrap()
            .1,
        b"net"
    );
}

#[test]
fn sealed_segment_corruption_is_not_silently_rolled_back() {
    use std::io::{Seek, SeekFrom, Write};
    let path = Directory::new(&std::env::temp_dir());
    let env = Environment::create(&path.0).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database(b"k", DatabaseKind::Kv).unwrap();
    w.put(b"k", b"key", b"value").unwrap();
    w.commit().unwrap();
    drop(env);
    let segment = std::fs::read_dir(path.0.join("segments"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&segment, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(segment)
        .unwrap();
    file.seek(SeekFrom::Start(4100)).unwrap();
    file.write_all(&[0x7f]).unwrap();
    file.sync_all().unwrap();
    assert!(Environment::open(&path.0).is_err());
}
