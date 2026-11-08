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
    pub(super) fn remove(&mut self, key: &Key) {
        if self.contains(key) {
            self.0 = remove(&self.0, key);
        }
    }
}
