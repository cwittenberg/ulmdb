use super::*;
use crate::{
    Error, Result, normalize_hostname,
    schema::{address_bytes, decode_address},
};
use sha2::{Digest, Sha256};

pub(super) const META: &[u8] = b"ipdon.rrsets.meta";
pub(super) const CONTENT: &[u8] = b"ipdon.rrsets.content";
pub(super) const STATE: &[u8] = b"ipdon.rrsets.state";
pub(super) const FORWARD: &[u8] = b"ipdon.rrsets.forward";
pub(super) const INVERSE: &[u8] = b"ipdon.rrsets.inverse";
pub(super) const DUE: &[u8] = b"ipdon.rrsets.due";
pub(crate) const TABLES: [&[u8]; 6] = [META, CONTENT, STATE, FORWARD, INVERSE, DUE];

pub(super) fn canonical(set: &Rrset) -> Result<Rrset> {
    if set.key.origin == 0 || set.revision == 0 || set.addresses.len() > MAX_ADDRESSES {
        return Err(Error::InvalidInput(
            "invalid RRset origin/revision/address limit",
        ));
    }
    let mut set = set.clone();
    set.key.hostname = normalize_hostname(&set.key.hostname)?;
    if set.addresses.iter().any(|a| {
        !matches!(
            (set.key.kind, a),
            (RecordType::A, Address::V4(_)) | (RecordType::Aaaa, Address::V6(_))
        )
    }) {
        return Err(Error::InvalidInput("RRset address family mismatch"));
    }
    set.addresses.sort_unstable();
    set.addresses.dedup();
    Ok(set)
}

pub(super) fn key_bytes(key: &Key) -> Result<Vec<u8>> {
    if key.origin == 0 {
        return Err(Error::InvalidInput("zero RRset origin"));
    }
    let name = normalize_hostname(&key.hostname)?;
    let mut bytes = Vec::with_capacity(52 + name.len());
    for n in [
        key.origin,
        key.source.0,
        key.scope.0,
        key.resolver.0,
        key.vantage.0,
    ] {
        bytes.extend_from_slice(&n.to_be_bytes());
    }
    bytes.push(key.role as u8);
    bytes.push(key.kind as u8);
    bytes.extend_from_slice(&(name.len() as u16).to_be_bytes());
    bytes.extend_from_slice(name.as_bytes());
    Ok(bytes)
}

impl Key {
    pub fn id(&self) -> Result<SetId> {
        let mut hash = Sha256::new();
        hash.update(b"ULMDB/ipdon/rrset/key/v1\0");
        hash.update(key_bytes(self)?);
        Ok(SetId(hash.finalize().into()))
    }
}

pub(crate) fn encode(set: &Rrset) -> Result<Vec<u8>> {
    let set = canonical(set)?;
    let mut bytes = vec![1];
    bytes.extend_from_slice(&key_bytes(&set.key)?);
    bytes.extend_from_slice(&set.revision.to_be_bytes());
    bytes.push(set.dnssec as u8);
    bytes.extend_from_slice(&(set.addresses.len() as u16).to_be_bytes());
    for address in &set.addresses {
        bytes.extend_from_slice(&address_bytes(*address));
    }
    Ok(bytes)
}

pub(crate) struct Decoder<'a> {
    pub bytes: &'a [u8],
    pub pos: usize,
}
impl<'a> Decoder<'a> {
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(Error::LimitExceeded)?;
        let value = self
            .bytes
            .get(self.pos..end)
            .ok_or(Error::Corrupt("short regional record"))?;
        self.pos = end;
        Ok(value)
    }
    pub fn n(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn finish(&self) -> Result<()> {
        if self.pos == self.bytes.len() {
            Ok(())
        } else {
            Err(Error::Corrupt("trailing regional record bytes"))
        }
    }
}

