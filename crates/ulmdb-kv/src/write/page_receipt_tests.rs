use super::*;
use crate::{Cursor, audit_tree, delete_with, get, get_with, height, put_with};
use std::{collections::{BTreeMap, BTreeSet}, ops::Bound};
use ulmdb_core::storage::ReadStore;

#[derive(Clone)]
struct Store {
    bytes: Vec<u8>,
    size: usize,
    base_end: usize,
    private: bool,
    patches: bool,
    receipts: Vec<Reference>,
    hook_calls: usize,
    fail_hook: Option<usize>,
    writes: usize,
}

impl Store {
    fn new(size: usize, private: bool, patches: bool) -> Self {
        Self {
            bytes: Vec::new(), size, base_end: 0, private, patches,
            receipts: Vec::new(), hook_calls: 0, fail_hook: None, writes: 0,
        }
    }

    fn begin(&mut self) {
        self.base_end = self.bytes.len();
        self.receipts.clear();
        self.hook_calls = 0;
        self.fail_hook = None;
    }
}

impl ReadStore for Store {
    fn page_size(&self) -> usize { self.size }
    fn read(&self, at: Reference, length: usize) -> Result<&[u8]> {
        if at.segment != 1 { return Err(Error::Corrupt("receipt test segment")); }
        let start = usize::try_from(at.offset).map_err(|_| Error::LimitExceeded)?;
        self.bytes.get(start..start.checked_add(length).ok_or(Error::LimitExceeded)?)
            .ok_or(Error::Corrupt("receipt test extent"))
    }
}

impl WriteStore for Store {
    fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
        let start = self.bytes.len().div_ceil(alignment) * alignment;
        self.bytes.resize(start, 0);
        self.bytes.extend_from_slice(bytes);
        self.writes += 1;
        Ok(Reference { segment: 1, offset: start as u64 })
    }
    fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
        if self.read(old, bytes.len())? == bytes { return Ok(old); }
        if self.private && old.offset as usize >= self.base_end {
            let start = old.offset as usize;
            self.bytes[start..start + bytes.len()].copy_from_slice(bytes);
            self.writes += 1;
            Ok(old)
        } else {
            self.allocate(bytes, alignment)
        }
    }
    fn supports_patch_many(&self, length: usize, alignment: usize) -> bool {
        self.patches && length == self.size && alignment == self.size
    }
    fn try_patch_many(&mut self, old: Reference, length: usize,
        patches: &[(usize, &[u8])], alignment: usize) -> Result<Option<Reference>> {
        if !self.supports_patch_many(length, alignment) { return Ok(None); }
        let mut bytes = self.read(old, length)?.to_vec();
        let mut end = 0;
        for &(at, patch) in patches {
            if at < end { return Err(Error::InvalidInput("receipt test patch order")); }
            end = at.checked_add(patch.len()).ok_or(Error::LimitExceeded)?;
            bytes.get_mut(at..end).ok_or(Error::InvalidInput("receipt test patch extent"))?
                .copy_from_slice(patch);
        }
        self.rewrite(old, &bytes, alignment).map(Some)
    }
    fn retire_page_edge(&mut self, old: Reference) -> Result<()> {
        let call = self.hook_calls;
        self.hook_calls += 1;
        if self.fail_hook == Some(call) {
            return Err(Error::Io(std::io::Error::other("injected page receipt failure")));
        }
        self.receipts.push(old);
        Ok(())
    }
}

// Independent oracle: traverse actual page child references in both roots. It
// does not use the editor's rewrite history, receipt list, or payload edges.
fn pages(store: &Store, root: Reference) -> BTreeSet<Reference> {
    let mut seen = BTreeSet::new();
    let mut pending = if root.is_null() { Vec::new() } else { vec![root] };
    while let Some(at) = pending.pop() {
        assert!(seen.insert(at), "fixture must have single-parent pages");
        let page = Page::load(store, at).unwrap();
        if page.level != 0 {
            for index in 0..=page.count {
                let child = page.child(store, index).unwrap();
                assert_eq!(Page::load(store, child).unwrap().level + 1, page.level);
                pending.push(child);
            }
        }
    }
    seen
}

