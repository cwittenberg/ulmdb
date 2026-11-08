use sha2::{Digest, Sha256};
use ulmdb::{Address, Error, Result};

macro_rules! numeric_id {
    ($($name:ident),*) => {$(#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub struct $name(pub u64);)*};
}
numeric_id!(HostnameId, ScopeId, SourceId, VantageId, ResolverId);

/// Stable source-record identity. Reusing this ID replaces its previous evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ObservationId(pub [u8; 32]);
impl ObservationId {
    pub fn from_source(source: SourceId, record_key: &[u8]) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"ULMDB/ipdon/observation/v1\0");
        hash.update(source.0.to_be_bytes());
        hash.update(record_key);
        Self(hash.finalize().into())
    }
}
/// SHA-256 identity of retained upstream evidence, separate from record identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct EvidenceId(pub [u8; 32]);
impl EvidenceId {
    pub fn from_bytes(raw: &[u8]) -> Self {
        Self(Sha256::digest(raw).into())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum Role {
    Direct = 1,
    NameserverAddress = 2,
    MailTarget = 3,
    Ptr = 4,
}
impl Role {
    pub(crate) fn decode(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::Direct),
            2 => Ok(Self::NameserverAddress),
            3 => Ok(Self::MailTarget),
            4 => Ok(Self::Ptr),
            _ => Err(Error::Corrupt("invalid observation role")),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Dnssec {
    Unchecked = 0,
    Secure = 1,
    Insecure = 2,
    Bogus = 3,
}
impl Dnssec {
    pub(crate) fn decode(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Unchecked),
            1 => Ok(Self::Secure),
            2 => Ok(Self::Insecure),
            3 => Ok(Self::Bogus),
            _ => Err(Error::Corrupt("invalid DNSSEC state")),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Observation {
    pub id: ObservationId,
    /// Original ASCII presentation retained separately from the normalized dictionary.
    pub hostname: String,
    pub address: Address,
    pub scope: ScopeId,
    pub source: SourceId,
    pub role: Role,
    /// Unix seconds: observation time and local collection time are independent.
    pub observed_at: u64,
    pub collected_at: u64,
    pub vantage: VantageId,
    pub resolver: ResolverId,
    pub ttl: u32,
    pub dnssec: Dnssec,
    pub evidence: EvidenceId,
}
/// A zero-copy canonical record whose presentation bytes borrow the read snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservationRef<'a> {
    pub id: ObservationId,
    pub hostname: &'a str,
    pub address: Address,
    pub scope: ScopeId,
    pub source: SourceId,
    pub role: Role,
    pub observed_at: u64,
    pub collected_at: u64,
    pub vantage: VantageId,
    pub resolver: ResolverId,
    pub ttl: u32,
    pub dnssec: Dnssec,
    pub evidence: EvidenceId,
}
impl ObservationRef<'_> {
    pub fn to_owned(self) -> Observation {
        Observation {
            id: self.id,
            hostname: self.hostname.to_owned(),
            address: self.address,
            scope: self.scope,
            source: self.source,
            role: self.role,
            observed_at: self.observed_at,
            collected_at: self.collected_at,
            vantage: self.vantage,
            resolver: self.resolver,
            ttl: self.ttl,
            dnssec: self.dnssec,
            evidence: self.evidence,
        }
    }
}
/// Normalize an ASCII DNS hostname/A-label. Unicode-to-IDNA conversion is upstream.
pub fn normalize_hostname(name: &str) -> Result<String> {
    Ok(validate_hostname(name)?.to_ascii_lowercase())
}
pub(crate) fn validate_hostname(name: &str) -> Result<&str> {
    if !name.is_ascii() {
        return Err(Error::InvalidInput(
            "hostname must already be ASCII/IDNA A-labels",
        ));
    }
    let name = name.strip_suffix('.').unwrap_or(name);
    if name.is_empty() || name.len() > 253 {
        return Err(Error::InvalidInput("hostname length must be 1..253 bytes"));
    }
    for label in name.split('.') {
        let b = label.as_bytes();
        if b.is_empty()
            || b.len() > 63
            || !b[0].is_ascii_alphanumeric()
            || !b[b.len() - 1].is_ascii_alphanumeric()
            || b.iter().any(|v| !v.is_ascii_alphanumeric() && *v != b'-')
            || (label.len() == 4 && label.eq_ignore_ascii_case("xn--"))
        {
            return Err(Error::InvalidInput("invalid hostname label"));
        }
    }
    Ok(name)
}
