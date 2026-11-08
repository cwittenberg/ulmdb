//! Incremental, field-specific prefix claims with compact independent freshness.
//!
//! Prefix buckets contain bounded, ranked claim identities. Updating one claim
//! touches only its previous/new field and prefix; a refresh writes only its
//! freshness record. Queries compare covering-prefix candidates by trust before
//! specificity, and evaluate expiry at an explicit caller-supplied time. This is
//! a view of the current snapshot, not an arbitrary historical query service.
mod codec;
mod read;

use crate::{Address, Error, Prefix, Result};
use sha2::{Digest, Sha256};
use ulmdb::{DatabaseKind, ReadTxn, WriteTxn};

pub use read::{
    BatchLimits, CandidateBatch, CandidateBatchLimits, CandidateEnriched, CandidateLimits,
    CandidatePage, CandidateToken, Enriched, Selected, View,
};

const META: &[u8] = b"ipdon.fields.v1.meta";
const CLAIMS: &[u8] = b"ipdon.fields.v1.claims";
const FRESH: &[u8] = b"ipdon.fields.v1.freshness";
const BUCKETS: &[u8] = b"ipdon.fields.v1.buckets";
const FIELDS: &[u8] = b"ipdon.fields.v1.catalog";
const TOMBSTONES: &[u8] = b"ipdon.fields.v1.withdrawn";
const TABLES: [&[u8]; 6] = [META, CLAIMS, FRESH, BUCKETS, FIELDS, TOMBSTONES];
const POLICY: &[u8] = b"policy";
const VERSION: &[u8] = b"version";

/// Maximum caller value in one claim; values remain borrowed during serving.
pub const MAX_VALUE_BYTES: usize = 16_384;
/// A hard per-field, exact-prefix fanout bound. Overflow fails atomically.
pub const MAX_CLAIMS_PER_PREFIX: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ClaimId(pub [u8; 32]);
impl ClaimId {
    /// Identity for a source record with one globally assigned publishing owner.
    /// Regional measurements should use `from_origin_source` to retain each
    /// vantage independently instead of colliding on a shared upstream key.
    pub fn from_source(source: u64, key: &[u8]) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"ULMDB/ipdon/field-claim/v1\0");
        hash.update(source.to_be_bytes());
        hash.update(key);
        Self(hash.finalize().into())
    }

    /// Stable identity for one origin's observation of an upstream record.
    pub fn from_origin_source(origin: u64, source: u64, key: &[u8]) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"ULMDB/ipdon/origin-field-claim/v1\0");
        hash.update(origin.to_be_bytes());
        hash.update(source.to_be_bytes());
        hash.update(key);
        Self(hash.finalize().into())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FieldId(pub u64);
/// Identity of the immutable baseline, including its generation and initial time.
/// Freshness renewals preserve this identity; replacement invalidates it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Revision(pub [u8; 32]);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Claim {
    pub id: ClaimId,
    /// Nonzero, strictly increasing incarnation for this stable ID. It orders
    /// content replacements independently of timestamp resolution or clock ties.
    pub generation: u64,
    pub field: FieldId,
    pub prefix: Prefix,
    pub source: u64,
    pub value: Vec<u8>,
    /// Higher values win before considering prefix specificity.
    pub priority: u32,
    pub observed_at: u64,
    pub recorded_at: u64,
    pub expires_at: u64,
    pub evidence: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Freshness {
    pub observed_at: u64,
    pub recorded_at: u64,
    pub expires_at: u64,
}
impl Claim {
    pub fn revision(&self) -> Result<Revision> {
        Ok(revision(&codec::encode(self)?))
    }
    pub fn freshness(&self) -> Freshness {
        Freshness {
            observed_at: self.observed_at,
            recorded_at: self.recorded_at,
            expires_at: self.expires_at,
        }
    }
}

pub struct Profiles;
impl Profiles {
    /// Initialize once; changing policy requires a separately planned rebuild.
    pub fn initialize(txn: &mut WriteTxn<'_>, policy_version: u64) -> Result<()> {
        if policy_version == 0 {
            return Err(Error::InvalidInput("zero field profile policy version"));
        }
        let mut save = txn.savepoint()?;
        match save.get(META, VERSION) {
            Err(Error::MissingDatabase) => {
                for name in TABLES {
                    save.create_database(name, DatabaseKind::Kv)?;
                }
                save.put(META, VERSION, &1u64.to_be_bytes())?;
                save.put(META, POLICY, &policy_version.to_be_bytes())?;
            }
            Ok(Some(bytes)) if bytes == 1u64.to_be_bytes() => {
                if policy(&save)? != policy_version {
                    return Err(Error::InvalidInput("field profile policy mismatch"));
                }
                for name in TABLES {
                    save.get(name, b"")?;
                }
            }
            Ok(_) => return Err(Error::Corrupt("unsupported field profile schema")),
            Err(error) => return Err(error),
        }
        save.release()
    }