fn exact_delta(store: &Store, before: &BTreeSet<Reference>, after: Reference) {
    let after = pages(store, after);
    for &at in &store.receipts {
        assert!(!at.is_null());
        Page::load(store, at).expect("receipts must describe pages, never payloads");
    }
    let recorded: Vec<_> = store.receipts.iter().copied().filter(|at| before.contains(at)).collect();
    let unique: BTreeSet<_> = recorded.iter().copied().collect();
    assert_eq!(recorded.len(), unique.len(), "committed page reported twice");
    assert_eq!(unique, before.difference(&after).copied().collect());
}

type Model = BTreeMap<Vec<u8>, Vec<u8>>;
fn key(n: u64) -> Vec<u8> { n.to_be_bytes().to_vec() }
fn check(store: &Store, root: Reference, model: &Model) {
    assert_eq!(audit_tree(store, root, 100_000).unwrap().leaf_records, model.len() as u64);
    for (key, value) in model { assert_eq!(get(store, root, key).unwrap(), Some(value.as_slice())); }
    let rows: Vec<_> = Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, false)
        .unwrap().map(|row| { let (k,v) = row.unwrap(); (k.to_vec(),v.to_vec()) }).collect();
    assert_eq!(rows, model.iter().map(|(k,v)| (k.clone(),v.clone())).collect::<Vec<_>>());
}

#[test]
fn ordinary_receipts_equal_base_minus_final_through_splits_pruning_and_collapse() {
    for size in [256, 1024, 4096] {
        for private in [false, true] {
            for patches in [false, true] {
                let mut store = Store::new(size, private, patches);
                let mut root = Reference::NULL;
                let mut model = Model::new();
                for n in 0..180u64 {
                    let value = vec![n as u8; 32];
                    root = put(&mut store, root, &key(n * 4), &value).unwrap();
                    model.insert(key(n * 4), value);
                }
                assert!(height(&store, root).unwrap() >= 2);
                store.begin();
                let before = pages(&store, root);
                let writes = store.writes;
                assert_eq!(put(&mut store, root, &key(0), &[0;32]).unwrap(), root);
                assert_eq!(delete(&mut store, root, b"absent").unwrap(), root);
                assert!(store.receipts.is_empty());
                assert_eq!(store.writes, writes);

                let held_root = root;
                let held_model = model.clone();
                // Same-length patches, non-tail insertion, and enough new rows
                // to exercise leaf/branch splits and private intermediate edits.
                for n in (0..180u64).step_by(7) {
                    let value = vec![0xa5;32];
                    root = put(&mut store, root, &key(n * 4), &value).unwrap();
                    model.insert(key(n * 4), value);
                }
                for n in (0..120u64).rev() {
                    let value = vec![0x5a;32];
                    root = put(&mut store, root, &key(n * 4 + 2), &value).unwrap();
                    model.insert(key(n * 4 + 2), value);
                }
                exact_delta(&store, &before, root);
                check(&store, root, &model);
                check(&store, held_root, &held_model);

                store.begin();
                let before = pages(&store, root);
                let held_root = root;
                let held_model = model.clone();
                let doomed: Vec<_> = model.keys().take(model.len()-1).cloned().collect();
                for k in doomed {
                    root = delete(&mut store, root, &k).unwrap();
                    model.remove(&k);
                }
                assert_eq!(height(&store, root).unwrap(), 1);
                exact_delta(&store, &before, root);
                check(&store, root, &model);
                check(&store, held_root, &held_model);

                store.begin();
                let before = pages(&store, root);
                let last = model.first_key_value().unwrap().0.clone();
                root = delete(&mut store, root, &last).unwrap();
                assert!(root.is_null());
                exact_delta(&store, &before, root);
                store.begin();
                root = put(&mut store, root, b"reinserted", b"value").unwrap();
                exact_delta(&store, &BTreeSet::new(), root);
            }
        }
    }
}

