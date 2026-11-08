use ulmdb::{DatabaseKind, Environment, MappedFlush, PublicationMode, ValueLimit};

#[test]
fn maximum_profile_is_explicit_and_max_plus_one_rejects_without_allocation() {
    let path = std::env::temp_dir().join(format!(
        "max-profile-{}",
        ulmdb_storage::new_identity().unwrap()
    ));
    let e = Environment::create_with_publication(&path, PublicationMode::Legacy).unwrap();
    let mut w = e.begin_write().unwrap();
    w.create_database(b"kv", DatabaseKind::Kv).unwrap();
    assert_eq!(w.value_limit(), ValueLimit::DEFAULT);
    #[cfg(target_pointer_width = "64")]
    {
        assert!(w.reserve(b"kv", b"x", ValueLimit::LARGE.get() + 1).is_err());
        let before = w.dirty_bytes();
        {
            let mut c = w.savepoint().unwrap();
            c.set_value_limit(ValueLimit::MAXIMUM).unwrap();
            assert!(
                c.reserve(b"kv", b"x", ValueLimit::MAXIMUM.get() + 1)
                    .is_err()
            );
            assert_eq!(c.dirty_bytes(), before);
            c.abort().unwrap();
        }
        assert_eq!(w.value_limit(), ValueLimit::DEFAULT);
        w.set_value_limit(ValueLimit::MAXIMUM).unwrap();
        assert!(w.reserve(b"kv", b"x", usize::MAX).is_err());
        assert_eq!(w.dirty_bytes(), before);
    }
    #[cfg(target_pointer_width = "32")]
    assert!(w.set_value_limit(ValueLimit::MAXIMUM).is_err());
    w.put(b"kv", b"small", b"ok").unwrap();
    w.commit().unwrap();
    drop(e);
    std::fs::remove_dir_all(path).unwrap();
}

/// One owned resource worker, explicitly invoked after compilation by the
/// bounded parent. It reserves real mapped bytes, never an input-sizedVec.
#[cfg(target_pointer_width = "64")]
#[test]
#[ignore = "opt-in sequential >2GiB and UINT32MAX mapped file-value worker"]
fn maximum_values_resource_worker() {
    let path = std::path::PathBuf::from(
        std::env::var_os("ULMDB_MAX_VALUE_WORKDIR").expect("owned worker path"),
    );
    assert!(!path.exists());
    let e = Environment::create_with_publication(&path, PublicationMode::Legacy).unwrap();
    {
        let mut w = e.begin_write().unwrap();
        w.create_database(b"kv", DatabaseKind::Kv).unwrap();
        w.put(b"kv", b"small", b"A").unwrap();
        w.commit().unwrap();
    }
    let mut old = None;
    for (index, length) in [(2u64, (2usize << 30) + 1), (3, ValueLimit::MAXIMUM.get())] {
        eprintln!("MAX_PHASE reserve-start sequence={index} len={length}");
        let mut w = e.begin_write().unwrap();
        w.enable_write_map(
            ValueLimit::MAXIMUM.segment_bytes(),
            MappedFlush::Synchronous,
        )
        .unwrap();
        w.set_value_limit(ValueLimit::MAXIMUM).unwrap();
        let info = w.mapped_allocation().unwrap().unwrap();
        let before = w.dirty_bytes();
        assert!(
            w.reserve(b"kv", b"invalid", ValueLimit::MAXIMUM.get() + 1)
                .is_err()
        );
        assert_eq!(before, w.dirty_bytes());
        let cap = w.reserve(b"kv", b"large", length).unwrap();
        let bytes = w.reservation_value_mut(&cap).unwrap();
        assert_eq!((bytes[0], bytes[length / 2], bytes[length - 1]), (0, 0, 0));
        bytes[0] = index as u8;
        bytes[length / 2] = 0x5a;
        bytes[length - 1] = 0xa5;
        let address = bytes.as_ptr() as usize;
        assert!((info.address..info.address + info.capacity).contains(&address));
        assert!(address.checked_add(length).unwrap() <= info.address + info.capacity);
        assert_eq!(
            w.get(b"kv", b"large").unwrap().unwrap().as_ptr() as usize,
            address
        );
        w.end_reservation(cap).unwrap();
        eprintln!("MAX_PHASE commit-start sequence={index}");
        assert_eq!(w.commit().unwrap(), index);
        let r = e.begin_read();
        let b = r.get(b"kv", b"large").unwrap().unwrap();
        assert_eq!(
            (b.len(), b[0], b[length / 2], b[length - 1]),
            (length, index as u8, 0x5a, 0xa5)
        );
        assert!(
            r.file_manifest()
                .unwrap()
                .segments
                .iter()
                .any(|s| s.length > 1 << 30)
        );
        if let Some(prior) = &old {
            let p: &ulmdb::ReadTxn = prior;
            let b = p.get(b"kv", b"large").unwrap().unwrap();
            assert_eq!(
                (b.len(), b[0], b[b.len() - 1]),
                ((2usize << 30) + 1, 2, 0xa5)
            );
        }
        if index == 2 {
            old = Some(r);
        }
        eprintln!("MAX_PHASE committed sequence={index}");
    }
    drop(old);
    drop(e);
    eprintln!("MAX_PHASE cold-open");
    let e = Environment::open(&path).unwrap();
    let r = e.begin_read();
    let b = r.get(b"kv", b"large").unwrap().unwrap();
    assert_eq!(
        (b.len(), b[0], b[b.len() / 2], b[b.len() - 1]),
        (ValueLimit::MAXIMUM.get(), 3, 0x5a, 0xa5)
    );
    assert_eq!(r.file_manifest().unwrap().value_limit, ValueLimit::MAXIMUM);
    assert_eq!(r.get(b"kv", b"small").unwrap(), Some(b"A".as_slice()));
    ulmdb::backup::verify(&e).unwrap();
    println!(
        "MAXIMUM_VALUE_PASS bytes={} sequence={}",
        b.len(),
        r.file_manifest().unwrap().sequence
    );
}