    /// Replace one claim and its exact-prefix bucket atomically. Replays do no
    /// writes. A new baseline needs a strictly newer generation and nonregressing
    /// recorded time. Effective observation time may regress for a correction.
    pub fn upsert(txn: &mut WriteTxn<'_>, claim: &Claim) -> Result<Revision> {
        let encoded = codec::encode(claim)?;
        let rev = revision(&encoded);
        let mut save = txn.savepoint()?;
        policy(&save)?;
        let old = save.get(CLAIMS, &claim.id.0)?.map(<[u8]>::to_vec);
        if let Some(old) = old {
            if old == encoded {
                save.release()?;
                return Ok(rev);
            }
            let old_claim = codec::decode(claim.id, &old)?;
            let freshness = write_freshness(&save, claim.id)?;
            if claim.generation <= old_claim.generation || claim.recorded_at < freshness.recorded_at
            {
                return Err(Error::InvalidInput(
                    "stale/conflicting field claim replacement",
                ));
            }
            change_bucket(
                &mut save,
                old_claim.field,
                old_claim.prefix,
                claim.id,
                false,
            )?;
        } else if let Some(bytes) = save.get(TOMBSTONES, &claim.id.0)? {
            let (recorded_at, generation, _) = tombstone(bytes)?;
            if claim.generation <= generation || claim.recorded_at < recorded_at {
                return Err(Error::InvalidInput(
                    "stale withdrawn field claim replacement",
                ));
            }
        }
        save.delete(TOMBSTONES, &claim.id.0)?;
        save.put(CLAIMS, &claim.id.0, &encoded)?;
        save.put(FRESH, &claim.id.0, &codec::encode_fresh(claim.freshness()))?;
        change_bucket(&mut save, claim.field, claim.prefix, claim.id, true)?;
        save.release()?;
        Ok(rev)
    }

    /// A compact refresh changes only one 24-byte freshness value. The baseline
    /// must still exist and match; late and conflicting renewals fail.
    pub fn refresh(
        txn: &mut WriteTxn<'_>,
        id: ClaimId,
        expected: Revision,
        freshness: Freshness,
    ) -> Result<bool> {
        validate_freshness(freshness)?;
        let mut save = txn.savepoint()?;
        policy(&save)?;
        let bytes = save
            .get(CLAIMS, &id.0)?
            .ok_or(Error::InvalidInput("refresh references absent field claim"))?;
        if revision(bytes) != expected {
            return Err(Error::InvalidInput("field claim refresh revision mismatch"));
        }
        let old = write_freshness(&save, id)?;
        if old == freshness {
            save.release()?;
            return Ok(false);
        }
        if freshness.observed_at < old.observed_at
            || freshness.recorded_at < old.recorded_at
            || (freshness.observed_at == old.observed_at
                && freshness.recorded_at == old.recorded_at)
        {
            return Err(Error::InvalidInput("stale/conflicting field claim refresh"));
        }
        save.put(FRESH, &id.0, &codec::encode_fresh(freshness))?;
        save.release()?;
        Ok(true)
    }

    /// Withdraw exactly the supplied revision. Retained journal events carry
    /// historical facts and retractions; this module stores current claims.
    /// This convenience form fences through the last freshness timestamp. Use
    /// `withdraw_at` when an independently timestamped withdrawal is available.
    pub fn withdraw(txn: &mut WriteTxn<'_>, id: ClaimId, expected: Revision) -> Result<bool> {
        policy(txn)?;
        let recorded_at = match txn.get(CLAIMS, &id.0)? {
            Some(_) => write_freshness(txn, id)?.recorded_at,
            None => return Ok(false),
        };
        Self::withdraw_at(txn, id, expected, recorded_at)
    }

