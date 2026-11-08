#![cfg(target_os = "linux")]
use std::{
    fs,
    ops::Bound,
    path::{Path, PathBuf},
};
use ulmdb::{
    DatabaseKind, Durability, Environment, Error, PageSize, PublicationMode, ReadAhead, ReadTxn,
    ResidencyPolicy,
};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "external-sync-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn create(p: &Path, m: PublicationMode) -> Environment {
    Environment::create_externally_synchronized(
        p,
        m,
        PageSize::DEFAULT,
        ResidencyPolicy::Demand,
        ReadAhead::Normal,
        None,
        Default::default(),
    )
    .unwrap()
}
fn absent(p: &Path) {
    assert!(!p.join(".ulmdb-coordination").exists());
    for n in [
        "LMDB-WRITER",
        "LMDB-ENV",
        "LMDB-ENV-GATE",
        "LMDB-READERS",
        "lmdb-readers",
        "retained",
    ] {
        assert!(!p.join(n).exists(), "unexpected {n}");
    }
}
fn journey(e: &Environment, p: &Path) {
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"k", b"A").unwrap();
    w.commit().unwrap();
    let old = e.begin_read();
    let bytes = old.get(b"kv", b"k").unwrap().unwrap();
    let mut cursor = old
        .range(b"kv", Bound::Unbounded, Bound::Unbounded, false)
        .unwrap();
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", b"k", b"discard").unwrap();
    w.abort();
    assert_eq!(
        e.begin_read().get(b"kv", b"k").unwrap(),
        Some(b"A".as_slice())
    );
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", b"k", b"B").unwrap();
    let prepared = w.prepare().unwrap();
    assert_eq!(
        ReadTxn::open_externally_synchronized(p)
            .unwrap()
            .get(b"kv", b"k")
            .unwrap(),
        Some(b"A".as_slice())
    );
    prepared.commit().unwrap();
    assert_eq!(bytes, b"A");
    assert_eq!(cursor.next().unwrap().unwrap().1, b"A");
    assert_eq!(
        e.begin_read().get(b"kv", b"k").unwrap(),
        Some(b"B".as_slice())
    );
    e.set_durability(Durability::NoSync).unwrap();
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", b"k", b"C").unwrap();
    w.commit().unwrap();
    e.set_durability(Durability::Full).unwrap();
    let mut w = e.begin_write().unwrap();
    w.put(b"kv", b"k", b"D").unwrap();
    w.commit().unwrap();
    e.sync().unwrap();
    absent(p);
}
#[test]
fn file_modes_create_read_prepare_abort_repair_and_reopen_without_coordination() {
    for m in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let p = Dir::new();
        let e = create(&p.0, m);
        journey(&e, &p.0);
        drop(e);
        let e = Environment::open_externally_synchronized(&p.0).unwrap();
        assert_eq!(
            e.begin_read().get(b"kv", b"k").unwrap(),
            Some(b"D".as_slice())
        );
        absent(&p.0);
    }
}
#[test]
fn stale_base_and_unsupported_maintenance_fail_before_side_effects() {
    let p = Dir::new();
    let copy = Dir::new();
    let e = create(&p.0, PublicationMode::Legacy);
    let stale = Environment::open_externally_synchronized(&p.0).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    w.put(b"kv", b"k", b"A").unwrap();
    w.commit().unwrap();
    assert!(matches!(stale.begin_write(), Err(Error::Busy)));
    assert!(e.compact().is_err());
    assert!(e.reclaim().is_err());
    assert!(e.rollback_last_commit(1).is_err());
    assert!(ulmdb::backup::export(&e, &copy.0).is_err());
    assert!(!copy.0.exists());
    absent(&p.0);
}
#[test]
fn bound_namespace_rejects_rename_and_foreign_realm() {
    let p = Dir::new();
    let e = create(&p.0, PublicationMode::Legacy);
    let read = e.begin_read();
    let authority = read.external_synchronization().unwrap();
    let moved = p.0.with_extension("old");
    fs::rename(&p.0, &moved).unwrap();
    let replacement = create(&p.0, PublicationMode::Legacy);
    assert!(authority.begin_read(&p.0, ReadAhead::Normal, None).is_err());
    assert!(e.begin_write().is_err());
    drop(replacement);
    drop(e);
    fs::remove_dir_all(&moved).unwrap();
}
#[test]
fn syscall_worker() {
    let Some(path) = std::env::var_os("EXTERNAL_SYSCALL_PATH") else {
        return;
    };
    let p = PathBuf::from(path);
    let normal = std::env::var_os("EXTERNAL_NORMAL_CONTROL").is_some();
    if normal {
        let e = Environment::create(&p).unwrap();
        let mut w = e.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"k", b"control").unwrap();
        w.commit().unwrap();
    } else {
        let e = create(&p, PublicationMode::Legacy);
        journey(&e, &p);
    }
}

#[test]
fn same_directory_inode_with_foreign_current_is_rejected() {
    let p = Dir::new();
    let foreign = Dir::new();
    let e = create(&p.0, PublicationMode::Legacy);
    let authority = e.begin_read().external_synchronization().unwrap();
    let other = create(&foreign.0, PublicationMode::Legacy);
    drop(other);
    for entry in fs::read_dir(foreign.0.join("commits")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), p.0.join("commits").join(entry.file_name())).unwrap();
    }
    fs::copy(foreign.0.join("CURRENT"), p.0.join("CURRENT")).unwrap();
    assert!(matches!(
        authority.begin_read(&p.0, ReadAhead::Normal, None),
        Err(Error::Corrupt(_))
    ));
    assert!(matches!(e.begin_write(), Err(Error::Corrupt(_))));
    absent(&p.0);
}
