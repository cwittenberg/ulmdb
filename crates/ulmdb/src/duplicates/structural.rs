//! Callback-free physical inorder traversal for quotas and copying. No sorting certificate.
use super::{bucket::Bucket, validate_stored};
use crate::{DatabaseKind, Error, Result};
use std::ops::Bound;
use ulmdb_core::storage::{ReadStore, Reference};
enum Inner<'a, S: ReadStore> {
    Inline {
        bucket: super::bucket::Inline<'a>,
        next: usize,
    },
    Tree(ulmdb_kv::Cursor<'a, S>),
}
pub struct StructuralPairs<'a, S: ReadStore> {
    keys: ulmdb_kv::Cursor<'a, S>,
    store: &'a S,
    kind: DatabaseKind,
    current: Option<(&'a [u8], Inner<'a, S>)>,
    failed: bool,
}
impl<'a, S: ReadStore> StructuralPairs<'a, S> {
    pub(crate) fn new(store: &'a S, root: Reference, kind: DatabaseKind) -> Result<Self> {
        if !matches!(kind, DatabaseKind::Kv | DatabaseKind::Duplicates { .. }) {
            return Err(Error::WrongKind);
        }
        Ok(Self {
            keys: ulmdb_kv::Cursor::new(store, root, Bound::Unbounded, Bound::Unbounded, false)?,
            store,
            kind,
            current: None,
            failed: false,
        })
    }
    fn advance(&mut self) -> Result<Option<(&'a [u8], &'a [u8])>> {
        loop {
            if let Some((key, inner)) = &mut self.current {
                match inner {
                    Inner::Inline { bucket, next } => {
                        if *next < bucket.count {
                            let v = bucket.value(*next);
                            *next += 1;
                            return Ok(Some((*key, v)));
                        }
                    }
                    Inner::Tree(cursor) => {
                        if let Some(pair) = cursor.next() {
                            let (v, empty) = pair?;
                            let DatabaseKind::Duplicates { width } = self.kind else {
                                unreachable!()
                            };
                            validate_stored(v, width)?;
                            if !empty.is_empty() {
                                return Err(Error::Corrupt("duplicate child data"));
                            }
                            return Ok(Some((*key, v)));
                        }
                    }
                }
                self.current = None;
            }
            let Some(pair) = self.keys.next() else {
                return Ok(None);
            };
            let (key, bytes) = pair?;
            if key.len() > self.store.max_key_size() {
                return Err(Error::Corrupt("structural key size"));
            }
            match self.kind {
                DatabaseKind::Kv => {
                    if bytes.len() > self.store.max_value_size() {
                        return Err(Error::Corrupt("structural value size"));
                    }
                    return Ok(Some((key, bytes)));
                }
                DatabaseKind::Duplicates { width } => {
                    let bucket = Bucket::decode_with(bytes, width, None)?;
                    let inner = match bucket {
                        Bucket::Inline(bucket) => Inner::Inline { bucket, next: 0 },
                        Bucket::Tree { root, .. } => Inner::Tree(ulmdb_kv::Cursor::new(
                            self.store,
                            root,
                            Bound::Unbounded,
                            Bound::Unbounded,
                            false,
                        )?),
                    };
                    self.current = Some((key, inner));
                }
                _ => return Err(Error::WrongKind),
            }
        }
    }
}
impl<'a, S: ReadStore> Iterator for StructuralPairs<'a, S> {
    type Item = Result<(&'a [u8], &'a [u8])>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.advance() {
            Ok(Some(v)) => Some(Ok(v)),
            Ok(None) => {
                self.failed = true;
                None
            }
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}
impl<S: ReadStore> std::iter::FusedIterator for StructuralPairs<'_, S> {}
/// Resume by exact stored bytes in physical inorder. This is not a range seek.
pub(crate) fn resume<'a, S: ReadStore>(
    store: &'a S,
    root: Reference,
    kind: DatabaseKind,
    after: Option<(&[u8], &[u8])>,
) -> Result<Option<(&'a [u8], &'a [u8])>> {
    let mut iter = StructuralPairs::new(store, root, kind)?;
    if let Some(after) = after {
        let mut found = false;
        for pair in iter.by_ref() {
            let pair = pair?;
            if pair == after {
                found = true;
                break;
            }
        }
        if !found {
            return Ok(None);
        }
    }
    iter.next().transpose()
}
