//! Revision-bound RRsets, compact freshness and incremental relationship indexes.
//!
//! An active-set refresh changes freshness and the due-time index, never stable
//! content or relationship records. Reviving a cleaned-up expired set restores
//! its memberships and is charged against the page work budget.
//! Queries filter at an explicit time; bounded expiry is
//! housekeeping rather than a prerequisite for correct reads.
pub(crate) mod codec;
mod read;
mod write;

use crate::{Address, Dnssec, EvidenceId, ResolverId, Role, ScopeId, SourceId, VantageId};
pub use read::{ExpiryPlan, Page, Query, Relation, ResumeToken, View};
pub use write::Rrsets;

pub const MAX_ADDRESSES: usize = 4096;
pub const MAX_PAGE: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SetId(pub [u8; 32]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordType {
    A = 1,
    Aaaa = 28,
}

/// The replacement boundary includes origin and vantage: GeoDNS observations
/// from different measurement locations cannot overwrite each other.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Key {
    pub origin: u64,
    pub source: SourceId,
    pub scope: ScopeId,
    pub resolver: ResolverId,
    pub vantage: VantageId,
    pub role: Role,
    pub hostname: String,
    pub kind: RecordType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rrset {
    pub key: Key,
    /// Strictly increasing for each replacement boundary, including after a
    /// withdrawal. Content hashes alone do not prevent an ABA stale refresh.
    pub revision: u64,
    /// Canonicalized, sorted and deduplicated on ingestion. An empty set is a
    /// positive source replacement with no address members, not a transport error.
    pub addresses: Vec<Address>,
    pub dnssec: Dnssec,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Freshness {
    pub observed_at: u64,
    pub recorded_at: u64,
    /// Caller supplies the effective lifetime, including CNAME dependencies.
    pub expires_at: u64,
    pub evidence: EvidenceId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Active = 1,
    Expired = 2,
    Withdrawn = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct State {
    pub revision: u64,
    pub freshness: Freshness,
    pub status: Status,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Delta {
    pub added: usize,
    pub removed: usize,
    pub retained: usize,
    pub freshness_changed: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_results: usize,
    /// Bounds stale-candidate scans as well as returned live relationships.
    pub max_scan: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_results: 256,
            max_scan: 1024,
        }
    }
}
