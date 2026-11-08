use super::*;
use crate::multibit::view::NodeView;

type Shape = (Prefix, [Reference; 15], [Reference; 16]);

fn shape(node: Node) -> Shape {
    (node.base, node.values, node.children)
}

fn decoded_pair(store: &Memory, root: Reference) -> (Result<Shape>, Result<Shape>) {
    let owned = Node::read(store, root, &mut Visits::new(Budget::default())).map(shape);
    let borrowed = NodeView::read(store, root, &mut Visits::new(Budget::default()))
        .and_then(NodeView::expand)
        .map(shape);
    (owned, borrowed)
}

#[test]
fn every_full_node_byte_mutation_matches_frozen_owned_decoder() {
    for address in [Address::V4(0), Address::V6(0)] {
        let mut store = Memory::default();
        let mut node = Node::empty(p(address, 0));
        for (i, reference) in node.values.iter_mut().chain(&mut node.children).enumerate() {
            *reference = store.put_payload(&[i as u8]).unwrap();
        }
        let root = node.persist(&mut store, None).unwrap();
        let start = root.offset as usize;
        let end = store.bytes.len();
        assert_eq!(end - start, wire::MAX_SIZE);
        let original = store.bytes[start..end].to_vec();
        let mut checked = 0;
        for (offset, original_byte) in original.iter().copied().enumerate() {
            for replacement in 0..=u8::MAX {
                store.bytes[start + offset] = replacement;
                let (owned, borrowed) = decoded_pair(&store, root);
                assert_eq!(
                    owned.map_err(|e| e.to_string()),
                    borrowed.map_err(|e| e.to_string()),
                    "family={address:?}, byte={offset}, replacement={replacement}"
                );
                checked += 1;
            }
            store.bytes[start + offset] = original_byte;
        }
        assert_eq!(checked, 143_360);
    }
}

#[test]
fn all_packed_bitmap_positions_and_sparse_ranks_match_owned_fields() {
    let mut store = Memory::default();
    let payloads: Vec<_> = (0..31u8)
        .map(|i| store.put_payload(&[i]).unwrap())
        .collect();
    // Every 16-bit mask covers every bit position, every possible population
    // count, and every sparse rank. Children are leaves only in route tests;
    // here the decoder checks reference encoding, as the frozen decoder does.
    for mask in 0..=u16::MAX {
        let mut node = Node::empty(p(Address::V6(0), 0));
        for (i, value) in node.values.iter_mut().enumerate() {
            if mask & (1 << i) != 0 {
                *value = payloads[i];
            }
        }
        for (i, child) in node.children.iter_mut().enumerate() {
            if mask.rotate_left(7) & (1 << i) != 0 {
                *child = payloads[15 + i];
            }
        }
        let (v, c) = node.masks();
        if v == 0 && c.count_ones() < 2 {
            continue;
        }
        // Keep the fixture bounded instead of retaining every historical node.
        let old = store.bytes.len();
        let root = node.persist(&mut store, None).unwrap();
        let (owned, borrowed) = decoded_pair(&store, root);
        assert_eq!(owned.unwrap(), shape(node));
        assert_eq!(borrowed.unwrap(), shape(node));
        store.bytes.truncate(old);
    }
}

#[test]
fn malformed_descendant_family_is_rejected_by_point_and_cursor_reads() {
    let mut store = Memory::default();
    let mut root = Reference::NULL;
    for (prefix, value) in [
        (p(Address::V6(0), 0), b"root"),
        (p(Address::V6(0), 8), b"leaf"),
    ] {
        root = insert(&mut store, root, prefix, value).unwrap();
    }
    let node = Node::read(&store, root, &mut Visits::new(Budget::default())).unwrap();
    let child = node.children.into_iter().find(|r| !r.is_null()).unwrap();
    let at = child.offset as usize;
    // Both encodings are valid for an all-zero /8 child. Its parent edge is
    // what makes the changed IPv4 family invalid in this IPv6 tree.
    store.bytes[at + 8] = 4;
    assert!(NodeView::read(&store, child, &mut Visits::new(Budget::default())).is_ok());
    let reader = Reader::new(&store, root).unwrap();
    assert!(reader.longest_match(Address::V6(0)).is_err());
    assert!(reader.exact(p(Address::V6(0), 8)).is_err());
    assert!(reader.covering(p(Address::V6(0), 8)).is_err());
    assert!(ancestors(&store, root, p(Address::V6(0), 8)).is_err());
    for selection in [Selection::Subnets, Selection::Overlaps] {
        let mut cursor = Cursor::new(
            &store,
            root,
            p(Address::V6(0), 0),
            selection,
            Budget::default(),
        )
        .unwrap();
        assert!(cursor.any(|item| item.is_err()));
        assert!(cursor.next().is_none());
    }
    // Removing the parent's last value promotes its sole surviving child.
    // Promotion must validate the edge too, rather than changing root family.
    assert!(delete(&mut store, root, p(Address::V6(0), 0)).is_err());
}

#[test]
fn prepared_root_and_full_node_reference_validation_remain_active() {
    let mut store = Memory::default();
    let mut node = Node::empty(p(Address::V4(0), 0));
    node.values[0] = store.put_payload(b"default").unwrap();
    node.values[14] = store.put_payload(b"unselected").unwrap();
    let root = node.persist(&mut store, None).unwrap();
    let reader = Reader::new(&store, root).unwrap();
    assert_eq!(
        reader.longest_match(Address::V4(0)).unwrap().unwrap().1,
        b"default"
    );
    assert!(std::mem::size_of::<NodeView<'_>>() < std::mem::size_of::<Node>());
    let at = root.offset as usize + HEADER + 16;
    // Corrupt the reference not selected by the queried address. Preparing the
    // borrowed root must still fail, preserving the prior full-node checks.
    store.bytes[at..at + 16].fill(0);
    assert!(Reader::new(&store, root).is_err());
    assert!(longest_match(&store, root, Address::V4(0)).is_err());
}