pub(super) fn content_header(bytes: &[u8]) -> Result<(Key, u64, Dnssec, &[u8])> {
    let mut d = Decoder { bytes, pos: 0 };
    if d.take(1)? != [1] {
        return Err(Error::Corrupt("RRset encoding version"));
    }
    let origin = d.n()?;
    let source = SourceId(d.n()?);
    let scope = ScopeId(d.n()?);
    let resolver = ResolverId(d.n()?);
    let vantage = VantageId(d.n()?);
    let role = Role::decode(d.take(1)?[0])?;
    let kind = match d.take(1)?[0] {
        1 => RecordType::A,
        28 => RecordType::Aaaa,
        _ => return Err(Error::Corrupt("RRset record type")),
    };
    let len = u16::from_be_bytes(d.take(2)?.try_into().unwrap()) as usize;
    let hostname = std::str::from_utf8(d.take(len)?)
        .map_err(|_| Error::Corrupt("RRset hostname UTF8"))?
        .to_owned();
    let revision = d.n()?;
    let dnssec = Dnssec::decode(d.take(1)?[0])?;
    let count = u16::from_be_bytes(d.take(2)?.try_into().unwrap()) as usize;
    if count > MAX_ADDRESSES {
        return Err(Error::LimitExceeded);
    }
    let addresses = d.take(count.checked_mul(17).ok_or(Error::LimitExceeded)?)?;
    d.finish()?;
    if origin == 0 || revision == 0 || normalize_hostname(&hostname)? != hostname {
        return Err(Error::Corrupt("invalid RRset header"));
    }
    Ok((
        Key {
            origin,
            source,
            scope,
            resolver,
            vantage,
            role,
            hostname,
            kind,
        },
        revision,
        dnssec,
        addresses,
    ))
}
pub(crate) fn decode(bytes: &[u8]) -> Result<Rrset> {
    let (key, revision, dnssec, raw) = content_header(bytes)?;
    let mut addresses = Vec::with_capacity(raw.len() / 17);
    for address in raw.chunks_exact(17) {
        addresses.push(decode_address(address)?);
    }
    let set = Rrset {
        key,
        revision,
        addresses,
        dnssec,
    };
    let checked = canonical(&set).map_err(|_| Error::Corrupt("invalid persisted RRset"))?;
    if checked != set {
        return Err(Error::Corrupt("noncanonical persisted RRset"));
    }
    Ok(set)
}

pub(crate) fn freshness_bytes(f: Freshness) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(56);
    for n in [f.observed_at, f.recorded_at, f.expires_at] {
        bytes.extend_from_slice(&n.to_be_bytes());
    }
    bytes.extend_from_slice(&f.evidence.0);
    bytes
}
pub(crate) fn read_freshness(d: &mut Decoder<'_>) -> Result<Freshness> {
    let f = Freshness {
        observed_at: d.n()?,
        recorded_at: d.n()?,
        expires_at: d.n()?,
        evidence: EvidenceId(d.take(32)?.try_into().unwrap()),
    };
    validate_freshness(f).map_err(|_| Error::Corrupt("invalid persisted freshness"))?;
    Ok(f)
}
pub(super) fn validate_freshness(f: Freshness) -> Result<()> {
    if f.expires_at < f.observed_at {
        return Err(Error::InvalidInput("expiry precedes observation"));
    }
    Ok(())
}
pub(super) fn state_bytes(state: State) -> Vec<u8> {
    let mut bytes = vec![state.status as u8];
    bytes.extend_from_slice(&state.revision.to_be_bytes());
    bytes.extend_from_slice(&freshness_bytes(state.freshness));
    bytes
}
pub(super) fn state(bytes: &[u8]) -> Result<State> {
    let mut d = Decoder { bytes, pos: 0 };
    let status = match d.take(1)?[0] {
        1 => Status::Active,
        2 => Status::Expired,
        3 => Status::Withdrawn,
        _ => return Err(Error::Corrupt("invalid RRset state")),
    };
    let revision = d.n()?;
    if revision == 0 {
        return Err(Error::Corrupt("zero RRset revision"));
    }
    let freshness = read_freshness(&mut d)?;
    d.finish()?;
    Ok(State {
        status,
        revision,
        freshness,
    })
}
pub(super) fn due(id: SetId, state: State) -> Vec<u8> {
    let mut bytes = state.freshness.expires_at.to_be_bytes().to_vec();
    bytes.extend_from_slice(&id.0);
    bytes
}
pub(super) fn group(scope: ScopeId, role: Role) -> Vec<u8> {
    let mut bytes = scope.0.to_be_bytes().to_vec();
    bytes.push(role as u8);
    bytes
}
pub(super) fn host_prefix(scope: ScopeId, role: Role, name: &str) -> Result<Vec<u8>> {
    let mut bytes = group(scope, role);
    bytes.extend_from_slice(normalize_hostname(name)?.as_bytes());
    bytes.push(0);
    Ok(bytes)
}
pub(super) fn ip_prefix(scope: ScopeId, role: Role, address: Address) -> Vec<u8> {
    let mut bytes = group(scope, role);
    bytes.extend_from_slice(&address_bytes(address));
    bytes
}
pub(super) fn edge_keys(set: &Rrset, id: SetId, address: Address) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut f = host_prefix(set.key.scope, set.key.role, &set.key.hostname)?;
    f.extend_from_slice(&address_bytes(address));
    f.extend_from_slice(&id.0);
    let mut i = ip_prefix(set.key.scope, set.key.role, address);
    i.extend_from_slice(set.key.hostname.as_bytes());
    i.push(0);
    i.extend_from_slice(&id.0);
    Ok((f, i))
}
pub(super) fn prefix_end(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut bytes = prefix.to_vec();
    while let Some(b) = bytes.pop() {
        if b != 255 {
            bytes.push(b + 1);
            return Some(bytes);
        }
    }
    None
}
