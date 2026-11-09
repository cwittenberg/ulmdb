//! Raw wire fixtures exercise the controlled verifier independently of the
//! writers and their construction invariants.
use super::*;
use ulmdb_core::storage::WriteStore;

#[derive(Default)]
struct Store {
    bytes: Vec<u8>,
    value_limit: Option<usize>,
}
impl ReadStore for Store {
    fn max_value_size(&self) -> usize {
        self.value_limit
            .unwrap_or(ulmdb_core::ValueLimit::DEFAULT.get())
    }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment != 1 {
            return Err(Error::Corrupt("fixture missing segment"));
        }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes
            .get(start..start.checked_add(length).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("fixture unreadable extent"))
    }
}
impl WriteStore for Store {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        let offset = self.bytes.len().div_ceil(alignment) * alignment;
        self.bytes.resize(offset, 0);
        self.bytes.extend_from_slice(bytes);
        Ok(Reference {
            segment: 1,
            offset: offset as u64,
        })
    }
    fn rewrite(&mut self, _: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        self.allocate(bytes, alignment)
    }
}
fn missing() -> Reference {
    Reference {
        segment: 99,
        offset: 4096,
    }
}
fn leaf_cell(key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut cell = Vec::new();
    cell.extend_from_slice(&(key.len() as u16).to_le_bytes());
    cell.extend_from_slice(&0u16.to_le_bytes());
    cell.extend_from_slice(&(value.len() as u32).to_le_bytes());
    cell.extend_from_slice(key);
    cell.extend_from_slice(value);
    cell.resize(cell.len().next_multiple_of(2), 0);
    cell
}
fn branch_cell(key: &[u8], child: Reference) -> Vec<u8> {
    let mut cell = Vec::new();
    cell.extend_from_slice(&(key.len() as u16).to_le_bytes());
    cell.extend_from_slice(&0u16.to_le_bytes());
    cell.extend_from_slice(&child.encode());
    cell.extend_from_slice(key);
    cell.resize(cell.len().next_multiple_of(2), 0);
    cell
}
fn page(store: &mut Store, level: u16, first: Reference, cells: &[Vec<u8>]) -> Reference {
    let mut bytes = [0; 4096];
    bytes[0..2].copy_from_slice(&(if level == 0 { 2u16 } else { 3 }).to_le_bytes());
    bytes[2..4].copy_from_slice(&1u16.to_le_bytes());
    bytes[4..6].copy_from_slice(&level.to_le_bytes());
    bytes[6..8].copy_from_slice(&(cells.len() as u16).to_le_bytes());
    bytes[8..10].copy_from_slice(&((32 + 2 * cells.len()) as u16).to_le_bytes());
    bytes[16..32].copy_from_slice(&first.encode());
    let mut end = bytes.len();
    for (i, cell) in cells.iter().enumerate() {
        end -= cell.len();
        bytes[end..end + cell.len()].copy_from_slice(cell);
        bytes[32 + 2 * i..34 + 2 * i].copy_from_slice(&(end as u16).to_le_bytes());
    }
    bytes[10..12].copy_from_slice(&(end as u16).to_le_bytes());
    store.allocate(&bytes, 4096).unwrap()
}
fn checked_kv(store: &Store, root: Reference, budget: u64) -> Result<u64> {
    let control = BackupControl::default();
    let checked = Controlled {
        inner: store,
        control: &control,
    };
    kv(&checked, root, budget, None, false, None, &control)
}

