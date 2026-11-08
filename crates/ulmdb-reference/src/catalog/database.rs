//! Typed database data, identity validation and catalog operations.

use crate::{ModelPrefix, PrefixStore};
use std::collections::{BTreeMap, BTreeSet};

type DuplicatePairs = BTreeSet<(Vec<u8>, Vec<u8>)>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Kv,
    Duplicates { width: Option<usize> },
    Ipv4,
    Ipv6,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidName,
    DatabaseExists,
    DatabaseNotFound,
    StaleHandle,
    WrongKind,
    WrongFamily,
    KeyTooLarge,
    ValueTooLarge,
    DuplicateTooLarge,
    WidthMismatch,
    InvalidSavepoint,
    LimitExceeded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Handle {
    namespace: u128,
    id: u64,
    kind: Kind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Data {
    Kv(BTreeMap<Vec<u8>, Vec<u8>>),
    Duplicates {
        pairs: DuplicatePairs,
        width: Option<usize>,
    },
    Prefix {
        ipv6: bool,
        values: PrefixStore,
    },
}

impl Data {
    fn kind(&self) -> Kind {
        match self {
            Self::Kv(_) => Kind::Kv,
            Self::Duplicates { width, .. } => Kind::Duplicates { width: *width },
            Self::Prefix { ipv6: false, .. } => Kind::Ipv4,
            Self::Prefix { ipv6: true, .. } => Kind::Ipv6,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Catalog {
    namespace: u128,
    databases: BTreeMap<Vec<u8>, (u64, Data)>,
}

impl Catalog {
    pub(super) const fn namespace(&self) -> u128 {
        self.namespace
    }

    pub fn new(namespace: u128) -> Self {
        Self {
            namespace,
            databases: BTreeMap::new(),
        }
    }

    pub(super) fn create(&mut self, name: &[u8], kind: Kind, id: u64) -> Result<Handle, Error> {
        if name.is_empty() || name.len() > 255 {
            return Err(Error::InvalidName);
        }
        if self.databases.contains_key(name) {
            return Err(Error::DatabaseExists);
        }
        let data = match kind {
            Kind::Kv => Data::Kv(BTreeMap::new()),
            Kind::Duplicates { width } => {
                if width.is_some_and(|n| n > 65_536) {
                    return Err(Error::DuplicateTooLarge);
                }
                Data::Duplicates {
                    pairs: BTreeSet::new(),
                    width,
                }
            }
            Kind::Ipv4 => Data::Prefix {
                ipv6: false,
                values: PrefixStore::default(),
            },
            Kind::Ipv6 => Data::Prefix {
                ipv6: true,
                values: PrefixStore::default(),
            },
        };
        self.databases.insert(name.to_vec(), (id, data));
        Ok(Handle {
            namespace: self.namespace,
            id,
            kind,
        })
    }

    pub fn open(&self, name: &[u8], expected: Kind) -> Result<Handle, Error> {
        let (id, data) = self.databases.get(name).ok_or(Error::DatabaseNotFound)?;
        if data.kind() != expected {
            return Err(Error::WrongKind);
        }
        Ok(Handle {
            namespace: self.namespace,
            id: *id,
            kind: expected,
        })
    }

    fn data(&self, h: Handle) -> Result<&Data, Error> {
        if h.namespace != self.namespace {
            return Err(Error::StaleHandle);
        }
        self.databases
            .values()
            .find(|(id, d)| *id == h.id && d.kind() == h.kind)
            .map(|(_, d)| d)
            .ok_or(Error::StaleHandle)
    }

    fn data_mut(&mut self, h: Handle) -> Result<&mut Data, Error> {
        if h.namespace != self.namespace {
            return Err(Error::StaleHandle);
        }
        self.databases
            .values_mut()
            .find(|(id, d)| *id == h.id && d.kind() == h.kind)
            .map(|(_, d)| d)
            .ok_or(Error::StaleHandle)
    }

    pub fn drop_database(&mut self, h: Handle) -> Result<(), Error> {
        self.data(h)?;
        self.databases.retain(|_, (id, _)| *id != h.id);
        Ok(())
    }

    pub fn kv(&self, h: Handle) -> Result<&BTreeMap<Vec<u8>, Vec<u8>>, Error> {
        match self.data(h)? {
            Data::Kv(map) => Ok(map),
            _ => Err(Error::WrongKind),
        }
    }

    pub fn put(&mut self, h: Handle, key: &[u8], value: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        self.data(h)?;
        if key.len() > 4096 {
            return Err(Error::KeyTooLarge);
        }
        if value.len() > 67_108_864 {
            return Err(Error::ValueTooLarge);
        }
        match self.data_mut(h)? {
            Data::Kv(map) => Ok(map.insert(key.to_vec(), value.to_vec())),
            _ => Err(Error::WrongKind),
        }
    }

    pub fn delete(&mut self, h: Handle, key: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        if key.len() > 4096 {
            return Err(Error::KeyTooLarge);
        }
        match self.data_mut(h)? {
            Data::Kv(map) => Ok(map.remove(key)),
            _ => Err(Error::WrongKind),
        }
    }

    pub fn pairs(&self, h: Handle) -> Result<&DuplicatePairs, Error> {
        match self.data(h)? {
            Data::Duplicates { pairs, .. } => Ok(pairs),
            _ => Err(Error::WrongKind),
        }
    }

    pub fn insert_pair(&mut self, h: Handle, key: &[u8], value: &[u8]) -> Result<bool, Error> {
        self.data(h)?;
        if key.len() > 4096 {
            return Err(Error::KeyTooLarge);
        }
        if value.len() > 65_536 {
            return Err(Error::DuplicateTooLarge);
        }
        match self.data_mut(h)? {
            Data::Duplicates { pairs, width } => {
                if width.is_some_and(|n| n != value.len()) {
                    return Err(Error::WidthMismatch);
                }
                Ok(pairs.insert((key.to_vec(), value.to_vec())))
            }
            _ => Err(Error::WrongKind),
        }
    }

    pub fn delete_pair(&mut self, h: Handle, key: &[u8], value: &[u8]) -> Result<bool, Error> {
        if key.len() > 4096 {
            return Err(Error::KeyTooLarge);
        }
        if value.len() > 65_536 {
            return Err(Error::DuplicateTooLarge);
        }
        match self.data_mut(h)? {
            Data::Duplicates { pairs, width } => {
                if width.is_some_and(|n| n != value.len()) {
                    return Err(Error::WidthMismatch);
                }
                Ok(pairs.remove(&(key.to_vec(), value.to_vec())))
            }
            _ => Err(Error::WrongKind),
        }
    }

    pub fn delete_pairs(&mut self, h: Handle, key: &[u8]) -> Result<usize, Error> {
        if key.len() > 4096 {
            return Err(Error::KeyTooLarge);
        }
        match self.data_mut(h)? {
            Data::Duplicates { pairs, .. } => {
                let old = pairs.len();
                pairs.retain(|(k, _)| k != key);
                Ok(old - pairs.len())
            }
            _ => Err(Error::WrongKind),
        }
    }

    pub fn prefixes(&self, h: Handle) -> Result<&PrefixStore, Error> {
        match self.data(h)? {
            Data::Prefix { values, .. } => Ok(values),
            _ => Err(Error::WrongKind),
        }
    }

    pub fn insert_prefix(
        &mut self,
        h: Handle,
        prefix: ModelPrefix,
        value: &[u8],
    ) -> Result<Option<Vec<u8>>, Error> {
        if value.len() > 67_108_864 {
            return Err(Error::ValueTooLarge);
        }
        match self.data_mut(h)? {
            Data::Prefix { ipv6, values } => {
                if *ipv6 != prefix.is_ipv6() {
                    return Err(Error::WrongFamily);
                }
                Ok(values.insert(prefix, value.to_vec()))
            }
            _ => Err(Error::WrongKind),
        }
    }

    pub fn delete_prefix(
        &mut self,
        h: Handle,
        prefix: &ModelPrefix,
    ) -> Result<Option<Vec<u8>>, Error> {
        match self.data_mut(h)? {
            Data::Prefix { ipv6, values } => {
                if *ipv6 != prefix.is_ipv6() {
                    return Err(Error::WrongFamily);
                }
                Ok(values.delete(prefix))
            }
            _ => Err(Error::WrongKind),
        }
    }
}
