//! Stable-slot mode for the snapshot log's ordered key index.
//!
//! The ordered tree stores one immutable 8-byte slot/incarnation handle per
//! current key. Replacing a value updates the versioned pool, without changing
//! this binding or copying key-tree paths. Only inserting/removing keys edits
//! the tree. A snapshot must own the matching pool View and key Index together.
//! Existing plaintext-value Index mode remains a separate control.
use super::index::{Bytes, Change, Index, MetadataReservation};
use super::lookup::{Lookup, Resolved};
use crate::{Error, Result};
use std::cmp::Ordering;
use std::mem::size_of;
use ulmdb_storage::snapshot_versions::{Head, Mutation, View};

/// Decode an exact typed handle. A width-valid handle still requires validation
/// against its snapshot's View before it grants access to a value.
pub(super) fn decode_handle(bytes: &[u8]) -> Result<Head> {
    let encoded: [u8; 8] = bytes
        .try_into()
        .map_err(|_| Error::Corrupt("snapshot-log stable head width"))?;
    Head::from_u64(u64::from_le_bytes(encoded))
        .ok_or(Error::Corrupt("snapshot-log invalid stable head"))
}

/// Resolve the stored word without reconstructing a typed Head on every row.
/// The pool rejects invalid words through the same bounds and MVCC gates.
#[inline(always)]
pub(super) fn cursor_value<'a>(view: &'a View, stored: &[u8]) -> Result<&'a [u8]> {
    let word: [u8; 8] = stored
        .try_into()
        .map_err(|_| Error::Corrupt("snapshot-log stable head width"))?;
    match view.get_encoded(u64::from_le_bytes(word)) {
        Some(value) => Ok(value),
        None => Err(cursor_value_error(stored)),
    }
}

#[cold]
#[inline(never)]
fn cursor_value_error(stored: &[u8]) -> Error {
    // Retain the existing invalid-word versus stale-binding diagnostics.
    decode_handle(stored)
        .err()
        .unwrap_or(Error::Corrupt("snapshot-log stale cursor value"))
}

#[inline(always)]
pub(super) fn cached_cursor_value<'a>(
    cache: &mut ulmdb_storage::snapshot_versions::ReadCache<'a>,
    stored: &[u8],
) -> (Result<&'a [u8]>, bool) {
    let Ok(word) = <[u8; 8]>::try_from(stored) else {
        return (Err(Error::Corrupt("snapshot-log stable head width")), false);
    };
    let (value, miss) = cache.get_encoded_with_locality(u64::from_le_bytes(word));
    (value.ok_or_else(|| cursor_value_error(stored)), miss)
}

pub(super) fn head(index: &Index, database: u64, key: &[u8]) -> Result<Option<Head>> {
    index.get(database, key).map(decode_handle).transpose()
}

/// Read through a coherent snapshot's immutable ordered index and pool View.
/// The result borrows the View, which owns the selected immutable value bytes.
pub(super) fn get<'a>(
    index: &Index,
    view: &'a View,
    database: u64,
    key: &[u8],
) -> Result<Option<&'a [u8]>> {
    let Some(head) = head(index, database, key)? else {
        return Ok(None);
    };
    view.get(head)
        .map(Some)
        .ok_or(Error::Corrupt("snapshot-log stale stable head"))
}

/// Build a membership insertion after the pool has privately reserved the
/// fresh handle. Do not create this Change for an existing-key replacement.
pub(super) fn link(database: u64, key: Bytes, head: Head) -> Result<Change> {
    Ok(Change {
        database,
        key,
        value: Some(Bytes::new(&head.to_u64().to_le_bytes())?),
    })
}

pub(super) fn unlink(database: u64, key: Bytes) -> Change {
    Change {
        database,
        key,
        value: None,
    }
}

/// Prepare membership-only effects. Existing live keys retain exactly their
/// stable handle; replacing their value is solely a pool-version operation.
/// Callers coalesce logical effects before pool preparation and pass at most
/// one final membership edit per (database,key). New keys may be linked and
/// removed in a later transaction; their old snapshots retain their old View.
///
/// This tree's encoded_rows_bytes counts handle bytes, NOT logical values.
/// The Environment maintains logical checkpoint byte accounting separately.
pub(super) fn prepare_membership(
    base: &Index,
    changes: &[Change],
    threshold: usize,
) -> Result<Index> {
    for change in changes {
        if let Some(value) = &change.value {
            let next = decode_handle(value.as_slice())?;
            if head(base, change.database, change.key.as_slice())?.is_some_and(|old| old != next) {
                return Err(Error::InvalidInput(
                    "snapshot-log existing key cannot change stable head",
                ));
            }
        }
    }
    base.prepare_hybrid(changes, threshold)
}

/// The Plan already resolved membership against this exact immutable base.
/// Validate the capability context once rather than searching every new key.
pub(super) fn prepare_membership_proven(
    base: &Index,
    view: &View,
    sequence: u64,
    proof: &MembershipProof<'_, '_>,
    threshold: usize,
) -> Result<Index> {
    proof.check(base, view, sequence)?;
    base.prepare_ordered(proof.changes(), threshold)
}

