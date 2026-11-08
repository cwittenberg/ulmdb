use crate::{Address, Error, Prefix, Result, codec, model::*, schema::*};
use std::ops::Bound;
use ulmdb::{KvRead, PrefixRead, ReadTxn};
use ulmdb_storage::SegmentTable;
pub(crate) type RawCursor<'a> =
    ulmdb_kv::Cursor<'a, SegmentTable, ulmdb::ordering::BoundComparator>;

/// Prepared roots borrowing one reader; every page, resume key and result belongs
/// to that reader's committed generation, even while the environment advances.
pub struct View<'a> {
    pub(crate) read: &'a ReadTxn,
    generation: u128,
    pub(crate) meta: KvRead<'a>,
    pub(crate) hosts: KvRead<'a>,
    pub(crate) names: KvRead<'a>,
    pub(crate) observations: KvRead<'a>,
    pub(crate) forward: KvRead<'a>,
    pub(crate) inverse: KvRead<'a>,
    pub(crate) evidence: KvRead<'a>,
    pub(crate) host_counts: KvRead<'a>,
    pub(crate) ip_counts: KvRead<'a>,
}
impl<'a> View<'a> {
    pub(crate) fn new(read: &'a ReadTxn) -> Result<Self> {
        let meta = read.kv(META)?;
        if meta.get(VERSION)? != Some(&1u64.to_be_bytes()) {
            return Err(Error::Corrupt("unsupported ipdon projection schema"));
        }
        Ok(Self {
            read,
            generation: read.generation(),
            meta,
            hosts: read.kv(HOSTS)?,
            names: read.kv(NAMES)?,
            observations: read.kv(OBS)?,
            forward: read.kv(FORWARD)?,
            inverse: read.kv(INVERSE)?,
            evidence: read.kv(EVIDENCE)?,
            host_counts: read.kv(HOST_COUNTS)?,
            ip_counts: read.kv(IP_COUNTS)?,
        })
    }
    pub fn sequence(&self) -> u64 {
        self.read.sequence()
    }
    pub fn generation(&self) -> u128 {
        self.generation
    }
    pub fn hostname_id(&self, name: &str) -> Result<Option<HostnameId>> {
        self.hosts
            .get(normalize_hostname(name)?.as_bytes())?
            .map(|b| {
                number(b).and_then(|n| {
                    if n == 0 {
                        Err(Error::Corrupt("zero hostname identity"))
                    } else {
                        Ok(HostnameId(n))
                    }
                })
            })
            .transpose()
    }
    pub fn hostname(&self, id: HostnameId) -> Result<Option<&'a str>> {
        self.names
            .get(&id.0.to_be_bytes())?
            .map(|b| {
                std::str::from_utf8(b)
                    .map_err(|_| Error::Corrupt("invalid hostname dictionary UTF8"))
            })
            .transpose()
    }
    pub fn observation(&self, id: ObservationId) -> Result<Option<ObservationRef<'a>>> {
        self.observations
            .get(&id.0)?
            .map(|b| codec::decode(id, b))
            .transpose()
    }
    pub fn host_edge_count(&self, scope: ScopeId, role: Role, name: &str) -> Result<u64> {
        let Some(id) = self.hostname_id(name)? else {
            return Ok(0);
        };
        self.host_edge_count_id(scope, role, id)
    }
    pub fn host_edge_count_id(&self, scope: ScopeId, role: Role, id: HostnameId) -> Result<u64> {
        read_count(&self.host_counts, &host_key(scope, role, id))
    }
    pub fn ip_edge_count(&self, scope: ScopeId, role: Role, address: Address) -> Result<u64> {
        read_count(&self.ip_counts, &ip_key(scope, role, address))
    }
    /// Distinct addresses for a hostname; resume exclusively after an address.
    pub fn addresses(
        &self,
        scope: ScopeId,
        role: Role,
        name: &str,
        after: Option<&PageToken>,
    ) -> Result<Edges<'_>> {
        let id = self.hostname_id(name)?.unwrap_or(HostnameId(0));
        self.addresses_id(scope, role, id, after)
    }
    /// Reuse an assigned ID to avoid repeating hostname normalization/dictionary lookup.
    pub fn addresses_id(
        &self,
        scope: ScopeId,
        role: Role,
        id: HostnameId,
        after: Option<&PageToken>,
    ) -> Result<Edges<'_>> {
        let prefix = host_key(scope, role, id);
        let query = Query::Forward(id);
        let position = check_token(after, self.generation, scope, role, query)?;
        let lower = position.map(|(address, _)| forward(scope, role, id, address));
        Ok(Edges {
            cursor: range(&self.forward, &prefix, lower)?,
            names: &self.names,
            direction: Direction::Forward,
            scope,
            role,
            failed: false,
            generation: self.generation,
            query,
        })
    }
    /// Exact-IP inverse index seek; unrelated IPs are not scanned.
    pub fn hostnames(
        &self,
        scope: ScopeId,
        role: Role,
        address: Address,
        after: Option<&PageToken>,
    ) -> Result<Edges<'_>> {
        let prefix = ip_key(scope, role, address);
        let query = Query::Exact(address);
        let position = check_token(after, self.generation, scope, role, query)?;
        let lower = position.map(|(_, id)| inverse(scope, role, id, address));
        Ok(Edges {
            cursor: range(&self.inverse, &prefix, lower)?,
            names: &self.names,
            direction: Direction::Inverse,
            scope,
            role,
            failed: false,
            generation: self.generation,
            query,
        })
    }
    /// Inclusive address-range seek over the inverse index for one CIDR.
    pub fn cidr(
        &self,
        scope: ScopeId,
        role: Role,
        prefix: Prefix,
        after: Option<&PageToken>,
    ) -> Result<Edges<'_>> {
        let (first, last) = prefix.address_bounds();
        let query = Query::Cidr(prefix);
        let lower = match check_token(after, self.generation, scope, role, query)? {
            Some((address, id)) => {
                if !prefix.contains_address(address) {
                    return Err(Error::InvalidInput("CIDR resume address outside query"));
                }
                Bound::Excluded(inverse(scope, role, id, address))
            }
            None => Bound::Included(inverse(scope, role, HostnameId(0), first)),
        };
        let upper = inverse(scope, role, HostnameId(u64::MAX), last);
        Ok(Edges {
            cursor: self
                .inverse
                .cursor(as_bound(&lower), Bound::Included(&upper), false)?,
            names: &self.names,
            direction: Direction::Inverse,
            scope,
            role,
            failed: false,
            generation: self.generation,
            query,
        })
    }
    pub fn evidence(
        &self,
        scope: ScopeId,
        role: Role,
        name: &str,
        address: Address,
        after: Option<&EvidenceToken>,
    ) -> Result<Evidence<'_>> {
        let id = self.hostname_id(name)?.unwrap_or(HostnameId(0));
        let prefix = forward(scope, role, id, address);
        let context = EvidenceToken {
            generation: self.generation,
            scope,
            role,
            hostname_id: id,
            address,
            observation: ObservationId([0; 32]),
        };
        if let Some(token) = after
            && (token.generation != context.generation
                || token.scope != scope
                || token.role != role
                || token.hostname_id != id
                || token.address != address)
        {
            return Err(Error::InvalidInput(
                "evidence token belongs to another snapshot/query",
            ));
        }
        let lower = after.map(|token| {
            let mut key = prefix.clone();
            key.extend_from_slice(&token.observation.0);
            key
        });
        Ok(Evidence {
            cursor: range(&self.evidence, &prefix, lower)?,
            observations: &self.observations,
            names: &self.names,
            failed: false,
            context,
        })
    }
    pub fn profiles(&self, scope: ScopeId) -> Result<Profiles<'a>> {
        fn optional<'r>(read: &'r ReadTxn, name: &[u8]) -> Result<Option<PrefixRead<'r>>> {
            match read.prefix_index(name) {
                Ok(view) => Ok(Some(view)),
                Err(Error::MissingDatabase) => Ok(None),
                Err(e) => Err(e),
            }
        }
        Ok(Profiles {
            v4: optional(self.read, &profile_name(scope, Address::V4(0)).0)?,
            v6: optional(self.read, &profile_name(scope, Address::V6(0)).0)?,
        })
    }
}
pub(crate) fn read_count(view: &KvRead<'_>, key: &[u8]) -> Result<u64> {
    match view.get(key)? {
        None => Ok(0),
        Some(b) => {
            let n = number(b)?;
            if n == 0 {
                Err(Error::Corrupt("zero ipdon stored count"))
            } else {
                Ok(n)
            }
        }
    }
}
fn as_bound(bound: &Bound<Vec<u8>>) -> Bound<&[u8]> {
    match bound {
        Bound::Included(b) => Bound::Included(b),
        Bound::Excluded(b) => Bound::Excluded(b),
        Bound::Unbounded => Bound::Unbounded,
    }
}
pub(crate) fn range<'a>(
    view: &KvRead<'a>,
    prefix: &[u8],
    after: Option<Vec<u8>>,
) -> Result<RawCursor<'a>> {
    let upper = successor(prefix);
    let lower = match &after {
        Some(key) => Bound::Excluded(key.as_slice()),
        None => Bound::Included(prefix),
    };
    view.cursor(
        lower,
        upper.as_deref().map_or(Bound::Unbounded, Bound::Excluded),
        false,
    )
}
#[derive(Clone, Copy)]
enum Direction {
    Forward,
    Inverse,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Query {
    Forward(HostnameId),
    Exact(Address),
    Cidr(Prefix),
}
/// Opaque in-process pagination position bound to one committed root and query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageToken {
    generation: u128,
    scope: ScopeId,
    role: Role,
    query: Query,
    address: Address,
    hostname_id: HostnameId,
}
impl PageToken {
    pub fn generation(&self) -> u128 {
        self.generation
    }
}
fn check_token(
    token: Option<&PageToken>,
    generation: u128,
    scope: ScopeId,
    role: Role,
    query: Query,
) -> Result<Option<(Address, HostnameId)>> {
    token
        .map(|t| {
            if t.generation != generation || t.scope != scope || t.role != role || t.query != query
            {
                return Err(Error::InvalidInput(
                    "pagination token belongs to another snapshot/query",
                ));
            }
            Ok((t.address, t.hostname_id))
        })
        .transpose()
}
/// One unique scoped relation, with the number of canonical supporting records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Edge<'a> {
    pub hostname_id: HostnameId,
    pub hostname: &'a str,
    pub address: Address,
    pub scope: ScopeId,
    pub role: Role,
    pub supports: u64,
    pub resume: PageToken,
}
pub struct Edges<'a> {
    cursor: RawCursor<'a>,
    names: &'a KvRead<'a>,
    direction: Direction,
    generation: u128,
    query: Query,
    scope: ScopeId,
    role: Role,
    failed: bool,
}
impl<'a> Iterator for Edges<'a> {
    type Item = Result<Edge<'a>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        let result = self.cursor.next()?.and_then(|(key, value)| {
            if key.len() != 34 {
                return Err(Error::Corrupt("invalid ipdon edge key"));
            }
            let (id, address) = match self.direction {
                Direction::Forward => (
                    HostnameId(number(&key[9..17])?),
                    decode_address(&key[17..34])?,
                ),
                Direction::Inverse => (
                    HostnameId(number(&key[26..34])?),
                    decode_address(&key[9..26])?,
                ),
            };
            if id.0 == 0 || number(&key[..8])? != self.scope.0 || Role::decode(key[8])? != self.role
            {
                return Err(Error::Corrupt("invalid ipdon edge identity"));
            }
            let name = self
                .names
                .get(&id.0.to_be_bytes())?
                .ok_or(Error::Corrupt("edge hostname missing"))?;
            let hostname =
                std::str::from_utf8(name).map_err(|_| Error::Corrupt("invalid edge hostname"))?;
            let supports = number(value)?;
            if supports == 0 {
                return Err(Error::Corrupt("zero edge support"));
            }
            Ok(Edge {
                hostname_id: id,
                hostname,
                address,
                scope: self.scope,
                role: self.role,
                supports,
                resume: PageToken {
                    generation: self.generation,
                    scope: self.scope,
                    role: self.role,
                    query: self.query,
                    address,
                    hostname_id: id,
                },
            })
        });
        if result.is_err() {
            self.failed = true
        }
        Some(result)
    }
}
impl std::iter::FusedIterator for Edges<'_> {}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceToken {
    generation: u128,
    scope: ScopeId,
    role: Role,
    hostname_id: HostnameId,
    address: Address,
    observation: ObservationId,
}
pub struct Evidence<'a> {
    context: EvidenceToken,
    cursor: RawCursor<'a>,
    observations: &'a KvRead<'a>,
    names: &'a KvRead<'a>,
    failed: bool,
}
impl Evidence<'_> {
    /// Resume after a returned record while retaining this cursor's generation/query.
    pub fn resume_after(&self, id: ObservationId) -> EvidenceToken {
        EvidenceToken {
            observation: id,
            ..self.context
        }
    }
}
impl<'a> Iterator for Evidence<'a> {
    type Item = Result<ObservationRef<'a>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        let result = self.cursor.next()?.and_then(|(key, value)| {
            if key.len() != 66 || !value.is_empty() {
                return Err(Error::Corrupt("invalid evidence link"));
            }
            let id = ObservationId(key[34..66].try_into().unwrap());
            let bytes = self
                .observations
                .get(&id.0)?
                .ok_or(Error::Corrupt("linked observation missing"))?;
            let observation = codec::decode(id, bytes)?;
            let name = self
                .names
                .get(&self.context.hostname_id.0.to_be_bytes())?
                .ok_or(Error::Corrupt("evidence hostname missing"))?;
            let name = std::str::from_utf8(name)
                .map_err(|_| Error::Corrupt("invalid evidence hostname"))?;
            if !validate_hostname(observation.hostname)?.eq_ignore_ascii_case(name)
                || observation.scope != self.context.scope
                || observation.role != self.context.role
                || observation.address != self.context.address
            {
                return Err(Error::Corrupt(
                    "evidence link disagrees with canonical record",
                ));
            }
            Ok(observation)
        });
        if result.is_err() {
            self.failed = true
        }
        Some(result)
    }
}
impl std::iter::FusedIterator for Evidence<'_> {}
pub struct Profiles<'a> {
    v4: Option<PrefixRead<'a>>,
    v6: Option<PrefixRead<'a>>,
}
impl<'a> Profiles<'a> {
    pub fn longest_match(&self, address: Address) -> Result<Option<(Prefix, &'a [u8])>> {
        match address {
            Address::V4(_) => &self.v4,
            Address::V6(_) => &self.v6,
        }
        .as_ref()
        .map_or(Ok(None), |view| view.longest_match(address))
    }
}
