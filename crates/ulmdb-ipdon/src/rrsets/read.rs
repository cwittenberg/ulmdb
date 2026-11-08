use super::{codec::*, *};
use crate::{
    Error, Prefix, Result, normalize_hostname,
    schema::{address_bytes, decode_address},
};
use sha2::{Digest, Sha256};
use std::ops::Bound;
use ulmdb::ReadTxn;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Query {
    Hostname {
        scope: ScopeId,
        role: Role,
        hostname: String,
    },
    Ip {
        scope: ScopeId,
        role: Role,
        address: Address,
    },
    Cidr {
        scope: ScopeId,
        role: Role,
        prefix: Prefix,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResumeToken {
    commit: u128,
    query: [u8; 32],
    at: u64,
    after: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Relation {
    pub hostname: String,
    pub address: Address,
    pub rrset: SetId,
    pub revision: u64,
    pub origin: u64,
    pub source: SourceId,
    pub resolver: ResolverId,
    pub vantage: VantageId,
    pub scope: ScopeId,
    pub role: Role,
    pub freshness: Freshness,
    pub dnssec: Dnssec,
}

#[derive(Clone, Debug)]
pub struct Page {
    /// Each item is one attributable RRset membership. Multiple sources may
    /// support the same hostname/IP; callers can retain or aggregate support.
    pub relations: Vec<Relation>,
    pub scanned: usize,
    pub resume: Option<ResumeToken>,
}

#[derive(Clone, Debug)]
pub struct ExpiryPlan {
    pub(super) at: u64,
    pub(super) entries: Vec<(SetId, State)>,
}
impl ExpiryPlan {
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub struct View<'a> {
    read: &'a ReadTxn,
}
impl<'a> View<'a> {
    pub(super) fn new(read: &'a ReadTxn) -> Result<Self> {
        if read.get(META, b"version")? != Some(&[1][..]) {
            return Err(Error::Corrupt("RRset schema version"));
        }
        Ok(Self { read })
    }

    pub fn rrset(&self, id: SetId) -> Result<Option<(Rrset, State)>> {
        match (self.read.get(CONTENT, &id.0)?, self.read.get(STATE, &id.0)?) {
            (None, None) => Ok(None),
            (Some(bytes), Some(meta)) => {
                let set = decode(bytes)?;
                let state = state(meta)?;
                if set.key.id()? != id || set.revision != state.revision {
                    return Err(Error::Corrupt("RRset state mismatch"));
                }
                Ok(Some((set, state)))
            }
            _ => Err(Error::Corrupt("incomplete RRset")),
        }
    }

    pub fn query(
        &self,
        query: &Query,
        at: u64,
        after: Option<&ResumeToken>,
        limits: Limits,
    ) -> Result<Page> {
        if limits.max_results == 0
            || limits.max_scan == 0
            || limits.max_results > MAX_PAGE
            || limits.max_scan > MAX_PAGE
        {
            return Err(Error::InvalidInput("RRset page bounds must be 1..4096"));
        }
        let (table, lower, upper) = bounds(query)?;
        let mut h = Sha256::new();
        h.update(table);
        h.update(&lower);
        if let Some(upper) = &upper {
            h.update(upper);
        }
        let digest: [u8; 32] = h.finalize().into();
        if let Some(token) = after
            && (token.commit != self.read.commit_identity()
                || token.query != digest
                || token.at != at)
        {
            return Err(Error::InvalidInput(
                "RRset token belongs to another snapshot/query/time",
            ));
        }
        let start = after.map_or(Bound::Included(lower.as_slice()), |token| {
            Bound::Excluded(token.after.as_slice())
        });
        let end = upper
            .as_ref()
            .map_or(Bound::Unbounded, |v| Bound::Excluded(v.as_slice()));
        let mut cursor = self.read.range(table, start, end, false)?;
        let mut relations = Vec::with_capacity(limits.max_results);
        let mut scanned = 0;
        let mut last = None;
        let mut complete = false;
        while scanned < limits.max_scan && relations.len() < limits.max_results {
            let Some(item) = cursor.next() else {
                complete = true;
                break;
            };
            let (key, value) = item?;
            if !value.is_empty() || key.len() < 32 {
                return Err(Error::Corrupt("RRset membership encoding"));
            }
            scanned += 1;
            last = Some(key.to_vec());
            let id = SetId(key[key.len() - 32..].try_into().unwrap());
            let current = state(
                self.read
                    .get(STATE, &id.0)?
                    .ok_or(Error::Corrupt("membership state missing"))?,
            )?;
            if current.status != Status::Active {
                return Err(Error::Corrupt("inactive RRset still indexed"));
            }
            if current.freshness.expires_at <= at || current.freshness.observed_at > at {
                continue;
            }
            let bytes = self
                .read
                .get(CONTENT, &id.0)?
                .ok_or(Error::Corrupt("membership content missing"))?;
            let (set_key, revision, dnssec, raw_addresses) = content_header(bytes)?;
            if set_key.id()? != id || revision != current.revision {
                return Err(Error::Corrupt("membership revision mismatch"));
            }
            let address = if table == FORWARD {
                let head = host_prefix(set_key.scope, set_key.role, &set_key.hostname)?;
                if key.len() != head.len() + 49 || !key.starts_with(&head) {
                    return Err(Error::Corrupt("invalid forward membership"));
                }
                decode_address(&key[head.len()..head.len() + 17])?
            } else {
                if key.len() < 26 {
                    return Err(Error::Corrupt("short inverse membership"));
                }
                decode_address(&key[9..26])?
            };
            let expected = if table == FORWARD {
                edge_keys_header(&set_key, id, address)?.0
            } else {
                edge_keys_header(&set_key, id, address)?.1
            };
            if expected != key {
                return Err(Error::Corrupt("membership key/content mismatch"));
            }
            // Binary search fixed-width canonical addresses without allocating
            // or decoding the entire RRset once per returned member.
            let encoded = address_bytes(address);
            let (mut lo, mut hi) = (0, raw_addresses.len() / 17);
            while lo < hi {
                let mid = (lo + hi) / 2;
                if raw_addresses[mid * 17..(mid + 1) * 17] < encoded[..] {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            if raw_addresses.get(lo * 17..lo * 17 + 17) != Some(encoded.as_slice()) {
                return Err(Error::Corrupt("membership absent from RRset"));
            }
            relations.push(Relation {
                hostname: set_key.hostname,
                address,
                rrset: id,
                revision,
                origin: set_key.origin,
                source: set_key.source,
                resolver: set_key.resolver,
                vantage: set_key.vantage,
                scope: set_key.scope,
                role: set_key.role,
                freshness: current.freshness,
                dnssec,
            });
        }
        let resume = if complete {
            None
        } else {
            last.map(|after| ResumeToken {
                commit: self.read.commit_identity(),
                query: digest,
                at,
                after,
            })
        };
        Ok(Page {
            relations,
            scanned,
            resume,
        })
    }

    pub fn plan_expiry(&self, at: u64, max_sets: usize) -> Result<ExpiryPlan> {
        self.plan_expiry_with_budget(at, max_sets, 65_536)
    }

    pub fn plan_expiry_with_budget(
        &self,
        at: u64,
        max_sets: usize,
        max_members: usize,
    ) -> Result<ExpiryPlan> {
        if max_sets == 0 || max_sets > MAX_PAGE {
            return Err(Error::InvalidInput("expiry plan bounds must be 1..4096"));
        }
        if max_members == 0 || max_members > 65_536 {
            return Err(Error::InvalidInput("expiry member budget must be 1..65536"));
        }
        let mut upper = at.to_be_bytes().to_vec();
        upper.extend_from_slice(&[255; 32]);
        let mut entries = Vec::with_capacity(max_sets);
        let mut members = 0usize;
        for item in self
            .read
            .range(DUE, Bound::Unbounded, Bound::Included(&upper), false)?
            .take(max_sets)
        {
            let (key, value) = item?;
            if key.len() != 40 || value.len() != 8 {
                return Err(Error::Corrupt("RRset due encoding"));
            }
            let id = SetId(key[8..].try_into().unwrap());
            let current = state(
                self.read
                    .get(STATE, &id.0)?
                    .ok_or(Error::Corrupt("due state missing"))?,
            )?;
            if current.status != Status::Active
                || due(id, current) != key
                || current.revision.to_be_bytes() != value
            {
                return Err(Error::Corrupt("RRset due/state mismatch"));
            }
            let content = self
                .read
                .get(CONTENT, &id.0)?
                .ok_or(Error::Corrupt("due content missing"))?;
            let (_, _, _, raw) = content_header(content)?;
            let next = members
                .checked_add(raw.len() / 17)
                .ok_or(Error::LimitExceeded)?;
            if next > max_members {
                if entries.is_empty() {
                    return Err(Error::InvalidInput(
                        "expiry member budget cannot fit next RRset",
                    ));
                }
                break;
            }
            members = next;
            entries.push((id, current));
        }
        Ok(ExpiryPlan { at, entries })
    }
}

fn edge_keys_header(key: &Key, id: SetId, address: Address) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut f = host_prefix(key.scope, key.role, &key.hostname)?;
    f.extend_from_slice(&address_bytes(address));
    f.extend_from_slice(&id.0);
    let mut i = ip_prefix(key.scope, key.role, address);
    i.extend_from_slice(key.hostname.as_bytes());
    i.push(0);
    i.extend_from_slice(&id.0);
    Ok((f, i))
}

type Bounds = (&'static [u8], Vec<u8>, Option<Vec<u8>>);
fn bounds(query: &Query) -> Result<Bounds> {
    match query {
        Query::Hostname {
            scope,
            role,
            hostname,
        } => {
            let p = host_prefix(*scope, *role, &normalize_hostname(hostname)?)?;
            Ok((FORWARD, p.clone(), prefix_end(&p)))
        }
        Query::Ip {
            scope,
            role,
            address,
        } => {
            let p = ip_prefix(*scope, *role, *address);
            Ok((INVERSE, p.clone(), prefix_end(&p)))
        }
        Query::Cidr {
            scope,
            role,
            prefix,
        } => {
            let (first, last) = prefix.address_bounds();
            let p = ip_prefix(*scope, *role, first);
            Ok((INVERSE, p, prefix_end(&ip_prefix(*scope, *role, last))))
        }
    }
}
