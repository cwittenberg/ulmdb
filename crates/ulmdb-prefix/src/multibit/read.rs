use super::{Budget, Visits, view::NodeView, wire};
use ulmdb_core::{
    Address, Prefix,
    storage::{Error, ReadStore, Reference, Result},
};
/// Prepared root; all descendant wire/parent checks remain active.
pub struct Reader<'a, S: ReadStore + ?Sized> {
    store: &'a S,
    root: Option<NodeView<'a>>,
    budget: Budget,
}
impl<'a, S: ReadStore + ?Sized> Reader<'a, S> {
    pub fn new(store: &'a S, root: Reference) -> Result<Self> {
        Self::with_budget(store, root, Budget::default())
    }
    pub fn with_budget(store: &'a S, root: Reference, budget: Budget) -> Result<Self> {
        let root = if root.is_null() {
            None
        } else {
            Some(NodeView::read(store, root, &mut Visits::new(budget))?)
        };
        Ok(Self {
            store,
            root,
            budget,
        })
    }
    pub fn root_prefix(&self) -> Option<Prefix> {
        self.root.map(|n| n.base)
    }
    pub fn longest_match(&self, address: Address) -> Result<Option<(Prefix, &'a [u8])>> {
        self.search(address, wire::width(address), false)
    }
    pub fn covering(&self, prefix: Prefix) -> Result<Option<(Prefix, &'a [u8])>> {
        self.search(prefix.network(), prefix.length(), false)
    }
    pub fn exact(&self, prefix: Prefix) -> Result<Option<(Prefix, &'a [u8])>> {
        self.search(prefix.network(), prefix.length(), true)
    }
    fn search(
        &self,
        address: Address,
        limit: u8,
        exact: bool,
    ) -> Result<Option<(Prefix, &'a [u8])>> {
        let Some(mut node) = self.root else {
            return Ok(None);
        };
        if !wire::family(node.base.network(), address) {
            return Err(Error::InvalidInput("multibit query family mismatch"));
        }
        let mut visits = Visits::new(self.budget);
        visits.take()?;
        let mut best = None;
        loop {
            if !node.base.contains_address(address) || node.base.length() > limit {
                break;
            }
            let available = (limit - node.base.length())
                .min(3)
                .min(wire::width(address) - node.base.length());
            for k in 0..=available {
                let p = Prefix::normalize(address, node.base.length() + k).unwrap();
                let r = node.value(wire::slot(p))?;
                if !r.is_null() && (!exact || p.length() == limit) {
                    best = Some((p, r));
                }
            }
            if node.base.length() + 4 > limit || node.base.length() == wire::width(address) {
                break;
            }
            let Some(child) = node.child(
                self.store,
                wire::nibble(address, node.base.length()),
                &mut visits,
            )?
            else {
                break;
            };
            node = child;
        }
        best.map(|(p, r)| Ok((p, self.store.payload(r)?)))
            .transpose()
    }
}
pub fn longest_match<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    address: Address,
) -> Result<Option<(Prefix, &[u8])>> {
    Reader::new(store, root)?.longest_match(address)
}
pub fn exact<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    prefix: Prefix,
) -> Result<Option<(Prefix, &[u8])>> {
    Reader::new(store, root)?.exact(prefix)
}
pub fn covering<S: ReadStore + ?Sized>(
    store: &S,
    root: Reference,
    prefix: Prefix,
) -> Result<Option<(Prefix, &[u8])>> {
    Reader::new(store, root)?.covering(prefix)
}
