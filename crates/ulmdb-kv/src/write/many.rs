//! Rare full compact-to-V1 normalization can need more than two siblings.
use super::*;
pub(super) fn persist_many<S: WriteStore>(
    store: &mut S,
    old: Reference,
    level: u16,
    first: Reference,
    cells: &[&[u8]],
) -> Result<Change> {
    let mut start = 0;
    let mut child = first;
    let mut root = Reference::NULL;
    let mut splits = Vec::new();
    let mut separator = None;
    while start < cells.len() {
        let mut end = start;
        let mut used = HEADER;
        while end < cells.len() && used + 2 + cells[end].len() <= store.page_size() {
            used += 2 + cells[end].len();
            end += 1;
        }
        if end == start {
            return Err(Error::LimitExceeded);
        }
        let bytes = page::encode_v1_for(store.page_size(), level, child, &cells[start..end])?;
        let at = if start == 0 && !old.is_null() {
            store.rewrite(old, &bytes, store.page_size())?
        } else {
            store.allocate(&bytes, store.page_size())?
        };
        if let Some(separator) = separator.take() {
            splits.push((separator, at));
        } else {
            root = at;
        }
        if end == cells.len() {
            break;
        }
        separator = Some(Separator::from_cell(cells[end], level)?);
        if level == 0 {
            start = end;
        } else {
            child = Reference::decode(&cells[end][4..20])?;
            start = end + 1;
            if start == cells.len() {
                let bytes = page::encode_v1_for(store.page_size(), level, child, &[])?;
                let at = store.allocate(&bytes, store.page_size())?;
                splits.push((separator.take().unwrap(), at));
                break;
            }
        }
    }
    let split = if splits.len() == 1 {
        Split::One(splits.pop().unwrap())
    } else {
        Split::Many(splits.into_boxed_slice())
    };
    Ok(Change { root, split })
}
pub(super) fn promote<S: WriteStore>(
    store: &mut S,
    mut root: Reference,
    mut level: u16,
    mut siblings: Vec<(Separator, Reference)>,
) -> Result<Reference> {
    loop {
        level = level
            .checked_add(1)
            .filter(|x| *x < MAX_DEPTH)
            .ok_or(Error::LimitExceeded)?;
        let records = siblings
            .iter()
            .map(|(separator, child)| branch_cell(store, separator, *child))
            .collect::<Result<Vec<_>>>()?;
        let refs: Vec<_> = records.iter().map(CellBuffer::as_bytes).collect();
        let changed = persist(store, Reference::NULL, level, root, &refs, false)?;
        root = changed.root;
        match changed.split {
            Split::None | Split::Unchanged => return Ok(root),
            Split::One(pair) => siblings = vec![pair],
            Split::Many(many) => siblings = many.into_vec(),
        }
    }
}