/// One final logical edit. Key/value bytes borrow bounded transaction input;
/// the old value is inspected while planning, never retained here.
// Only the Plan's originating Lookup core may reuse an Absent hash. A missing
// or permanently disabled lookup leaves an unused placeholder instead.
#[derive(Clone, Copy)]
enum Prior {
    Absent { hash: u64 },
    Present(Head),
}
impl Prior {
    fn head(self) -> Option<Head> {
        match self {
            Self::Absent { .. } => None,
            Self::Present(head) => Some(head),
        }
    }
    fn hash(self) -> Option<u64> {
        match self {
            Self::Absent { hash } => Some(hash),
            Self::Present(_) => None,
        }
    }
}
#[derive(Clone, Copy)]
struct Edit<'a> {
    database: u64,
    key: &'a Bytes,
    prior: Prior,
    value: Option<&'a Bytes>,
}
enum Edits<'a> {
    Empty,
    One(Edit<'a>),
    Many(Vec<Edit<'a>>),
}

/// A pre-I/O value/membership transition. Keep this Plan alive until its
/// mutation/membership output vectors have been consumed or dropped: its
/// temporary quota ticket covers those requested capacities on the Many route.
/// Raw zero/one-operation planning allocates no heap and reserves no scratch.
pub(super) struct Plan<'a> {
    base: &'a Index,
    view: &'a View,
    lookup: Option<&'a Lookup>,
    edits: Edits<'a>,
    dirty: bool,
    rows_bytes: usize,
    _scratch: Option<MetadataReservation>,
}
impl<'a> Plan<'a> {
    pub(super) fn build(
        index: &'a Index,
        view: &'a View,
        changes: &'a [Change],
        base_rows_bytes: usize,
    ) -> Result<Self> {
        Self::build_with_lookup(index, view, changes, base_rows_bytes, None)
    }
    pub(super) fn build_with_lookup(
        index: &'a Index,
        view: &'a View,
        changes: &'a [Change],
        base_rows_bytes: usize,
        lookup: Option<&'a Lookup>,
    ) -> Result<Self> {
        if changes.is_empty() {
            return Ok(Self {
                base: index,
                view,
                lookup,
                edits: Edits::Empty,
                dirty: false,
                rows_bytes: base_rows_bytes,
                _scratch: None,
            });
        }
        if changes.len() == 1 {
            let change = &changes[0];
            let (prior, old_value) = old_value(index, view, change, lookup)?;
            let value = change.value.as_ref();
            let current = value.map(Bytes::as_slice);
            let dirty = old_value != current;
            let rows_bytes = replace_row_bytes(
                base_rows_bytes,
                change.key.as_slice().len(),
                old_value,
                current,
            )?;
            return Ok(Self {
                base: index,
                view,
                lookup,
                edits: if dirty {
                    Edits::One(Edit {
                        database: change.database,
                        key: &change.key,
                        prior,
                        value,
                    })
                } else {
                    Edits::Empty
                },
                dirty,
                rows_bytes,
                _scratch: None,
            });
        }
        if changes.len() > 1_048_576 {
            return Err(Error::LimitExceeded);
        }
        // Input ordering, final descriptors and caller output vectors overlap
        // until publication. All their requested capacities are reserved here.
        let per_change = size_of::<usize>()
            .checked_add(size_of::<Edit<'a>>())
            .and_then(|n| n.checked_add(size_of::<Mutation<'a>>()))
            .and_then(|n| n.checked_add(size_of::<Change>()))
            .ok_or(Error::LimitExceeded)?;
        let scratch_bytes = changes
            .len()
            .checked_mul(per_change)
            .ok_or(Error::LimitExceeded)?;
        let scratch = index.reserve_metadata(scratch_bytes)?;
        let mut order = Vec::new();
        order
            .try_reserve_exact(changes.len())
            .map_err(|_| Error::LimitExceeded)?;
        order.extend(0..changes.len());
        order.sort_unstable_by(|a, b| compare(&changes[*a], &changes[*b]).then_with(|| a.cmp(b)));
        let mut edits = Vec::new();
        edits
            .try_reserve_exact(changes.len())
            .map_err(|_| Error::LimitExceeded)?;
        let mut rows_bytes = base_rows_bytes;
        let mut dirty = false;
        let mut start = 0;
        while start < order.len() {
            let first = &changes[order[start]];
            let mut end = start + 1;
            while end < order.len() && compare(first, &changes[order[end]]) == Ordering::Equal {
                end += 1;
            }
            // Resolve the stable key binding and old selected value once.
            let (prior, old_value) = old_value(index, view, first, lookup)?;
            let mut previous = old_value;
            for position in &order[start..end] {
                let value = changes[*position].value.as_ref().map(Bytes::as_slice);
                if value != previous {
                    dirty = true;
                }
                previous = value;
            }
            let final_change = &changes[order[end - 1]];
            let value = final_change.value.as_ref();
            let final_value = value.map(Bytes::as_slice);
            if final_value != old_value {
                rows_bytes = replace_row_bytes(
                    rows_bytes,
                    final_change.key.as_slice().len(),
                    old_value,
                    final_value,
                )?;
                edits.push(Edit {
                    database: final_change.database,
                    key: &final_change.key,
                    prior,
                    value,
                });
            }
            start = end;
        }
        Ok(Self {
            base: index,
            view,
            lookup,
            edits: Edits::Many(edits),
            dirty,
            rows_bytes,
            _scratch: Some(scratch),
        })
    }
    pub(super) fn dirty(&self) -> bool {
        self.dirty
    }
    pub(super) fn rows_bytes(&self) -> usize {
        self.rows_bytes
    }
    pub(super) fn len(&self) -> usize {
        match &self.edits {
            Edits::Empty => 0,
            Edits::One(_) => 1,
            Edits::Many(edits) => edits.len(),
        }
    }
    fn edit(&self, index: usize) -> &Edit<'a> {
        match &self.edits {
            Edits::One(edit) if index == 0 => edit,
            Edits::Many(edits) => &edits[index],
            _ => panic!("versioned Plan index outside len"),
        }
    }
    pub(super) fn mutation(&self, index: usize) -> Mutation<'_> {
        let edit = self.edit(index);
        Mutation {
            head: edit.prior.head(),
            value: edit.value.map(Bytes::as_slice),
        }
    }
    /// Inline single-mutation membership result, for Option::as_slice callers.
    pub(super) fn membership_one(&self, head: Head) -> Result<Option<Change>> {
        if self.len() != 1 {
            return Err(Error::InvalidInput("versioned one-membership count"));
        }
        membership_edit(self.edit(0), head)
    }
    pub(super) fn membership(&self, heads: &[Head]) -> Result<Vec<Change>> {
        if heads.len() != self.len() {
            return Err(Error::InvalidInput("versioned pool result count"));
        }
        let count = (0..self.len())
            .filter(|index| {
                let edit = self.edit(*index);
                edit.prior.head().is_none() || edit.value.is_none()
            })
            .count();
        let mut changes = Vec::new();
        if count != 0 {
            changes
                .try_reserve_exact(count)
                .map_err(|_| Error::LimitExceeded)?;
        }
        for (index, head) in heads.iter().copied().enumerate() {
            if let Some(change) = membership_edit(self.edit(index), head)? {
                changes.push(change);
            }
        }
        Ok(changes)
    }
    /// Construct effects and their capability together. The immutable owner
    /// prevents a second caller from replacing/reordering the generated bytes,
    /// so they need no subsequent byte-for-byte validation pass.
    pub(super) fn prepare_membership<'p>(
        &'p self,
        sequence: u64,
        ready: &ulmdb_storage::snapshot_versions::Ready<'_>,
    ) -> Result<PreparedMembership<'p, 'a>> {
        if self.len() <= 1 {
            return Err(Error::InvalidInput(
                "versioned owned membership requires many edits",
            ));
        }
        self.check_ready(sequence, ready)?;
        Ok(PreparedMembership {
            plan: self,
            changes: self.membership(ready.heads())?,
            sequence,
        })
    }
    fn check_ready(
        &self,
        sequence: u64,
        ready: &ulmdb_storage::snapshot_versions::Ready<'_>,
    ) -> Result<()> {
        if !ready.is_successor_of(self.view)
            || self.view.epoch().checked_add(1) != Some(sequence)
            || ready.heads().len() != self.len()
        {
            return Err(Error::InvalidInput(
                "versioned membership proof sequence/heads",
            ));
        }
        Ok(())
    }
    /// Bind immutable generated membership to this Plan and the exact opaque
    /// pool preparation. No arbitrary raw-head slice mints this capability.
    pub(super) fn prove_membership<'p>(
        &'p self,
        sequence: u64,
        ready: &ulmdb_storage::snapshot_versions::Ready<'_>,
        changes: &'p [Change],
    ) -> Result<MembershipProof<'p, 'a>> {
        self.check_ready(sequence, ready)?;
        let mut member = 0;
        for (position, head) in ready.heads().iter().copied().enumerate() {
            let edit = self.edit(position);
            if edit.prior.head().is_some_and(|old| old != head) {
                return Err(Error::Corrupt("versioned membership proof stable head"));
            }
            if edit.prior.head().is_some() && edit.value.is_some() {
                continue;
            }
            let change = changes.get(member).ok_or(Error::InvalidInput(
                "versioned membership proof omitted effect",
            ))?;
            if change.database != edit.database || change.key.as_slice() != edit.key.as_slice() {
                return Err(Error::InvalidInput("versioned membership proof key"));
            }
            match (edit.prior.head(), edit.value, change.value.as_ref()) {
                (None, Some(_), Some(value)) if decode_handle(value.as_slice())? == head => {}
                (Some(_), None, None) => {}
                _ => return Err(Error::InvalidInput("versioned membership proof effect")),
            }
            member += 1;
        }
        if member != changes.len() {
            return Err(Error::InvalidInput(
                "versioned membership proof extra effect",
            ));
        }
        Ok(MembershipProof {
            plan: self,
            changes,
            sequence,
        })
    }
}

