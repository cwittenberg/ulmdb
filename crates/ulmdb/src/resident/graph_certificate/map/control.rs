//! Persistent AVL: updates copy only a search path, never the full object table.
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
    fn node(key: K, value: V, left: Self, right: Self) -> Self {
        super::baseline_allocation();
        let height = 1 + left.height().max(right.height());
        let size = 1 + left.len() + right.len();
        Self(Some(Arc::new(Node {
            key,
            value,
            left,
            right,
            height,
            size,
        })))
    }
    fn balance(key: K, value: V, left: Self, right: Self) -> Self {
        if left.height() > right.height() + 1 {
            let l = left.0.as_ref().unwrap();
            if l.left.height() >= l.right.height() {
                return Self::node(
                    l.key.clone(),
                    l.value.clone(),
                    l.left.clone(),
                    Self::node(key, value, l.right.clone(), right),
                );
            }
            let m = l.right.0.as_ref().unwrap();
            return Self::node(
                m.key.clone(),
                m.value.clone(),
                Self::node(
                    l.key.clone(),
                    l.value.clone(),
                    l.left.clone(),
                    m.left.clone(),
                ),
                Self::node(key, value, m.right.clone(), right),
            );
        }
        if right.height() > left.height() + 1 {
            let r = right.0.as_ref().unwrap();
            if r.right.height() >= r.left.height() {
                return Self::node(
                    r.key.clone(),
                    r.value.clone(),
                    Self::node(key, value, left, r.left.clone()),
                    r.right.clone(),
                );
            }
            let m = r.left.0.as_ref().unwrap();
            return Self::node(
                m.key.clone(),
                m.value.clone(),
                Self::node(key, value, left, m.left.clone()),
                Self::node(
                    r.key.clone(),
                    r.value.clone(),
                    m.right.clone(),
                    r.right.clone(),
                ),
            );
        }
        Self::node(key, value, left, right)
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
        *self = self.put(key, value)
    }
    fn put(&self, key: K, value: V) -> Self {
        let Some(n) = &self.0 else {
            return Self::node(key, value, Self::default(), Self::default());
        };
        match key.cmp(&n.key) {
            Ordering::Less => Self::balance(
                n.key.clone(),
                n.value.clone(),
                n.left.put(key, value),
                n.right.clone(),
            ),
            Ordering::Greater => Self::balance(
                n.key.clone(),
                n.value.clone(),
                n.left.clone(),
                n.right.put(key, value),
            ),
            Ordering::Equal => Self::node(key, value, n.left.clone(), n.right.clone()),
        }
    }
    pub fn remove(&mut self, key: &K) {
        *self = self.delete(key)
    }
    fn delete(&self, key: &K) -> Self {
        let Some(n) = &self.0 else {
            return self.clone();
        };
        match key.cmp(&n.key) {
            Ordering::Less => Self::balance(
                n.key.clone(),
                n.value.clone(),
                n.left.delete(key),
                n.right.clone(),
            ),
            Ordering::Greater => Self::balance(
                n.key.clone(),
                n.value.clone(),
                n.left.clone(),
                n.right.delete(key),
            ),
            Ordering::Equal => {
                if n.left.0.is_none() {
                    return n.right.clone();
                }
                if n.right.0.is_none() {
                    return n.left.clone();
                }
                let mut successor = n.right.0.as_ref().unwrap();
                while let Some(x) = successor.left.0.as_ref() {
                    successor = x;
                }
                Self::balance(
                    successor.key.clone(),
                    successor.value.clone(),
                    n.left.clone(),
                    n.right.delete(&successor.key),
                )
            }
        }
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
