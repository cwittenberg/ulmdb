use ulmdb_platform::FileExt;
use super::support::*;
use crate::{
    Error,
    hybrid::{
        journal::{Journal, WalLayout, wire},
        recovery,
    },
};
use std::{fs::OpenOptions, path::Path};
const CAP: u64 = 16 * 1024 * 1024;
fn create(t: &Temp) -> Journal {
    Journal::create_with_layout(&t.wal(), HEADER, WalLayout::PreSized { capacity: CAP }).unwrap()
}
fn committed(t: &Temp, count: usize) -> crate::hybrid::redo::WalReceipt {
    let mut j = create(t);
    assert_eq!(j.selected(), wire::genesis(HEADER).unwrap().receipt);
    for _ in 0..count {
        j.commit_checked(&small_record(&j), &Registry::default(), |_| Ok(()))
            .unwrap();
    }
    assert_eq!(j.physical_bytes().unwrap(), CAP);
    j.selected()
}
#[test]
fn layout_is_authenticated_explicit_and_logical_genesis_stable() {
    for capacity in [
        wire::DATA_START - 1,
        wire::DATA_START + 1,
        wire::MAX_WAL_BYTES + 4096,
    ] {
        assert!(wire::encode_layout_header(HEADER, WalLayout::PreSized { capacity }).is_err());
    }
    let layout = WalLayout::PreSized { capacity: CAP };
    let h = wire::encode_layout_header(HEADER, layout).unwrap();
    assert_eq!(wire::decode_layout_header(&h).unwrap(), (HEADER, layout));
    assert!(wire::decode_header(&h).is_err());
    let w = wire::genesis_layout(HEADER, layout).unwrap();
    assert_eq!(w, wire::genesis(HEADER).unwrap());
    let bytes = wire::encode_layout_witness(HEADER, w, layout);
    assert!(wire::decode_witness(HEADER, &bytes).is_err());
    assert!(
        wire::decode_layout_witness(HEADER, &bytes, WalLayout::PreSized { capacity: CAP * 2 })
            .is_err()
    );
}
#[test]
fn fully_written_capacity_constant_and_unselected_clear_after_callback() {
    for base in [Path::new("/tmp"), Path::new("/dev/shm")] {
        let t = Temp::new(base, "presized");
        let selected = committed(&t, 2);
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(t.wal())
            .unwrap();
        assert_eq!(f.metadata().unwrap().len(), CAP);
        assert!(f.metadata().unwrap().len() > selected.end().unwrap());
        f.write_all_at(b"unselected garbage", selected.end().unwrap() + 32)
            .unwrap();
        let before = std::fs::read(t.wal()).unwrap();
        drop(f);
        assert!(
            Journal::open_checked(&t.wal(), |_| Err::<(), _>(Error::Corrupt("typed graph")))
                .is_err()
        );
        assert_eq!(std::fs::read(t.wal()).unwrap(), before);
        let (j, _) = Journal::open_checked(&t.wal(), |_| Ok(())).unwrap();
        assert_eq!(j.selected(), selected);
        assert_eq!(j.physical_bytes().unwrap(), CAP);
        assert!(
            std::fs::read(t.wal()).unwrap()[selected.end().unwrap() as usize..]
                .iter()
                .all(|b| *b == 0)
        );
    }
}
#[test]
fn torn_witness_requires_one_successor_and_zero_entire_unused_suffix() {
    for bad_tail in [false, true] {
        let t = Temp::new(Path::new("/tmp"), "presized-heal");
        let selected = committed(&t, 2);
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(t.wal())
            .unwrap();
        f.write_all_at(&[0xff], wire::WITNESS_OFFSETS[0]).unwrap();
        if bad_tail {
            f.write_all_at(&[1], CAP - 1).unwrap();
        }
        let bytes = std::fs::read(t.wal()).unwrap();
        let plan = recovery::inspect(&f);
        drop(f);
        if bad_tail {
            assert!(plan.is_err());
            assert!(Journal::open_checked(&t.wal(), |_| Ok(())).is_err());
            assert_eq!(std::fs::read(t.wal()).unwrap(), bytes);
        } else {
            assert!(plan.unwrap().heal);
            let (j, _) = Journal::open_checked(&t.wal(), |_| Ok(())).unwrap();
            assert_eq!(j.selected(), selected);
        }
    }
    let t = Temp::new(Path::new("/tmp"), "presized-missing");
    let j = create(&t);
    drop(j);
    let f = OpenOptions::new().write(true).open(t.wal()).unwrap();
    f.write_all_at(&[0xff], wire::WITNESS_OFFSETS[0]).unwrap();
    drop(f);
    assert!(Journal::open_checked(&t.wal(), |_| Ok(())).is_err());
}
#[test]
fn extra_valid_or_truncated_successor_cannot_hide_in_capacity() {
    for tail in [false, true] {
        let t = Temp::new(Path::new("/tmp"), "presized-extra");
        let mut j = create(&t);
        let first = small_record(&j);
        j.commit_checked(&first, &Registry::default(), |_| Ok(()))
            .unwrap();
        let second = small_record(&j);
        let encoded = crate::hybrid::codec::encode(&second).unwrap();
        let end = j.selected().end().unwrap();
        drop(j);
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(t.wal())
            .unwrap();
        f.write_all_at(
            &encoded.bytes[..if tail {
                encoded.bytes.len() / 2
            } else {
                encoded.bytes.len()
            }],
            end,
        )
        .unwrap();
        // Invalid latest head anchors genesis; record1 is valid but record2 or
        // its partial bytes make the unused suffix noncanonical.
        f.write_all_at(&[0xff], wire::WITNESS_OFFSETS[1]).unwrap();
        drop(f);
        let before = std::fs::read(t.wal()).unwrap();
        assert!(Journal::open_checked(&t.wal(), |_| Ok(())).is_err());
        assert_eq!(std::fs::read(t.wal()).unwrap(), before);
    }
}
#[test]
fn acknowledged_corruption_and_physical_size_mismatch_fail_closed() {
    for case in 0..3 {
        let t = Temp::new(Path::new("/tmp"), "presized-bad");
        let r = committed(&t, 2);
        let f = OpenOptions::new().write(true).open(t.wal()).unwrap();
        match case {
            0 => f.write_all_at(&[0xff], r.offset + 260).unwrap(),
            1 => f.set_len(CAP - 1).unwrap(),
            _ => f.set_len(CAP + 1).unwrap(),
        };
        drop(f);
        let before = std::fs::read(t.wal()).unwrap();
        assert!(Journal::open_checked(&t.wal(), |_| Ok(())).is_err());
        assert_eq!(std::fs::read(t.wal()).unwrap(), before);
    }
}
#[test]
fn capacity_exhaustion_is_known_nonpublication_without_callback_or_write() {
    let t = Temp::new(Path::new("/tmp"), "presized-full");
    let mut j = Journal::create_with_layout(
        &t.wal(),
        HEADER,
        WalLayout::PreSized {
            capacity: wire::DATA_START,
        },
    )
    .unwrap();
    let before = std::fs::read(t.wal()).unwrap();
    let mut called = false;
    let r = small_record(&j);
    let result = j.commit_checked(&r, &Registry::default(), |_| {
        called = true;
        Ok(())
    });
    assert!(matches!(result, Err(Error::LimitExceeded)));
    assert!(!called);
    assert!(!j.is_poisoned());
    assert_eq!(std::fs::read(t.wal()).unwrap(), before);
}
#[test]
fn dense_and_presized_have_identical_logical_receipts_and_record_bytes() {
    let a = Temp::new(Path::new("/tmp"), "dense-control");
    let b = Temp::new(Path::new("/tmp"), "presized-control");
    let mut dense = Journal::create(&a.wal(), HEADER).unwrap();
    let mut fixed = create(&b);
    for _ in 0..4 {
        assert_eq!(dense.selected(), fixed.selected());
        let x = small_record(&dense);
        let y = small_record(&fixed);
        assert_eq!(x, y);
        let receipt = dense
            .commit_checked(&x, &Registry::default(), |_| Ok(()))
            .unwrap();
        assert_eq!(
            receipt,
            fixed
                .commit_checked(&y, &Registry::default(), |_| Ok(()))
                .unwrap()
        );
        let start = receipt.offset as usize;
        let end = receipt.end().unwrap() as usize;
        assert_eq!(
            std::fs::read(a.wal()).unwrap()[start..end],
            std::fs::read(b.wal()).unwrap()[start..end]
        );
    }
}