pub(super) struct PreparedMembership<'p, 'a> {
    plan: &'p Plan<'a>,
    changes: Vec<Change>,
    sequence: u64,
}
impl<'p, 'a> PreparedMembership<'p, 'a> {
    pub(super) fn changes(&self) -> &[Change] {
        &self.changes
    }
    pub(super) fn proof(&self) -> MembershipProof<'_, 'a> {
        MembershipProof {
            plan: self.plan,
            changes: &self.changes,
            sequence: self.sequence,
        }
    }
}

/// Private minting fields and immutable borrows prevent forged, reordered or
/// subsequently modified effects from bypassing the raw-input validation APIs.
pub(super) struct MembershipProof<'p, 'a> {
    plan: &'p Plan<'a>,
    changes: &'p [Change],
    sequence: u64,
}
impl<'p, 'a> MembershipProof<'p, 'a> {
    pub(super) fn check(&self, base: &Index, view: &View, sequence: u64) -> Result<()> {
        if !std::ptr::eq(base, self.plan.base)
            || !std::ptr::eq(view, self.plan.view)
            || sequence != self.sequence
        {
            return Err(Error::InvalidInput(
                "versioned membership proof foreign context",
            ));
        }
        Ok(())
    }
    pub(super) fn changes(&self) -> &[Change] {
        self.changes
    }
    pub(super) fn lookup(&self) -> Option<&Lookup> {
        self.plan.lookup
    }
    pub(super) fn members(&self) -> MembershipIter<'_, 'a> {
        MembershipIter {
            plan: self.plan,
            changes: self.changes,
            edit: 0,
            member: 0,
        }
    }
}
pub(super) struct ProvenMember<'a> {
    pub old_head: Option<Head>,
    pub absent_hash: Option<u64>,
    pub change: &'a Change,
}
pub(super) struct MembershipIter<'p, 'a> {
    plan: &'p Plan<'a>,
    changes: &'p [Change],
    edit: usize,
    member: usize,
}
impl<'p, 'a> Iterator for MembershipIter<'p, 'a> {
    type Item = ProvenMember<'p>;
    fn next(&mut self) -> Option<Self::Item> {
        while self.edit < self.plan.len() {
            let edit = self.plan.edit(self.edit);
            self.edit += 1;
            if edit.prior.head().is_some() && edit.value.is_some() {
                continue;
            }
            let change = &self.changes[self.member];
            self.member += 1;
            return Some(ProvenMember {
                old_head: edit.prior.head(),
                absent_hash: edit.prior.hash(),
                change,
            });
        }
        None
    }
}
fn old_value<'a>(
    index: &Index,
    view: &'a View,
    change: &Change,
    lookup: Option<&Lookup>,
) -> Result<(Prior, Option<&'a [u8]>)> {
    let mut absent_hash = 0;
    if let Some(lookup) = lookup {
        let (resolved, hash) =
            lookup.resolve_with_hash(view, change.database, change.key.as_slice());
        absent_hash = hash;
        match resolved {
            Resolved::Present { head, value } => return Ok((Prior::Present(head), Some(value))),
            Resolved::Absent => return Ok((Prior::Absent { hash }, None)),
            Resolved::Fallback => {}
        }
    }
    match head(index, change.database, change.key.as_slice())? {
        Some(head) => Ok((
            Prior::Present(head),
            Some(
                view.get(head)
                    .ok_or(Error::Corrupt("versioned Plan stale head"))?,
            ),
        )),
        None => Ok((Prior::Absent { hash: absent_hash }, None)),
    }
}
fn compare(a: &Change, b: &Change) -> Ordering {
    a.database
        .cmp(&b.database)
        .then_with(|| a.key.as_slice().cmp(b.key.as_slice()))
}
fn replace_row_bytes(
    base: usize,
    key_bytes: usize,
    old: Option<&[u8]>,
    new: Option<&[u8]>,
) -> Result<usize> {
    let row_size = |value: Option<&[u8]>| -> Result<usize> {
        value.map_or(Ok(0), |value| {
            16usize
                .checked_add(key_bytes)
                .and_then(|n| n.checked_add(value.len()))
                .ok_or(Error::LimitExceeded)
        })
    };
    let old_bytes = row_size(old)?;
    let new_bytes = row_size(new)?;
    base.checked_sub(old_bytes)
        .and_then(|n| n.checked_add(new_bytes))
        .ok_or(Error::LimitExceeded)
}
fn membership_edit(edit: &Edit<'_>, head: Head) -> Result<Option<Change>> {
    if edit.prior.head().is_some_and(|old| old != head) {
        return Err(Error::Corrupt("versioned pool changed stable head"));
    }
    match (edit.prior.head(), edit.value) {
        (None, Some(_)) => Ok(Some(link(edit.database, edit.key.clone(), head)?)),
        (Some(_), None) => Ok(Some(unlink(edit.database, edit.key.clone()))),
        (Some(_), Some(_)) => Ok(None),
        (None, None) => Err(Error::Corrupt("versioned Plan empty edit")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ulmdb_storage::snapshot_versions::{Limits, PoolWriter};
    fn limits() -> Limits {
        Limits {
            max_bytes: 64 << 10,
            max_heads: 16,
            max_versions: 32,
            max_views: 16,
        }
    }
    #[test]
    fn cached_absence_hash_keeps_edit_and_scratch_width() {
        assert_eq!(size_of::<Prior>(), size_of::<Option<Head>>());
        #[cfg(target_pointer_width = "64")]
        assert_eq!(size_of::<Edit<'_>>(), 40);
    }
    #[test]
    fn cursor_word_resolution_preserves_invalid_and_stale_diagnostics() {
        let (mut pool, old, heads) = PoolWriter::seed(0, &[b"old"], limits()).unwrap();
        let word = heads[0].to_le_bytes();
        assert_eq!(cursor_value(&old, &word).unwrap(), b"old");
        assert!(matches!(
            cursor_value(&old, &[]),
            Err(Error::Corrupt("snapshot-log stable head width"))
        ));
        assert!(matches!(
            cursor_value(&old, &[0; 8]),
            Err(Error::Corrupt("snapshot-log invalid stable head"))
        ));
        let deleted = pool
            .prepare(
                1,
                &[Mutation {
                    head: Some(heads[0]),
                    value: None,
                }],
            )
            .unwrap()
            .publish();
        assert_eq!(cursor_value(&old, &word).unwrap(), b"old");
        assert!(matches!(
            cursor_value(&deleted, &word),
            Err(Error::Corrupt("snapshot-log stale cursor value"))
        ));
    }
    fn effect(database: u64, key: &[u8], value: Option<&[u8]>) -> Change {
        Change {
            database,
            key: Bytes::new(key).unwrap(),
            value: value.map(|v| Bytes::new(v).unwrap()),
        }
    }
    #[test]
    fn owned_membership_requires_matching_pool_and_stable_heads() {
        let (mut pool, view, index, _, rows) = seeded();
        let changes = [
            effect(1, b"new", Some(b"third")),
            effect(1, b"new2", Some(b"fourth")),
        ];
        let plan = Plan::build(&index, &view, &changes, rows).unwrap();
        let ready = pool
            .prepare(1, &[plan.mutation(0), plan.mutation(1)])
            .unwrap();
        let members = plan.prepare_membership(1, &ready).unwrap();
        let next = prepare_membership_proven(&index, &view, 1, &members.proof(), 16).unwrap();
        let (mut foreign, _, _, _, _) = seeded();
        let foreign_ready = foreign
            .prepare(
                1,
                &[
                    Mutation {
                        head: None,
                        value: Some(b"third"),
                    },
                    Mutation {
                        head: None,
                        value: Some(b"fourth"),
                    },
                ],
            )
            .unwrap();
        assert!(plan.prepare_membership(1, &foreign_ready).is_err());
        assert!(plan.prepare_membership(2, &ready).is_err());
        let current = ready.publish();
        assert_eq!(
            get(&next, &current, 1, b"new").unwrap(),
            Some(b"third".as_slice())
        );

        let (mut pool, view, index, heads, rows) = seeded();
        let changes = [
            effect(1, b"a", Some(b"replacement")),
            effect(1, b"b", Some(b"other")),
        ];
        let plan = Plan::build(&index, &view, &changes, rows).unwrap();
        let wrong = pool
            .prepare(
                1,
                &[
                    Mutation {
                        head: Some(heads[1]),
                        value: Some(b"replacement"),
                    },
                    Mutation {
                        head: Some(heads[0]),
                        value: Some(b"other"),
                    },
                ],
            )
            .unwrap();
        assert!(plan.prepare_membership(1, &wrong).is_err());
    }
    fn seeded() -> (PoolWriter, View, Index, Vec<Head>, usize) {
        let (pool, view, heads) =
            PoolWriter::seed(0, &[b"original".as_slice(), b"keep".as_slice()], limits()).unwrap();
        let index = Index::new(4 << 20).unwrap();
        let index = prepare_membership(
            &index,
            &[
                link(1, Bytes::new(b"a").unwrap(), heads[0]).unwrap(),
                link(1, Bytes::new(b"b").unwrap(), heads[1]).unwrap(),
            ],
            usize::MAX,
        )
        .unwrap();
        let bytes = (16 + 1 + b"original".len()) + (16 + 1 + b"keep".len());
        (pool, view, index, heads, bytes)
    }
    #[test]
    fn proven_membership_matches_strict_tree_and_shared_lookup_publication() {
        let (mut pool, old, index, _, bytes) = seeded();
        let lookup = Lookup::new(&index, &old, 256, 16).unwrap();
        let changes = [
            effect(u64::MAX, b"a", Some(b"other database")),
            effect(1, b"new", Some(b"temporary")),
            effect(1, b"b", None),
            effect(1, b"a", Some(b"replace")),
            effect(1, b"new", Some(b"new value")),
        ];
        let plan = Plan::build_with_lookup(&index, &old, &changes, bytes, Some(&lookup)).unwrap();
        let mutations: Vec<_> = (0..plan.len()).map(|i| plan.mutation(i)).collect();
        let ready = pool.prepare(1, &mutations).unwrap();
        let membership = plan.membership(ready.heads()).unwrap();
        let proof = plan.prove_membership(1, &ready, &membership).unwrap();
        let strict = prepare_membership(&index, &membership, 1).unwrap();
        let proven = prepare_membership_proven(&index, &old, 1, &proof, 1).unwrap();
        assert_eq!(
            strict.iter().collect::<Vec<_>>(),
            proven.iter().collect::<Vec<_>>()
        );
        let hash_ready = lookup
            .prepare_proven_with_next(1, &proof, &index, &old, &proven)
            .unwrap();
        assert_eq!(
            lookup.get(&old, 1, b"b"),
            super::super::lookup::Outcome::Hit(b"keep")
        );
        assert_eq!(
            lookup.get(&old, 1, b"new"),
            super::super::lookup::Outcome::Absent
        );
        let next = ready.publish();
        hash_ready.publish();
        assert_eq!(
            get(&proven, &next, 1, b"a").unwrap(),
            Some(b"replace".as_slice())
        );
        assert_eq!(get(&proven, &next, 1, b"b").unwrap(), None);
        assert_eq!(
            lookup.get(&next, 1, b"new"),
            super::super::lookup::Outcome::Hit(b"new value")
        );
        assert_eq!(
            lookup.get(&next, u64::MAX, b"a"),
            super::super::lookup::Outcome::Hit(b"other database")
        );
    }
    #[test]
    fn membership_proof_rejects_foreign_context_and_modified_omitted_reordered_effects() {
        let (mut pool, old, index, heads, bytes) = seeded();
        let changes = [effect(1, b"b", None), effect(1, b"new", Some(b"new value"))];
        let plan = Plan::build(&index, &old, &changes, bytes).unwrap();
        let mutations: Vec<_> = (0..plan.len()).map(|i| plan.mutation(i)).collect();
        let ready = pool.prepare(1, &mutations).unwrap();
        let membership = plan.membership(ready.heads()).unwrap();
        let proof = plan.prove_membership(1, &ready, &membership).unwrap();
        assert!(proof.check(&index, &old, 1).is_ok());
        let (mut foreign_pool, _, _, _, _) = seeded();
        let foreign_ready = foreign_pool.prepare(1, &mutations).unwrap();
        // Numeric handles can coincide across pools; the opaque preparation's
        // source/epoch check must reject even this otherwise identical output.
        assert_eq!(foreign_ready.heads(), ready.heads());
        assert!(
            plan.prove_membership(1, &foreign_ready, &membership)
                .is_err()
        );
        assert!(proof.check(&index.clone(), &old, 1).is_err());
        assert!(proof.check(&index, &old.clone(), 1).is_err());
        assert!(proof.check(&index, &old, 2).is_err());
        assert!(plan.prove_membership(2, &ready, &membership).is_err());
        assert!(plan.prove_membership(1, &ready, &membership[..1]).is_err());
        let mut modified = membership.clone();
        modified[0].key = Bytes::new(b"wrong key").unwrap();
        assert!(plan.prove_membership(1, &ready, &modified).is_err());
        let mut reordered = membership.clone();
        reordered.swap(0, 1);
        assert!(plan.prove_membership(1, &ready, &reordered).is_err());
        let mut wrong_head = membership.clone();
        wrong_head
            .iter_mut()
            .find(|change| change.value.is_some())
            .unwrap()
            .value = Some(Bytes::new(&heads[0].to_le_bytes()).unwrap());
        assert!(plan.prove_membership(1, &ready, &wrong_head).is_err());
        let mut extra = membership.clone();
        extra.push(unlink(1, Bytes::new(b"a").unwrap()));
        assert!(plan.prove_membership(1, &ready, &extra).is_err());
        let lookup = Lookup::new(&index, &old, 256, 16).unwrap();
        assert!(
            lookup
                .prepare_proven_with_next(2, &proof, &index, &old, &index)
                .is_err()
        );
        assert!(
            lookup
                .prepare_proven_with_next(1, &proof, &index.clone(), &old, &index)
                .is_err()
        );
        assert!(prepare_membership_proven(&index, &old.clone(), 1, &proof, 16).is_err());
    }
    #[test]
    fn lookup_planning_uses_stable_heads_and_each_views_selected_value_without_scratch() {
        let (mut pool, old, index, heads, bytes) = seeded();
        let lookup = Lookup::new(&index, &old, 256, 16).unwrap();
        let changes = [effect(1, b"a", Some(b"next"))];
        let before = index.stats().charged_bytes;
        let plan = Plan::build_with_lookup(&index, &old, &changes, bytes, Some(&lookup)).unwrap();
        assert_eq!(index.stats().charged_bytes, before);
        assert_eq!(plan.mutation(0).head, Some(heads[0]));
        let next_bytes = plan.rows_bytes();
        let next = pool.prepare(1, &[plan.mutation(0)]).unwrap().publish();
        assert!(plan.membership_one(heads[0]).unwrap().is_none());
        assert_eq!(
            lookup.resolve(&old, 1, b"a"),
            Resolved::Present {
                head: heads[0],
                value: b"original"
            }
        );
        assert_eq!(
            lookup.resolve(&next, 1, b"a"),
            Resolved::Present {
                head: heads[0],
                value: b"next"
            }
        );
        let restore = [effect(1, b"a", Some(b"original"))];
        let old_noop =
            Plan::build_with_lookup(&index, &old, &restore, bytes, Some(&lookup)).unwrap();
        assert!(!old_noop.dirty());
        let next_restore =
            Plan::build_with_lookup(&index, &next, &restore, next_bytes, Some(&lookup)).unwrap();
        assert!(next_restore.dirty());
        assert_eq!(next_restore.mutation(0).head, Some(heads[0]));
        assert_eq!(next_restore.rows_bytes(), bytes);
        assert_eq!(index.stats().charged_bytes, before);
    }
    #[test]
    fn lookup_plans_match_ordered_coalescing_absence_and_disabled_fallback() {
        let (_, old, index, _, bytes) = seeded();
        let lookup = Lookup::new(&index, &old, 256, 16).unwrap();
        let changes = [
            effect(1, b"a", Some(b"temporary")),
            effect(1, b"a", Some(b"original")),
            effect(1, b"b", None),
            effect(1, b"new", Some(b"first")),
            effect(1, b"new", Some(b"last")),
            effect(u64::MAX, b"a", Some(b"other database")),
            effect(1, b"absent", None),
        ];
        assert_eq!(lookup.resolve(&old, 1, b"absent"), Resolved::Absent);
        assert_eq!(lookup.resolve(&old, u64::MAX, b"a"), Resolved::Absent);
        let ordered = Plan::build(&index, &old, &changes, bytes).unwrap();
        let signature = |plan: &Plan<'_>| {
            (
                plan.dirty(),
                plan.rows_bytes(),
                (0..plan.len())
                    .map(|i| {
                        let mutation = plan.mutation(i);
                        (mutation.head, mutation.value.map(<[u8]>::to_vec))
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let cached = Plan::build_with_lookup(&index, &old, &changes, bytes, Some(&lookup)).unwrap();
        assert_eq!(signature(&cached), signature(&ordered));
        lookup.disable();
        assert_eq!(lookup.resolve(&old, 1, b"a"), Resolved::Fallback);
        let disabled =
            Plan::build_with_lookup(&index, &old, &changes, bytes, Some(&lookup)).unwrap();
        assert_eq!(signature(&disabled), signature(&ordered));
    }
    #[test]
    fn lookup_fallback_preserves_authoritative_stale_binding_error() {
        let (mut pool, old, index, heads, bytes) = seeded();
        let lookup = Lookup::new(&index, &old, 256, 16).unwrap();
        let deleted = pool
            .prepare(
                1,
                &[Mutation {
                    head: Some(heads[0]),
                    value: None,
                }],
            )
            .unwrap()
            .publish();
        let changes = [effect(1, b"a", Some(b"replacement"))];
        // Deliberately pair a stale authoritative binding with a later View.
        // The derived lookup must fall back so the existing corruption error
        // survives rather than treating the row as a fresh insertion.
        assert_eq!(lookup.resolve(&deleted, 1, b"a"), Resolved::Fallback);
        assert!(matches!(
            Plan::build(&index, &deleted, &changes, bytes),
            Err(Error::Corrupt(_))
        ));
        assert!(matches!(
            Plan::build_with_lookup(&index, &deleted, &changes, bytes, Some(&lookup)),
            Err(Error::Corrupt(_))
        ));
    }
    #[test]
    fn lookup_plan_resolves_new_delta_key_and_old_view_absence() {
        let (mut pool, old, index, _, bytes) = seeded();
        let lookup = Lookup::new(&index, &old, 256, 16).unwrap();
        let key = [255; 40];
        let ready = pool
            .prepare(
                1,
                &[Mutation {
                    head: None,
                    value: Some(b"new"),
                }],
            )
            .unwrap();
        let head = ready.heads()[0];
        let membership = link(u64::MAX, Bytes::new(&key).unwrap(), head).unwrap();
        let lookup_ready = lookup
            .prepare(1, std::slice::from_ref(&membership), &index)
            .unwrap();
        let next_index = index.prepare(std::slice::from_ref(&membership)).unwrap();
        let next = ready.publish();
        lookup_ready.publish();
        let changes = [effect(u64::MAX, &key, Some(b"updated"))];
        let before_insert =
            Plan::build_with_lookup(&index, &old, &changes, bytes, Some(&lookup)).unwrap();
        assert_eq!(before_insert.mutation(0).head, None);
        let after_insert = Plan::build_with_lookup(
            &next_index,
            &next,
            &changes,
            bytes + 16 + key.len() + 3,
            Some(&lookup),
        )
        .unwrap();
        assert_eq!(after_insert.mutation(0).head, Some(head));
        assert!(after_insert.membership_one(head).unwrap().is_none());
    }
    #[test]
    fn plan_single_replacement_uses_no_index_scratch_or_membership_edit() {
        let (mut pool, old, index, heads, bytes) = seeded();
        let borrowed = old.get(heads[0]).unwrap();
        let pointer = borrowed.as_ptr();
        let before = index.stats().charged_bytes;
        let changes = [effect(1, b"a", Some(b"next"))];
        let plan = Plan::build(&index, &old, &changes, bytes).unwrap();
        assert!(plan.dirty());
        assert_eq!(plan.len(), 1);
        assert_eq!(plan.rows_bytes(), bytes - b"original".len() + b"next".len());
        assert_eq!(index.stats().charged_bytes, before);
        let mutation = plan.mutation(0);
        assert_eq!(mutation.head, Some(heads[0]));
        let ready = pool.prepare(1, &[mutation]).unwrap();
        let membership = plan.membership(ready.heads()).unwrap();
        assert_eq!(membership.capacity(), 0);
        assert!(plan.membership_one(ready.heads()[0]).unwrap().is_none());
        let next_index = prepare_membership(&index, &membership, 16).unwrap();
        assert!(next_index.same_state(&index));
        let next = ready.publish();
        assert_eq!(
            get(&next_index, &next, 1, b"a").unwrap(),
            Some(b"next".as_slice())
        );
        assert_eq!(borrowed, b"original");
        assert_eq!(borrowed.as_ptr(), pointer);
        assert_eq!(index.stats().charged_bytes, before);
    }
    #[test]
    fn plan_coalesces_keys_once_preserves_transient_dirty_and_actual_byte_count() {
        let (mut pool, old, index, heads, bytes) = seeded();
        let changes = [
            effect(1, b"a", Some(b"temporary")),
            effect(1, b"a", Some(b"original")),
            effect(1, b"b", None),
            effect(1, b"new", Some(b"first")),
            effect(1, b"new", None),
            effect(1, b"new", Some(&[0, 255, 0])),
            effect(1, b"absent", None),
        ];
        let before = index.stats().charged_bytes;
        let plan = Plan::build(&index, &old, &changes, bytes).unwrap();
        assert!(plan.dirty());
        assert_eq!(plan.len(), 2);
        assert_eq!(
            plan.rows_bytes(),
            bytes - (16 + 1 + b"keep".len()) + (16 + 3 + 3)
        );
        assert!(index.stats().charged_bytes > before);
        let edits: Vec<_> = (0..plan.len()).map(|i| plan.mutation(i)).collect();
        assert_eq!(edits.iter().filter(|e| e.head == Some(heads[0])).count(), 0);
        let ready = pool.prepare(1, &edits).unwrap();
        let membership = plan.membership(ready.heads()).unwrap();
        assert_eq!(membership.len(), 2);
        let next_index = prepare_membership(&index, &membership, usize::MAX).unwrap();
        let next = ready.publish();
        assert_eq!(
            get(&next_index, &next, 1, b"a").unwrap(),
            Some(b"original".as_slice())
        );
        assert!(get(&next_index, &next, 1, b"b").unwrap().is_none());
        assert_eq!(
            get(&next_index, &next, 1, b"new").unwrap(),
            Some([0, 255, 0].as_slice())
        );
        assert_eq!(
            get(&index, &old, 1, b"b").unwrap(),
            Some(b"keep".as_slice())
        );
        drop(membership);
        drop(edits);
        drop(plan);
        drop(next_index);
        assert_eq!(index.stats().charged_bytes, before);
    }
    #[test]
    fn plan_empty_equal_single_and_transient_restore_have_distinct_dirty_semantics() {
        let (mut pool, view, index, _, bytes) = seeded();
        let before = index.stats().charged_bytes;
        let empty = Plan::build(&index, &view, &[], bytes).unwrap();
        assert!(!empty.dirty());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.rows_bytes(), bytes);
        let equal_changes = [effect(1, b"a", Some(b"original"))];
        let equal = Plan::build(&index, &view, &equal_changes, bytes).unwrap();
        assert!(!equal.dirty());
        assert_eq!(equal.len(), 0);
        assert_eq!(index.stats().charged_bytes, before);
        let changes = [
            effect(1, b"a", None),
            effect(1, b"a", Some(b"original")),
            effect(1, b"new", Some(b"x")),
            effect(1, b"new", None),
        ];
        let restore = Plan::build(&index, &view, &changes, bytes).unwrap();
        assert!(restore.dirty());
        assert_eq!(restore.len(), 0);
        assert_eq!(restore.rows_bytes(), bytes);
        assert_eq!(restore.membership(&[]).unwrap().capacity(), 0);
        let next = pool.prepare(1, &[]).unwrap().publish();
        assert_eq!(
            get(&index, &next, 1, b"a").unwrap(),
            Some(b"original".as_slice())
        );
    }
    #[test]
    fn plan_duplicate_existing_updates_become_one_pool_edit_and_invalid_heads_refuse() {
        let (mut pool, view, index, heads, bytes) = seeded();
        let changes: Vec<_> = (0u64..100)
            .map(|n| effect(1, b"a", Some(&n.to_be_bytes())))
            .collect();
        let plan = Plan::build(&index, &view, &changes, bytes).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan.mutation(0).head, Some(heads[0]));
        let next = pool.prepare(1, &[plan.mutation(0)]).unwrap().publish();
        assert_eq!(next.get(heads[0]), Some(99u64.to_be_bytes().as_slice()));
        assert!(plan.membership(&[]).is_err());
        assert!(plan.membership(&[heads[1]]).is_err());
        let malformed = Index::new(4 << 20)
            .unwrap()
            .prepare(&[link(1, Bytes::new(b"stale").unwrap(), handle(16)).unwrap()])
            .unwrap();
        let changes = [effect(1, b"stale", Some(b"next"))];
        assert!(Plan::build(&malformed, &view, &changes, 16 + 5 + 3).is_err());
    }
    #[test]
    fn plan_many_scratch_refusal_is_atomic() {
        let (_, view, _, _, bytes) = seeded();
        let index = Index::new(8192).unwrap();
        let before = index.stats().charged_bytes;
        let changes: Vec<_> = (0u64..1000)
            .map(|n| effect(9, &n.to_be_bytes(), Some(b"v")))
            .collect();
        assert!(Plan::build(&index, &view, &changes, bytes).is_err());
        assert_eq!(index.stats().charged_bytes, before);
    }

    // These exercise typed key bindings without manufacturing a serving View.
    // Pool/Environment integration independently proves value lifetimes/reuse.
    fn handle(number: u64) -> Head {
        Head::from_u64((1u64 << 32) | number).expect("valid generation-one test handle")
    }
    #[test]
    fn stable_membership_roundtrips_and_empty_edits_share_root() {
        let empty = Index::new(4 << 20).unwrap();
        let key = Bytes::new(&[0, 255, 0, 128]).unwrap();
        let first = prepare_membership(
            &empty,
            &[link(7, key.clone(), handle(1)).unwrap()],
            usize::MAX,
        )
        .unwrap();
        assert_eq!(head(&first, 7, key.as_slice()).unwrap(), Some(handle(1)));
        let unchanged = prepare_membership(&first, &[], 16).unwrap();
        assert!(first.same_state(&unchanged));
        let deleted = prepare_membership(&first, &[unlink(7, key.clone())], usize::MAX).unwrap();
        assert!(head(&deleted, 7, key.as_slice()).unwrap().is_none());
        assert_eq!(head(&first, 7, key.as_slice()).unwrap(), Some(handle(1)));
        let reinserted = prepare_membership(
            &deleted,
            &[link(7, key.clone(), handle(2)).unwrap()],
            usize::MAX,
        )
        .unwrap();
        assert_eq!(
            head(&reinserted, 7, key.as_slice()).unwrap(),
            Some(handle(2))
        );
        assert_eq!(head(&first, 7, key.as_slice()).unwrap(), Some(handle(1)));
    }
    #[test]
    fn malformed_and_existing_key_rebinding_fail_without_changing_root() {
        let empty = Index::new(4 << 20).unwrap();
        let first = prepare_membership(
            &empty,
            &[link(1, Bytes::new(b"key").unwrap(), handle(1)).unwrap()],
            usize::MAX,
        )
        .unwrap();
        let charge = first.stats().charged_bytes;
        assert!(
            prepare_membership(
                &first,
                &[link(1, Bytes::new(b"key").unwrap(), handle(2)).unwrap()],
                usize::MAX
            )
            .is_err()
        );
        assert!(
            prepare_membership(
                &first,
                &[Change {
                    database: 1,
                    key: Bytes::new(b"new").unwrap(),
                    value: Some(Bytes::new(b"bad").unwrap())
                }],
                usize::MAX
            )
            .is_err()
        );
        assert_eq!(first.stats().charged_bytes, charge);
        assert_eq!(head(&first, 1, b"key").unwrap(), Some(handle(1)));
        assert!(head(&first, 1, b"new").unwrap().is_none());
    }
}
