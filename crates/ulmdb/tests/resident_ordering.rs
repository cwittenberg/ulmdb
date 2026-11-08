use std::{fs, ops::Bound, path::PathBuf, sync::Arc};
use ulmdb::{
    DatabaseKind, Error,
    ordering::{DatabaseComparators, DatabaseOrdering, Endian, OrderingSpec, Width},
    resident::{Environment, IncrementalLimits, Options, ReadTxn, RedoEncoding, RedoFormat},
};
use ulmdb_storage::resident::Config;
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "resident-order-{:032x}",
            ulmdb_storage::new_identity().unwrap()
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn options() -> Options {
    Options {
        pool: Config {
            kv_pages: 8192,
            tiny_pages: 128,
            max_reserved_bytes: 128 << 20,
            max_metadata_bytes: 64 << 20,
            max_views: 128,
        },
        wal_capacity: 32 << 20,
        graph_read_budget: 16_000_000,
        auto_collect_below: 32,
    }
}
fn integer() -> OrderingSpec {
    OrderingSpec::UnsignedInteger {
        width: Width::U32,
        endian: Endian::Little,
    }
}
fn callbacks() -> DatabaseComparators {
    DatabaseComparators {
        keys: Some(Arc::new(|a, b| {
            a.iter()
                .map(u8::to_ascii_lowercase)
                .cmp(b.iter().map(u8::to_ascii_lowercase))
        })),
        duplicates: Some(Arc::new(|a, b| b.cmp(a))),
    }
}
fn verify(r: &ReadTxn, cb: &DatabaseComparators) {
    assert_eq!(r.database_ordering(b"integer").unwrap().keys, integer());
    assert_eq!(
        r.get(b"integer", &257u32.to_le_bytes()).unwrap(),
        Some(257u32.to_be_bytes().as_slice())
    );
    let keys = r
        .kv(b"integer")
        .unwrap()
        .cursor(
            Bound::Included(254u32.to_le_bytes().as_slice()),
            Bound::Included(258u32.to_le_bytes().as_slice()),
            false,
        )
        .unwrap()
        .map(|p| u32::from_le_bytes(p.unwrap().0.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(keys, vec![254, 255, 256, 257, 258]);
    let reverse = r
        .kv(b"reverse")
        .unwrap()
        .cursor(Bound::Unbounded, Bound::Unbounded, false)
        .unwrap()
        .map(|p| p.unwrap().0.to_vec())
        .collect::<Vec<_>>();
    assert_eq!(
        reverse,
        vec![b"za".to_vec(), b"ab".to_vec(), b"ac".to_vec()]
    );
    assert!(r.get(b"custom", b"Alpha").is_err());
    assert_eq!(
        r.get_with_ordering(b"custom", b"ALPHA", cb).unwrap(),
        Some(b"two".as_slice())
    );
    let vals = r
        .duplicates(b"dups")
        .unwrap()
        .values(
            &1u32.to_le_bytes(),
            Bound::Unbounded,
            Bound::Unbounded,
            false,
        )
        .unwrap()
        .map(|v| u32::from_le_bytes(v.unwrap().try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(vals, (0u32..40).collect::<Vec<_>>());

    for n in [0u32, 257, 699] {
        let key = format!("MIXED{:04}", n);
        assert_eq!(
            r.get_with_ordering(b"custom", key.as_bytes(), cb).unwrap(),
            Some(n.to_be_bytes().as_slice())
        );
    }
    let many = r.duplicates_with_ordering(b"custom-dups", cb).unwrap();
    assert_eq!(many.count(b"MANY").unwrap(), 32);
    assert_eq!(
        many.first(b"MANY").unwrap(),
        Some(31u32.to_be_bytes().as_slice())
    );
    let vals = r
        .duplicates_with_ordering(b"custom-dups", cb)
        .unwrap()
        .values(b"K", Bound::Unbounded, Bound::Unbounded, false)
        .unwrap()
        .map(|v| v.unwrap().to_vec())
        .collect::<Vec<_>>();
    assert_eq!(vals, vec![b"z".to_vec(), b"m".to_vec(), b"a".to_vec()]);
}
#[test]
fn builtin_custom_private_mutation_savepoints_cold_collection_checkpoint_and_export() {
    for (format, encoding) in [
        (RedoFormat::LiteralV1, RedoEncoding::Owned),
        (RedoFormat::CompactV2, RedoEncoding::Owned),
        (RedoFormat::FullPostimageV2, RedoEncoding::Owned),
        (RedoFormat::CompactV2, RedoEncoding::Borrowed),
    ] {
        let dir = Dir::new();
        let exported = Dir::new();
        let o = options();
        let env = Environment::create_with_encoding(&dir.0, o, format, encoding).unwrap();
        let cb = callbacks();
        let mut w = env.begin_write().unwrap();
        for (name, kind, order) in [
            (
                b"integer".as_slice(),
                DatabaseKind::Kv,
                DatabaseOrdering {
                    keys: integer(),
                    duplicates: OrderingSpec::Bytewise,
                },
            ),
            (
                b"reverse",
                DatabaseKind::Kv,
                DatabaseOrdering {
                    keys: OrderingSpec::ReverseBytes,
                    duplicates: OrderingSpec::Bytewise,
                },
            ),
            (
                b"custom",
                DatabaseKind::Kv,
                DatabaseOrdering {
                    keys: OrderingSpec::Custom,
                    duplicates: OrderingSpec::Bytewise,
                },
            ),
            (
                b"dups",
                DatabaseKind::Duplicates { width: Some(4) },
                DatabaseOrdering {
                    keys: integer(),
                    duplicates: integer(),
                },
            ),
            (
                b"custom-dups",
                DatabaseKind::Duplicates { width: None },
                DatabaseOrdering {
                    keys: OrderingSpec::Custom,
                    duplicates: OrderingSpec::Custom,
                },
            ),
        ] {
            w.create_database_with_ordering(name, kind, order).unwrap();
        }
        // Invalid typed input is recoverable, before any private mutation.
        assert!(w.put(b"integer", b"bad", b"x").is_err());
        for n in (0u32..700).rev() {
            w.put(b"integer", &n.to_le_bytes(), &n.to_be_bytes())
                .unwrap();
        }
        for k in [b"za", b"ab", b"ac"] {
            w.put(b"reverse", k, k).unwrap();
        }
        w.put_with_ordering(b"custom", b"Alpha", b"one", &cb)
            .unwrap();
        w.put_with_ordering(b"custom", b"ALPHA", b"two", &cb)
            .unwrap();
        for n in (0u32..700).rev() {
            let key = format!("{}{:04}", if n % 2 == 0 { "MiXeD" } else { "mixed" }, n);
            w.put_with_ordering(b"custom", key.as_bytes(), &n.to_be_bytes(), &cb)
                .unwrap();
        }
        for n in 0u32..32 {
            w.insert_duplicate_with_ordering(b"custom-dups", b"many", &n.to_be_bytes(), &cb)
                .unwrap();
        }
        for n in (0u32..40).rev() {
            w.insert_duplicate(b"dups", &1u32.to_le_bytes(), &n.to_le_bytes())
                .unwrap();
        }
        for v in [b"a", b"z", b"m"] {
            w.insert_duplicate_with_ordering(b"custom-dups", b"k", v, &cb)
                .unwrap();
        }
        {
            let mut child = w.savepoint().unwrap();
            child
                .put(b"integer", &257u32.to_le_bytes(), b"child")
                .unwrap();
            child
                .delete_with_ordering(b"custom", b"alpha", &cb)
                .unwrap();
            child.abort().unwrap();
        }
        assert_eq!(
            w.get_with_ordering(b"custom", b"alpha", &cb).unwrap(),
            Some(b"two".as_slice())
        );
        let custom_rows = w
            .kv_cursor_with_ordering(
                b"custom",
                Bound::Included(b"ALPHA"),
                Bound::Included(b"alpha"),
                false,
                &cb,
            )
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(custom_rows.len(), 1);
        assert!(
            w.set_database_ordering(b"integer", Default::default())
                .is_err()
        );
        w.commit().unwrap();
        verify(&env.begin_read(), &cb);
        let old = env.begin_read();
        let borrow = old.get(b"integer", &257u32.to_le_bytes()).unwrap().unwrap();
        if format != RedoFormat::LiteralV1 {
            env.enable_incremental(IncrementalLimits::default())
                .unwrap();
        }
        let mut w = env.begin_write().unwrap();
        assert!(w.delete(b"integer", &699u32.to_le_bytes()).unwrap());
        assert!(
            w.delete_duplicate(b"dups", &1u32.to_le_bytes(), &39u32.to_le_bytes())
                .unwrap()
        );
        w.insert_duplicate(b"dups", &1u32.to_le_bytes(), &39u32.to_le_bytes())
            .unwrap();
        w.commit().unwrap();
        assert_eq!(borrow, 257u32.to_be_bytes());
        env.collect().unwrap();
        env.checkpoint().unwrap();
        verify(&env.begin_read(), &cb);
        env.enable_readonly_capture().unwrap();
        let captured = ReadTxn::open_coordinated_snapshot(&dir.0, o).unwrap();
        verify(&captured, &cb);
        env.export_snapshot(&exported.0).unwrap();
        drop(captured);
        drop(old);
        drop(env);
        let reopened = Environment::open(&dir.0, o).unwrap();
        verify(&reopened.begin_read(), &cb);
        drop(reopened);
        let copied = Environment::open(&exported.0, o).unwrap();
        verify(&copied.begin_read(), &cb);
    }
}
#[test]
fn ordered_overflow_16mib_and_wide_65536_duplicates_survive_savepoint_and_reopen() {
    let dir = Dir::new();
    let o = options();
    let env = Environment::create_with_redo(&dir.0, o, RedoFormat::CompactV2).unwrap();
    let mut w = env.begin_write().unwrap();
    w.create_database_with_ordering(
        b"large",
        DatabaseKind::Kv,
        DatabaseOrdering {
            keys: OrderingSpec::ReverseBytes,
            duplicates: OrderingSpec::Bytewise,
        },
    )
    .unwrap();
    w.create_database_with_ordering(
        b"wide",
        DatabaseKind::Duplicates { width: Some(65536) },
        DatabaseOrdering {
            keys: OrderingSpec::ReverseBytes,
            duplicates: OrderingSpec::ReverseBytes,
        },
    )
    .unwrap();
    let huge = vec![0x69; 16 << 20];
    w.put(b"large", b"key", &huge).unwrap();
    let a = vec![0x31; 65536];
    let b = vec![0x32; 65536];
    w.insert_duplicate(b"wide", b"key", &a).unwrap();
    {
        let mut c = w.savepoint().unwrap();
        c.put(b"large", b"key", b"undo").unwrap();
        c.insert_duplicate(b"wide", b"key", &b).unwrap();
        c.abort().unwrap();
    }
    assert_eq!(w.get(b"large", b"key").unwrap(), Some(huge.as_slice()));
    assert_eq!(w.duplicate_count(b"wide", b"key").unwrap(), 1);
    w.commit().unwrap();
    env.collect().unwrap();
    env.checkpoint().unwrap();
    drop(env);
    let env = Environment::open(&dir.0, o).unwrap();
    let r = env.begin_read();
    assert_eq!(r.get(b"large", b"key").unwrap(), Some(huge.as_slice()));
    assert_eq!(
        r.duplicate_first(b"wide", b"key").unwrap(),
        Some(a.as_slice())
    );
}
#[test]
fn invalid_order_kind_width_and_unregistered_callbacks_do_not_change_valid_writer() {
    let dir = Dir::new();
    let env = Environment::create(&dir.0, options()).unwrap();
    let mut w = env.begin_write().unwrap();
    assert!(
        w.create_database_with_ordering(
            b"bad",
            DatabaseKind::Ipv4,
            DatabaseOrdering {
                keys: integer(),
                duplicates: OrderingSpec::Bytewise
            }
        )
        .is_err()
    );
    assert!(matches!(
        w.database_ordering(b"bad"),
        Err(Error::MissingDatabase)
    ));
    assert!(
        w.create_database_with_ordering(
            b"bad",
            DatabaseKind::Duplicates { width: Some(8) },
            DatabaseOrdering {
                keys: OrderingSpec::Bytewise,
                duplicates: integer()
            }
        )
        .is_err()
    );
    w.create_database_with_ordering(
        b"custom",
        DatabaseKind::Kv,
        DatabaseOrdering {
            keys: OrderingSpec::Custom,
            duplicates: OrderingSpec::Bytewise,
        },
    )
    .unwrap();
    assert!(w.put(b"custom", b"x", b"bad").is_err());
    w.put_with_ordering(b"custom", b"x", b"ok", &callbacks())
        .unwrap();
    w.create_database(b"empty", DatabaseKind::Kv).unwrap();
    {
        let mut child = w.savepoint().unwrap();
        child
            .set_database_ordering(
                b"empty",
                DatabaseOrdering {
                    keys: integer(),
                    duplicates: OrderingSpec::Bytewise,
                },
            )
            .unwrap();
        child.abort().unwrap();
    }
    assert_eq!(
        w.database_ordering(b"empty").unwrap(),
        DatabaseOrdering::default()
    );
    {
        let mut child = w.savepoint().unwrap();
        child
            .set_database_ordering(
                b"empty",
                DatabaseOrdering {
                    keys: integer(),
                    duplicates: OrderingSpec::Bytewise,
                },
            )
            .unwrap();
        child.release().unwrap();
    }
    w.commit().unwrap();
    assert_eq!(
        env.begin_read().database_ordering(b"empty").unwrap().keys,
        integer()
    );
}
