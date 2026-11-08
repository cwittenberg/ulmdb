//! Structural work budgets on real immutable mappings, independent of timings.
use std::{cell::Cell, fs, path::PathBuf};
use ulmdb_core::{
    Address, Prefix,
    storage::{ReadStore, Reference, Result},
};
use ulmdb_storage::{Arena, SegmentTable, create_layout, new_identity};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("ulmdb-scale-work-{:032x}", new_identity().unwrap()));
        create_layout(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Counted<'a> {
    inner: &'a SegmentTable,
    reads: Cell<usize>,
    bytes: Cell<usize>,
}
impl ReadStore for Counted<'_> {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        self.reads.set(self.reads.get() + 1);
        self.bytes.set(self.bytes.get() + length);
        self.inner.read(at, length)
    }
}
impl<'a> Counted<'a> {
    fn new(inner: &'a SegmentTable) -> Self {
        Self {
            inner,
            reads: Cell::new(0),
            bytes: Cell::new(0),
        }
    }
    fn reset(&self) {
        self.reads.set(0);
        self.bytes.set(0);
    }
}
fn address(n: u32, v6: bool) -> Address {
    // Spread records across the address space, including early branch bits.
    let bits = n.wrapping_mul(0x9e37_79b1);
    if v6 {
        Address::V6((u128::from(bits) << 96) | u128::from(n))
    } else {
        Address::V4(bits)
    }
}
fn host(n: u32, v6: bool) -> Prefix {
    Prefix::new(address(n, v6), if v6 { 128 } else { 32 }).unwrap()
}

#[test]
fn prefix_point_work_is_bounded_by_address_width_as_records_grow() {
    for v6 in [false, true] {
        let width = if v6 { 128 } else { 32 };
        for count in [32u32, 1024, 32768] {
            let directory = Directory::new();
            let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
            let mut root = Reference::NULL;
            for n in 0..count {
                root =
                    ulmdb_prefix::insert(&mut arena, root, host(n, v6), &n.to_be_bytes()).unwrap();
            }
            let (mapped, _) = arena.seal(&directory.0).unwrap();
            let counted = Counted::new(&mapped);
            // A valid radix path strictly increases depth; one node per bit
            // plus the root, then two exact payload reads for a hit.
            let bound = width + 1 + 2;
            for n in [0, count / 2, count - 1, count, u32::MAX] {
                let expected = (n < count).then(|| n.to_be_bytes());
                for exact in [false, true] {
                    counted.reset();
                    let found = if exact {
                        ulmdb_prefix::exact(&counted, root, host(n, v6))
                    } else {
                        ulmdb_prefix::longest_match(&counted, root, address(n, v6))
                    }
                    .unwrap();
                    assert_eq!(
                        found.map(|(_, bytes)| bytes),
                        expected.as_ref().map(|v| v.as_slice())
                    );
                    assert!(
                        counted.reads.get() <= bound,
                        "{count} records, IPv{}: {} reads",
                        if v6 { 6 } else { 4 },
                        counted.reads.get()
                    );
                    assert!(counted.bytes.get() <= (width + 1) * 96 + 20);
                }
            }
            // Replacing one immutable value copies a path, never the full trie.
            let mut writer = Arena::new(mapped.clone(), 2).unwrap();
            let changed =
                ulmdb_prefix::insert(&mut writer, root, host(count / 2, v6), b"next").unwrap();
            assert!(writer.dirty_bytes() <= (width + 1) * 96 + 32);
            assert_eq!(
                ulmdb_prefix::exact(&mapped, root, host(count / 2, v6))
                    .unwrap()
                    .unwrap()
                    .1,
                (count / 2).to_be_bytes()
            );
            assert_eq!(
                ulmdb_prefix::exact(&writer, changed, host(count / 2, v6))
                    .unwrap()
                    .unwrap()
                    .1,
                b"next"
            );
        }
    }
}

#[test]
fn deepest_prefix_chains_keep_the_same_fixed_work_ceiling() {
    for v6 in [false, true] {
        let width = if v6 { 128 } else { 32 };
        let address = if v6 {
            Address::V6(u128::MAX)
        } else {
            Address::V4(u32::MAX)
        };
        let directory = Directory::new();
        let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
        let mut root = Reference::NULL;
        for length in 0..=width {
            let prefix = Prefix::normalize(address, length as u8).unwrap();
            root = ulmdb_prefix::insert(&mut arena, root, prefix, &[length as u8]).unwrap();
        }
        let (mapped, _) = arena.seal(&directory.0).unwrap();
        let counted = Counted::new(&mapped);
        assert_eq!(
            ulmdb_prefix::get(&counted, root, address)
                .unwrap()
                .unwrap()
                .1,
            &[width as u8]
        );
        assert_eq!(counted.reads.get(), width + 3);
        let mut writer = Arena::new(mapped, 2).unwrap();
        ulmdb_prefix::insert(
            &mut writer,
            root,
            Prefix::new(address, width as u8).unwrap(),
            b"z",
        )
        .unwrap();
        assert!(writer.dirty_bytes() <= (width + 1) * 96 + 32);
    }
}

#[test]
fn inline_kv_point_reads_and_replacements_touch_only_a_tree_path() {
    for count in [32u64, 1024, 32768] {
        let directory = Directory::new();
        let mut arena = Arena::new(SegmentTable::default(), 1).unwrap();
        let mut root = Reference::NULL;
        for n in 0..count {
            root = ulmdb_kv::put(&mut arena, root, &n.to_be_bytes(), &n.to_be_bytes()).unwrap();
        }
        let (mapped, _) = arena.seal(&directory.0).unwrap();
        let height = usize::from(ulmdb_kv::height(&mapped, root).unwrap());
        assert!(
            height <= 3,
            "unexpected small-record tree growth: {count}/{height}"
        );
        let counted = Counted::new(&mapped);
        for n in [0, count / 2, count - 1, count, u64::MAX] {
            counted.reset();
            let expected = (n < count).then(|| n.to_be_bytes());
            assert_eq!(
                ulmdb_kv::get(&counted, root, &n.to_be_bytes()).unwrap(),
                expected.as_ref().map(|v| v.as_slice())
            );
            assert_eq!(
                counted.reads.get(),
                height,
                "{count} records: point lookup scanned outside its path"
            );
            assert_eq!(counted.bytes.get(), height * ulmdb_kv::PAGE_SIZE);
        }
        let mut writer = Arena::new(mapped.clone(), 2).unwrap();
        let changed =
            ulmdb_kv::put(&mut writer, root, &(count / 2).to_be_bytes(), b"changed!").unwrap();
        assert!(writer.dirty_bytes() <= height * ulmdb_kv::PAGE_SIZE);
        assert_eq!(
            ulmdb_kv::get(&mapped, root, &(count / 2).to_be_bytes())
                .unwrap()
                .unwrap(),
            (count / 2).to_be_bytes()
        );
        assert_eq!(
            ulmdb_kv::get(&writer, changed, &(count / 2).to_be_bytes())
                .unwrap()
                .unwrap(),
            b"changed!"
        );
    }
}
