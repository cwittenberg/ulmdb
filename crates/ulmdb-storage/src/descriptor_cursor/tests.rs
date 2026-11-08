use super::*;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

struct Source {
    path: PathBuf,
    offset: u64,
    length: usize,
    checkpoint: [u8; 64],
}
impl Drop for Source {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
impl Source {
    fn new(bytes: &[u8], checkpoint: [u8; 64]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "descriptor-cursor-{:032x}",
            crate::new_identity().unwrap()
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.write_all(&[0x9a; 37]).unwrap();
        file.write_all(bytes).unwrap();
        file.write_all(&[0xac; 53]).unwrap();
        file.sync_all().unwrap();
        Self {
            path,
            offset: 37,
            length: bytes.len(),
            checkpoint,
        }
    }
    fn cursor(&self, entries: usize) -> FlatDescriptorCursor {
        FlatDescriptorCursor::new(
            File::open(&self.path).unwrap(),
            self.offset,
            self.length,
            self.checkpoint,
            entries,
        )
        .unwrap()
    }
}
fn descriptor(minor: u16, count: usize) -> Manifest {
    let mut m = Manifest::empty().unwrap();
    m.format_minor = minor;
    m.next_segment = count as u64 + 1;
    m.segments = (1..=count as u64)
        .map(|id| SegmentEntry {
            id,
            length: 4096 + id % 7,
            hash: [id as u8; 32],
            cohort: if minor >= 3 { m.generation } else { 0 },
        })
        .collect();
    m
}
fn exhaust(cursor: &mut FlatDescriptorCursor, budget: DescriptorCursorBudget) -> usize {
    let mut read = 0;
    for _ in 0..10000 {
        let p = cursor.advance(budget).unwrap();
        assert!(p.read_bytes <= budget.bytes);
        assert!(p.decoded_entries <= budget.entries);
        read += p.read_bytes;
        if p.ready {
            return read;
        }
    }
    panic!("bounded descriptor cursor did not make progress");
}

#[test]
fn real_flat_codecs_resume_with_exact_byte_and_entry_budgets() {
    for minor in [1, 2, 3] {
        let manifest = descriptor(minor, 4097);
        let bytes = manifest.encode().unwrap();
        let old = Manifest::decode(&bytes).unwrap();
        let source = Source::new(&bytes, crate::checkpoint_bytes(&manifest).unwrap());
        let mut cursor = source.cursor(4097);
        assert_eq!(
            cursor
                .advance(DescriptorCursorBudget {
                    bytes: 0,
                    entries: 0
                })
                .unwrap(),
            DescriptorCursorProgress::default()
        );
        assert_eq!(
            exhaust(
                &mut cursor,
                DescriptorCursorBudget {
                    bytes: 4096,
                    entries: 7
                }
            ),
            bytes.len()
        );
        let footprint = cursor.finish().unwrap();
        assert_eq!(footprint.header().uuid, old.uuid);
        assert_eq!(footprint.header().generation, old.generation);
        assert_eq!(footprint.header().next_segment, old.next_segment);
        assert_eq!(footprint.header().format_minor, minor);
        assert_eq!(footprint.header().segment_count, 4097);
        assert_eq!(footprint.entries().count(), old.segments.len());
        for (index, (new, old)) in footprint.entries().zip(old.segments.iter()).enumerate() {
            assert_eq!(
                (new.id, new.length, new.hash, new.cohort),
                (old.id, old.length, old.hash, old.cohort)
            );
            assert_eq!(footprint.entry(index).unwrap().id, new.id);
        }
        assert_eq!(footprint.pages.len(), 65);
        assert!(
            footprint
                .pages
                .values()
                .all(|page| page.len() <= PAGE_ENTRIES)
        );
    }
    // Header and row fragments can cross every call boundary, without needing
    // one call to admit a complete descriptor or one row's encoded bytes.
    let m = descriptor(2, 3);
    let encoded = m.encode().unwrap();
    let source = Source::new(&encoded, crate::checkpoint_bytes(&m).unwrap());
    let mut cursor = source.cursor(3);
    assert_eq!(
        exhaust(
            &mut cursor,
            DescriptorCursorBudget {
                bytes: 1,
                entries: 1
            }
        ),
        encoded.len()
    );
    assert_eq!(cursor.finish().unwrap().header().segment_count, 3);
    let mut cursor = source.cursor(3);
    let p = cursor
        .advance(DescriptorCursorBudget {
            bytes: HEADER,
            entries: 0,
        })
        .unwrap();
    assert_eq!(
        (p.read_bytes, p.decoded_entries, p.ready),
        (HEADER, 0, false)
    );
    let p = cursor
        .advance(DescriptorCursorBudget {
            bytes: 0,
            entries: 1,
        })
        .unwrap();
    assert_eq!((p.read_bytes, p.decoded_entries, p.ready), (0, 1, false));
    let p = cursor
        .advance(DescriptorCursorBudget {
            bytes: ROW,
            entries: 0,
        })
        .unwrap();
    assert_eq!((p.read_bytes, p.decoded_entries, p.ready), (ROW, 0, false));
    let p = cursor
        .advance(DescriptorCursorBudget {
            bytes: 0,
            entries: 1,
        })
        .unwrap();
    assert_eq!((p.read_bytes, p.decoded_entries, p.ready), (0, 1, false));
    assert_eq!(
        exhaust(
            &mut cursor,
            DescriptorCursorBudget {
                bytes: ROW,
                entries: 1
            }
        ),
        2 * ROW
    );
    assert_eq!(cursor.finish().unwrap().entries().count(), 3);
}

#[test]
fn corruption_never_exposes_a_footprint_and_matches_strict_flat_rejection() {
    for minor in [2, 3] {
        let m = descriptor(minor, 3);
        let bytes = m.encode().unwrap();
        let cp = crate::checkpoint_bytes(&m).unwrap();
        let mut altered = bytes.clone();
        altered[HEADER + 16] ^= 1; // structurally valid, wrong authenticated hash
        let source = Source::new(&altered, cp);
        let mut cursor = source.cursor(3);
        assert!(
            cursor
                .advance(DescriptorCursorBudget {
                    bytes: 4096,
                    entries: 8
                })
                .is_err()
        );
        assert!(cursor.finish().is_err());
        for (offset, replacement) in [(HEADER, 0u64), (HEADER + 8, 4095), (184, 3)] {
            let mut bad = bytes.clone();
            bad[offset..offset + 8].copy_from_slice(&replacement.to_le_bytes());
            assert!(Manifest::decode(&bad).is_err());
            let checkpoint = crate::publication::record(&m, Sha256::digest(&bad).into());
            let source = Source::new(&bad, checkpoint);
            let mut cursor = source.cursor(3);
            assert!(
                cursor
                    .advance(DescriptorCursorBudget {
                        bytes: 4096,
                        entries: 8
                    })
                    .is_err()
            );
            assert!(cursor.finish().is_err());
        }
        for (offset, replacement) in [
            (8, 1),
            (12, 0),
            (16, 0),
            (48, 0),
            (64, 0),
            (116, 1),
            (160, 1),
            (176, 0),
        ] {
            let mut bad = bytes.clone();
            if [16, 64].contains(&offset) {
                bad[offset..offset + 16].fill(0);
            } else {
                bad[offset..offset + 4].copy_from_slice(&(replacement as u32).to_le_bytes());
            }
            assert!(
                Manifest::decode(&bad).is_err(),
                "strict decoder accepted header damage at{offset}"
            );
            let checkpoint = crate::publication::record(&m, Sha256::digest(&bad).into());
            let source = Source::new(&bad, checkpoint);
            let mut cursor = source.cursor(3);
            assert!(
                cursor
                    .advance(DescriptorCursorBudget {
                        bytes: 4096,
                        entries: 8
                    })
                    .is_err(),
                "cursor accepted header damage at{offset}"
            );
            assert!(cursor.finish().is_err());
        }
    }
}

#[test]
fn source_change_io_error_limits_and_inherited_creator_fail_closed() {
    let m = descriptor(2, 3);
    let bytes = m.encode().unwrap();
    let source = Source::new(&bytes, crate::checkpoint_bytes(&m).unwrap());
    let mut limited = source.cursor(2);
    assert!(matches!(
        limited.advance(DescriptorCursorBudget {
            bytes: HEADER,
            entries: 1
        }),
        Err(Error::LimitExceeded)
    ));
    assert!(limited.finish().is_err());
    let write_only = OpenOptions::new().write(true).open(&source.path).unwrap();
    let mut io = FlatDescriptorCursor::new(
        write_only,
        source.offset,
        source.length,
        source.checkpoint,
        3,
    )
    .unwrap();
    assert!(matches!(
        io.advance(DescriptorCursorBudget {
            bytes: HEADER,
            entries: 1
        }),
        Err(Error::Io(_))
    ));
    assert!(io.finish().is_err());
    let mut changed = source.cursor(3);
    let p = changed
        .advance(DescriptorCursorBudget {
            bytes: HEADER + 17,
            entries: 1,
        })
        .unwrap();
    assert_eq!(
        (p.read_bytes, p.decoded_entries, p.ready),
        (HEADER + 17, 1, false)
    );
    OpenOptions::new()
        .write(true)
        .open(&source.path)
        .unwrap()
        .set_len(source.offset + 1)
        .unwrap();
    assert!(matches!(
        changed.advance(DescriptorCursorBudget {
            bytes: 1,
            entries: 1
        }),
        Err(Error::Corrupt(_))
    ));
    assert!(changed.failed);
    assert!(changed.finish().is_err());
    let source = Source::new(&bytes, source.checkpoint);
    let mut inherited = source.cursor(3);
    inherited.creator = inherited.creator.wrapping_add(1);
    fs::remove_file(&source.path).unwrap();
    assert!(matches!(
        inherited.advance(DescriptorCursorBudget {
            bytes: 1,
            entries: 1
        }),
        Err(Error::InvalidInput("inherited descriptor cursor"))
    ));
}

#[test]
fn indexed_and_incomplete_inputs_are_explicitly_declined() {
    let m = descriptor(2, 0);
    let bytes = m.encode().unwrap();
    let source = Source::new(&bytes, crate::checkpoint_bytes(&m).unwrap());
    assert!(source.cursor(0).finish().is_err());
    let mut cursor = source.cursor(0);
    assert_eq!(
        exhaust(
            &mut cursor,
            DescriptorCursorBudget {
                bytes: 17,
                entries: 1
            }
        ),
        HEADER
    );
    assert_eq!(cursor.finish().unwrap().entries().count(), 0);
    let mut cp = source.checkpoint;
    cp[..8].copy_from_slice(b"ULMDBT04");
    let crc = crate::publication::crc(&cp[..60]);
    cp[60..].copy_from_slice(&crc.to_le_bytes());
    assert!(matches!(
        FlatDescriptorCursor::new(
            File::open(&source.path).unwrap(),
            source.offset,
            source.length,
            cp,
            0
        ),
        Err(Error::InvalidInput("indexed descriptor needs node cursor"))
    ));
}