#[test]
fn bytewise_single_pass_preserves_local_order_ancestor_bounds_levels_and_visit_budget() {
    for (left_key, level, corrupt) in [
        (b'a', 1, false),
        (b'z', 1, true),
        (b'm', 1, true),
        (b'a', 2, true),
    ] {
        let mut store = Store::default();
        let left = page(
            &mut store,
            0,
            Reference::NULL,
            &[leaf_cell(&[left_key], b"v")],
        );
        let right = page(&mut store, 0, Reference::NULL, &[leaf_cell(b"z", b"v")]);
        let root = page(&mut store, level, left, &[branch_cell(b"m", right)]);
        let result = checked_kv(&store, root, 3);
        if corrupt {
            assert!(matches!(result, Err(Error::Corrupt(_))));
        } else {
            assert_eq!(result.unwrap(), 3);
            assert!(matches!(
                checked_kv(&store, root, 2),
                Err(Error::LimitExceeded)
            ));
        }
    }
    let mut store = Store::default();
    let root = page(
        &mut store,
        0,
        Reference::NULL,
        &[leaf_cell(b"a", b"first"), leaf_cell(b"a", b"duplicate")],
    );
    assert!(matches!(
        checked_kv(&store, root, 1),
        Err(Error::Corrupt(_))
    ));

    // A nonempty aliased leaf cannot satisfy the disjoint child intervals.
    let shared = page(&mut store, 0, Reference::NULL, &[leaf_cell(b"a", b"v")]);
    let root = page(&mut store, 1, shared, &[branch_cell(b"m", shared)]);
    assert!(matches!(
        checked_kv(&store, root, 3),
        Err(Error::Corrupt(_))
    ));
    // Empty aliases remain legal, and each incoming edge consumes a visit.
    let shared = page(&mut store, 0, Reference::NULL, &[]);
    let root = page(&mut store, 1, shared, &[branch_cell(b"m", shared)]);
    assert_eq!(checked_kv(&store, root, 3).unwrap(), 3);
    assert!(matches!(
        checked_kv(&store, root, 2),
        Err(Error::LimitExceeded)
    ));
}

#[test]
fn bytewise_single_pass_checks_unused_separator_and_leaf_overflow_extents() {
    let mut store = Store::default();
    let left = page(&mut store, 0, Reference::NULL, &[]);
    let right = page(&mut store, 0, Reference::NULL, &[]);
    let mut separator = vec![255, 255, 1, 0];
    separator.extend_from_slice(&right.encode());
    separator.extend_from_slice(&missing().encode());
    let root = page(&mut store, 1, left, &[separator]);
    assert!(matches!(
        checked_kv(&store, root, 3),
        Err(Error::Corrupt(_))
    ));

    let mut cell = vec![2, 0, 2, 0];
    cell.extend_from_slice(&600u32.to_le_bytes());
    cell.extend_from_slice(b"aa");
    cell.extend_from_slice(&missing().encode());
    let root = page(&mut store, 0, Reference::NULL, &[cell]);
    assert!(matches!(
        checked_kv(&store, root, 1),
        Err(Error::Corrupt(_))
    ));
}

#[test]
fn bytewise_leaf_constraints_reject_duplicate_values_width_and_oversized_keys() {
    for (key, value, width) in [
        (b"ab".as_slice(), b"value".as_slice(), None),
        (b"ab".as_slice(), b"".as_slice(), Some(4)),
    ] {
        let mut store = Store::default();
        let root = page(&mut store, 0, Reference::NULL, &[leaf_cell(key, value)]);
        let control = BackupControl::default();
        assert!(matches!(
            kv(&store, root, 1, None, true, width, &control),
            Err(Error::Corrupt("ordered KV size/duplicate width"))
        ));
    }
    let mut store = Store::default();
    let key = store
        .put_payload(&vec![b'k'; store.max_key_size() + 1])
        .unwrap();
    let mut cell = vec![255, 255, 1, 0, 0, 0, 0, 0];
    cell.extend_from_slice(&key.encode());
    let root = page(&mut store, 0, Reference::NULL, &[cell]);
    assert!(matches!(
        checked_kv(&store, root, 1),
        Err(Error::Corrupt(_))
    ));
}

