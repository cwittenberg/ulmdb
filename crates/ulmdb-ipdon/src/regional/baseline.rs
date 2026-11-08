//! Concrete baseline verification for the typed regional model. Hashing streams
//! logical rows; snapshot copying reuses native files rather than rebuilding all
//! relationship and field indexes in one giant write transaction.
use super::*;
use crate::{Address, Prefix};
use sha2::{Digest, Sha256};
use std::{ops::Bound, path::Path};
use ulmdb::{Environment, backup};

fn field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}

impl Regional {
    /// Canonical digest of all owned typed application tables/indexes from one
    /// pinned commit. Journal bytes, native page layout and local-only metadata
    /// are excluded. Current raw evidence, freshness, withdrawals and ownership
    /// are included. This is an O(application-size), bounded-memory operation.
    pub fn application_digest(read: &ReadTxn) -> Result<[u8; 32]> {
        if read.get(META, b"version")? != Some(&[1][..]) {
            return Err(Error::Corrupt("regional baseline schema"));
        }
        let mut databases = profiles::application_databases(read)?;
        databases.extend(
            rrsets::codec::TABLES
                .iter()
                .map(|name| (name.to_vec(), DatabaseKind::Kv)),
        );
        databases.extend(
            [META, OWNERS]
                .into_iter()
                .map(|name| (name.to_vec(), DatabaseKind::Kv)),
        );
        databases.sort_by(|a, b| a.0.cmp(&b.0));
        let mut hash = Sha256::new();
        hash.update(b"IPDon/regional-application-baseline/v1\0");
        for (name, kind) in databases {
            if read.database_kind(&name)? != kind {
                return Err(Error::Corrupt("regional baseline database kind"));
            }
            field(&mut hash, &name);
            match kind {
                DatabaseKind::Kv => {
                    hash.update([1]);
                    for item in read.range(&name, Bound::Unbounded, Bound::Unbounded, false)? {
                        let (key, value) = item?;
                        hash.update([1]);
                        field(&mut hash, key);
                        field(&mut hash, value);
                    }
                }
                DatabaseKind::Ipv4 | DatabaseKind::Ipv6 => {
                    hash.update([if kind == DatabaseKind::Ipv4 { 4 } else { 6 }]);
                    let all = Prefix::new(
                        if kind == DatabaseKind::Ipv4 {
                            Address::V4(0)
                        } else {
                            Address::V6(0)
                        },
                        0,
                    )
                    .map_err(|_| Error::Corrupt("regional baseline family"))?;
                    for item in read.prefix_index(&name)?.subnets(all)? {
                        let (prefix, value) = item?;
                        hash.update([1]);
                        field(&mut hash, &crate::schema::address_bytes(prefix.network()));
                        hash.update([prefix.length()]);
                        field(&mut hash, value);
                    }
                }
                _ => return Err(Error::WrongKind),
            }
            hash.update([0]);
        }
        Ok(hash.finalize().into())
    }

    fn verify_baseline_source(
        read: &ReadTxn,
        baseline: &journal::Baseline,
    ) -> journal::ProtocolResult<journal::BaselineActivation> {
        let view = Self::view(read)?;
        let versions = journal::Versions {
            schema: 1,
            policy: view.profiles.policy_version(),
        };
        if versions != baseline.versions {
            return Err(journal::ProtocolError::VersionMismatch {
                expected: baseline.versions,
                actual: versions,
            });
        }
        let actual = journal::cursors(read)?
            .into_iter()
            .map(|(id, c)| (id, c.applied))
            .collect::<BTreeMap<_, _>>();
        let expected = baseline
            .origins
            .iter()
            .filter(|(_, seq)| **seq > 0)
            .map(|(&id, &seq)| (id, seq))
            .collect::<BTreeMap<_, _>>();
        if actual != expected {
            return Err(journal::ProtocolError::BaselineMismatch(
                "complete source cursor vector differs",
            ));
        }
        let application_digest = Self::application_digest(read)?;
        if application_digest != baseline.application_digest {
            return Err(journal::ProtocolError::BaselineMismatch(
                "installed regional application digest differs",
            ));
        }
        Ok(journal::BaselineActivation {
            application_digest,
            versions,
            origins: baseline.origins.clone(),
        })
    }

    /// Create a new-realm candidate from a pinned existing typed database,
    /// including a source whose old history has already been pruned. Native
    /// copying preserves application indexes and avoids record reinsertion.
    /// Only the new candidate's copied journal is replaced. Failure leaves the
    /// source unchanged and the candidate unactivated; destination must be new.
    pub fn create_from_baseline(
        source: &Environment,
        read: &ReadTxn,
        target: impl AsRef<Path>,
        baseline: &journal::Baseline,
    ) -> journal::ProtocolResult<(Environment, journal::BaselineReceipt)> {
        baseline.identity()?;
        if journal::realm(read)? == baseline.realm {
            return Err(journal::ProtocolError::BaselineMismatch(
                "baseline requires a distinct new realm",
            ));
        }
        Self::verify_baseline_source(read, baseline)?;
        backup::export_snapshot(source, read, target.as_ref())?;
        let candidate = Environment::open(target)?;
        let copy = candidate.begin_read();
        let activated = Self::verify_baseline_source(&copy, baseline)?;
        let mut write = candidate.begin_write()?;
        journal::reset_copied_journal(&mut write)?;
        let receipt = journal::import_baseline(&mut write, baseline, |_| Ok(activated))?;
        write.commit()?;
        drop(copy);
        Self::view(&candidate.begin_read())?;
        Ok((candidate, receipt))
    }

    /// Guard a deliberate new-realm cutover. Both source and candidate are
    /// fenced while their complete actual application digests and source
    /// cursor vector are verified against the imported baseline. The normal
    /// full-handover guard continues to reject cross-realm switches.
    pub fn validate_baseline_handover<'a>(
        source: &'a Environment,
        candidate: &'a Environment,
    ) -> journal::ProtocolResult<journal::HandoverGuard<'a>> {
        journal::validate_baseline_handover(source, candidate, |source, candidate, baseline| {
            let activation = Self::verify_baseline_source(source, baseline)?;
            if Self::application_digest(candidate)? != baseline.application_digest {
                return Err(journal::ProtocolError::BaselineMismatch(
                    "candidate application digest changed before activation",
                ));
            }
            Ok(activation)
        })
    }
}
