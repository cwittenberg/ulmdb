use std::path::{Path, PathBuf};
use ulmdb_core::{
    Address, Prefix,
    storage::{Error, ReadStore, Reference, Result, WriteStore},
};
use ulmdb_prefix::{build_sorted, exact, insert, subnets};
use ulmdb_storage::{Arena, SegmentBuilder, SegmentTable, create_layout, new_identity};
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        let path = parent.join(format!(
            "ulmdb-prefix-bulk-{:032x}",
            new_identity().unwrap()
        ));
        create_layout(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn p(address: Address, length: u8) -> Prefix {
    Prefix::normalize(address, length).unwrap()
}
struct FailAfter<'a> {
    arena: &'a mut Arena,
    remaining: usize,
}
impl ReadStore for FailAfter<'_> {
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        self.arena.read(at, length)
    }
}
impl WriteStore for FailAfter<'_> {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        if self.remaining == 0 {
            return Err(std::io::Error::other("injected allocation failure").into());
        }
        self.remaining -= 1;
        self.arena.allocate(bytes, alignment)
    }
    fn rewrite(&mut self, _: Reference, _: &[u8], _: usize) -> Result<Reference> {
        panic!("sorted construction must not rewrite existing objects")
    }
}
#[test]
fn mapped_build_reopen_and_checkpoint_rollback_preserve_old_roots() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        let dir = Directory::new(&parent);
        let id = (1u64 << 40) + 33;
        let mut arena = Arena::new(SegmentTable::default(), id).unwrap();
        let old = insert(&mut arena, Reference::NULL, p(Address::V4(0), 0), b"old").unwrap();
        let (old_table, _) = arena.seal(&dir.0).unwrap();
        let borrowed = exact(&old_table, old, p(Address::V4(0), 0))
            .unwrap()
            .unwrap()
            .1;
        let mut target = Arena::new(old_table.clone(), id + 1).unwrap();
        let checkpoint = target.checkpoint().unwrap();
        let before = (target.dirty_bytes(), target.revision());
        let rows = (0..1000u32)
            .map(|n| Ok((p(Address::V4(n), 32), n.to_be_bytes().to_vec())))
            .chain(std::iter::once(Err(Error::Corrupt("late source failure"))));
        assert!(matches!(
            build_sorted(&mut target, rows),
            Err(Error::Corrupt("late source failure"))
        ));
        assert!(target.dirty_bytes() > before.0);
        target.rollback_checkpoint(&checkpoint).unwrap();
        assert_eq!((target.dirty_bytes(), target.revision()), before);
        assert_eq!(
            exact(&target, old, p(Address::V4(0), 0))
                .unwrap()
                .unwrap()
                .1,
            b"old"
        );
        let checkpoint = target.checkpoint().unwrap();
        let failed = build_sorted(
            &mut FailAfter {
                arena: &mut target,
                remaining: 37,
            },
            (0..1000u32).map(|n| Ok((p(Address::V4(n), 32), n.to_be_bytes()))),
        );
        assert!(matches!(failed, Err(Error::Io(_))));
        assert!(target.dirty_bytes() > before.0);
        target.rollback_checkpoint(&checkpoint).unwrap();
        assert_eq!((target.dirty_bytes(), target.revision()), before);
        assert_eq!(
            exact(&target, old, p(Address::V4(0), 0))
                .unwrap()
                .unwrap()
                .1,
            b"old"
        );
        let expected: Vec<_> = (0..4000u128)
            .map(|n| {
                (
                    p(Address::V6(n << 64 | n), 128),
                    if n % 5 == 0 {
                        Vec::new()
                    } else {
                        n.to_be_bytes().to_vec()
                    },
                )
            })
            .collect();
        let built = build_sorted(
            &mut target,
            expected.iter().map(|(p, v)| Ok((*p, v.as_slice()))),
        )
        .unwrap();
        let (table, _) = target.seal(&dir.0).unwrap();
        let entries = table.entries();
        drop(table);
        let cold = SegmentTable::load(&dir.0, &entries).unwrap();
        let actual: Vec<_> = subnets(&cold, built.root, p(Address::V6(0), 0))
            .unwrap()
            .map(|r| {
                let (p, v) = r.unwrap();
                (p, v.to_vec())
            })
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(borrowed, b"old");
        assert_eq!(
            exact(&cold, old, p(Address::V4(0), 0)).unwrap().unwrap().1,
            b"old"
        );
    }
}
#[test]
fn maximum_values_roll_across_segments_without_forward_nodes_or_lost_bytes() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        let dir = Directory::new(&parent);
        let mut target = SegmentBuilder::new(&dir.0, (1u64 << 40) + 64).unwrap();
        let mut value = vec![0x5b; 67_108_864];
        value[0] = 0xa3;
        let end = value.len() - 1;
        value[end] = 0x17;
        let built = build_sorted(
            &mut target,
            (0..5u32).map(|n| Ok((p(Address::V4(n), 32), value.as_slice()))),
        )
        .unwrap();
        let (table, _) = target.finish().unwrap();
        assert!(table.entries().len() >= 2);
        assert_eq!(built.records, 5);
        let entries = table.entries();
        drop(table);
        let cold = SegmentTable::load(&dir.0, &entries).unwrap();
        for n in 0..5u32 {
            let actual = exact(&cold, built.root, p(Address::V4(n), 32))
                .unwrap()
                .unwrap()
                .1;
            assert_eq!(actual, value);
        }
        let mut arena = Arena::new(cold, (1u64 << 40) + 70).unwrap();
        value.push(0);
        assert!(matches!(
            build_sorted(&mut arena, [Ok((p(Address::V4(9), 32), value.as_slice()))]),
            Err(Error::LimitExceeded)
        ));
        assert_eq!(arena.dirty_bytes(), 0);
    }
}
