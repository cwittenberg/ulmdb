//! One-call validation mechanism and public-API/byte/error controls.
use ulmdb_platform::FileExt;
use super::support::*;
use crate::hybrid::{
    checked::CheckedRecord,
    codec,
    journal::{Journal, WalLayout},
    redo::*,
};
use crate::{Error, Result};
use std::{cell::Cell, path::Path};
struct Count<'a> {
    registry: &'a Registry,
    calls: Cell<usize>,
    short: bool,
}
impl SourceResolver for Count<'_> {
    fn resolve_committed<'a>(&'a self, e: &Extent, r: &SourceReceipt) -> Result<&'a [u8]> {
        self.calls.set(self.calls.get() + 1);
        let b = self.registry.resolve_committed(e, r)?;
        Ok(if self.short { &b[..b.len() - 1] } else { b })
    }
}
struct Dest {
    extent: Extent,
    bytes: Vec<u8>,
    reject: bool,
}
impl DestinationResolver for Dest {
    fn resolve_prepared(&self, e: &Extent) -> Result<&[u8]> {
        if self.reject || *e != self.extent {
            return Err(Error::Corrupt("test prepared extent"));
        }
        Ok(&self.bytes)
    }
}
fn base(t: &Temp, layout: WalLayout) -> (Journal, Registry, Record, Dest) {
    let mut j = Journal::create_with_layout(&t.wal(), HEADER, layout).unwrap();
    let first = small_record(&j);
    let receipt = j
        .commit_checked(&first, &Registry::default(), |_| Ok(()))
        .unwrap();
    let mut registry = Registry::default();
    let materialized = replay(&first, &registry).unwrap();
    registry.register(&first, receipt, materialized);
    let mut record = small_record(&j);
    let source = registry.0.values().next().unwrap();
    let dest = record.objects[0].dest();
    let mut bytes = source.bytes.clone();
    bytes[8..11].copy_from_slice(&[1, 2, 3]);
    record.objects[0] = ObjectRedo::BaseCopy {
        dest,
        source: source.extent,
        receipt: source.receipt,
        patches: vec![Patch {
            offset: 8,
            bytes: vec![1, 2, 3],
        }],
    };
    (
        j,
        registry,
        record,
        Dest {
            extent: dest,
            bytes,
            reject: false,
        },
    )
}
fn same_error<T>(a: Result<T>, b: Result<T>) {
    let a = match a {
        Err(e) => e,
        Ok(_) => panic!("expected rejection"),
    };
    let b = match b {
        Err(e) => e,
        Ok(_) => panic!("expected rejection"),
    };
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
}
#[test]
fn one_source_lookup_equals_old_three_and_exact_wire_bytes() {
    for layout in [
        WalLayout::Dense,
        WalLayout::PreSized {
            capacity: 1024 * 1024,
        },
    ] {
        let a = Temp::new(Path::new("/tmp"), "checked-old");
        let b = Temp::new(Path::new("/tmp"), "checked-new");
        let (mut old, ra, r, da) = base(&a, layout);
        let (mut new, rb, s, db) = base(&b, layout);
        assert_eq!(r, s);
        let ca = Count {
            registry: &ra,
            calls: Cell::new(0),
            short: false,
        };
        let cb = Count {
            registry: &rb,
            calls: Cell::new(0),
            short: false,
        };
        verify_materialized(&r, &ca, &da).unwrap();
        let x = old
            .commit_checked_with_receipt(&r, &ca, |_, _| Ok(()))
            .unwrap();
        let before = std::fs::read(b.wal()).unwrap();
        let y = new
            .commit_materialized_checked_with_receipt(&s, &cb, &db, |_, receipt| {
                assert_eq!(std::fs::read(b.wal()).unwrap(), before);
                assert_eq!(receipt, x);
                Ok(())
            })
            .unwrap();
        assert_eq!(ca.calls.get(), 3);
        assert_eq!(cb.calls.get(), 1);
        assert_eq!(x, y);
        assert_eq!(
            std::fs::read(a.wal()).unwrap(),
            std::fs::read(b.wal()).unwrap()
        );
        let checked = CheckedRecord::new(&r).unwrap();
        let encoded = codec::encode_checked(&checked).unwrap();
        let control = codec::encode(&r).unwrap();
        assert_eq!(encoded.bytes, control.bytes);
        assert_eq!(encoded.digest, control.digest);
    }
}
#[test]
fn invalid_structure_sources_and_afterimages_match_legacy_errors_without_io() {
    for case in 0..14 {
        let t = Temp::new(Path::new("/tmp"), "checked-invalid");
        let (mut j, registry, mut record, mut dest) = base(
            &t,
            WalLayout::PreSized {
                capacity: 1024 * 1024,
            },
        );
        let mut short = false;
        match case {
            0 => record.uuid = [0; 16],
            1 => record.lsn += 1,
            2 => {
                record.objects[0] = ObjectRedo::Literal {
                    dest: dest.extent,
                    bytes: vec![1; 63],
                }
            }
            3 => {
                if let ObjectRedo::BaseCopy { patches, .. } = &mut record.objects[0] {
                    patches[0].offset = 64;
                }
            }
            4 => {
                if let ObjectRedo::BaseCopy { source, .. } = &mut record.objects[0] {
                    source.at.offset = dest.extent.at.offset;
                }
            }
            5 => {
                if let ObjectRedo::BaseCopy { receipt, .. } = &mut record.objects[0] {
                    receipt.uuid[0] ^= 1;
                }
            }
            6 => short = true,
            7 => dest.bytes.pop().map(|_| ()).unwrap(),
            8 => dest.reject = true,
            9 => dest.bytes[8] ^= 1,
            10 => dest.bytes[63] ^= 1,
            11 => {
                record.objects[0] = ObjectRedo::Literal {
                    dest: dest.extent,
                    bytes: vec![7; 64],
                };
            }
            12 => {
                if let ObjectRedo::BaseCopy { receipt, .. } = &mut record.objects[0] {
                    receipt.provenance_digest[0] ^= 1;
                }
            }
            _ => {
                if let ObjectRedo::BaseCopy { source, .. } = &mut record.objects[0] {
                    source.logical_generation += 1;
                }
            }
        }
        let counter = Count {
            registry: &registry,
            calls: Cell::new(0),
            short,
        };
        let before = std::fs::read(t.wal()).unwrap();
        let old = verify_materialized(&record, &counter, &dest);
        let mut callback = false;
        let new = j.commit_materialized_checked_with_receipt(&record, &counter, &dest, |_, _| {
            callback = true;
            Ok(())
        });
        same_error(old, new.map(|_| ()));
        assert!(!callback);
        assert!(!j.is_poisoned());
        assert_eq!(std::fs::read(t.wal()).unwrap(), before);
    }
}
#[test]
fn no_token_reuse_after_preio_rejection_or_record_mutation() {
    let t = Temp::new(Path::new("/tmp"), "checked-retry");
    let (mut j, registry, mut r, mut dest) = base(&t, WalLayout::Dense);
    let c = Count {
        registry: &registry,
        calls: Cell::new(0),
        short: false,
    };
    let before = std::fs::read(t.wal()).unwrap();
    assert!(matches!(
        j.commit_materialized_checked_with_receipt(&r, &c, &dest, |_, _| Err(Error::LimitExceeded)),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(c.calls.get(), 1);
    assert_eq!(std::fs::read(t.wal()).unwrap(), before);
    if let ObjectRedo::BaseCopy { patches, .. } = &mut r.objects[0] {
        patches[0].bytes[0] = 4;
    }
    assert!(
        j.commit_materialized_checked_with_receipt(&r, &c, &dest, |_, _| Ok(()))
            .is_err()
    );
    assert_eq!(c.calls.get(), 2);
    assert!(!j.is_poisoned());
    assert_eq!(std::fs::read(t.wal()).unwrap(), before);
    dest.bytes[8] = 4;
    j.commit_materialized_checked_with_receipt(&r, &c, &dest, |_, _| Ok(()))
        .unwrap();
    assert_eq!(c.calls.get(), 3);
    assert!(
        j.commit_materialized_checked_with_receipt(&r, &c, &dest, |_, _| Ok(()))
            .is_err()
    );
    assert_eq!(j.selected().lsn, 2);
}
#[test]
fn parent_head_and_physical_tail_checks_are_still_enforced() {
    use std::{fs::OpenOptions};
    for layout in [
        WalLayout::Dense,
        WalLayout::PreSized {
            capacity: 1024 * 1024,
        },
    ] {
        for case in 0..3 {
            let a = Temp::new(Path::new("/tmp"), "checked-gates-old");
            let b = Temp::new(Path::new("/tmp"), "checked-gates-new");
            let (mut old, ra, mut r, _da) = base(&a, layout);
            let (mut new, rb, mut s, db) = base(&b, layout);
            if case == 0 {
                r.parent_commit ^= 1;
                s.parent_commit ^= 1;
            } else {
                for t in [&a, &b] {
                    let f = OpenOptions::new().write(true).open(t.wal()).unwrap();
                    if case == 1 {
                        f.write_all_at(&[0xff], 65536).unwrap();
                    } else {
                        f.set_len(f.metadata().unwrap().len() + 1).unwrap();
                    }
                }
            }
            let before = std::fs::read(b.wal()).unwrap();
            let x = old.commit_checked_with_receipt(&r, &ra, |_, _| Ok(()));
            let y = new.commit_materialized_checked_with_receipt(&s, &rb, &db, |_, _| Ok(()));
            same_error(x, y);
            assert_eq!(old.is_poisoned(), new.is_poisoned());
            assert_eq!(std::fs::read(b.wal()).unwrap(), before);
        }
    }
}
#[test]
fn metadata_only_materialized_commit_has_no_destination_or_source_reads() {
    let t = Temp::new(Path::new("/tmp"), "checked-metadata");
    let (mut j, registry, mut r, mut dest) = base(&t, WalLayout::Dense);
    r.objects.clear();
    r.chunks.clear();
    r.root = j.root();
    dest.reject = true;
    let c = Count {
        registry: &registry,
        calls: Cell::new(0),
        short: false,
    };
    j.commit_materialized_checked_with_receipt(&r, &c, &dest, |_, _| Ok(()))
        .unwrap();
    assert_eq!(c.calls.get(), 0);
    assert_eq!(j.selected().lsn, 2);
}