fn prefix(v6: bool, network: u8, length: u8) -> Prefix {
    Prefix::new(
        if v6 {
            Address::V6(u128::from(network) << 120)
        } else {
            Address::V4(u32::from(network) << 24)
        },
        length,
    )
    .unwrap()
}
fn node(
    store: &mut Store,
    prefix: Prefix,
    children: [Reference; 2],
    value: Reference,
) -> Reference {
    let mut bytes = [0; 96];
    bytes[..8].copy_from_slice(&[4, 0, 0, 0, 96, 0, 0, 0]);
    bytes[9] = prefix.length();
    match prefix.network() {
        Address::V4(network) => {
            bytes[8] = 4;
            bytes[16..20].copy_from_slice(&network.to_be_bytes());
        }
        Address::V6(network) => {
            bytes[8] = 6;
            bytes[16..32].copy_from_slice(&network.to_be_bytes());
        }
    }
    bytes[32..48].copy_from_slice(&children[0].encode());
    bytes[48..64].copy_from_slice(&children[1].encode());
    bytes[64..80].copy_from_slice(&value.encode());
    store.allocate(&bytes, 16).unwrap()
}
fn checked_prefixes(store: &Store, root: Reference, v6: bool) -> Result<()> {
    let control = BackupControl::default();
    let checked = Controlled {
        inner: store,
        control: &control,
    };
    prefixes(
        &checked,
        root,
        if v6 {
            DatabaseKind::Ipv6
        } else {
            DatabaseKind::Ipv4
        },
        &control,
    )
}

#[test]
fn full_prefix_traversal_rejects_bad_side_containment_length_family_and_cycles() {
    for v6 in [false, true] {
        for (network, length, side, wrong_family, corrupt) in [
            (0, 2, 0, false, false),
            (0, 2, 1, false, true),
            (192, 2, 0, false, true),
            (0, 1, 0, false, true),
            (0, 2, 0, true, true),
        ] {
            let mut store = Store::default();
            let value = store.put_payload(b"value").unwrap();
            let child = node(
                &mut store,
                prefix(v6 ^ wrong_family, network, length),
                [Reference::NULL; 2],
                value,
            );
            let mut children = [Reference::NULL; 2];
            children[side] = child;
            let root = node(&mut store, prefix(v6, 0, 1), children, Reference::NULL);
            let result = checked_prefixes(&store, root, v6);
            if corrupt {
                assert!(matches!(result, Err(Error::Corrupt(_))));
            } else {
                result.unwrap();
            }
        }
        let mut store = Store::default();
        let root = node(
            &mut store,
            prefix(v6, 0, 1),
            [Reference::NULL; 2],
            Reference::NULL,
        );
        let offset = root.offset as usize;
        store.bytes[offset + 32..offset + 48].copy_from_slice(&root.encode());
        assert!(matches!(
            checked_prefixes(&store, root, v6),
            Err(Error::Corrupt(_))
        ));
    }
}

#[test]
fn full_prefix_traversal_checks_payload_extent_family_and_canonical_node_geometry() {
    for v6 in [false, true] {
        let mut store = Store::default();
        let value = store.put_payload(b"value").unwrap();
        let root = node(&mut store, prefix(v6, 0, 2), [Reference::NULL; 2], value);
        checked_prefixes(&store, root, v6).unwrap();
        store.value_limit = Some(3);
        assert!(matches!(
            checked_prefixes(&store, root, v6),
            Err(Error::Corrupt(_))
        ));
        store.value_limit = None;
        assert!(matches!(
            checked_prefixes(&store, root, !v6),
            Err(Error::Corrupt(_))
        ));
        let original = store.bytes.clone();
        let offset = root.offset as usize;
        store.bytes[offset + 64..offset + 80].copy_from_slice(&missing().encode());
        assert!(matches!(
            checked_prefixes(&store, root, v6),
            Err(Error::Corrupt(_))
        ));
        store.bytes = original.clone();
        store.bytes[value.offset as usize + 8..value.offset as usize + 16]
            .copy_from_slice(&1_048_576u64.to_le_bytes());
        assert!(matches!(
            checked_prefixes(&store, root, v6),
            Err(Error::Corrupt(_)) | Err(Error::LimitExceeded)
        ));
        store.bytes = original.clone();
        store.bytes[offset + 16] = 1; // Host bits in a /2 network.
        assert!(matches!(
            checked_prefixes(&store, root, v6),
            Err(Error::Corrupt(_))
        ));
        store.bytes = original;
        store.bytes[offset + 9] = if v6 { 128 } else { 32 };
        store.bytes[offset + 32..offset + 48].copy_from_slice(&root.encode());
        assert!(matches!(
            checked_prefixes(&store, root, v6),
            Err(Error::Corrupt(_))
        ));
    }
}
