#![cfg(target_os = "linux")]
use std::{fs, path::PathBuf};
use ulmdb::{DatabaseKind, Environment, PublicationMode};
use ulmdb_storage::fixed_map::FixedSlotReclaimLimits;
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "fixed-reuse-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn put(env: &Environment, value: u8) {
    let mut w = env.begin_write().unwrap();
    w.put(b"db", b"key", &vec![value; 8192]).unwrap();
    w.commit().unwrap();
}
#[test]
fn reuse_keeps_old_addresses_and_named_roots_then_bounds_churn_and_reopens() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let dir = Dir::new();
        let env = Environment::create_reusable_fixed_map(&dir.0, mode, 4 << 20, None).unwrap();
        // This journey explicitly reclaims slots; automatic reclamation has its
        // own coverage in fixed_slot_automatic. Keep these removals caller-owned.
        env.set_maintenance_policy(ulmdb::MaintenancePolicy::Disabled)
            .unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"db", DatabaseKind::Kv).unwrap();
        w.put(b"db", b"key", &vec![1; 8192]).unwrap();
        w.commit().unwrap();
        let old = env.begin_read();
        let bytes = old.get(b"db", b"key").unwrap().unwrap();
        let pointer = bytes.as_ptr();
        env.retain_snapshot("old", &old).unwrap();
        for n in 2..10 {
            put(&env, n);
            env.compact().unwrap();
            env.reclaim_fixed_map_slots(Default::default()).unwrap();
            assert_eq!(bytes, &vec![1; 8192]);
            assert_eq!(bytes.as_ptr(), pointer);
        }
        drop(old);
        let named = env.open_retained("old").unwrap();
        assert_eq!(named.get(b"db", b"key").unwrap().unwrap(), vec![1; 8192]);
        drop(named);
        env.release_snapshot("old").unwrap();
        let mut removed = 0;
        for n in 10..65 {
            put(&env, n);
            env.compact().unwrap();
            removed += env
                .reclaim_fixed_map_slots(Default::default())
                .unwrap()
                .removed_slots;
        }
        assert!(removed > 50, "mode={mode:?}, removed={removed}");
        let info = env.fixed_map_info().unwrap().unwrap();
        assert!(
            info.assigned_bytes < 512 << 10,
            "assigned={}",
            info.assigned_bytes
        );
        assert!(info.segments < 64, "slots={}", info.segments);
        let current = env.begin_read();
        assert_eq!(current.get(b"db", b"key").unwrap().unwrap(), vec![64; 8192]);
        drop(current);
        drop(env);
        let env = Environment::open_fixed_map(&dir.0).unwrap();
        assert_eq!(env.fixed_map_info().unwrap().unwrap().address, info.address);
        assert_eq!(
            env.begin_read().get(b"db", b"key").unwrap().unwrap(),
            vec![64; 8192]
        );
        let previous = env.begin_read();
        let old_bytes = previous.get(b"db", b"key").unwrap().unwrap();
        let mut writer = env.begin_write().unwrap();
        writer
            .enable_write_map(1 << 20, ulmdb::MappedFlush::Synchronous)
            .unwrap();
        let reserve = writer.reserve(b"db", b"mapped", 32_000).unwrap();
        let buffer = writer.reservation_value_mut(&reserve).unwrap();
        let mapped_pointer = buffer.as_ptr();
        buffer.fill(99);
        writer.end_reservation(reserve).unwrap();
        writer.commit().unwrap();
        let read = env.begin_read();
        let value = read.get(b"db", b"mapped").unwrap().unwrap();
        assert_eq!(value, vec![99; 32_000]);
        assert_eq!(value.as_ptr(), mapped_pointer);
        env.reclaim_fixed_map_slots(Default::default()).unwrap();
        assert_eq!(old_bytes, vec![64; 8192]);
        drop(read);
        drop(previous);
        put(&env, 99);
        env.reclaim_fixed_map_slots(FixedSlotReclaimLimits {
            max_reclaimed_slots: 1,
            ..Default::default()
        })
        .unwrap();
    }
}
#[test]
fn v1_is_not_upgraded_and_v2_rejects_replaced_lease_inode() {
    let old = Dir::new();
    let env =
        Environment::create_fixed_map(&old.0, PublicationMode::Legacy, 4 << 20, None).unwrap();
    assert!(matches!(
        env.reclaim_fixed_map_slots(Default::default()),
        Err(ulmdb::Error::InvalidInput(_))
    ));
    assert_eq!(&fs::read(old.0.join("FIXEDMAP")).unwrap()[..8], b"ULMDBFM1");
    drop(env);
    let dir = Dir::new();
    let env =
        Environment::create_reusable_fixed_map(&dir.0, PublicationMode::Legacy, 4 << 20, None)
            .unwrap();
    fs::rename(dir.0.join("LOCK"), dir.0.join("old-lock")).unwrap();
    fs::write(dir.0.join("LOCK"), []).unwrap();
    assert!(matches!(
        env.fixed_map_info(),
        Err(ulmdb::Error::Corrupt(_))
    ));
    assert!(matches!(
        env.reclaim_fixed_map_slots(Default::default()),
        Err(ulmdb::Error::Corrupt(_))
    ));
}
#[test]
fn genuine_layout_export_rebinds_lease_inode_and_external_binding_rejects() {
    let source = Dir::new();
    let target = Dir::new();
    let env =
        Environment::create_reusable_fixed_map(&source.0, PublicationMode::Legacy, 4 << 20, None)
            .unwrap();
    let mut writer = env.begin_write().unwrap();
    writer.create_database(b"db", DatabaseKind::Kv).unwrap();
    writer.put(b"db", b"key", b"value").unwrap();
    writer.commit().unwrap();
    let ext = ulmdb::ExternalSynchronization::open_namespace(&source.0, None).unwrap();
    let external = ext
        .open_environment(
            &source.0,
            ulmdb::ResidencyPolicy::Demand,
            ulmdb::ReadAhead::Normal,
            None,
        )
        .unwrap();
    let fixed = ulmdb::FixedMap::open_namespace(&source.0).unwrap();
    assert!(matches!(
        external.bind_fixed_map_layout(fixed),
        Err(ulmdb::Error::InvalidInput(_))
    ));
    drop(external);
    ulmdb::backup::export(&env, &target.0).unwrap();
    let root = ulmdb_storage::read_current(&target.0).unwrap();
    ulmdb::FixedMap::export_layout(&source.0, &target.0, root.uuid, root.epoch, &root, false)
        .unwrap();
    ulmdb::FixedMap::validate_layout(&target.0, &[&root]).unwrap();
    // Same numeric address cannot be reserved twice. Genuine target metadata
    // was validated without mapping; source owner ends before fixed target open.
    drop(env);
    let copy = Environment::open_fixed_map(&target.0).unwrap();
    assert_eq!(
        copy.begin_read().get(b"db", b"key").unwrap(),
        Some(b"value".as_slice())
    );
    assert!(copy.reclaim_fixed_map_slots(Default::default()).is_ok());
}
