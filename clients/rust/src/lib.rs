//! Safe Rust access to ULMDB's transactions, ordered bytes, and IP indexes.
//!
//! All engine APIs are reexported unchanged, including borrowed read views,
//! checked database handles, duplicate indexes, savepoints, backup and replication.
//! [`Env`] adds a small convenience facade without copying read values.
//!
//! ```no_run
//! use ulmdb_rs::{DatabaseKind, Env, address, prefix};
//! # fn main() -> ulmdb_rs::Result<()> {
//! let env = Env::open_or_create("routes.ulmdb")?;
//! env.update(|write| {
//!     write.create_database(b"routes", DatabaseKind::Ipv4)?;
//!     write.insert_prefix(b"routes", prefix("192.0.2.0/24")?, b"example")
//! })?;
//! let read = env.read();
//! let route = read.longest_match(b"routes", address("192.0.2.42")?)?;
//! assert_eq!(route.map(|(_, value)| value), Some(b"example".as_slice()));
//! # Ok(())
//! # }
//! ```

pub use ulmdb::*;

use std::{net::IpAddr, ops::Deref, path::Path};

/// An owned environment with convenience transaction methods.
///
/// Dereferencing exposes the entire [`Environment`] API. Writes are serialized
/// by the engine, and a read transaction pins its snapshot across later commits.
pub struct Env(Environment);

impl Env {
    /// Create a database at a path that does not exist. Its parent must exist.
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Environment::create(path).map(Self)
    }

    /// Open an existing ULMDB environment; invalid files are returned as errors.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Environment::open(path).map(Self)
    }

    /// Open an existing path, or create an environment if the path is absent.
    ///
    /// An existing empty directory is treated as an existing environment, so it
    /// must contain valid ULMDB files. Parent directories are not created.
    pub fn open_or_create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if path.try_exists()? {
            Self::open(path)
        } else {
            Self::create(path)
        }
    }

    /// Pin the current committed snapshot. Values can be borrowed from it.
    pub fn read(&self) -> ReadTxn {
        self.0.begin_read()
    }

    /// Begin a writer; dropping it without committing aborts its changes.
    pub fn write(&self) -> Result<WriteTxn<'_>> {
        self.0.begin_write()
    }

    /// Commit the closure's changes when it returns `Ok`, returning its value.
    ///
    /// A closure error (or unwind) drops the writer and aborts its changes.
    /// Commit errors are returned to the caller, including [`Error::OutcomeUnknown`]
    /// when the engine requires reopening to resolve a publication outcome.
    pub fn update<T>(&self, operation: impl FnOnce(&mut WriteTxn<'_>) -> Result<T>) -> Result<T> {
        let mut write = self.write()?;
        let value = operation(&mut write)?;
        write.commit()?;
        Ok(value)
    }

    /// Wrap an environment configured through the engine's richer constructors.
    pub fn from_environment(environment: Environment) -> Self {
        Self(environment)
    }

    /// Extract the underlying engine environment.
    pub fn into_environment(self) -> Environment {
        self.0
    }
}

impl Deref for Env {
    type Target = Environment;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Parse an IPv4 or IPv6 address in standard text notation.
pub fn address(text: &str) -> Result<Address> {
    match text.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => Ok(Address::V4(u32::from(address))),
        Ok(IpAddr::V6(address)) => Ok(Address::V6(u128::from(address))),
        Err(_) => Err(Error::InvalidInput("invalid IP address")),
    }
}

/// Parse a canonical CIDR prefix, rejecting host bits and invalid family lengths.
///
/// To normalize host bits explicitly, use [`Prefix::normalize`] instead.
pub fn prefix(text: &str) -> Result<Prefix> {
    let (network, length) = text.split_once('/').ok_or(Error::InvalidInput(
        "CIDR prefix requires a slash and length",
    ))?;
    let length = length
        .parse::<u8>()
        .map_err(|_| Error::InvalidInput("invalid CIDR prefix length"))?;
    Prefix::new(address(network)?, length)
        .map_err(|_| Error::InvalidInput("invalid CIDR length or nonzero host bits"))
}
