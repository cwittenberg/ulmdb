use std::{
    collections::BTreeMap,
    ops::Bound,
    path::{Path, PathBuf},
};
use ulmdb_core::storage::{Error, ReadStore, Reference, Result, WriteStore};
use ulmdb_kv::{Cursor, SortedBuilder, audit_tree, build_sorted, get, put};
use ulmdb_storage::{Arena, SegmentTable, create_layout, new_identity};
struct Directory(PathBuf);
impl Directory {
    fn new(parent: &Path) -> Self {
        let path = parent.join(format!("ulmdb-bulk-{:032x}", new_identity().unwrap()));
        create_layout(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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
            return Err(std::io::Error::other("injected allocation").into());
        }
        self.remaining -= 1;
        self.arena.allocate(bytes, alignment)
    }
    fn rewrite(&mut self, _: Reference, _: &[u8], _: usize) -> Result<Reference> {
        panic!("bulk construction rewrote an object")
    }
}
#[test]
fn mapped_builder_and_cold_reopen_preserve_old_roots_and_rollback_private_failures() {
    for parent in [std::env::temp_dir(), PathBuf::from("/dev/shm")] {
        let directory = Directory::new(&parent);
        let id = (1u64 << 40) + 17;
        let mut arena = Arena::new(SegmentTable::default(), id).unwrap();
        let old = put(&mut arena, Reference::NULL, b"old", b"kept").unwrap();
        let (old_table, _) = arena.seal(&directory.0).unwrap();
        let borrowed = get(&old_table, old, b"old").unwrap().unwrap();
        let mut target = Arena::new(old_table.clone(), id + 1).unwrap();
        let checkpoint = target.checkpoint().unwrap();
        let size = target.dirty_bytes();
        let revision = target.revision();
        let invalid = (0..1000u64)
            .map(|n| Ok((n.to_be_bytes().to_vec(), n.to_le_bytes().to_vec())))
            .chain(std::iter::once(Ok((
                0u64.to_be_bytes().to_vec(),
                vec![0x55; 65537],
            ))));
        assert!(matches!(
            build_sorted(&mut target, invalid),
            Err(Error::InvalidInput(_))
        ));
        assert!(
            target.dirty_bytes() > size,
            "late input failure may allocate private pages"
        );
        target.rollback_checkpoint(&checkpoint).unwrap();
        assert_eq!((target.dirty_bytes(), target.revision()), (size, revision));
        assert_eq!(get(&target, old, b"old").unwrap(), Some(&b"kept"[..]));
        let checkpoint = target.checkpoint().unwrap();
        let mut builder = SortedBuilder::new();
        let mut limited = FailAfter {
            arena: &mut target,
            remaining: 3,
        };
        let mut failed = false;
        for n in 0..4000u64 {
            if let Err(error) = builder.push(&mut limited, &n.to_be_bytes(), &n.to_le_bytes()) {
                assert!(matches!(error, Error::Io(_)));
                failed = true;
                break;
            }
        }
        assert!(failed);
        limited.remaining = usize::MAX;
        assert!(matches!(
            builder.push(&mut limited, b"later", b"value"),
            Err(Error::InvalidInput("sorted builder is poisoned"))
        ));
        assert!(matches!(
            builder.finish(&mut limited),
            Err(Error::InvalidInput("sorted builder is poisoned"))
        ));
        target.rollback_checkpoint(&checkpoint).unwrap();
        assert_eq!((target.dirty_bytes(), target.revision()), (size, revision));
        assert_eq!(get(&target, old, b"old").unwrap(), Some(&b"kept"[..]));
        let mut expected: BTreeMap<Vec<u8>, Vec<u8>> = (0..4000u64)
            .map(|n| {
                (
                    n.to_be_bytes().to_vec(),
                    if n % 5 == 0 {
                        Vec::new()
                    } else {
                        n.to_le_bytes().to_vec()
                    },
                )
            })
            .collect();
        expected.insert(Vec::new(), Vec::new());
        expected.insert(vec![0xff; 257], vec![0x41; 513]);
        expected.insert(vec![0xff; 4096], vec![0x62; 65537]);
        let built = build_sorted(
            &mut target,
            expected
                .iter()
                .map(|(k, v)| Ok((k.as_slice(), v.as_slice()))),
        )
        .unwrap();
        let (table, _) = target.seal(&directory.0).unwrap();
        assert_eq!(get(&old_table, old, b"old").unwrap(), Some(&b"kept"[..]));
        assert_eq!(borrowed, b"kept");
        let entries = table.entries();
        drop(table);
        let cold = SegmentTable::load(&directory.0, &entries).unwrap();
        let actual: Vec<_> =
            Cursor::new(&cold, built.root, Bound::Unbounded, Bound::Unbounded, false)
                .unwrap()
                .map(|pair| pair.map(|(k, v)| (k.to_vec(), v.to_vec())).unwrap())
                .collect();
        assert_eq!(actual, expected.into_iter().collect::<Vec<_>>());
        assert_eq!(
            audit_tree(&cold, built.root, u64::MAX).unwrap().pages,
            built.pages
        );
        assert_eq!(get(&cold, old, b"old").unwrap(), Some(&b"kept"[..]));
        assert_eq!(borrowed, b"kept");
    }
}
