use super::*;
pub(super) fn original_plan<S: WriteStore>(
    store: &S,
    root: Reference,
    key: &[u8],
    value: Option<&[u8]>,
    expected: Option<u16>,
) -> Result<Plan> {
    let page = Page::load(store, root)?;
    if expected.is_some_and(|level| level != page.level) {
        return Err(Error::Corrupt("tree level mismatch"));
    }
    let index = page.search(store, key, page.level != 0)?;
    if page.level == 0 {
        let exists = index < page.count && page.key_at(store, index)? == key;
        if value.is_none() && !exists {
            return Ok(Plan::Noop);
        }
        if let Some(value) = value.filter(|_| exists) {
            if let Some(layout) = page.compact() {
                let cell = page.cell(store, index)?;
                if cell.value == value {
                    return Ok(Plan::Noop);
                }
                if cfg!(feature = "compact-pages") && layout.value == value.len() {
                    return Ok(Plan::PatchValue {
                        offset: cell.start + layout.key,
                    });
                }
                return Ok(Plan::Leaf { index, exists });
            }
            let raw = page.raw_cell(index)?;
            let flags = page::u16_at(raw, 2)?;
            // The encoded inline span must already have this exact size.
            // An overflow payload's semantic length is never an inline span.
            if flags & 2 == 0
                && u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize == value.len()
            {
                let cell = page.cell(store, index)?;
                if cell.value == value {
                    return Ok(Plan::Noop);
                }
                let key_size = if flags & 1 != 0 {
                    16
                } else {
                    usize::from(page::u16_at(raw, 0)?)
                };
                let offset = cell
                    .start
                    .checked_add(8 + key_size)
                    .ok_or(Error::LimitExceeded)?;
                return Ok(Plan::PatchValue { offset });
            }
        }
        return Ok(Plan::Leaf { index, exists });
    }
    let child = page.child(store, index)?;
    let offset = page.child_offset(index)?;
    Ok(Plan::Branch {
        level: page.level,
        index,
        child,
        offset,
        last_child: index == page.count,
        migrate: page.compact().is_some() && !cfg!(feature = "compact-pages"),
    })
}

use std::cell::RefCell;
use ulmdb_core::storage::ReadStore;
#[derive(Default)]
struct Store {
    bytes: Vec<u8>,
    reads: RefCell<Vec<(Reference, usize)>>,
}
impl ReadStore for Store {
    fn read(&self, at: Reference, len: usize) -> Result<&[u8]> {
        self.reads.borrow_mut().push((at, len));
        if at.segment != 1 {
            return Err(Error::Corrupt("oracle segment"));
        }
        let p = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes
            .get(p..p.checked_add(len).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("oracle extent"))
    }
}
impl WriteStore for Store {
    fn allocate(&mut self, bytes: &[u8], align: usize) -> Result<Reference> {
        let p = self.bytes.len().div_ceil(align) * align;
        self.bytes.resize(p, 0);
        self.bytes.extend_from_slice(bytes);
        Ok(Reference {
            segment: 1,
            offset: p as u64,
        })
    }
    fn rewrite(&mut self, _: Reference, bytes: &[u8], align: usize) -> Result<Reference> {
        self.allocate(bytes, align)
    }
}
fn outcome(plan: Result<Plan>) -> String {
    match plan {
        Err(e) => format!("{e:?}"),
        Ok(Plan::Noop) => "noop".into(),
        Ok(Plan::PatchValue { offset }) => format!("patch:{offset}"),
        Ok(Plan::Leaf { index, exists }) => format!("leaf:{index}:{exists}"),
        Ok(Plan::Branch {
            level,
            index,
            child,
            offset,
            last_child,
            migrate,
        }) => format!("branch:{level}:{index}:{child:?}:{offset}:{last_child}:{migrate}"),
    }
}
fn compare(
    store: &Store,
    root: Reference,
    key: &[u8],
    value: Option<&[u8]>,
    expected: Option<u16>,
) {
    store.reads.borrow_mut().clear();
    let a = outcome(original_plan(store, root, key, value, expected));
    let reads = store.reads.borrow().clone();
    store.reads.borrow_mut().clear();
    let b = outcome(super::plan(store, root, key, value, expected));
    assert_eq!(a, b, "key={key:?} valueLen={:?}", value.map(<[u8]>::len));
    assert_eq!(reads, *store.reads.borrow());
}
#[test]
fn packed_hit_miss_delete_and_value_sizes_keep_frozen_plan_errors_offsets_and_store_reads() {
    for (k, v) in [(0, 0), (0, 5), (1, 0), (3, 5), (8, 8), (16, 0), (256, 512)] {
        for count in [
            0,
            if k == 0 {
                1
            } else {
                crate::compact::Layout { key: k, value: v }
                    .capacity(true)
                    .min(17)
            },
        ] {
            let mut store = Store::default();
            let mut cells = Vec::new();
            let mut keys = Vec::new();
            for n in 0..count {
                let mut key = vec![0; k];
                if k > 0 {
                    key[k - 1] = (n * 2) as u8;
                }
                keys.push(key.clone());
                cells.push(leaf_cell(&mut store, &key, &vec![n as u8; v], None).unwrap());
            }
            let refs: Vec<_> = cells.iter().map(CellBuffer::as_bytes).collect();
            let bytes = crate::compact::encode(
                0,
                Reference::NULL,
                &refs,
                crate::compact::Layout { key: k, value: v },
            )
            .unwrap();
            let root = store.allocate(&bytes, PAGE_SIZE).unwrap();
            let mut queries = keys;
            queries.extend([vec![], vec![1; k], vec![255; k], vec![0; k + 1]]);
            for key in queries {
                for value in [
                    None,
                    Some(vec![]),
                    Some(vec![0; v]),
                    Some(vec![77; v]),
                    Some(vec![55; v + 1]),
                ] {
                    compare(&store, root, &key, value.as_deref(), None);
                    compare(&store, root, &key, value.as_deref(), Some(1));
                }
            }
        }
    }
}
#[test]
fn v1_mixed_overflow_and_corrupt_headers_preserve_frozen_access_order() {
    let mut store = Store::default();
    let mut entries = [
        (vec![], vec![]),
        (b"abc".to_vec(), vec![7; 4097]),
        (vec![0x55; 1025], vec![9; 8]),
    ];
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let cells: Vec<_> = entries
        .iter()
        .map(|(k, v)| leaf_cell(&mut store, k, v, None).unwrap())
        .collect();
    let refs: Vec<_> = cells.iter().map(CellBuffer::as_bytes).collect();
    let bytes = page::encode_v1(0, Reference::NULL, &refs).unwrap();
    let root = store.allocate(&bytes, PAGE_SIZE).unwrap();
    for key in entries
        .iter()
        .map(|(k, _)| k.as_slice())
        .chain([b"missing".as_slice(), b"ab", b"z"])
    {
        for val in [None, Some(b"".as_slice()), Some(b"changed".as_slice())] {
            compare(&store, root, key, val, None);
        }
    }
    for (at, n) in [
        (0, 3u16),
        (2, 99),
        (4, 64),
        (6, 65535),
        (8, 0),
        (10, 65535),
        (12, 65535),
        (14, 65535),
    ] {
        let mut bad = bytes;
        bad[at..at + 2].copy_from_slice(&n.to_le_bytes());
        let root = store.allocate(&bad, PAGE_SIZE).unwrap();
        compare(&store, root, b"abc", Some(b"value"), None);
    }
}