#[test]
fn private_equal_reference_edits_have_no_receipt_until_an_edge_is_removed() {
    let mut store = Store::new(4096, true, true);
    let root = put(&mut store, Reference::NULL, b"a", b"one").unwrap();
    store.receipts.clear();
    assert_eq!(put(&mut store, root, b"a", b"two").unwrap(), root);
    assert_eq!(put(&mut store, root, b"b", b"three").unwrap(), root);
    assert!(store.receipts.is_empty());
    assert_eq!(delete(&mut store, root, b"a").unwrap(), root);
    assert!(store.receipts.is_empty());
    assert!(delete(&mut store, root, b"b").unwrap().is_null());
    assert_eq!(store.receipts, vec![root]);
}

#[test]
fn unary_root_collapse_reports_bypassed_pages_even_without_a_rewrite() {
    let mut store = Store::new(256, true, false);
    let a = leaf_cell(&mut store, b"a", b"one", None).unwrap();
    let b = leaf_cell(&mut store, b"b", b"two", None).unwrap();
    let bytes = page::encode_v1_for(256, 0, Reference::NULL, &[a.as_bytes(), b.as_bytes()]).unwrap();
    let leaf = store.allocate(&bytes, 256).unwrap();
    let bytes = page::encode_v1_for(256, 1, leaf, &[]).unwrap();
    let middle = store.allocate(&bytes, 256).unwrap();
    let bytes = page::encode_v1_for(256, 2, middle, &[]).unwrap();
    let root = store.allocate(&bytes, 256).unwrap();
    let before = pages(&store, root);
    let writes = store.writes;
    assert_eq!(delete(&mut store, root, b"absent").unwrap(), root);
    assert_eq!(store.writes, writes);
    assert!(store.receipts.is_empty());
    // The leaf edit stays at the same private reference. No ancestor rewrite
    // happens; only the explicit collapse hooks can report the two removals.
    assert_eq!(delete(&mut store, root, b"a").unwrap(), leaf);
    assert_eq!(store.receipts, vec![root, middle]);
    exact_delta(&store, &before, leaf);
    assert_eq!(get(&store, leaf, b"b").unwrap(), Some(&b"two"[..]));
}

#[test]
fn comparator_and_reserved_recursions_report_exact_committed_page_deltas() {
    let descending = |left: &[u8], right: &[u8]| right.cmp(left);
    let mut store = Store::new(256, true, true);
    let mut root = Reference::NULL;
    let mut model = Model::new();
    for n in 0..90u64 {
        root = put_with(&mut store, root, &key(n), b"value", &descending).unwrap();
        model.insert(key(n), b"value".to_vec());
    }
    store.begin();
    let before = pages(&store, root);
    let writes = store.writes;
    assert_eq!(put_with(&mut store, root, &key(10), b"value", &descending).unwrap(), root);
    assert_eq!(delete_with(&mut store, root, &key(200), &descending).unwrap(), root);
    assert!(store.receipts.is_empty());
    assert_eq!(store.writes, writes);
    for n in 0..30u64 {
        root = put_with(&mut store, root, &key(n), b"updated", &descending).unwrap();
        model.insert(key(n), b"updated".to_vec());
    }
    for n in 30..55u64 {
        root = delete_with(&mut store, root, &key(n), &descending).unwrap();
        model.remove(&key(n));
    }
    exact_delta(&store, &before, root);
    for (k,v) in &model { assert_eq!(get_with(&store, root, k, &descending).unwrap(), Some(v.as_slice())); }

    store.begin();
    let before = pages(&store, root);
    let reserved = store.reserve_payload(5000).unwrap();
    for n in [0, 10, 20, 100, 120] {
        root = put_reserved_with(&mut store, root, &key(n), &reserved, &descending).unwrap();
    }
    exact_delta(&store, &before, root);
    for n in [0, 10, 20, 100, 120] {
        assert_eq!(get_with(&store, root, &key(n), &descending).unwrap(), Some(&vec![0;5000][..]));
    }
    let mut leaf_store = Store::new(4096, true, false);
    let leaf = put_with(&mut leaf_store, Reference::NULL, b"x", b"v", &descending).unwrap();
    leaf_store.begin();
    let before = pages(&leaf_store, leaf);
    let empty = delete_with(&mut leaf_store, leaf, b"x", &descending).unwrap();
    assert!(empty.is_null());
    exact_delta(&leaf_store, &before, empty);

    let mut plain = Store::new(256, true, true);
    let mut root = Reference::NULL;
    let reserved = plain.reserve_payload(513).unwrap();
    for n in 0..50 { root = put_reserved(&mut plain, root, &key(n), &reserved).unwrap(); }
    plain.begin();
    let before = pages(&plain, root);
    let reserved = plain.reserve_payload(1025).unwrap();
    for n in [0, 4, 20, 60] { root = put_reserved(&mut plain, root, &key(n), &reserved).unwrap(); }
    exact_delta(&plain, &before, root);
}

