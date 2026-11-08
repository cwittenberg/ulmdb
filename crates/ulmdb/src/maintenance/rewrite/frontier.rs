//! Persistent AVL frontier: preparation clones only one Arc, and each edit
//! copies one logarithmic path. Aborting a candidate preserves committed work.
use super::Kind;
use std::sync::Arc;
use ulmdb_core::storage::Reference;
type Key = (Reference, Kind);
type Tree = Option<Arc<Node>>;
struct Node {
    key: Key,
    left: Tree,
    right: Tree,
    height: u8,
    count: usize,
}
#[derive(Clone, Default)]
pub(super) struct Frontier(Tree);
fn height(t: &Tree) -> u8 {
    t.as_ref().map_or(0, |n| n.height)
}
fn count(t: &Tree) -> usize {
    t.as_ref().map_or(0, |n| n.count)
}
fn node(key: Key, left: Tree, right: Tree) -> Arc<Node> {
    Arc::new(Node {
        key,
        height: 1 + height(&left).max(height(&right)),
        count: 1 + count(&left) + count(&right),
        left,
        right,
    })
}
fn balance(key: Key, left: Tree, right: Tree) -> Arc<Node> {
    let (lh, rh) = (height(&left), height(&right));
    if lh > rh.saturating_add(1) {
        let l = left.as_ref().expect("nonempty tall left tree");
        if height(&l.left) >= height(&l.right) {
            return node(
                l.key,
                l.left.clone(),
                Some(node(key, l.right.clone(), right)),
            );
        }
        let lr = l.right.as_ref().expect("nonempty left-right tree");
        return node(
            lr.key,
            Some(node(l.key, l.left.clone(), lr.left.clone())),
            Some(node(key, lr.right.clone(), right)),
        );
    }
    if rh > lh.saturating_add(1) {
        let r = right.as_ref().expect("nonempty tall right tree");
        if height(&r.right) >= height(&r.left) {
            return node(
                r.key,
                Some(node(key, left, r.left.clone())),
                r.right.clone(),
            );
        }
        let rl = r.left.as_ref().expect("nonempty right-left tree");
        return node(
            rl.key,
            Some(node(key, left, rl.left.clone())),
            Some(node(r.key, rl.right.clone(), r.right.clone())),
        );
    }
    node(key, left, right)
}
fn insert(t: &Tree, key: Key) -> Arc<Node> {
    let Some(n) = t else {
        return node(key, None, None);
    };
    match key.cmp(&n.key) {
        std::cmp::Ordering::Less => balance(n.key, Some(insert(&n.left, key)), n.right.clone()),
        std::cmp::Ordering::Greater => balance(n.key, n.left.clone(), Some(insert(&n.right, key))),
        std::cmp::Ordering::Equal => n.clone(),
    }
}
fn remove(t: &Tree, key: &Key) -> Tree {
    let n = t.as_ref()?;
    Some(match key.cmp(&n.key) {
        std::cmp::Ordering::Less => balance(n.key, remove(&n.left, key), n.right.clone()),
        std::cmp::Ordering::Greater => balance(n.key, n.left.clone(), remove(&n.right, key)),
        std::cmp::Ordering::Equal => {
            if n.left.is_none() {
                return n.right.clone();
            }
            if n.right.is_none() {
                return n.left.clone();
            }
            let mut next = n.right.as_ref().unwrap();
            while let Some(left) = &next.left {
                next = left;
            }
            balance(next.key, n.left.clone(), remove(&n.right, &next.key))
        }
    })
}
fn remove_min(t: &Tree) -> (Tree, Option<Key>) {
    let Some(n) = t else { return (None, None); };
    if n.left.is_none() {
        return (n.right.clone(), Some(n.key));
    }
    let (left, key) = remove_min(&n.left);
    (Some(balance(n.key, left, n.right.clone())), key)
}
impl Frontier {
    pub(super) fn len(&self) -> usize {
        count(&self.0)
    }
    pub(super) fn contains(&self, key: &Key) -> bool {
        let mut next = self.0.as_ref();
        while let Some(n) = next {
            match key.cmp(&n.key) {
                std::cmp::Ordering::Less => next = n.left.as_ref(),
                std::cmp::Ordering::Greater => next = n.right.as_ref(),
                std::cmp::Ordering::Equal => return true,
            }
        }
        false
    }
    pub(super) fn insert(&mut self, key: Key) {
        self.0 = Some(insert(&self.0, key));
    }
    pub(super) fn remove(&mut self, key: &Key) -> bool {
        if self.contains(key) {
            self.0 = remove(&self.0, key);
            true
        } else {
            false
        }
    }
    pub(super) fn remove_min(&mut self) -> Option<Key> {
        let (root, key) = remove_min(&self.0);
        self.0 = root;
        key
    }
}

#[cfg(test)]
mod minimum_tests {
    use super::*;
    use std::collections::BTreeSet;
    fn check(t: &Tree, lower: Option<Key>, upper: Option<Key>) -> (u8, usize) {
        let Some(n) = t else { return (0, 0); };
        assert!(lower.is_none_or(|key| key < n.key));
        assert!(upper.is_none_or(|key| n.key < key));
        let (lh, lc) = check(&n.left, lower, Some(n.key));
        let (rh, rc) = check(&n.right, Some(n.key), upper);
        assert!(lh.abs_diff(rh) <= 1);
        assert_eq!(n.height, 1 + lh.max(rh));
        assert_eq!(n.count, 1 + lc + rc);
        (n.height, n.count)
    }
    #[test]
    fn minimum_removal_matches_model_and_preserves_held_versions() {
        let mut set = Frontier::default();
        let mut model = BTreeSet::new();
        let mut held = Vec::new();
        for i in 0..1024u64 {
            let key = (Reference { segment: 1 + i % 7, offset: (i * 7919 % 1024) * 16 }, Kind::Kv);
            set.insert(key);
            model.insert(key);
            if i % 127 == 0 { held.push((set.clone(), model.clone())); }
            check(&set.0, None, None);
        }
        while let Some(first) = model.pop_first() {
            assert!(set.remove_min() == Some(first));
            assert_eq!(set.len(), model.len());
            check(&set.0, None, None);
        }
        assert!(set.remove_min().is_none());
        assert!(!set.remove(&(Reference::NULL, Kind::Kv)));
        for (snapshot, expected) in held {
            assert_eq!(snapshot.len(), expected.len());
            check(&snapshot.0, None, None);
            for key in expected { assert!(snapshot.contains(&key)); }
        }
    }
}
