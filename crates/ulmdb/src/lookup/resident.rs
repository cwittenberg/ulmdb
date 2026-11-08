//! Registered inside crate::resident so private catalog/root admission remains
//! confined to its existing visibility boundary. Serving readers are unchanged.
use crate::{
    Error, Result,
    lookup::{Backend, Identity, Native, Options, PreparedLookup},
    ordering::DatabaseOrdering,
};

impl super::ReadTxn {
    pub fn prepare_lookup<'a>(
        &'a self,
        name: &'a [u8],
        options: Options,
    ) -> Result<PreparedLookup<'a>> {
        let database = self
            .snapshot
            .catalog
            .get(name)
            .ok_or(Error::MissingDatabase)?;
        if database.kind != crate::DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        if matches!(options, Options::Hash(_)) {
            if database.ordering != DatabaseOrdering::default() {
                return Err(Error::InvalidInput(
                    "hash lookup requires bytewise ordering",
                ));
            }
            if self.snapshot.image.has_transform() {
                return Err(Error::InvalidInput(
                    "hash lookup requires immutable untransformed managed storage",
                ));
            }
        }
        let identity = Identity {
            backend: Backend::Resident,
            sequence: self.sequence(),
            commit: self.commit_identity(),
            database_id: database.id,
            root: database.root,
            name,
        };
        PreparedLookup::new(Native::Resident(self.kv(name)?), identity, 4080, options)
    }
}
