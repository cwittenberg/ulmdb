//! Owned-control correspondence, workspace lifetime and complete publication tests.
use super::support::*;
use crate::hybrid::{
    borrowed::*,
    checked::CheckedRecord,
    codec,
    journal::{Journal, WalLayout},
    redo::*,
};
use crate::{Error, ReadStore, Reference, Result};
use std::path::Path;
fn header(r: &Record) -> Record {
    Record {
        uuid: r.uuid,
        epoch: r.epoch,
        lsn: r.lsn,
        sequence: r.sequence,
        commit: r.commit,
        parent_commit: r.parent_commit,
        parent: r.parent,
        root: r.root,
        chunks: r.chunks.clone(),
        objects: Vec::new(),
    }
}
fn recipe<'a, 'w>(
    r: &Record,
    store: &'a Store,
    sources: &Registry,
    w: &'w mut RecipeWorkspace,
) -> BorrowedRecord<'a, 'w> {
    let mut b = RecipeBuilder::new(header(r), w, r.objects.len(), 1024 * 1024).unwrap();
    for (&at, &(length, base)) in &store.allocations {
        if at.offset < store.private_from {
            continue;
        }
        let dest = store.extent(at);
        let after = store.read(at, length).unwrap();
        if let Some(source) = base
            .and_then(|r| sources.0.get(&r))
            .filter(|s| s.extent.length == dest.length)
        {
            b.committed(dest, source.extent, source.receipt, &source.bytes, after)
                .unwrap();
        } else {
            b.literal(dest, after).unwrap();
        }
    }
    b.finish()
}
#[test]
fn real_kv_and_both_prefix_families_match_owned_wire_and_whole_journal() {
    for fs in ["/tmp", "/dev/shm"] {
        for layout in [
            WalLayout::Dense,
            WalLayout::PreSized {
                capacity: 1024 * 1024,
            },
        ] {
            let a = Temp::new(Path::new(fs), "borrowed-old");
            let b = Temp::new(Path::new(fs), "borrowed-new");
            let mut old = Journal::create_with_layout(&a.wal(), HEADER, layout).unwrap();
            let mut new = Journal::create_with_layout(&b.wal(), HEADER, layout).unwrap();
            let mut store = Store::new();
            let mut sources = Registry::default();
            let mut ws = RecipeWorkspace::default();
            let mut kv = Reference::NULL;
            let mut p4 = Reference::NULL;
            let mut p6 = Reference::NULL;
            for round in 0..4 {
                for i in 0..96u64 {
                    kv = ulmdb_kv::put(&mut store, kv, &i.to_be_bytes(), &[round; 32]).unwrap();
                }
                let v4 = ulmdb_core::Prefix::new(ulmdb_core::Address::V4(0x0a00_0000), 8).unwrap();
                let v6 =
                    ulmdb_core::Prefix::new(ulmdb_core::Address::V6(0x2001_0db8u128 << 96), 32)
                        .unwrap();
                p4 = ulmdb_prefix::insert(&mut store, p4, v4, &[round; 24]).unwrap();
                p6 = ulmdb_prefix::insert(&mut store, p6, v6, &[round; 24]).unwrap();
                let record = store.record(&old, kv, &sources, true);
                let r = recipe(&record, &store, &sources, &mut ws);
                let c = CheckedRecord::new(&r).unwrap();
                let mut buf = Vec::new();
                let d = codec::encode_checked_into(&c, &mut buf, 1024 * 1024).unwrap();
                let control = codec::encode(&record).unwrap();
                assert_eq!(buf, control.bytes);
                assert_eq!(d, control.digest);
                let x = old.commit_checked(&record, &sources, |_| Ok(())).unwrap();
                let before = std::fs::read(b.wal()).unwrap();
                let y = new
                    .commit_borrowed_checked_with_receipt(
                        &r,
                        &sources,
                        &store,
                        1024 * 1024,
                        |_, receipt| {
                            assert_eq!(receipt, x);
                            assert_eq!(std::fs::read(b.wal()).unwrap(), before);
                            Ok(())
                        },
                    )
                    .unwrap();
                assert_eq!(x, y);
                assert_eq!(
                    std::fs::read(a.wal()).unwrap(),
                    std::fs::read(b.wal()).unwrap()
                );
                drop(r);
                sources.register(&record, x, replay(&record, &sources).unwrap());
                for i in 0..96u64 {
                    assert_eq!(
                        ulmdb_kv::get(&store, kv, &i.to_be_bytes()).unwrap(),
                        Some([round; 32].as_slice())
                    );
                }
                assert_eq!(
                    ulmdb_prefix::get(&store, p4, v4.network())
                        .unwrap()
                        .unwrap()
                        .1,
                    [round; 24]
                );
                assert_eq!(
                    ulmdb_prefix::get(&store, p6, v6.network())
                        .unwrap()
                        .unwrap()
                        .1,
                    [round; 24]
                );
                store.freeze();
            }
        }
    }
}
#[test]
fn exhaustive_small_and_randomized_patch_boundaries_match_owned_control() {
    let t = Temp::new(Path::new("/tmp"), "borrowed-diff");
    let j = Journal::create(&t.wal(), HEADER).unwrap();
    let template = small_record(&j);
    let source = Extent {
        at: Reference {
            segment: 1,
            offset: 4096,
        },
        logical_generation: GENERATION,
        length: 4096,
    };
    let receipt = SourceReceipt {
        uuid: HEADER.uuid,
        epoch: HEADER.epoch,
        creation_lsn: 0,
        creation_sequence: 0,
        creation_commit: HEADER.commit,
        provenance_digest: [1; 32],
        authority: Authority::Checkpoint {
            generation: 1,
            sequence: 0,
            digest: [2; 32],
        },
    };
    let mut ws = RecipeWorkspace::default();
    let mut random = 73u64;
    for case in 0..1536usize {
        let n = if case < 256 {
            512
        } else {
            [64, 96, 511, 512, 4095, 4096][case % 6]
        };
        let before = vec![0u8; n];
        let mut after = before.clone();
        if case < 256 {
            for bit in 0..8 {
                after[31 + bit] = ((case >> bit) & 1) as u8;
            }
        } else {
            for _ in 0..case % 100 {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                after[random as usize % n] ^= 1;
            }
        }
        let dest = Extent {
            at: Reference {
                segment: 1,
                offset: 8192,
            },
            length: n as u32,
            ..source
        };
        let src = Extent {
            length: n as u32,
            ..source
        };
        let object = ObjectRedo::from_committed_bytes(dest, src, receipt, &before, &after).unwrap();
        let mut record = header(&template);
        record.chunks[0].before = 8192;
        record.chunks[0].after = 8192 + n as u64;
        record.objects = vec![object];
        let mut b = RecipeBuilder::new(header(&record), &mut ws, 1, 1024 * 1024).unwrap();
        b.committed(dest, src, receipt, &before, &after).unwrap();
        let r = b.finish();
        let checked = CheckedRecord::new(&r).unwrap();
        let mut bytes = Vec::new();
        codec::encode_checked_into(&checked, &mut bytes, 1024 * 1024).unwrap();
        assert_eq!(bytes, codec::encode(&record).unwrap().bytes, "case{case}");
        let decoded = codec::decode(&bytes, record.parent.end().unwrap()).unwrap();
        assert_eq!(decoded.record, record);
    }
    assert!(ws.retained_bytes() <= 1024 * 1024);
}
#[test]
fn callback_rejection_buffer_reuse_limits_and_metadata_only_never_publish_early() {
    let t = Temp::new(Path::new("/tmp"), "borrowed-reuse");
    let mut j = Journal::create_with_layout(
        &t.wal(),
        HEADER,
        WalLayout::PreSized {
            capacity: 1024 * 1024,
        },
    )
    .unwrap();
    let mut ws = RecipeWorkspace::default();
    let registry = Registry::default();
    struct NoDest;
    impl DestinationResolver for NoDest {
        fn resolve_prepared(&self, _: &Extent) -> Result<&[u8]> {
            panic!("metadata-only record has no destinations")
        }
    }
    for i in 0..8u128 {
        let mut m = small_record(&j);
        m.chunks.clear();
        m.objects.clear();
        m.root = j.root();
        m.commit = 200 + i;
        let b = RecipeBuilder::new(m, &mut ws, 0, 1024).unwrap();
        let r = b.finish();
        let before = std::fs::read(t.wal()).unwrap();
        assert!(matches!(
            j.commit_borrowed_checked_with_receipt(&r, &registry, &NoDest, 1024, |_, _| Err(
                Error::Busy
            )),
            Err(Error::Busy)
        ));
        assert!(!j.is_poisoned());
        assert_eq!(std::fs::read(t.wal()).unwrap(), before);
        assert!(matches!(
            j.commit_borrowed_checked_with_receipt(&r, &registry, &NoDest, 8, |_, _| panic!(
                "oversize callback"
            )),
            Err(Error::LimitExceeded)
        ));
        assert!(j.encoding_workspace_bytes() <= 8);
        assert_eq!(std::fs::read(t.wal()).unwrap(), before);
        j.commit_borrowed_checked_with_receipt(&r, &registry, &NoDest, 1024, |_, _| Ok(()))
            .unwrap();
        assert!(j.encoding_workspace_bytes() <= 1024);
    }
}

