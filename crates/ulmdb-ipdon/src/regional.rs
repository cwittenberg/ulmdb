//! Typed IPDon events applied atomically with multi-origin journal cursors.
mod baseline;
mod wire;

use crate::{Error, Result, journal, profiles, rrsets};
use std::collections::BTreeMap;
use ulmdb::{DatabaseKind, ReadTxn, WriteTxn};

const OWNERS: &[u8] = b"ipdon.regional.claim-owners";
const META: &[u8] = b"ipdon.regional.meta";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    ReplaceRrset {
        set: rrsets::Rrset,
        freshness: rrsets::Freshness,
    },
    RefreshRrset {
        id: rrsets::SetId,
        revision: u64,
        freshness: rrsets::Freshness,
    },
    WithdrawRrset {
        id: rrsets::SetId,
        revision: u64,
    },
    PutClaim(profiles::Claim),
    RefreshClaim {
        id: profiles::ClaimId,
        revision: profiles::Revision,
        freshness: profiles::Freshness,
    },
    WithdrawClaim {
        id: profiles::ClaimId,
        revision: profiles::Revision,
    },
}

impl Command {
    pub fn encode(&self) -> Result<Vec<u8>> {
        wire::encode(self)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        wire::decode(bytes)
    }
    pub fn event(&self, origin: u64, sequence: u64, recorded_at: u64) -> Result<journal::Event> {
        if origin == 0 || sequence == 0 {
            return Err(Error::InvalidInput("zero event identity"));
        }
        check_time(self, recorded_at)?;
        if let Self::ReplaceRrset { set, .. } = self
            && set.key.origin != origin
        {
            return Err(Error::InvalidInput("RRset belongs to another origin"));
        }
        Ok(journal::Event {
            origin,
            sequence,
            recorded_at,
            payload: self.encode()?,
        })
    }
}

pub struct Regional;
#[derive(Debug, Default)]
pub struct Work {
    pub added_members: usize,
    pub removed_members: usize,
    pub retained_members: usize,
    pub refreshes: usize,
    pub claim_changes: usize,
}
#[derive(Debug)]
pub struct ApplyReport {
    pub journal: journal::ApplyReport,
    pub work: Work,
}

pub struct View<'a> {
    pub rrsets: rrsets::View<'a>,
    pub profiles: profiles::View<'a>,
    pub watermarks: BTreeMap<u64, journal::Cursor>,
}

impl Regional {
    pub fn initialize(txn: &mut WriteTxn<'_>, policy_version: u64) -> Result<()> {
        Self::initialize_inner(txn, policy_version, None)
    }

    /// Independently created regional replicas must configure the same nonzero
    /// replication realm. The default initializer creates a standalone realm.
    pub fn initialize_in_realm(
        txn: &mut WriteTxn<'_>,
        policy_version: u64,
        realm: u128,
    ) -> Result<()> {
        Self::initialize_inner(txn, policy_version, Some(realm))
    }

    fn initialize_inner(
        txn: &mut WriteTxn<'_>,
        policy_version: u64,
        realm: Option<u128>,
    ) -> Result<()> {
        let mut save = txn.savepoint()?;
        match save.get(META, b"version") {
            Err(Error::MissingDatabase) => {
                save.create_database(META, DatabaseKind::Kv)?;
                save.create_database(OWNERS, DatabaseKind::Kv)?;
                save.put(META, b"version", &[1])?;
            }
            Ok(Some(b"\x01")) => {
                save.get(OWNERS, b"")?;
            }
            _ => return Err(Error::Corrupt("regional schema version")),
        }
        if let Some(realm) = realm {
            journal::initialize_with_realm(&mut save, realm)?;
        } else {
            journal::initialize(&mut save)?;
        }
        journal::set_versions(
            &mut save,
            journal::Versions {
                schema: 1,
                policy: policy_version,
            },
        )?;
        rrsets::Rrsets::initialize(&mut save)?;
        profiles::Profiles::initialize(&mut save, policy_version)?;
        save.release()
    }

