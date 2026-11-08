use super::*;
use crate::{Environment, MaintenancePolicy};

#[test]
fn kv_position_rejects_another_snapshot_and_retains_no_snapshot_owner() {
    let directory = std::env::temp_dir().join(format!(
        "ulmdb-read-position-{}-{}",
        std::process::id(),
        ulmdb_storage::new_identity().unwrap()
    ));
    let env = Environment::create(&directory).unwrap();
    env.set_maintenance_policy(MaintenancePolicy::Disabled)
        .unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    write.put(b"kv", b"a", b"old").unwrap();
    write.put(b"kv", b"b", b"second").unwrap();
    write.commit().unwrap();
    let read = env.begin_read();
    let handle = read.open_database(b"kv").unwrap();
    let mut position = read.kv_position(&handle).unwrap();
    assert_eq!(
        position.next(&read).unwrap(),
        Some((&b"a"[..], &b"old"[..]))
    );
    let mut write = env.begin_write().unwrap();
    write.put(b"kv", b"a", b"new").unwrap();
    write.commit().unwrap();
    let newer = env.begin_read();
    assert!(!position.matches_snapshot(&newer));
    assert!(matches!(position.next(&newer), Err(Error::StaleHandle)));
    assert_eq!(
        position.next(&read).unwrap(),
        Some((&b"b"[..], &b"second"[..]))
    );
    drop(read);
    assert!(position.snapshot.upgrade().is_none());
    drop(newer);
    drop(env);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn scoped_kv_position_rejects_other_snapshot_and_retains_no_snapshot_owner() {
    let directory = std::env::temp_dir().join(format!(
        "ulmdb-scoped-position-{}-{}", std::process::id(), ulmdb_storage::new_identity().unwrap()
    ));
    let env = Environment::create(&directory).unwrap();
    env.set_maintenance_policy(MaintenancePolicy::Disabled).unwrap();
    let mut write = env.begin_write().unwrap();
    write.create_database(b"kv", DatabaseKind::Kv).unwrap();
    write.put(b"kv", b"a", b"old").unwrap();
    write.put(b"kv", b"b", b"second").unwrap();
    write.commit().unwrap();
    let read = env.begin_read();
    let handle = read.open_database(b"kv").unwrap();
    let mut position = read.kv_position(&handle).unwrap();
    let retained = position.current_or_step(&read, Bound::Unbounded, false).unwrap().unwrap();
    assert_eq!(retained, (&b"a"[..], &b"old"[..]));
    let mut write = env.begin_write().unwrap();
    write.put(b"kv", b"a", b"new").unwrap();
    write.commit().unwrap();
    let newer = env.begin_read();
    assert!(matches!(position.current_or_step(&newer, Bound::Excluded(&b"a"[..]), false), Err(Error::StaleHandle)));
    assert_eq!(position.current_or_step(&read, Bound::Excluded(&b"a"[..]), false).unwrap(), Some((&b"b"[..], &b"second"[..])));
    assert_eq!(retained, (&b"a"[..], &b"old"[..]));
    let oversized = vec![0; read.snapshot.manifest.page_size.native_key_limit() + 1];
    assert!(matches!(position.current_or_step(&read, Bound::Included(&oversized), false), Err(Error::InvalidInput(_))));
    assert_eq!(position.current(&read).unwrap(), Some((&b"b"[..], &b"second"[..])));
    drop(read);
    assert!(position.snapshot.upgrade().is_none());
    drop(newer);
    drop(env);
    std::fs::remove_dir_all(directory).unwrap();
}
