//! Named-database transaction and savepoint lifecycle oracle.

use super::{Catalog, Error, Handle, Kind};

/// Explicit unique namespace corresponds to a persisted environment UUID.
pub struct Environment {
    current: Catalog,
    sequence: u64,
    next_database: u64,
    next_writer: u64,
}

impl Environment {
    pub fn new(namespace: u128) -> Self {
        Self {
            current: Catalog::new(namespace),
            sequence: 0,
            next_database: 1,
            next_writer: 1,
        }
    }
    pub fn snapshot(&self) -> (u64, Catalog) {
        (self.sequence, self.current.clone())
    }
    pub fn begin_write(&mut self) -> Result<Write<'_>, Error> {
        let writer = self.next_writer;
        self.next_writer = self
            .next_writer
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        let private = self.current.clone();
        Ok(Write {
            env: self,
            private,
            writer,
            next_savepoint: 1,
            savepoints: Vec::new(),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Savepoint {
    namespace: u128,
    writer: u64,
    serial: u64,
}

pub struct Write<'a> {
    env: &'a mut Environment,
    pub private: Catalog,
    writer: u64,
    next_savepoint: u64,
    savepoints: Vec<(Savepoint, Catalog)>,
}

impl Write<'_> {
    pub fn create(&mut self, name: &[u8], kind: Kind) -> Result<Handle, Error> {
        let id = self.env.next_database;
        // Burn provisional IDs even when a transaction aborts or creation fails.
        self.env.next_database = self
            .env
            .next_database
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        self.private.create(name, kind, id)
    }
    pub fn savepoint(&mut self) -> Result<Savepoint, Error> {
        let token = Savepoint {
            namespace: self.private.namespace(),
            writer: self.writer,
            serial: self.next_savepoint,
        };
        self.next_savepoint = self
            .next_savepoint
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        self.savepoints.push((token, self.private.clone()));
        Ok(token)
    }
    pub fn rollback(&mut self, token: Savepoint) -> Result<(), Error> {
        let index = self
            .savepoints
            .iter()
            .position(|(t, _)| *t == token)
            .ok_or(Error::InvalidSavepoint)?;
        self.private = self.savepoints[index].1.clone();
        self.savepoints.truncate(index + 1);
        Ok(())
    }
    pub fn commit(self) -> Result<u64, Error> {
        let next = self
            .env
            .sequence
            .checked_add(1)
            .ok_or(Error::LimitExceeded)?;
        self.env.current = self.private;
        self.env.sequence = next;
        Ok(next)
    }
    pub fn abort(self) {}
}

#[cfg(test)]
mod tests;
