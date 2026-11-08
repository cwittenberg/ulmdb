use std::{fs, io::Cursor, path::PathBuf};
use ulmdb::{DatabaseKind, Environment, PageSize, PublicationMode, backup};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "incremental-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
#[test]
fn nondefault_page_geometry_survives_incremental_restore() {
    for size in [256, 8192, 65536] {
        let source = Dir::new();
        let destination = Dir::new();
        let env = Environment::create_with_page_size(
            &source.0,
            PublicationMode::Indexed,
            PageSize::new(size).unwrap(),
        )
        .unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        for index in 0..1024u32 {
            w.put(b"kv", &index.to_be_bytes(), &[7; 96]).unwrap();
        }
        w.put(b"kv", b"key", b"before").unwrap();
        w.commit().unwrap();
        backup::export(&env, &destination.0).unwrap();
        let mut w = env.begin_write().unwrap();
        for index in 0..512u32 {
            w.put(b"kv", &index.to_be_bytes(), &[9; 96]).unwrap();
        }
        w.put(b"kv", b"key", b"after").unwrap();
        w.commit().unwrap();
        let mut bytes = Vec::new();
        env.dump_incremental(1, &mut bytes).unwrap();
        let target = Environment::open(&destination.0).unwrap();
        target.load_incremental(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(
            target.begin_read().get(b"kv", b"key").unwrap(),
            Some(b"after".as_slice())
        );
        assert_eq!(
            target.begin_read().file_manifest().unwrap().page_size.get(),
            size
        );
    }
}

#[test]
fn custom_order_and_large_keys_survive_incremental_restore_without_global_callbacks() {
    use std::sync::Arc;
    use ulmdb::ordering::{DatabaseComparators, DatabaseOrdering, OrderingSpec};
    let source = Dir::new();
    let destination = Dir::new();
    let env = Environment::create_with_page_size(
        &source.0,
        PublicationMode::Indexed,
        PageSize::new(65536).unwrap(),
    )
    .unwrap();
    let callbacks = DatabaseComparators {
        keys: Some(Arc::new(|a, b| {
            a.iter()
                .map(u8::to_ascii_lowercase)
                .cmp(b.iter().map(u8::to_ascii_lowercase))
        })),
        duplicates: None,
    };
    let mut w = env.begin_write().unwrap();
    w.create_database_ordered(
        b"custom",
        DatabaseKind::Kv,
        DatabaseOrdering {
            keys: OrderingSpec::Custom,
            duplicates: OrderingSpec::Bytewise,
        },
    )
    .unwrap();
    let lower = vec![b'a'; 6000];
    let upper = vec![b'A'; 6000];
    w.put_with_ordering(b"custom", &lower, b"before", &callbacks)
        .unwrap();
    w.commit().unwrap();
    backup::export(&env, &destination.0).unwrap();
    let mut w = env.begin_write().unwrap();
    w.put_with_ordering(b"custom", &upper, b"after", &callbacks)
        .unwrap();
    w.commit().unwrap();
    let mut bytes = Vec::new();
    env.dump_incremental(1, &mut bytes).unwrap();
    let target = Environment::open(&destination.0).unwrap();
    target.load_incremental(&mut Cursor::new(bytes)).unwrap();
    assert_eq!(
        target
            .begin_read()
            .kv_with_ordering(b"custom", &callbacks)
            .unwrap()
            .get(&upper)
            .unwrap(),
        Some(b"after".as_slice())
    );
    assert_eq!(
        target
            .begin_read()
            .structural_pairs(b"custom")
            .unwrap()
            .count(),
        1
    );
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn multi_commit_deltas_preserve_old_reads_duplicates_and_reject_damage_and_replay() {
    for mode in [
        PublicationMode::Legacy,
        PublicationMode::Cohort,
        PublicationMode::Indexed,
    ] {
        let source = Dir::new();
        let restored = Dir::new();
        let env = Environment::create_with_publication(&source.0, mode).unwrap();
        let mut w = env.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.create_database(b"dups", DatabaseKind::Duplicates { width: None })
            .unwrap();
        for i in 0..1000u32 {
            w.put(b"kv", &i.to_be_bytes(), &vec![7; 64]).unwrap();
        }
        w.insert_duplicate(b"dups", b"key", b"old").unwrap();
        w.commit().unwrap();
        backup::export(&env, &restored.0).unwrap();
        let target = Environment::open(&restored.0).unwrap();
        let old = target.begin_read();
        for value in [8, 9] {
            let mut w = env.begin_write().unwrap();
            w.put(b"kv", &0u32.to_be_bytes(), &[value; 64]).unwrap();
            w.delete(b"kv", &1u32.to_be_bytes()).unwrap();
            w.insert_duplicate(b"dups", b"key", &[value]).unwrap();
            w.commit().unwrap();
        }
        let mut delta = Vec::new();
        let report = env.dump_incremental(1, &mut delta).unwrap();
        assert_eq!(report.sequence, 3);
        assert!(report.files > 0);
        assert!(report.payload_bytes < env.stats().segment_bytes);
        let before = target.file_checkpoint().unwrap();
        let mut damaged = delta.clone();
        let last = damaged.len() - 1;
        damaged[last] ^= 1;
        assert!(target.load_incremental(&mut Cursor::new(damaged)).is_err());
        assert_eq!(target.file_checkpoint().unwrap(), before);
        target.load_incremental(&mut Cursor::new(&delta)).unwrap();
        assert_eq!(
            target.file_checkpoint().unwrap(),
            env.file_checkpoint().unwrap()
        );
        let read = target.begin_read();
        assert_eq!(
            read.get(b"kv", &0u32.to_be_bytes()).unwrap(),
            Some([9; 64].as_slice())
        );
        assert!(read.get(b"kv", &1u32.to_be_bytes()).unwrap().is_none());
        assert_eq!(read.duplicates(b"dups").unwrap().count(b"key").unwrap(), 3);
        assert_eq!(
            old.get(b"kv", &0u32.to_be_bytes()).unwrap(),
            Some([7; 64].as_slice())
        );
        assert!(target.load_incremental(&mut Cursor::new(&delta)).is_err());
        assert_eq!(
            target.file_checkpoint().unwrap(),
            env.file_checkpoint().unwrap()
        );
        let mut empty = Vec::new();
        env.dump_incremental(3, &mut empty).unwrap();
        assert!(empty.is_empty());
        target.load_incremental(&mut Cursor::new(empty)).unwrap();
        drop(read);
        drop(old);
        drop(target);
        let target = Environment::open(&restored.0).unwrap();
        assert_eq!(target.begin_read().sequence(), 3);
        backup::verify(&target).unwrap();
    }
}
