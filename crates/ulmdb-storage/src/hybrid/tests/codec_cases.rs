use super::support::*;
use crate::Reference;
use crate::hybrid::{codec, journal::Journal, redo::*};

#[test]
fn dense_roundtrip_hash_and_every_truncation() {
    let t = Temp::new(std::path::Path::new("/tmp"), "codec");
    let j = Journal::create(&t.wal(), HEADER).unwrap();
    let r = small_record(&j);
    let e = codec::encode(&r).unwrap();
    assert_eq!(e.bytes.len() % 8, 0);
    let d = codec::decode(&e.bytes, 4096).unwrap();
    assert_eq!(d.record, r);
    assert_eq!(d.receipt.digest, e.digest);
    for n in 0..e.bytes.len() {
        assert!(codec::decode(&e.bytes[..n], 4096).is_err());
    }
    for i in 0..e.bytes.len() {
        let mut b = e.bytes.clone();
        b[i] ^= 1;
        assert!(codec::decode(&b, 4096).is_err(), "mutation{i}");
    }
}
#[test]
fn compact_sources_and_final_ranges_are_exact() {
    let t = Temp::new(std::path::Path::new("/tmp"), "patch");
    let j = Journal::create(&t.wal(), HEADER).unwrap();
    let mut r = small_record(&j);
    let source = Extent {
        at: Reference {
            segment: 1,
            offset: 4096,
        },
        logical_generation: GENERATION,
        length: 64,
    };
    r.parent.lsn = 1;
    r.parent.sequence = 1;
    r.lsn = 2;
    r.sequence = 2;
    r.chunks[0].before = 4160;
    r.chunks[0].after = 4224;
    r.objects = vec![ObjectRedo::BaseCopy {
        dest: Extent {
            at: Reference {
                segment: 1,
                offset: 4160,
            },
            ..source
        },
        source,
        receipt: SourceReceipt {
            uuid: HEADER.uuid,
            epoch: HEADER.epoch,
            creation_lsn: 1,
            creation_sequence: 1,
            creation_commit: 23,
            provenance_digest: [9; 32],
            authority: Authority::Wal {
                lsn: 1,
                sequence: 1,
                digest: [9; 32],
            },
        },
        patches: vec![
            Patch {
                offset: 2,
                bytes: vec![1, 2],
            },
            Patch {
                offset: 60,
                bytes: vec![3, 4],
            },
        ],
    }];
    r.validate().unwrap();
    let e = codec::encode(&r).unwrap();
    assert_eq!(codec::decode(&e.bytes, 4096).unwrap().record, r);
    for mode in 0..6 {
        let mut bad = r.clone();
        if let ObjectRedo::BaseCopy {
            dest,
            source,
            receipt,
            patches,
        } = &mut bad.objects[0]
        {
            match mode {
                0 => receipt.epoch += 1,
                1 => source.logical_generation += 1,
                2 => source.at = dest.at,
                3 => patches[1].offset = 3,
                4 => patches[1].offset = 63,
                _ => receipt.creation_lsn = 2,
            }
        }
        assert!(bad.validate().is_err(), "mode{mode}");
    }
}
#[test]
fn private_source_and_generation_never_authorize_overlap() {
    let a = Extent {
        at: Reference {
            segment: 1,
            offset: 4096,
        },
        logical_generation: 1,
        length: 64,
    };
    let b = Extent {
        logical_generation: 2,
        ..a
    };
    assert!(a.overlaps(b).unwrap());
    assert_eq!(
        final_patches(&[1, 2, 3, 4, 5], &[1, 8, 9, 4, 7]).unwrap(),
        vec![
            Patch {
                offset: 1,
                bytes: vec![8, 9]
            },
            Patch {
                offset: 4,
                bytes: vec![7]
            }
        ]
    );
}
#[test]
fn payload_header_and_compact_expansion_are_bounded() {
    let mut extent = Extent {
        at: Reference {
            segment: 1,
            offset: 4096,
        },
        logical_generation: 1,
        length: 67_108_880,
    };
    extent.validate().unwrap();
    extent.length += 1;
    assert!(extent.validate().is_err());
    let t = Temp::new(std::path::Path::new("/tmp"), "expansion");
    let j = Journal::create(&t.wal(), HEADER).unwrap();
    let mut r = small_record(&j);
    r.parent.lsn = 1;
    r.parent.sequence = 1;
    r.lsn = 2;
    r.sequence = 2;
    r.root.next_chunk = 7;
    r.objects.clear();
    r.chunks.clear();
    let receipt = SourceReceipt {
        uuid: HEADER.uuid,
        epoch: HEADER.epoch,
        creation_lsn: 1,
        creation_sequence: 1,
        creation_commit: 99,
        provenance_digest: [0; 32],
        authority: Authority::Wal {
            lsn: 1,
            sequence: 1,
            digest: [0; 32],
        },
    };
    for chunk in 1..=5 {
        r.chunks.push(ChunkDelta {
            chunk,
            logical_generation: 1,
            before: 4096,
            after: 4096 + 67_108_864,
            capacity: MAX_CHUNK_BYTES,
        });
        r.objects.push(ObjectRedo::BaseCopy {
            dest: Extent {
                at: Reference {
                    segment: chunk,
                    offset: 4096,
                },
                logical_generation: 1,
                length: 67_108_864,
            },
            source: Extent {
                at: Reference {
                    segment: 6,
                    offset: 4096,
                },
                logical_generation: 2,
                length: 67_108_864,
            },
            receipt,
            patches: vec![],
        });
    }
    assert!(matches!(r.validate(), Err(crate::Error::LimitExceeded)));
}
