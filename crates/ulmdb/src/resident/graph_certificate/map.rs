//! Persistent AVL: detach shared paths once, mutate uniquely owned nodes in place.
use std::{cmp::Ordering, sync::Arc};
#[derive(Clone)]
pub(super) struct Map<K, V>(Option<Arc<Node<K, V>>>);
struct Node<K, V> {
    key: K,
    value: V,
    left: Map<K, V>,
    right: Map<K, V>,
    height: u16,
    size: usize,
}
impl<K: Clone, V: Clone> Clone for Node<K, V> {
    fn clone(&self) -> Self {
        #[cfg(test)]
        allocation();
        Self {
            key: self.key.clone(),
            value: self.value.clone(),
            left: self.left.clone(),
            right: self.right.clone(),
            height: self.height,
            size: self.size,
        }
    }
}
impl<K, V> Default for Map<K, V> {
    fn default() -> Self {
        Self(None)
    }
}
impl<K: Ord + Clone, V: Clone> Map<K, V> {
    fn height(&self) -> u16 {
        self.0.as_ref().map_or(0, |n| n.height)
    }
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |n| n.size)
    }
    fn node(key: K, value: V) -> Arc<Node<K, V>> {
        #[cfg(test)]
        allocation();
        Arc::new(Node {
            key,
            value,
            left: Self::default(),
            right: Self::default(),
            height: 1,
            size: 1,
        })
    }
    fn repair(n: &mut Node<K, V>) {
        n.height = 1 + n.left.height().max(n.right.height());
        n.size = 1 + n.left.len() + n.right.len();
    }
    fn rotate_left(&mut self) {
        let mut root = self.0.take().expect("AVL left rotation root");
        let mut right = Arc::make_mut(&mut root)
            .right
            .0
            .take()
            .expect("AVL right pivot");
        let middle = Arc::make_mut(&mut right).left.0.take();
        let n = Arc::make_mut(&mut root);
        n.right.0 = middle;
        Self::repair(n);
        let n = Arc::make_mut(&mut right);
        n.left.0 = Some(root);
        Self::repair(n);
        self.0 = Some(right);
    }
    fn rotate_right(&mut self) {
        let mut root = self.0.take().expect("AVL right rotation root");
        let mut left = Arc::make_mut(&mut root)
            .left
            .0
            .take()
            .expect("AVL left pivot");
        let middle = Arc::make_mut(&mut left).right.0.take();
        let n = Arc::make_mut(&mut root);
        n.left.0 = middle;
        Self::repair(n);
        let n = Arc::make_mut(&mut left);
        n.right.0 = Some(root);
        Self::repair(n);
        self.0 = Some(left);
    }
    fn balance(&mut self) {
        let Some(root) = self.0.as_mut() else { return };
        let n = Arc::make_mut(root);
        Self::repair(n);
        if n.left.height() > n.right.height() + 1 {
            let l = n.left.0.as_ref().expect("AVL left heavy");
            if l.left.height() < l.right.height() {
                n.left.rotate_left();
            }
            self.rotate_right();
        } else if n.right.height() > n.left.height() + 1 {
            let r = n.right.0.as_ref().expect("AVL right heavy");
            if r.right.height() < r.left.height() {
                n.right.rotate_right();
            }
            self.rotate_left();
        }
    }
    pub fn get(&self, key: &K) -> Option<&V> {
        let mut next = self.0.as_ref();
        while let Some(n) = next {
            match key.cmp(&n.key) {
                Ordering::Less => next = n.left.0.as_ref(),
                Ordering::Greater => next = n.right.0.as_ref(),
                Ordering::Equal => return Some(&n.value),
            }
        }
        None
    }
    pub fn insert(&mut self, key: K, value: V) {
        let Some(root) = self.0.as_mut() else {
            self.0 = Some(Self::node(key, value));
            return;
        };
        let n = Arc::make_mut(root);
        match key.cmp(&n.key) {
            Ordering::Less => n.left.insert(key, value),
            Ordering::Greater => n.right.insert(key, value),
            Ordering::Equal => {
                n.key = key;
                n.value = value;
                return;
            }
        }
        self.balance();
    }
    pub fn remove(&mut self, key: &K) {
        // Do not detach an absent search path; removal is semantically a no-op.
        if self.get(key).is_some() {
            self.delete_existing(key);
        }
    }
    fn delete_existing(&mut self, key: &K) {
        let root = self.0.as_mut().expect("AVL existing deletion");
        let n = Arc::make_mut(root);
        match key.cmp(&n.key) {
            Ordering::Less => n.left.delete_existing(key),
            Ordering::Greater => n.right.delete_existing(key),
            Ordering::Equal => {
                if n.left.0.is_none() {
                    self.0 = n.right.0.take();
                    return;
                }
                if n.right.0.is_none() {
                    self.0 = n.left.0.take();
                    return;
                }
                let (successor_key, successor_value) = n.right.take_first();
                n.key = successor_key;
                n.value = successor_value;
            }
        }
        self.balance();
    }
    /// Remove and move the successor, detaching each shared path node before take.
    fn take_first(&mut self) -> (K, V) {
        let root = self.0.as_mut().expect("AVL successor");
        if root.left.0.is_none() {
            let mut first = self.0.take().unwrap();
            // There are no Weak handles: after make_mut this is the sole strong owner.
            let _ = Arc::make_mut(&mut first);
            let mut owned = match Arc::try_unwrap(first) {
                Ok(n) => n,
                Err(_) => unreachable!("private AVL ownership"),
            };
            self.0 = owned.right.0.take();
            return (owned.key, owned.value);
        }
        let out = Arc::make_mut(root).left.take_first();
        self.balance();
        out
    }
    pub fn first(&self) -> Option<(&K, &V)> {
        let mut n = self.0.as_ref()?;
        while let Some(l) = n.left.0.as_ref() {
            n = l;
        }
        Some((&n.key, &n.value))
    }
    pub fn entries(&self) -> Vec<(K, V)> {
        let mut out = Vec::with_capacity(self.len());
        self.walk(&mut out);
        out
    }
    fn walk(&self, out: &mut Vec<(K, V)>) {
        if let Some(n) = &self.0 {
            n.left.walk(out);
            out.push((n.key.clone(), n.value.clone()));
            n.right.walk(out)
        }
    }
}
#[cfg(test)]
thread_local! {static NODE_ALLOCATIONS:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
#[cfg(test)]
fn allocation() {
    NODE_ALLOCATIONS.with(|n| n.set(n.get() + 1));
}
#[cfg(test)]
fn allocation_count() -> usize {
    NODE_ALLOCATIONS.with(std::cell::Cell::get)
}
#[cfg(test)]
mod tests;
