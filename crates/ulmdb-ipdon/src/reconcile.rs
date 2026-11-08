//! Independent reconstruction from canonical observations, never from edge counts.
use crate::{Error, Result, View, codec, model::*, schema::*};
use std::{collections::BTreeMap, ops::Bound};
use ulmdb::KvRead;
type Records = BTreeMap<Vec<u8>, Vec<u8>>;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Reconciliation {
    pub observations: u64,
    pub unique_edges: u64,
    pub assigned_hostnames: u64,
}
fn increment(records: &mut Records, key: Vec<u8>) -> Result<()> {
    let old = records
        .get(&key)
        .map(|b| number(b))
        .transpose()?
        .unwrap_or(0);
    records.insert(
        key,
        old.checked_add(1)
            .ok_or(Error::LimitExceeded)?
            .to_be_bytes()
            .to_vec(),
    );
    Ok(())
}
fn compare(view: &KvRead<'_>, expected: &Records) -> Result<()> {
    let mut cursor = view.cursor(Bound::Unbounded, Bound::Unbounded, false)?;
    for (key, value) in expected {
        let Some(item) = cursor.next() else {
            return Err(Error::Corrupt("missing reconstructed projection record"));
        };
        let (actual_key, actual_value) = item?;
        if actual_key != key || actual_value != value {
            return Err(Error::Corrupt(
                "projection disagrees with canonical observations",
            ));
        }
    }
    if let Some(item) = cursor.next() {
        item?;
        return Err(Error::Corrupt("extra projection record"));
    }
    Ok(())
}
impl View<'_> {
    /// Offline diagnostic: reconstruct all relationship indexes and counts from
    /// canonical observations and compare both dictionaries and evidence links.
    /// Memory is proportional to the projection; serving queries never call it.
    pub fn reconcile(&self) -> Result<Reconciliation> {
        let mut hostnames = BTreeMap::new();
        let mut names = Records::new();
        let mut highest = 0u64;
        for item in self
            .hosts
            .cursor(Bound::Unbounded, Bound::Unbounded, false)?
        {
            let (name, id) = item?;
            let name = std::str::from_utf8(name)
                .map_err(|_| Error::Corrupt("hostname dictionary UTF8"))?;
            let normalized = normalize_hostname(name)
                .map_err(|_| Error::Corrupt("invalid dictionary hostname"))?;
            let id = number(id)?;
            if normalized != name
                || id == 0
                || names
                    .insert(id.to_be_bytes().to_vec(), name.as_bytes().to_vec())
                    .is_some()
                || hostnames.insert(name.to_owned(), HostnameId(id)).is_some()
            {
                return Err(Error::Corrupt(
                    "hostname dictionary collision/normalization",
                ));
            }
            highest = highest.max(id);
        }
        compare(&self.names, &names)?;
        if number(
            self.meta
                .get(NEXT)?
                .ok_or(Error::Corrupt("hostname sequence missing"))?,
        )? <= highest
        {
            return Err(Error::Corrupt("hostname sequence is not monotonic"));
        }
        let mut forward_records = Records::new();
        let mut inverse_records = Records::new();
        let mut evidence_records = Records::new();
        let mut observations = 0u64;
        for item in self
            .observations
            .cursor(Bound::Unbounded, Bound::Unbounded, false)?
        {
            let (id, bytes) = item?;
            let id = ObservationId(
                id.try_into()
                    .map_err(|_| Error::Corrupt("invalid observation identity"))?,
            );
            let observation = codec::decode(id, bytes)?;
            let name = normalize_hostname(observation.hostname)?;
            let hostname = *hostnames
                .get(&name)
                .ok_or(Error::Corrupt("canonical hostname missing from dictionary"))?;
            let f = forward(
                observation.scope,
                observation.role,
                hostname,
                observation.address,
            );
            let i = inverse(
                observation.scope,
                observation.role,
                hostname,
                observation.address,
            );
            increment(&mut forward_records, f.clone())?;
            increment(&mut inverse_records, i)?;
            let mut key = f;
            key.extend_from_slice(&id.0);
            evidence_records.insert(key, Vec::new());
            observations = observations.checked_add(1).ok_or(Error::LimitExceeded)?;
        }
        let mut host_counts = Records::new();
        let mut ip_counts = Records::new();
        for key in forward_records.keys() {
            let scope = ScopeId(number(&key[..8])?);
            let role = Role::decode(key[8])?;
            let id = HostnameId(number(&key[9..17])?);
            let address = decode_address(&key[17..34])?;
            increment(&mut host_counts, host_key(scope, role, id))?;
            increment(&mut ip_counts, ip_key(scope, role, address))?;
        }
        for (view, expected) in [
            (&self.forward, &forward_records),
            (&self.inverse, &inverse_records),
            (&self.evidence, &evidence_records),
            (&self.host_counts, &host_counts),
            (&self.ip_counts, &ip_counts),
        ] {
            compare(view, expected)?;
        }
        Ok(Reconciliation {
            observations,
            unique_edges: forward_records.len() as u64,
            assigned_hostnames: hostnames.len() as u64,
        })
    }
}
