use super::{
    Budget, Visits,
    wire::{self, Node},
};
use ulmdb_core::{
    Prefix,
    storage::{Error, ReadStore, Reference, Result},
};
#[derive(Clone, Copy)]
pub enum Selection {
    Ancestors,
    Subnets,
    Overlaps,
}
#[derive(Clone, Copy)]
enum Task {
    Value(Prefix, Reference),
    Child(Prefix, Reference),
}
impl Task {
    fn prefix(self) -> Prefix {
        match self {
            Self::Value(p, _) | Self::Child(p, _) => p,
        }
    }
}
/// Bounded DFS, no result-size allocation. Ancestors longest-first; others
/// ordered by network then prefix length. Returned values borrow the store.
pub struct Cursor<'a, S: ReadStore + ?Sized> {
    store: &'a S,
    query: Prefix,
    selection: Selection,
    visits: Visits,
    pending: [Option<Task>; 1024],
    count: usize,
    failed: bool,
}
impl<'a, S: ReadStore + ?Sized> Cursor<'a, S> {
    pub fn new(
        store: &'a S,
        root: Reference,
        query: Prefix,
        selection: Selection,
        budget: Budget,
    ) -> Result<Self> {
        let mut c = Self {
            store,
            query,
            selection,
            visits: Visits::new(budget),
            pending: [None; 1024],
            count: 0,
            failed: false,
        };
        if root.is_null() {
            return Ok(c);
        }
        let mut node = Node::read(store, root, &mut c.visits)?;
        if !wire::family(node.base.network(), query.network()) {
            return Err(Error::InvalidInput("multibit cursor family mismatch"));
        }
        if matches!(selection, Selection::Ancestors) {
            loop {
                if !node.base.contains_prefix(query) {
                    break;
                }
                for i in 0..15 {
                    if !node.values[i].is_null() {
                        let p = node.prefix(i);
                        if p.contains_prefix(query) {
                            c.push(Task::Value(p, node.values[i]))?;
                        }
                    }
                }
                if node.base.length() + 4 > query.length()
                    || node.base.length() == wire::width(node.base.network())
                {
                    break;
                }
                let Some((_, child)) = node.child(
                    store,
                    wire::nibble(query.network(), node.base.length()),
                    &mut c.visits,
                )?
                else {
                    break;
                };
                node = child;
            }
        } else if node.base.overlaps(query) {
            c.expand(node)?;
        }
        Ok(c)
    }
    fn push(&mut self, t: Task) -> Result<()> {
        let item = self
            .pending
            .get_mut(self.count)
            .ok_or(Error::Corrupt("multibit traversal stack exceeded"))?;
        *item = Some(t);
        self.count += 1;
        Ok(())
    }
    fn expand(&mut self, node: Node) -> Result<()> {
        let mut tasks = [None; 31];
        let mut count = 0;
        for i in 0..15 {
            if !node.values[i].is_null() {
                let p = node.prefix(i);
                let selected = match self.selection {
                    Selection::Subnets => self.query.contains_prefix(p),
                    _ => self.query.overlaps(p),
                };
                if selected {
                    tasks[count] = Some(Task::Value(p, node.values[i]));
                    count += 1;
                }
            }
        }
        if node.base.length() != wire::width(node.base.network()) {
            for i in 0..16 {
                if !node.children[i].is_null() {
                    let region = node.region(i);
                    if region.overlaps(self.query) {
                        tasks[count] = Some(Task::Child(region, node.children[i]));
                        count += 1;
                    }
                }
            }
        }
        tasks[..count].sort_unstable_by_key(|t| {
            let p = t.unwrap().prefix();
            (wire::number(p.network()), p.length())
        });
        for task in tasks[..count].iter().rev() {
            self.push(task.unwrap())?;
        }
        Ok(())
    }
    fn advance(&mut self) -> Result<Option<(Prefix, &'a [u8])>> {
        while self.count > 0 {
            self.count -= 1;
            let task = self.pending[self.count]
                .take()
                .ok_or(Error::Corrupt("multibit cursor stack"))?;
            match task {
                Task::Value(p, r) => return Ok(Some((p, self.store.payload(r)?))),
                Task::Child(region, at) => {
                    let node = Node::read(self.store, at, &mut self.visits)?;
                    if !region.contains_prefix(node.base) {
                        return Err(Error::Corrupt("multibit cursor child route/depth"));
                    }
                    self.expand(node)?;
                }
            }
        }
        Ok(None)
    }
}
impl<'a, S: ReadStore + ?Sized> Iterator for Cursor<'a, S> {
    type Item = Result<(Prefix, &'a [u8])>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.advance() {
            Ok(Some(v)) => Some(Ok(v)),
            Ok(None) => None,
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}
impl<S: ReadStore + ?Sized> std::iter::FusedIterator for Cursor<'_, S> {}
pub fn ancestors<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    prefix: Prefix,
) -> Result<Cursor<'_, S>> {
    Cursor::new(store, root, prefix, Selection::Ancestors, Budget::default())
}
pub fn subnets<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    prefix: Prefix,
) -> Result<Cursor<'_, S>> {
    Cursor::new(store, root, prefix, Selection::Subnets, Budget::default())
}
pub fn overlaps<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    prefix: Prefix,
) -> Result<Cursor<'_, S>> {
    Cursor::new(store, root, prefix, Selection::Overlaps, Budget::default())
}
