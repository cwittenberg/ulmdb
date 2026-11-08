use ulmdb_platform::FileExt;
use super::support::*;
use crate::hybrid::{
    journal::{Journal, wire},
    recovery,
};
use std::{fs::OpenOptions, path::Path};
#[test]
fn journal_reopen_selected_corruption_and_unselected_tail() {
    for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
        let t = Temp::new(base, "journal");
        let mut j = Journal::create(&t.wal(), HEADER).unwrap();
        let r = small_record(&j);
        j.commit_checked(&r, &Registry::default(), |_| Ok(()))
            .unwrap();
        let selected = j.selected();
        drop(j);
        let f = OpenOptions::new().write(true).open(t.wal()).unwrap();
        f.write_all_at(b"private orphan", selected.end().unwrap())
            .unwrap();
        drop(f);
        let (j, _) = Journal::open_checked(&t.wal(), |p| {
            assert_eq!(p.records.len(), 1);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            std::fs::metadata(t.wal()).unwrap().len(),
            selected.end().unwrap()
        );
        drop(j);
        let f = OpenOptions::new().write(true).open(t.wal()).unwrap();
        f.write_all_at(&[0xff], selected.offset + 260).unwrap();
        drop(f);
        assert!(Journal::open_checked(&t.wal(), |_| Ok(())).is_err());
    }
}
#[test]
fn one_invalid_head_requires_exact_successor_and_validation_before_heal() {
    let t = Temp::new(Path::new("/tmp"), "heal");
    let mut j = Journal::create(&t.wal(), HEADER).unwrap();
    let r = small_record(&j);
    j.commit_checked(&r, &Registry::default(), |_| Ok(()))
        .unwrap();
    drop(j);
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .open(t.wal())
        .unwrap();
    f.write_all_at(&[0xff], wire::WITNESS_OFFSETS[1]).unwrap();
    let before = std::fs::read(t.wal()).unwrap();
    let plan = recovery::inspect(&f).unwrap();
    assert!(plan.heal);
    assert_eq!(plan.selected.receipt.lsn, 1);
    drop(f);
    assert!(
        Journal::open_checked(&t.wal(), |_| Err::<(), _>(crate::Error::Corrupt(
            "typed graph"
        )))
        .is_err()
    );
    assert_eq!(std::fs::read(t.wal()).unwrap(), before);
    let (j, _) = Journal::open_checked(&t.wal(), |_| Ok(())).unwrap();
    assert_eq!(j.selected().lsn, 1);
    drop(j);
    let f = OpenOptions::new().write(true).open(t.wal()).unwrap();
    f.write_all_at(&[0xff], wire::WITNESS_OFFSETS[1]).unwrap();
    f.write_all_at(&[0], before.len() as u64).unwrap();
    drop(f);
    assert!(Journal::open_checked(&t.wal(), |_| Ok(())).is_err());
}
#[test]
fn no_publication_when_readiness_rejects() {
    let t = Temp::new(Path::new("/tmp"), "ready");
    let mut j = Journal::create(&t.wal(), HEADER).unwrap();
    let before = std::fs::read(t.wal()).unwrap();
    assert!(
        j.commit_checked(&small_record(&j), &Registry::default(), |_| Err(
            crate::Error::LimitExceeded
        ))
        .is_err()
    );
    assert!(!j.is_poisoned());
    assert_eq!(before, std::fs::read(t.wal()).unwrap());
}
#[test]
fn correctly_rehashed_bad_selected_graph_is_rejected_before_heal() {
    let t = Temp::new(Path::new("/tmp"), "bad-graph");
    let mut j = Journal::create(&t.wal(), HEADER).unwrap();
    // The low-level journal deliberately does not know what kind of tree catalog
    // refers to; H3's callback must reject this correctly hashed non-KV object.
    let r = small_record(&j);
    j.commit_checked(&r, &Registry::default(), |_| Ok(()))
        .unwrap();
    drop(j);
    let f = OpenOptions::new().write(true).open(t.wal()).unwrap();
    f.write_all_at(&[0xff], wire::WITNESS_OFFSETS[1]).unwrap();
    drop(f);
    let before = std::fs::read(t.wal()).unwrap();
    let result = Journal::open_checked(&t.wal(), |p| {
        let mut image = Store::new();
        for decoded in &p.records {
            for object in crate::hybrid::redo::replay(&decoded.record, &Registry::default())? {
                let end = object.extent.end()? as usize;
                image.bytes.resize(end, 0);
                image.bytes[object.extent.at.offset as usize..end].copy_from_slice(&object.bytes);
            }
        }
        ulmdb_kv::audit_tree(&image, p.root().catalog, 100)?;
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(before, std::fs::read(t.wal()).unwrap());
}
#[test]
fn exact_source_extent_membership_is_checked_even_with_valid_record_hash() {
    let t = Temp::new(Path::new("/tmp"), "source-member");
    let mut j = Journal::create(&t.wal(), HEADER).unwrap();
    let mut first = small_record(&j);
    if let crate::hybrid::redo::ObjectRedo::Literal { dest, bytes } = &mut first.objects[0] {
        dest.length = 32;
        bytes.truncate(32);
    }
    let first_receipt = j
        .commit_checked(&first, &Registry::default(), |_| Ok(()))
        .unwrap();
    let mut registry = Registry::default();
    registry.register(
        &first,
        first_receipt,
        crate::hybrid::redo::replay(&first, &registry).unwrap(),
    );
    let mut second = small_record(&j);
    let origin = registry.0.values().next().unwrap();
    second.objects[0] = crate::hybrid::redo::ObjectRedo::BaseCopy {
        dest: crate::hybrid::redo::Extent {
            length: 32,
            ..second.objects[0].dest()
        },
        source: origin.extent,
        receipt: origin.receipt,
        patches: vec![],
    };
    j.commit_checked(&second, &registry, |_| Ok(())).unwrap();
    drop(j);
    // Re-encode a semantically invalid exact source range and update its selected
    // witness correctly, so SHA corruption rejection cannot mask membership logic.
    if let crate::hybrid::redo::ObjectRedo::BaseCopy { source, .. } = &mut second.objects[0] {
        source.at.offset += 1;
    }
    let encoded = crate::hybrid::codec::encode(&second).unwrap();
    let f = OpenOptions::new().write(true).open(t.wal()).unwrap();
    let receipt = crate::hybrid::redo::WalReceipt {
        lsn: 2,
        sequence: 2,
        offset: first_receipt.end().unwrap(),
        length: encoded.bytes.len() as u64,
        digest: encoded.digest,
    };
    f.write_all_at(&encoded.bytes, receipt.offset).unwrap();
    f.set_len(receipt.end().unwrap()).unwrap();
    f.write_all_at(
        &wire::encode_witness(
            HEADER,
            wire::Witness {
                receipt,
                commit: second.commit,
            },
        ),
        wire::WITNESS_OFFSETS[0],
    )
    .unwrap();
    drop(f);
    let before = std::fs::read(t.wal()).unwrap();
    assert!(Journal::open_checked(&t.wal(), |_| Ok(())).is_err());
    assert_eq!(before, std::fs::read(t.wal()).unwrap());
}
#[test]
fn exact_future_receipt_is_available_before_any_publication() {
    let t = Temp::new(Path::new("/tmp"), "future-receipt");
    let mut j = Journal::create(&t.wal(), HEADER).unwrap();
    let before = std::fs::read(t.wal()).unwrap();
    let record = small_record(&j);
    let mut future = None;
    let selected = j
        .commit_checked_with_receipt(&record, &Registry::default(), |r, receipt| {
            assert_eq!(*r, record);
            assert_eq!(before, std::fs::read(t.wal()).unwrap());
            future = Some(receipt);
            Ok(())
        })
        .unwrap();
    assert_eq!(future, Some(selected));
    let bytes = std::fs::read(t.wal()).unwrap();
    let mut rejected = None;
    assert!(
        j.commit_checked_with_receipt(&small_record(&j), &Registry::default(), |_, receipt| {
            rejected = Some(receipt);
            Err(crate::Error::LimitExceeded)
        })
        .is_err()
    );
    assert!(rejected.is_some());
    assert_eq!(j.selected(), selected);
    assert!(!j.is_poisoned());
    assert_eq!(bytes, std::fs::read(t.wal()).unwrap());
}
