use crate::{
    Error, Result,
    model::*,
    schema::{address_bytes, decode_address},
};
pub(crate) fn encode(observation: &Observation) -> Result<Vec<u8>> {
    validate_hostname(&observation.hostname)?;
    let mut b = vec![1, observation.role as u8, observation.dnssec as u8, 0];
    b.extend_from_slice(&address_bytes(observation.address));
    for n in [
        observation.scope.0,
        observation.source.0,
        observation.vantage.0,
        observation.resolver.0,
        observation.observed_at,
        observation.collected_at,
    ] {
        b.extend_from_slice(&n.to_be_bytes());
    }
    b.extend_from_slice(&observation.ttl.to_be_bytes());
    b.extend_from_slice(&observation.evidence.0);
    b.extend_from_slice(&(observation.hostname.len() as u16).to_be_bytes());
    b.extend_from_slice(observation.hostname.as_bytes());
    Ok(b)
}
struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Decoder<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(n)
            .ok_or(Error::Corrupt("observation extent"))?;
        let b = self
            .bytes
            .get(self.offset..end)
            .ok_or(Error::Corrupt("short observation"))?;
        self.offset = end;
        Ok(b)
    }
    fn number(&mut self) -> Result<u64> {
        crate::schema::number(self.take(8)?)
    }
}
pub(crate) fn decode(id: ObservationId, bytes: &[u8]) -> Result<ObservationRef<'_>> {
    let mut d = Decoder { bytes, offset: 0 };
    let h = d.take(4)?;
    if h[0] != 1 || h[3] != 0 {
        return Err(Error::Corrupt("unsupported observation encoding"));
    }
    let role = Role::decode(h[1])?;
    let dnssec = Dnssec::decode(h[2])?;
    let address = decode_address(d.take(17)?)?;
    let scope = ScopeId(d.number()?);
    let source = SourceId(d.number()?);
    let vantage = VantageId(d.number()?);
    let resolver = ResolverId(d.number()?);
    let observed_at = d.number()?;
    let collected_at = d.number()?;
    let ttl = u32::from_be_bytes(d.take(4)?.try_into().unwrap());
    let evidence = EvidenceId(d.take(32)?.try_into().unwrap());
    let length = u16::from_be_bytes(d.take(2)?.try_into().unwrap()) as usize;
    let hostname = std::str::from_utf8(d.take(length)?)
        .map_err(|_| Error::Corrupt("invalid observation hostname"))?;
    if d.offset != bytes.len() || validate_hostname(hostname).is_err() {
        return Err(Error::Corrupt("noncanonical observation hostname/extent"));
    }
    Ok(ObservationRef {
        id,
        hostname,
        address,
        scope,
        source,
        role,
        observed_at,
        collected_at,
        vantage,
        resolver,
        ttl,
        dnssec,
        evidence,
    })
}
