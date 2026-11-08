use super::support::*;
use crate::hybrid::{codec, journal::Journal, redo::*};
use crate::{ReadStore, Reference};
use ulmdb_core::{Address, Prefix};
fn prefix(v: u32) -> Prefix {
    Prefix::normalize(Address::V4(v), 24).unwrap()
}
fn ipv6(v: u128) -> Prefix {
    Prefix::normalize(Address::V6(v), 64).unwrap()
}
#[test]
fn real_kv_ipv4_ipv6_pages_compact_replay_and_fresh_image() {
    let temp = Temp::new(std::path::Path::new("/tmp"), "indexes");
    let mut journal = Journal::create(&temp.wal(), HEADER).unwrap();
    let mut store = Store::new();
    let mut kv = Reference::NULL;
    let mut v4 = Reference::NULL;
    let mut v6 = Reference::NULL;
    for i in 0..256u64 {
        kv = ulmdb_kv::put(&mut store, kv, &i.to_be_bytes(), &i.to_le_bytes()).unwrap();
    }
    for i in 0..32u32 {
        v4 = ulmdb_prefix::insert(
            &mut store,
            v4,
            prefix(0x0a000000 + i * 256),
            &i.to_le_bytes(),
        )
        .unwrap();
        v6 = ulmdb_prefix::insert(
            &mut store,
            v6,
            ipv6(0x20010db8000000000000000000000000u128 + ((i as u128) << 64)),
            &i.to_le_bytes(),
        )
        .unwrap();
    }
    let mut catalog = Reference::NULL;
    for (name, root) in [
        (b"kv".as_slice(), kv),
        (b"v4".as_slice(), v4),
        (b"v6".as_slice(), v6),
    ] {
        catalog = ulmdb_kv::put(&mut store, catalog, name, &root.encode()).unwrap();
    }
    let mut sources = Registry::default();
    let first = store.record(&journal, catalog, &sources, false);
    verify_materialized(&first, &sources, &store).unwrap();
    let staged = replay(&first, &sources).unwrap();
    let receipt = journal
        .commit_checked(&first, &sources, |_| Ok(()))
        .unwrap();
    sources.register(&first, receipt, staged);
    store.freeze();
    let old = store.clone();
    let old_kv = kv;
    for i in [1u64, 7, 200] {
        kv = ulmdb_kv::put(&mut store, kv, &i.to_be_bytes(), &(i + 1000).to_le_bytes()).unwrap();
    }
    v4 = ulmdb_prefix::insert(&mut store, v4, prefix(0x0a000700), &777u32.to_le_bytes()).unwrap();
    v6 = ulmdb_prefix::insert(
        &mut store,
        v6,
        ipv6(0x20010db8000000070000000000000000),
        &888u32.to_le_bytes(),
    )
    .unwrap();
    for (name, root) in [
        (b"kv".as_slice(), kv),
        (b"v4".as_slice(), v4),
        (b"v6".as_slice(), v6),
    ] {
        catalog = ulmdb_kv::put(&mut store, catalog, name, &root.encode()).unwrap();
    }
    let compact = store.record(&journal, catalog, &sources, true);
    let full = store.record(&journal, catalog, &sources, false);
    verify_materialized(&compact, &sources, &store).unwrap();
    let compact_bytes = codec::encode(&compact).unwrap().bytes.len();
    let full_bytes = codec::encode(&full).unwrap().bytes.len();
    assert!(
        compact_bytes * 2 < full_bytes,
        "compact{compact_bytes} full{full_bytes}"
    );
    println!(
        "actual encoded KV/prefix objects: compact={compact_bytes} full={full_bytes} objects={}",
        compact.objects.len()
    );
    let staged = replay(&compact, &sources).unwrap();
    let receipt = journal
        .commit_checked(&compact, &sources, |_| Ok(()))
        .unwrap();
    sources.register(&compact, receipt, staged);
    drop(journal);
    let (_journal, rebuilt) = Journal::open_checked(&temp.wal(), |plan| {
        let mut registry = Registry::default();
        let mut image = Store::new();
        for decoded in &plan.records {
            let objects = replay(&decoded.record, &registry)?;
            for object in &objects {
                let end = object.extent.end()? as usize;
                image.bytes.resize(image.bytes.len().max(end), 0);
                let start = object.extent.at.offset as usize;
                image.bytes[start..end].copy_from_slice(&object.bytes);
            }
            registry.register(&decoded.record, decoded.receipt, objects);
        }
        Ok(image)
    })
    .unwrap();
    for i in 0..256u64 {
        let actual = ulmdb_kv::get(&rebuilt, kv, &i.to_be_bytes())
            .unwrap()
            .unwrap();
        let expected = if [1, 7, 200].contains(&i) {
            i + 1000
        } else {
            i
        };
        assert_eq!(actual, expected.to_le_bytes());
        assert_eq!(
            ulmdb_kv::get(&old, old_kv, &i.to_be_bytes())
                .unwrap()
                .unwrap(),
            i.to_le_bytes()
        );
    }
    assert_eq!(
        ulmdb_prefix::get(&rebuilt, v4, Address::V4(0x0a000701))
            .unwrap()
            .unwrap()
            .1,
        777u32.to_le_bytes()
    );
    assert_eq!(
        ulmdb_prefix::get(
            &rebuilt,
            v6,
            Address::V6(0x20010db8000000070000000000000001)
        )
        .unwrap()
        .unwrap()
        .1,
        888u32.to_le_bytes()
    );
    // Read bytes remain ordinary core References; no WAL/delta branch on the path.
    assert_eq!(
        rebuilt.read(kv, 4096).unwrap(),
        store.read(kv, 4096).unwrap()
    );
}
#[test]
fn one_real_leaf_update_byte_savings_and_large_batch_control() {
    let temp = Temp::new(std::path::Path::new("/tmp"), "page-savings");
    let mut journal = Journal::create(&temp.wal(), HEADER).unwrap();
    let mut store = Store::new();
    let mut root = Reference::NULL;
    for i in 0..8192u64 {
        root = ulmdb_kv::put(&mut store, root, &i.to_be_bytes(), &i.to_le_bytes()).unwrap();
    }
    let mut sources = Registry::default();
    let first = store.record(&journal, root, &sources, false);
    let objects = replay(&first, &sources).unwrap();
    let receipt = journal
        .commit_checked(&first, &sources, |_| Ok(()))
        .unwrap();
    sources.register(&first, receipt, objects);
    store.freeze();
    for count in [1usize, 64, 1000] {
        let mut tx = store.clone();
        let mut next = root;
        for i in 0..count {
            let key = ((i * 4051 + 17) % 8192) as u64;
            next = ulmdb_kv::put(
                &mut tx,
                next,
                &key.to_be_bytes(),
                &(key + 99999).to_le_bytes(),
            )
            .unwrap();
        }
        let compact = tx.record(&journal, next, &sources, true);
        let full = tx.record(&journal, next, &sources, false);
        let cb = codec::encode(&compact).unwrap().bytes.len();
        let fb = codec::encode(&full).unwrap().bytes.len();
        assert!(cb < fb);
        verify_materialized(&compact, &sources, &tx).unwrap();
        let staged = replay(&compact, &sources).unwrap();
        for object in staged {
            assert_eq!(
                object.bytes,
                tx.read(object.extent.at, object.extent.length as usize)
                    .unwrap()
            );
        }
        println!(
            "actual KV batch={count} compact={cb} full={fb} objects={}",
            compact.objects.len()
        );
    }
}
