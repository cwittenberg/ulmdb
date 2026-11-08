use super::*;
use std::collections::BTreeMap;
use ulmdb::{KvRead, PrefixRead};

/// Selected bytes borrow one immutable read snapshot, including batch results.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Selected<'a> {
    pub claim_id: ClaimId,
    pub generation: u64,
    pub revision: Revision,
    pub field: FieldId,
    pub prefix: Prefix,
    pub source: u64,
    pub priority: u32,
    pub value: &'a [u8],
    pub observed_at: u64,
    pub recorded_at: u64,
    pub expires_at: u64,
    pub evidence: [u8; 32],
    pub policy_version: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchLimits {
    pub max_results: usize,
}
impl Default for BatchLimits {
    fn default() -> Self {
        Self { max_results: 4096 }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Enriched<'a> {
    pub address: Address,
    /// Same field order as the request. None is unknown; values such as `false`
    /// remain explicit bytes and are never substituted for unknown fields.
    pub fields: Vec<Option<Selected<'a>>>,
}

/// Bounds candidate examinations as well as live results. Expired and future
/// observations consume the scan budget. Both fields must be in 1..=65_536.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CandidateLimits {
    pub max_results: usize,
    pub max_scan: usize,
}
impl Default for CandidateLimits {
    fn default() -> Self {
        Self {
            max_results: 256,
            max_scan: 1024,
        }
    }
}

type CandidateRank = (std::cmp::Reverse<u32>, u64, ClaimId);

/// Opaque continuation bound to one immutable commit and complete query context.
/// Position is a direct bucket offset: resumed pages never reread earlier claims.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateToken {
    commit: u128,
    generation: u128,
    sequence: u64,
    address: Address,
    field: FieldId,
    policy: u64,
    cutoff: u64,
    bucket: usize,
    offset: usize,
    previous: Option<CandidateRank>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidatePage<'a> {
    /// Most-specific prefix first; each prefix uses priority/source/ID order.
    /// This enumeration preserves evidence and does not perform source-global
    /// fencing or application-level inference.
    pub candidates: Vec<Selected<'a>>,
    pub scanned: usize,
    pub complete: bool,
    pub resume: Option<CandidateToken>,
}

/// Aggregate limits apply to the entire batch. Repeated outputs count toward
/// max_results; candidate scans count once per distinct address/field query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CandidateBatchLimits {
    pub per_query: CandidateLimits,
    /// Address/field output slots, including repeats, plus an address-row bound.
    pub max_queries: usize,
    pub max_results: usize,
    pub max_scan: usize,
}
impl Default for CandidateBatchLimits {
    fn default() -> Self {
        Self {
            per_query: CandidateLimits::default(),
            max_queries: 4096,
            max_results: 4096,
            max_scan: 16_384,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateEnriched<'a> {
    pub address: Address,
    /// Same field order as requested. An empty complete page means unknown;
    /// an empty incomplete page means that the query needs continuation.
    pub fields: Vec<CandidatePage<'a>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateBatch<'a> {
    pub rows: Vec<CandidateEnriched<'a>>,
    pub scanned: usize,
    pub complete: bool,
}

pub struct View<'a> {
    read: &'a ReadTxn,
    claims: KvRead<'a>,
    fresh: KvRead<'a>,
    fields: KvRead<'a>,
    policy_version: u64,
}
impl<'a> View<'a> {
    pub(super) fn new(read: &'a ReadTxn) -> Result<Self> {
        if read.get(META, VERSION)? != Some(&1u64.to_be_bytes()) {
            return Err(Error::Corrupt("unsupported field profile schema"));
        }
        let policy_version = number(
            read.get(META, POLICY)?
                .ok_or(Error::Corrupt("missing field policy"))?,
        )?;
        if policy_version == 0 {
            return Err(Error::Corrupt("zero field policy"));
        }
        Ok(Self {
            read,
            claims: read.kv(CLAIMS)?,
            fresh: read.kv(FRESH)?,
            fields: read.kv(FIELDS)?,
            policy_version,
        })
    }
    pub fn policy_version(&self) -> u64 {
        self.policy_version
    }
    pub fn generation(&self) -> u128 {
        self.read.generation()
    }
    /// Retrieve current evidence independently of selection/expiry.
    pub fn claim(&self, id: ClaimId) -> Result<Option<Selected<'a>>> {
        let Some((mut claim, bytes)) = self.candidate(id)? else {
            return Ok(None);
        };
        claim.revision = revision(bytes);
        Ok(Some(claim))
    }
    fn candidate(&self, id: ClaimId) -> Result<Option<(Selected<'a>, &'a [u8])>> {
        let Some(bytes) = self.claims.get(&id.0)? else {
            return Ok(None);
        };
        let claim = codec::decode(id, bytes)?;
        let freshness = codec::decode_fresh(
            self.fresh
                .get(&id.0)?
                .ok_or(Error::Corrupt("field claim freshness missing"))?,
        )?;
        Ok(Some((
            Selected {
                claim_id: claim.id,
                generation: claim.generation,
                revision: Revision([0; 32]),
                field: claim.field,
                prefix: claim.prefix,
                source: claim.source,
                priority: claim.priority,
                value: claim.value,
                observed_at: freshness.observed_at,
                recorded_at: freshness.recorded_at,
                expires_at: freshness.expires_at,
                evidence: claim.evidence,
                policy_version: self.policy_version,
            },
            bytes,
        )))
    }
    /// Resolve current-snapshot evidence at an explicit freshness cutoff. A
    /// maximum of 33/129 covering buckets is examined for IPv4/IPv6 respectively.
    /// Each bucket is independently bounded; expired winners fall back safely
    /// without requiring a background expiration worker to have caught up.
    pub fn lookup(
        &self,
        address: Address,
        field: FieldId,
        now: u64,
    ) -> Result<Option<Selected<'a>>> {
        if field.0 == 0 {
            return Err(Error::InvalidInput("zero field id"));
        }
        if self.fields.get(&field_key(field, address))?.is_none() {
            return Ok(None);
        }
        let index = self.read.prefix_index(&index_name(field, address))?;
        self.lookup_index(address, field, now, &index)
    }

