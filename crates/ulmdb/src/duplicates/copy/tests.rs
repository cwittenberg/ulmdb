use super::*;
use ulmdb_storage::{Arena, SegmentTable};
#[test]
fn relocation_rejects_count_width_and_nonempty_child_values() {
    for (value, empty, count, width) in [
        (b"valid".as_slice(), b"".as_slice(), 2, None),
        (b"bad".as_slice(), b"".as_slice(), 1, Some(4)),
        (b"valid".as_slice(), b"nonempty".as_slice(), 1, None),
        (&vec![0x55; 65_537], b"".as_slice(), 1, None),
    ] {
        let mut source = Arena::new(SegmentTable::default(), 1).unwrap();
        let child = ulmdb_kv::put(&mut source, Reference::NULL, value, empty).unwrap();
        let outer = ulmdb_kv::put(
            &mut source,
            Reference::NULL,
            b"key",
            &tree_bytes(child, count),
        )
        .unwrap();
        let mut target = Arena::new(SegmentTable::default(), 2).unwrap();
        assert!(matches!(
            rebuild(&source, outer, &mut target, width),
            Err(Error::Corrupt(_))
        ));
    }
}
#[test]
fn legacy_small_subtree_demotes_to_inline_and_large_values_build_directly() {
    for values in [
        vec![Vec::new(), b"a".to_vec()],
        vec![vec![0x33; 65_536], vec![0x55; 65_536]],
    ] {
        let mut source = Arena::new(SegmentTable::default(), 1).unwrap();
        let mut child = Reference::NULL;
        for value in &values {
            child = ulmdb_kv::put(&mut source, child, value, b"").unwrap();
        }
        let outer = ulmdb_kv::put(
            &mut source,
            Reference::NULL,
            b"key",
            &tree_bytes(child, values.len() as u64),
        )
        .unwrap();
        let mut target = Arena::new(SegmentTable::default(), 2).unwrap();
        let (root, count) = rebuild(&source, outer, &mut target, None).unwrap();
        assert_eq!(count, values.len() as u64);
        equivalent(&source, outer, &target, root, None).unwrap();
        let bytes = ulmdb_kv::get(&target, root, b"key").unwrap().unwrap();
        let bucket = Bucket::decode(bytes, None).unwrap();
        assert_eq!(matches!(bucket, Bucket::Inline(_)), values[0].is_empty());
        let actual: Vec<_> = Values::new(&target, bucket)
            .unwrap()
            .map(|r| r.unwrap().to_vec())
            .collect();
        assert_eq!(actual, values);
    }
}
