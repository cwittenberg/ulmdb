//! Writer-side inspection used by language bindings. Owned duplicate pairs
//! remain valid across subsequent writer mutations without aliasing its arena.
use crate::{DatabaseKind, Error, Prefix, Result, WriteTxn};
use std::ops::Bound;

impl WriteTxn<'_> {
    pub fn range<'a>(
        &'a self,
        name: &[u8],
        lower: Bound<&[u8]>,
        upper: Bound<&[u8]>,
        reverse: bool,
    ) -> Result<ulmdb_kv::Cursor<'a, ulmdb_storage::Arena>> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        if db.kind != DatabaseKind::Kv {
            return Err(Error::WrongKind);
        }
        ulmdb_kv::Cursor::new(&self.arena, db.root, lower, upper, reverse)
    }

    pub fn exact_prefix(&self, name: &[u8], prefix: Prefix) -> Result<Option<(Prefix, &[u8])>> {
        self.live()?;
        let db = self.catalog.get(name).ok_or(Error::MissingDatabase)?;
        crate::read::check_family(db.kind, prefix.network())?;
        ulmdb_prefix::exact(&self.arena, db.root, prefix)
    }
}