    /// Withdraw with the event's actual recorded time. A delayed replacement
    /// recorded before this retraction cannot subsequently resurrect the claim.
    pub fn withdraw_at(
        txn: &mut WriteTxn<'_>,
        id: ClaimId,
        expected: Revision,
        recorded_at: u64,
    ) -> Result<bool> {
        let mut save = txn.savepoint()?;
        policy(&save)?;
        let Some(bytes) = save.get(CLAIMS, &id.0)?.map(<[u8]>::to_vec) else {
            let Some(bytes) = save.get(TOMBSTONES, &id.0)? else {
                save.release()?;
                return Ok(false);
            };
            let (old_time, generation, old_revision) = tombstone(bytes)?;
            if expected != old_revision || recorded_at < old_time {
                return Err(Error::InvalidInput(
                    "stale/conflicting field claim withdrawal",
                ));
            }
            if recorded_at == old_time {
                save.release()?;
                return Ok(false);
            }
            save.put(
                TOMBSTONES,
                &id.0,
                &encode_tombstone(recorded_at, generation, expected),
            )?;
            save.release()?;
            return Ok(true);
        };
        if revision(&bytes) != expected {
            return Err(Error::InvalidInput(
                "field claim withdrawal revision mismatch",
            ));
        }
        let old = codec::decode(id, &bytes)?;
        let freshness = write_freshness(&save, id)?;
        if recorded_at < freshness.recorded_at {
            return Err(Error::InvalidInput("stale field claim withdrawal"));
        }
        change_bucket(&mut save, old.field, old.prefix, id, false)?;
        save.put(
            TOMBSTONES,
            &id.0,
            &encode_tombstone(recorded_at, old.generation, expected),
        )?;
        save.delete(CLAIMS, &id.0)?;
        save.delete(FRESH, &id.0)?;
        save.release()?;
        Ok(true)
    }

    pub fn view(read: &ReadTxn) -> Result<View<'_>> {
        View::new(read)
    }
}

/// The regional journal owns framing; this checked codec includes the stable ID
/// and immutable baseline and rejects trailing or oversized data.
pub(crate) fn encode_claim(claim: &Claim) -> Result<Vec<u8>> {
    let mut bytes = claim.id.0.to_vec();
    bytes.extend_from_slice(&codec::encode(claim)?);
    Ok(bytes)
}
pub(crate) fn decode_claim(bytes: &[u8]) -> Result<Claim> {
    if bytes.len() < 32 {
        return Err(Error::Corrupt("field claim event extent"));
    }
    let id = ClaimId(bytes[..32].try_into().unwrap());
    let claim = codec::decode(id, &bytes[32..])?;
    let fresh = claim.freshness;
    Ok(Claim {
        id,
        generation: claim.generation,
        field: claim.field,
        prefix: claim.prefix,
        source: claim.source,
        priority: claim.priority,
        value: claim.value.to_vec(),
        observed_at: fresh.observed_at,
        recorded_at: fresh.recorded_at,
        expires_at: fresh.expires_at,
        evidence: claim.evidence,
    })
}

fn policy(txn: &WriteTxn<'_>) -> Result<u64> {
    if txn.get(META, VERSION)? != Some(&1u64.to_be_bytes()) {
        return Err(Error::Corrupt("unsupported field profile schema"));
    }
    let version = number(
        txn.get(META, POLICY)?
            .ok_or(Error::Corrupt("missing field policy"))?,
    )?;
    if version == 0 {
        return Err(Error::Corrupt("zero field policy"));
    }
    Ok(version)
}
fn revision(bytes: &[u8]) -> Revision {
    let mut hash = Sha256::new();
    hash.update(b"ULMDB/ipdon/field-revision/v1\0");
    hash.update(bytes);
    Revision(hash.finalize().into())
}
fn number(bytes: &[u8]) -> Result<u64> {
    Ok(u64::from_be_bytes(
        bytes
            .try_into()
            .map_err(|_| Error::Corrupt("field number extent"))?,
    ))
}
fn encode_tombstone(recorded_at: u64, generation: u64, revision: Revision) -> [u8; 48] {
    let mut bytes = [0; 48];
    bytes[..8].copy_from_slice(&recorded_at.to_be_bytes());
    bytes[8..16].copy_from_slice(&generation.to_be_bytes());
    bytes[16..].copy_from_slice(&revision.0);
    bytes
}
fn tombstone(bytes: &[u8]) -> Result<(u64, u64, Revision)> {
    if bytes.len() != 48 || number(&bytes[8..16])? == 0 {
        return Err(Error::Corrupt("field withdrawal extent"));
    }
    Ok((
        number(&bytes[..8])?,
        number(&bytes[8..16])?,
        Revision(bytes[16..].try_into().unwrap()),
    ))
}
fn validate_freshness(freshness: Freshness) -> Result<()> {
    if freshness.expires_at < freshness.observed_at {
        return Err(Error::InvalidInput(
            "field claim expiry precedes observation",
        ));
    }
    Ok(())
}
fn write_freshness(txn: &WriteTxn<'_>, id: ClaimId) -> Result<Freshness> {
    codec::decode_fresh(
        txn.get(FRESH, &id.0)?
            .ok_or(Error::Corrupt("field claim freshness missing"))?,
    )
}
fn field_key(field: FieldId, address: Address) -> [u8; 9] {
    let mut key = [0; 9];
    key[..8].copy_from_slice(&field.0.to_be_bytes());
    key[8] = match address {
        Address::V4(_) => 4,
        Address::V6(_) => 6,
    };
    key
}
fn index_name(field: FieldId, address: Address) -> Vec<u8> {
    format!(
        "ipdon.fields.v1.{:016x}.v{}",
        field.0,
        match address {
            Address::V4(_) => 4,
            Address::V6(_) => 6,
        }
    )
    .into_bytes()
}