#[test]
fn untrusted_diff_input_never_authenticates_a_wrong_materialized_source() {
    let t = Temp::new(Path::new("/tmp"), "borrowed-bad-source");
    let mut j = Journal::create(&t.wal(), HEADER).unwrap();
    let src = Extent {
        at: Reference {
            segment: 1,
            offset: 4096,
        },
        logical_generation: GENERATION,
        length: 4096,
    };
    let dest = Extent {
        at: Reference {
            segment: 1,
            offset: 8192,
        },
        ..src
    };
    let proof = SourceReceipt {
        uuid: HEADER.uuid,
        epoch: HEADER.epoch,
        creation_lsn: 0,
        creation_sequence: 0,
        creation_commit: HEADER.commit,
        provenance_digest: [1; 32],
        authority: Authority::Checkpoint {
            generation: 1,
            sequence: 0,
            digest: [2; 32],
        },
    };
    let mut registry = Registry::default();
    registry.0.insert(
        src.at,
        Source {
            extent: src,
            receipt: proof,
            bytes: vec![0; 4096],
        },
    );
    let after = vec![1; 4096];
    let fake_before = after.clone();
    struct Dest<'a>(&'a [u8]);
    impl DestinationResolver for Dest<'_> {
        fn resolve_prepared(&self, _: &Extent) -> Result<&[u8]> {
            Ok(self.0)
        }
    }
    let mut m = small_record(&j);
    m.chunks[0].before = 8192;
    m.chunks[0].after = 12288;
    m.root.catalog = dest.at;
    m.objects =
        vec![ObjectRedo::from_committed_bytes(dest, src, proof, &fake_before, &after).unwrap()];
    let expected = verify_materialized(&m, &registry, &Dest(&after)).unwrap_err();
    let mut ws = RecipeWorkspace::default();
    let mut b = RecipeBuilder::new(header(&m), &mut ws, 1, 1024 * 1024).unwrap();
    b.committed(dest, src, proof, &fake_before, &after).unwrap();
    let r = b.finish();
    let before = std::fs::read(t.wal()).unwrap();
    let error = j
        .commit_borrowed_checked_with_receipt(&r, &registry, &Dest(&after), 1024 * 1024, |_, _| {
            panic!("wrong afterimage reached IO callback")
        })
        .unwrap_err();
    assert_eq!(format!("{error:?}"), format!("{expected:?}"));
    assert_eq!(before, std::fs::read(t.wal()).unwrap());
    assert!(!j.is_poisoned());
}