#[test]
fn receipt_failure_propagates_and_aborted_candidate_leaves_selected_root_unchanged() {
    let mut store = Store::new(256, true, true);
    let mut selected = Reference::NULL;
    let mut model = Model::new();
    for n in 0..100u64 {
        selected = put(&mut store, selected, &key(n), b"old value").unwrap();
        model.insert(key(n), b"old value".to_vec());
    }
    store.begin();
    let original_bytes = store.bytes.clone();
    store.fail_hook = Some(1); // A leaf receipt succeeds; an ancestor fails.
    assert!(matches!(put(&mut store, selected, &key(30), b"new value"), Err(Error::Io(_))));
    assert_eq!(store.hook_calls, 2);
    assert!(store.bytes.len() > original_bytes.len(), "failure is not an automatic private rollback");
    assert_eq!(&store.bytes[..original_bytes.len()], original_bytes.as_slice());
    check(&store, selected, &model);
    // Abandon all candidate receipts. A subsequent transaction uses the exact
    // still-selected old root, not any root from the failed edit.
    store.begin();
    let before = pages(&store, selected);
    let candidate = delete(&mut store, selected, &key(30)).unwrap();
    exact_delta(&store, &before, candidate);
    check(&store, selected, &model);
    store.begin(); // Explicit successful-candidate abort also drops receipts.
    assert!(store.receipts.is_empty());
    check(&store, selected, &model);
}

#[test]
fn comparator_reserved_empty_leaf_and_collapse_hook_failures_are_not_silenced() {
    let descending = |left: &[u8], right: &[u8]| right.cmp(left);
    let mut store = Store::new(4096, true, true);
    let root = put_with(&mut store, Reference::NULL, b"x", b"old", &descending).unwrap();
    store.begin();
    store.fail_hook = Some(0);
    assert!(matches!(put_with(&mut store, root, b"x", b"new", &descending), Err(Error::Io(_))));
    assert_eq!(get_with(&store, root, b"x", &descending).unwrap(), Some(&b"old"[..]));
    store.begin();
    let reserved = store.reserve_payload(1025).unwrap();
    store.fail_hook = Some(0);
    assert!(matches!(put_reserved(&mut store, root, b"x", &reserved), Err(Error::Io(_))));
    assert_eq!(get(&store, root, b"x").unwrap(), Some(&b"old"[..]));
    store.begin();
    store.fail_hook = Some(0);
    assert!(matches!(delete(&mut store, root, b"x"), Err(Error::Io(_))));
    assert_eq!(get(&store, root, b"x").unwrap(), Some(&b"old"[..]));

    let bytes = page::encode_v1_for(4096, 1, root, &[]).unwrap();
    let unary = store.allocate(&bytes, 4096).unwrap();
    store.begin();
    store.fail_hook = Some(0);
    assert!(matches!(prune::collapse_root(&mut store, unary), Err(Error::Io(_))));
    assert_eq!(Page::load(&store, unary).unwrap().first, root);
}