    pub fn view(read: &ReadTxn) -> Result<View<'_>> {
        if read.get(META, b"version")? != Some(&[1][..]) {
            return Err(Error::Corrupt("regional schema version"));
        }
        let profiles = profiles::Profiles::view(read)?;
        if journal::versions(read)?
            != (journal::Versions {
                schema: 1,
                policy: profiles.policy_version(),
            })
        {
            return Err(Error::Corrupt("regional journal/profile versions disagree"));
        }
        let watermarks = journal::cursors(read)?;
        if watermarks.values().any(|c| c.applied != c.serving) {
            return Err(Error::InvalidInput(
                "regional serving projection has an unapplied backlog",
            ));
        }
        Ok(View {
            rrsets: rrsets::Rrsets::view(read)?,
            profiles,
            watermarks,
        })
    }

    /// Apply typed events with synchronous projections. Evidence and serving
    /// watermarks advance together; journal::EvidenceOnly is for callers that
    /// implement their own separate materialization protocol.
    pub fn apply_page(
        txn: &mut WriteTxn<'_>,
        events: &[journal::Event],
        limits: journal::Limits,
    ) -> Result<ApplyReport> {
        Self::apply_page_with_budget(txn, events, limits, 65_536)
    }

    /// In addition to event bytes/count, cap RRset membership work. This also
    /// charges refreshes that revive physically expired relationships.
    pub fn apply_page_with_budget(
        txn: &mut WriteTxn<'_>,
        events: &[journal::Event],
        limits: journal::Limits,
        max_members: usize,
    ) -> Result<ApplyReport> {
        if max_members == 0 || max_members > 65_536 {
            return Err(Error::InvalidInput(
                "regional member budget must be 1..65536",
            ));
        }
        let mut work = Work::default();
        let report = journal::apply_page(
            txn,
            events,
            limits,
            journal::ApplyMode::Synchronous,
            |txn, event| Self::apply_inner(txn, event, &mut work, max_members),
        )?;
        Ok(ApplyReport {
            journal: report,
            work,
        })
    }

    /// Check the peer's realm, policy, baseline and origin history before any
    /// typed projection mutation. The existing unverified page API remains for
    /// applications that already establish those conditions at their boundary.
    pub fn apply_verified_page(
        txn: &mut WriteTxn<'_>,
        page: &journal::VerifiedPage,
        limits: journal::Limits,
    ) -> journal::ProtocolResult<ApplyReport> {
        Self::apply_verified_page_with_budget(txn, page, limits, 65_536)
    }

    pub fn apply_verified_page_with_budget(
        txn: &mut WriteTxn<'_>,
        page: &journal::VerifiedPage,
        limits: journal::Limits,
        max_members: usize,
    ) -> journal::ProtocolResult<ApplyReport> {
        if max_members == 0 || max_members > 65_536 {
            return Err(Error::InvalidInput("regional member budget must be 1..65536").into());
        }
        let mut work = Work::default();
        let journal = journal::apply_verified_page(
            txn,
            page,
            limits,
            journal::ApplyMode::Synchronous,
            |txn, event| Self::apply_inner(txn, event, &mut work, max_members),
        )?;
        Ok(ApplyReport { journal, work })
    }

    /// Replay callback for journal snapshot rebase. The journal owns the page
    /// savepoint and cursors; this callback changes only application state.
    pub fn apply_event(txn: &mut WriteTxn<'_>, event: &journal::Event) -> Result<()> {
        let mut save = txn.savepoint()?;
        Self::apply_inner(&mut save, event, &mut Work::default(), 65_536)?;
        save.release()
    }

    fn apply_inner(
        txn: &mut WriteTxn<'_>,
        event: &journal::Event,
        work: &mut Work,
        max_members: usize,
    ) -> Result<()> {
        if event.origin == 0 || event.sequence == 0 {
            return Err(Error::InvalidInput("zero regional event identity"));
        }
        if txn.get(META, b"version")? != Some(&[1][..]) {
            return Err(Error::Corrupt("regional schema version"));
        }
        let command = Command::decode(&event.payload)?;
        check_time(&command, event.recorded_at)?;
        let mut delta = None;
        match command {
            Command::ReplaceRrset { set, freshness } => {
                if set.key.origin != event.origin {
                    return Err(Error::InvalidInput("RRset belongs to another origin"));
                }
                delta = Some(rrsets::Rrsets::replace_inner(txn, &set, freshness)?);
            }
            Command::RefreshRrset {
                id,
                revision,
                freshness,
            } => {
                check_owner(rrsets::Rrsets::owner(txn, id)?, event.origin)?;
                let refreshed = rrsets::Rrsets::refresh_inner(txn, id, revision, freshness)?;
                if refreshed.freshness_changed {
                    work.refreshes += 1;
                }
                delta = Some(refreshed);
            }
            Command::WithdrawRrset { id, revision } => {
                check_owner(rrsets::Rrsets::owner(txn, id)?, event.origin)?;
                delta = Some(rrsets::Rrsets::withdraw_inner(
                    txn,
                    id,
                    revision,
                    event.recorded_at,
                )?);
            }
            Command::PutClaim(claim) => {
                match txn.get(OWNERS, &claim.id.0)? {
                    Some(bytes) => check_owner(owner(bytes)?, event.origin)?,
                    None => txn.put(OWNERS, &claim.id.0, &event.origin.to_be_bytes())?,
                }
                profiles::Profiles::upsert(txn, &claim)?;
                work.claim_changes += 1;
            }
            Command::RefreshClaim {
                id,
                revision,
                freshness,
            } => {
                check_owner(
                    owner(
                        txn.get(OWNERS, &id.0)?
                            .ok_or(Error::InvalidInput("claim owner missing"))?,
                    )?,
                    event.origin,
                )?;
                if profiles::Profiles::refresh(txn, id, revision, freshness)? {
                    work.refreshes += 1;
                }
            }
            Command::WithdrawClaim { id, revision } => {
                check_owner(
                    owner(
                        txn.get(OWNERS, &id.0)?
                            .ok_or(Error::InvalidInput("claim owner missing"))?,
                    )?,
                    event.origin,
                )?;
                if profiles::Profiles::withdraw_at(txn, id, revision, event.recorded_at)? {
                    work.claim_changes += 1;
                }
            }
        }
        if let Some(delta) = delta {
            work.added_members += delta.added;
            work.removed_members += delta.removed;
            work.retained_members += delta.retained;
        }
        if work.added_members + work.removed_members + work.retained_members > max_members {
            return Err(Error::LimitExceeded);
        }
        Ok(())
    }
}

fn owner(bytes: &[u8]) -> Result<u64> {
    Ok(u64::from_be_bytes(
        bytes
            .try_into()
            .map_err(|_| Error::Corrupt("claim owner encoding"))?,
    ))
}
fn check_owner(actual: u64, expected: u64) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(Error::InvalidInput(
            "event cannot mutate another origin's claims",
        ))
    }
}
fn check_time(command: &Command, recorded_at: u64) -> Result<()> {
    let time = match command {
        Command::ReplaceRrset { freshness, .. } | Command::RefreshRrset { freshness, .. } => {
            freshness.recorded_at
        }
        Command::PutClaim(claim) => claim.recorded_at,
        Command::RefreshClaim { freshness, .. } => freshness.recorded_at,
        Command::WithdrawRrset { .. } | Command::WithdrawClaim { .. } => recorded_at,
    };
    if time == recorded_at {
        Ok(())
    } else {
        Err(Error::InvalidInput(
            "event and evidence recorded times disagree",
        ))
    }
}