/// Internal logical inventory for the typed facade's streaming baseline proof.
/// This excludes local application metadata and journal history intentionally.
pub(crate) fn application_databases(read:&ReadTxn)->Result<Vec<(Vec<u8>,DatabaseKind)>> {
    use std::ops::Bound;
    let mut databases=TABLES.iter().map(|name|(name.to_vec(),DatabaseKind::Kv)).collect::<Vec<_>>();
    for item in read.range(FIELDS,Bound::Unbounded,Bound::Unbounded,false)? {
        let (key,value)=item?;
        if key.len()!=9 || !value.is_empty() {return Err(Error::Corrupt("field catalog baseline encoding"));}
        let field=FieldId(number(&key[..8])?);
        if field.0==0 {return Err(Error::Corrupt("zero baseline field id"));}
        let (address,kind)=match key[8] {
            4=>(Address::V4(0),DatabaseKind::Ipv4),
            6=>(Address::V6(0),DatabaseKind::Ipv6),
            _=>return Err(Error::Corrupt("field catalog baseline family")),
        };
        databases.push((index_name(field,address),kind));
        if databases.len()>4096 {return Err(Error::LimitExceeded);}
    }
    Ok(databases)
}
fn bucket_key(field: FieldId, prefix: Prefix) -> Vec<u8> {
    let mut key = field.0.to_be_bytes().to_vec();
    key.extend_from_slice(&codec::encode_prefix(prefix));
    key
}
fn ensure_index(txn: &mut WriteTxn<'_>, field: FieldId, address: Address) -> Result<Vec<u8>> {
    let key = field_key(field, address);
    let name = index_name(field, address);
    if txn.get(FIELDS, &key)?.is_none() {
        txn.create_database(
            &name,
            match address {
                Address::V4(_) => DatabaseKind::Ipv4,
                Address::V6(_) => DatabaseKind::Ipv6,
            },
        )?;
        txn.put(FIELDS, &key, b"")?;
    }
    Ok(name)
}
fn change_bucket(
    txn: &mut WriteTxn<'_>,
    field: FieldId,
    prefix: Prefix,
    id: ClaimId,
    insert: bool,
) -> Result<()> {
    let key = bucket_key(field, prefix);
    let mut ids = txn
        .get(BUCKETS, &key)?
        .map(codec::decode_bucket)
        .transpose()?
        .unwrap_or_default();
    let position = ids.iter().position(|candidate| *candidate == id);
    match (insert, position) {
        (true, Some(_)) => return Err(Error::Corrupt("duplicate field bucket identity")),
        (true, None) => {
            if ids.len() >= MAX_CLAIMS_PER_PREFIX {
                return Err(Error::LimitExceeded);
            }
            ids.push(id);
        }
        (false, Some(position)) => {
            ids.remove(position);
        }
        (false, None) => return Err(Error::Corrupt("field claim bucket identity missing")),
    }
    let name = ensure_index(txn, field, prefix.network())?;
    if ids.is_empty() {
        txn.delete(BUCKETS, &key)?;
        txn.delete_prefix(&name, prefix)?;
    } else {
        // A bucket is bounded; ranking never reads unrelated fields/prefixes.
        let mut ranked = Vec::with_capacity(ids.len());
        for id in ids {
            let bytes = txn
                .get(CLAIMS, &id.0)?
                .ok_or(Error::Corrupt("field bucket claim missing"))?;
            let claim = codec::decode(id, bytes)?;
            if claim.field != field || claim.prefix != prefix {
                return Err(Error::Corrupt("field bucket claim scope mismatch"));
            }
            ranked.push((std::cmp::Reverse(claim.priority), claim.source, id));
        }
        ranked.sort_unstable();
        let ids: Vec<_> = ranked.into_iter().map(|(_, _, id)| id).collect();
        let bytes = codec::encode_bucket(&ids);
        txn.put(BUCKETS, &key, &bytes)?;
        txn.insert_prefix(&name, prefix, &bytes)?;
    }
    Ok(())
}