#[test]
fn default_hook_store_retains_existing_mutation_behavior() {
    // This store intentionally does not implement the optional hook.
    struct DefaultStore(Store);
    impl ReadStore for DefaultStore {
        fn page_size(&self) -> usize { self.0.page_size() }
        fn read(&self, at: Reference, length: usize) -> Result<&[u8]> { self.0.read(at, length) }
    }
    impl WriteStore for DefaultStore {
        fn allocate(&mut self, bytes: &[u8], alignment: usize) -> Result<Reference> {
            self.0.allocate(bytes, alignment)
        }
        fn rewrite(&mut self, old: Reference, bytes: &[u8], alignment: usize) -> Result<Reference> {
            self.0.rewrite(old, bytes, alignment)
        }
    }
    let mut plain = DefaultStore(Store::new(256, true, false));
    let mut recording = Store::new(256, true, false);
    let (mut a, mut b) = (Reference::NULL, Reference::NULL);
    for n in 0..100 {
        a = put(&mut plain, a, &key(n), b"value").unwrap();
        b = put(&mut recording, b, &key(n), b"value").unwrap();
    }
    plain.0.begin();
    recording.begin();
    for n in 0..100 {
        a = delete(&mut plain, a, &key(n)).unwrap();
        b = delete(&mut recording, b, &key(n)).unwrap();
        assert_eq!(a, b);
        assert_eq!(plain.0.bytes, recording.bytes);
    }
    assert!(a.is_null());
    assert!(plain.0.receipts.is_empty());
    assert!(!recording.receipts.is_empty());
}

#[test]
fn overflow_separator_alias_survives_leaf_key_removal_and_is_not_a_page_receipt() {
    let mut store = Store::new(256, true, false);
    let mut root = Reference::NULL;
    let mut keys = Vec::new();
    for n in 0..45u64 {
        let mut k = key(n);
        k.resize(300, n as u8);
        root = put(&mut store, root, &k, b"v").unwrap();
        keys.push(k);
    }
    // Pick a real separator made by the builder. Its exact overflow reference
    // is shared with a leaf key, rather than a manually forged payload alias.
    let (separator, payload) = {
        let branch = Page::load(&store, root).unwrap();
        assert!(branch.level != 0 && branch.count != 0);
        let raw = branch.raw_cell(0).unwrap();
        assert_ne!(page::u16_at(raw, 2).unwrap() & 1, 0);
        (branch.key_at(&store, 0).unwrap().to_vec(), Reference::decode(&raw[20..36]).unwrap())
    };
    let before_root = root;
    store.begin();
    let before = pages(&store, root);
    root = delete(&mut store, root, &separator).unwrap();
    exact_delta(&store, &before, root);
    assert_eq!(get(&store, root, &separator).unwrap(), None);
    assert_eq!(store.payload(payload).unwrap(), separator.as_slice());
    assert!(!store.receipts.contains(&payload));
    let retained_by_branch = pages(&store, root).into_iter().any(|at| {
        let p = Page::load(&store, at).unwrap();
        p.level != 0 && (0..p.count).any(|i| {
            let raw = p.raw_cell(i).unwrap();
            page::u16_at(raw, 2).unwrap() & 1 != 0
                && Reference::decode(&raw[20..36]).unwrap() == payload
        })
    });
    assert!(retained_by_branch, "inclusive separator must still own the removed leaf key's payload");
    assert_eq!(get(&store, before_root, &separator).unwrap(), Some(&b"v"[..]));
    for k in keys.iter().filter(|k| **k != separator) {
        assert_eq!(get(&store, root, k).unwrap(), Some(&b"v"[..]));
    }
}
