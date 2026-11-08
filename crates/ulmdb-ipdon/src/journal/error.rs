use super::Versions;
use std::fmt;

/// Stable protocol categories for replication and bootstrap callers. Integrity
/// checks do not authenticate peers; applications still authorize transport.
#[derive(Debug)]
#[non_exhaustive]
pub enum ProtocolError {
    SnapshotRequired {
        origin: u64,
        requested: u64,
        floor: u64,
    },
    ForeignRealm {
        expected: u128,
        actual: u128,
    },
    VersionMismatch {
        expected: Versions,
        actual: Versions,
    },
    Fork {
        origin: u64,
        sequence: u64,
    },
    InvalidPage(&'static str),
    BaselineMismatch(&'static str),
    Storage(ulmdb::Error),
}

pub type ProtocolResult<T> = std::result::Result<T, ProtocolError>;

impl From<ulmdb::Error> for ProtocolError {
    fn from(error: ulmdb::Error) -> Self {
        Self::Storage(error)
    }
}
impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SnapshotRequired {
                origin,
                requested,
                floor,
            } => write!(
                f,
                "origin {origin} cursor {requested} precedes retained floor {floor}; snapshot required"
            ),
            Self::ForeignRealm { expected, actual } => write!(
                f,
                "foreign journal realm {actual:032x}; expected {expected:032x}"
            ),
            Self::VersionMismatch { expected, actual } => write!(
                f,
                "journal schema/policy {actual:?} differs from {expected:?}"
            ),
            Self::Fork { origin, sequence } => write!(
                f,
                "origin {origin} has divergent history at sequence {sequence}"
            ),
            Self::InvalidPage(message) => write!(f, "invalid verified page: {message}"),
            Self::BaselineMismatch(message) => write!(f, "baseline import rejected: {message}"),
            Self::Storage(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for ProtocolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            _ => None,
        }
    }
}