    /// Enumerate every live claim in all covering prefixes. The fixed routing
    /// work visits at most 33 IPv4 or 129 IPv6 bucket headers. Candidate record
    /// examinations, including expired/future claims, are charged to max_scan.
    /// Truncation always returns complete=false and an opaque continuation.
    pub fn covering_candidates(
        &self,
        address: Address,
        field: FieldId,
        now: u64,
        after: Option<&CandidateToken>,
        limits: CandidateLimits,
    ) -> Result<CandidatePage<'a>> {
        candidate_limits(limits)?;
        self.check_candidate_token(address, field, now, after)?;
        let index = self.bind_candidate_index(address, field)?;
        self.candidate_page(address, field, now, after, limits, index.as_ref())
    }

    fn check_candidate_token(
        &self,
        address: Address,
        field: FieldId,
        now: u64,
        after: Option<&CandidateToken>,
    ) -> Result<()> {
        if field.0 == 0 {
            return Err(Error::InvalidInput("zero field id"));
        }
        if let Some(token) = after
            && (token.commit != self.read.commit_identity()
                || token.generation != self.read.generation()
                || token.sequence != self.read.sequence()
                || token.address != address
                || token.field != field
                || token.policy != self.policy_version
                || token.cutoff != now)
        {
            return Err(Error::InvalidInput(
                "candidate token belongs to another snapshot/query/policy/cutoff",
            ));
        }
        Ok(())
    }

    fn bind_candidate_index(
        &self,
        address: Address,
        field: FieldId,
    ) -> Result<Option<PrefixRead<'a>>> {
        if self.fields.get(&field_key(field, address))?.is_some() {
            Ok(Some(self.read.prefix_index(&index_name(field, address))?))
        } else {
            Ok(None)
        }
    }

    fn candidate_page(
        &self,
        address: Address,
        field: FieldId,
        now: u64,
        after: Option<&CandidateToken>,
        limits: CandidateLimits,
        index: Option<&PrefixRead<'a>>,
    ) -> Result<CandidatePage<'a>> {
        let Some(index) = index else {
            if after.is_some() {
                return Err(Error::InvalidInput("candidate resume index missing"));
            }
            return Ok(CandidatePage {
                candidates: Vec::new(),
                scanned: 0,
                complete: true,
                resume: None,
            });
        };
        let host = Prefix::new(
            address,
            match address {
                Address::V4(_) => 32,
                Address::V6(_) => 128,
            },
        )
        .map_err(|_| Error::InvalidInput("field address"))?;
        let buckets = index.ancestors(host)?.collect::<Result<Vec<_>>>()?;
        let mut bucket = after.map_or(0, |token| token.bucket);
        let mut offset = after.map_or(0, |token| token.offset);
        let mut previous = after.and_then(|token| token.previous);
        if bucket > buckets.len() || (bucket == buckets.len() && after.is_some()) {
            return Err(Error::InvalidInput("candidate resume position is invalid"));
        }
        // Reserve only for live results. Eagerly reserving the remaining global
        // budget for each all-expired batch query would multiply unused vector
        // capacities while neither its result nor scan counter charged them.
        let mut candidates = Vec::new();
        let mut scanned = 0;
        while let Some((prefix, bytes)) = buckets.get(bucket) {
            let count = candidate_bucket_count(bytes)?;
            if offset > count || (offset > 0 && previous.is_none()) {
                return Err(Error::InvalidInput("candidate resume offset is invalid"));
            }
            if offset == count {
                bucket += 1;
                offset = 0;
                previous = None;
                continue;
            }
            if scanned == limits.max_scan || candidates.len() == limits.max_results {
                break;
            }
            let start = 1 + offset * 32;
            let id = ClaimId(bytes[start..start + 32].try_into().unwrap());
            if id.0 == [0; 32] {
                return Err(Error::Corrupt("zero candidate bucket identity"));
            }
            // Direct offset access and one candidate load per scan unit ensure
            // resuming cannot conceal repeated scans of earlier stale claims.
            scanned += 1;
            let (mut candidate, baseline) = self
                .candidate(id)?
                .ok_or(Error::Corrupt("candidate bucket claim missing"))?;
            if candidate.field != field || candidate.prefix != *prefix {
                return Err(Error::Corrupt("candidate bucket claim scope mismatch"));
            }
            let rank = (
                std::cmp::Reverse(candidate.priority),
                candidate.source,
                candidate.claim_id,
            );
            if previous.is_some_and(|previous| rank <= previous) {
                return Err(Error::Corrupt("candidate bucket order/identity mismatch"));
            }
            previous = Some(rank);
            offset += 1;
            if candidate.observed_at <= now && candidate.expires_at > now {
                candidate.revision = revision(baseline);
                candidates
                    .try_reserve(1)
                    .map_err(|_| Error::LimitExceeded)?;
                candidates.push(candidate);
            }
        }
        let complete = bucket == buckets.len();
        let resume = (!complete).then_some(CandidateToken {
            commit: self.read.commit_identity(),
            generation: self.read.generation(),
            sequence: self.read.sequence(),
            address,
            field,
            policy: self.policy_version,
            cutoff: now,
            bucket,
            offset,
            previous,
        });
        Ok(CandidatePage {
            candidates,
            scanned,
            complete,
            resume,
        })
    }

    /// Enumerate bounded first pages for a batch of address/field queries. Roots
    /// are bound once per field/family; duplicate queries are scanned once and
    /// restored in caller order. Each incomplete page can continue through
    /// covering_candidates using its own token. Exhausted aggregate budgets
    /// return empty incomplete pages rather than silently dropping queries.
    pub fn batch_candidates(
        &self,
        addresses: &[Address],
        fields: &[FieldId],
        now: u64,
        limits: CandidateBatchLimits,
    ) -> Result<CandidateBatch<'a>> {
        candidate_limits(limits.per_query)?;
        for bound in [limits.max_queries, limits.max_results, limits.max_scan] {
            if bound == 0 || bound > 65_536 {
                return Err(Error::LimitExceeded);
            }
        }
        let slots = addresses
            .len()
            .checked_mul(fields.len())
            .ok_or(Error::LimitExceeded)?;
        if slots > limits.max_queries || addresses.len() > limits.max_queries || fields.len() > 64 {
            return Err(Error::LimitExceeded);
        }
        if fields.iter().any(|field| field.0 == 0) {
            return Err(Error::InvalidInput("zero field id"));
        }
        let mut multiplicity = BTreeMap::<(Address, FieldId), usize>::new();
        let mut order = Vec::new();
        let mut indexes = BTreeMap::new();
        for address in addresses {
            for field in fields {
                let count = multiplicity.entry((*address, *field)).or_default();
                if *count == 0 {
                    order.push((*address, *field));
                }
                *count += 1;
                let key = field_key(*field, *address);
                if let std::collections::btree_map::Entry::Vacant(entry) = indexes.entry(key) {
                    entry.insert(self.bind_candidate_index(*address, *field)?);
                }
            }
        }
        let mut unique = BTreeMap::new();
        let mut scanned = 0;
        let mut results = 0;
        let mut complete = true;
        for (address, field) in order {
            let copies = multiplicity[&(address, field)];
            let per_query = CandidateLimits {
                max_results: limits
                    .per_query
                    .max_results
                    .min((limits.max_results - results) / copies),
                max_scan: limits.per_query.max_scan.min(limits.max_scan - scanned),
            };
            let page = self.candidate_page(
                address,
                field,
                now,
                None,
                per_query,
                indexes[&field_key(field, address)].as_ref(),
            )?;
            scanned += page.scanned;
            results += page.candidates.len() * copies;
            complete &= page.complete;
            unique.insert((address, field), page);
        }
        let rows = addresses
            .iter()
            .map(|address| CandidateEnriched {
                address: *address,
                fields: fields
                    .iter()
                    .map(|field| unique[&(*address, *field)].clone())
                    .collect(),
            })
            .collect();
        Ok(CandidateBatch {
            rows,
            scanned,
            complete,
        })
    }
    fn lookup_index(
        &self,
        address: Address,
        field: FieldId,
        now: u64,
        index: &PrefixRead<'a>,
    ) -> Result<Option<Selected<'a>>> {
        let host = Prefix::new(
            address,
            match address {
                Address::V4(_) => 32,
                Address::V6(_) => 128,
            },
        )
        .map_err(|_| Error::InvalidInput("field address"))?;
        let mut selected = None;
        for item in index.ancestors(host)? {
            let (prefix, bytes) = item?;
            for id in codec::decode_bucket(bytes)? {
                let (candidate, baseline) = self
                    .candidate(id)?
                    .ok_or(Error::Corrupt("field bucket claim missing"))?;
                if candidate.field != field || candidate.prefix != prefix {
                    return Err(Error::Corrupt("field bucket claim scope mismatch"));
                }
                if candidate.observed_at > now || candidate.expires_at <= now {
                    continue;
                }
                if selected
                    .as_ref()
                    .is_none_or(|(old, _)| better(&candidate, old))
                {
                    selected = Some((candidate, baseline));
                }
                // Bucket identities are ranked by priority/source/id. Its first
                // unexpired claim dominates every later candidate at this prefix.
                break;
            }
        }
        Ok(selected.map(|(mut claim, baseline)| {
            claim.revision = revision(baseline);
            claim
        }))
    }
    /// Bounded snapshot-consistent batch. Repeated addresses are evaluated once;
    /// selected values remain borrowed, and caller address/field order is kept.
    pub fn batch(
        &self,
        addresses: &[Address],
        fields: &[FieldId],
        now: u64,
        limits: BatchLimits,
    ) -> Result<Vec<Enriched<'a>>> {
        let results = addresses
            .len()
            .checked_mul(fields.len())
            .ok_or(Error::LimitExceeded)?;
        if limits.max_results == 0
            || limits.max_results > 65_536
            || results > limits.max_results
            || addresses.len() > limits.max_results
            || fields.len() > 64
        {
            return Err(Error::LimitExceeded);
        }
        if fields.iter().any(|field| field.0 == 0) {
            return Err(Error::InvalidInput("zero field id"));
        }
        // Bind each requested field/family root once for the entire batch.
        let mut indexes = BTreeMap::new();
        for address in addresses {
            for field in fields {
                let key = field_key(*field, *address);
                if let std::collections::btree_map::Entry::Vacant(entry) = indexes.entry(key) {
                    entry.insert(if self.fields.get(&key)?.is_some() {
                        Some(self.read.prefix_index(&index_name(*field, *address))?)
                    } else {
                        None
                    });
                }
            }
        }
        let mut unique = BTreeMap::new();
        for address in addresses {
            if !unique.contains_key(address) {
                let resolved = fields
                    .iter()
                    .map(|field| match &indexes[&field_key(*field, *address)] {
                        Some(index) => self.lookup_index(*address, *field, now, index),
                        None => Ok(None),
                    })
                    .collect::<Result<Vec<_>>>()?;
                unique.insert(*address, resolved);
            }
        }
        Ok(addresses
            .iter()
            .map(|address| Enriched {
                address: *address,
                fields: unique[address].clone(),
            })
            .collect())
    }
}
fn candidate_limits(limits: CandidateLimits) -> Result<()> {
    if limits.max_results == 0
        || limits.max_results > 65_536
        || limits.max_scan == 0
        || limits.max_scan > 65_536
    {
        return Err(Error::InvalidInput("candidate bounds must be 1..65536"));
    }
    Ok(())
}

/// Header-only validation is constant work. Candidate identity, rank and scope
/// validation happens when each record is examined and consumes a scan unit.
fn candidate_bucket_count(bytes: &[u8]) -> Result<usize> {
    if bytes.len() < 33 || bytes[0] != 1 || !(bytes.len() - 1).is_multiple_of(32) {
        return Err(Error::Corrupt("candidate bucket extent"));
    }
    let count = (bytes.len() - 1) / 32;
    if count > MAX_CLAIMS_PER_PREFIX {
        return Err(Error::Corrupt("candidate bucket capacity"));
    }
    Ok(count)
}

fn better(candidate: &Selected<'_>, old: &Selected<'_>) -> bool {
    (
        candidate.priority,
        candidate.prefix.length(),
        std::cmp::Reverse(candidate.source),
        std::cmp::Reverse(candidate.claim_id),
    ) > (
        old.priority,
        old.prefix.length(),
        std::cmp::Reverse(old.source),
        std::cmp::Reverse(old.claim_id),
    )
}
