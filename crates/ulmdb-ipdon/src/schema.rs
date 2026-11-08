use crate::{Address, Error, HostnameId, Result, Role, ScopeId};
use ulmdb::DatabaseKind;
pub(crate) const META: &[u8] = b"ipdon.meta";
pub(crate) const HOSTS: &[u8] = b"ipdon.hosts";
pub(crate) const NAMES: &[u8] = b"ipdon.names";
pub(crate) const OBS: &[u8] = b"ipdon.observations";
pub(crate) const FORWARD: &[u8] = b"ipdon.forward";
pub(crate) const INVERSE: &[u8] = b"ipdon.inverse";
pub(crate) const EVIDENCE: &[u8] = b"ipdon.evidence";
pub(crate) const HOST_COUNTS: &[u8] = b"ipdon.host_counts";
pub(crate) const IP_COUNTS: &[u8] = b"ipdon.ip_counts";
pub(crate) const TABLES: [&[u8]; 9] = [
    META,
    HOSTS,
    NAMES,
    OBS,
    FORWARD,
    INVERSE,
    EVIDENCE,
    HOST_COUNTS,
    IP_COUNTS,
];
pub(crate) const VERSION: &[u8] = b"projection-version";
pub(crate) const NEXT: &[u8] = b"next-hostname-id";
pub(crate) fn number(bytes: &[u8]) -> Result<u64> {
    Ok(u64::from_be_bytes(
        bytes
            .try_into()
            .map_err(|_| Error::Corrupt("invalid ipdon integer"))?,
    ))
}
pub(crate) fn address_bytes(address: Address) -> [u8; 17] {
    let mut b = [0; 17];
    match address {
        Address::V4(v) => {
            b[0] = 4;
            b[1..5].copy_from_slice(&v.to_be_bytes());
        }
        Address::V6(v) => {
            b[0] = 6;
            b[1..].copy_from_slice(&v.to_be_bytes());
        }
    }
    b
}
pub(crate) fn decode_address(b: &[u8]) -> Result<Address> {
    if b.len() != 17 {
        return Err(Error::Corrupt("invalid ipdon address length"));
    }
    match b[0] {
        4 if b[5..].iter().all(|v| *v == 0) => {
            Ok(Address::V4(u32::from_be_bytes(b[1..5].try_into().unwrap())))
        }
        6 => Ok(Address::V6(u128::from_be_bytes(
            b[1..17].try_into().unwrap(),
        ))),
        _ => Err(Error::Corrupt("invalid ipdon address")),
    }
}
pub(crate) fn group(scope: ScopeId, role: Role) -> Vec<u8> {
    let mut k = scope.0.to_be_bytes().to_vec();
    k.push(role as u8);
    k
}
pub(crate) fn host_key(scope: ScopeId, role: Role, id: HostnameId) -> Vec<u8> {
    let mut k = group(scope, role);
    k.extend_from_slice(&id.0.to_be_bytes());
    k
}
pub(crate) fn ip_key(scope: ScopeId, role: Role, address: Address) -> Vec<u8> {
    let mut k = group(scope, role);
    k.extend_from_slice(&address_bytes(address));
    k
}
pub(crate) fn forward(scope: ScopeId, role: Role, id: HostnameId, address: Address) -> Vec<u8> {
    let mut k = host_key(scope, role, id);
    k.extend_from_slice(&address_bytes(address));
    k
}
pub(crate) fn inverse(scope: ScopeId, role: Role, id: HostnameId, address: Address) -> Vec<u8> {
    let mut k = ip_key(scope, role, address);
    k.extend_from_slice(&id.0.to_be_bytes());
    k
}
pub(crate) fn successor(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut next = prefix.to_vec();
    while let Some(last) = next.pop() {
        if last != 255 {
            next.push(last + 1);
            return Some(next);
        }
    }
    None
}
pub(crate) fn profile_name(scope: ScopeId, address: Address) -> (Vec<u8>, DatabaseKind) {
    let (family, kind) = match address {
        Address::V4(_) => (4, DatabaseKind::Ipv4),
        Address::V6(_) => (6, DatabaseKind::Ipv6),
    };
    (
        format!("ipdon.profile{family}.{:016x}", scope.0).into_bytes(),
        kind,
    )
}
