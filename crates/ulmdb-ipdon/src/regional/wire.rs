use super::Command;
use crate::{Error, Result, profiles, rrsets};
use rrsets::codec::{Decoder, freshness_bytes, read_freshness};

const MAX_BYTES: usize = 131_072;

pub(super) fn encode(command: &Command) -> Result<Vec<u8>> {
    let mut bytes = vec![1];
    match command {
        Command::ReplaceRrset { set, freshness } => {
            bytes.push(1);
            bytes.extend_from_slice(&freshness_bytes(*freshness));
            bytes.extend_from_slice(&rrsets::codec::encode(set)?);
        }
        Command::RefreshRrset {
            id,
            revision,
            freshness,
        } => {
            bytes.push(2);
            bytes.extend_from_slice(&id.0);
            bytes.extend_from_slice(&revision.to_be_bytes());
            bytes.extend_from_slice(&freshness_bytes(*freshness));
        }
        Command::WithdrawRrset { id, revision } => {
            bytes.push(3);
            bytes.extend_from_slice(&id.0);
            bytes.extend_from_slice(&revision.to_be_bytes());
        }
        Command::PutClaim(claim) => {
            bytes.push(4);
            bytes.extend_from_slice(&profiles::encode_claim(claim)?);
        }
        Command::RefreshClaim {
            id,
            revision,
            freshness,
        } => {
            bytes.push(5);
            bytes.extend_from_slice(&id.0);
            bytes.extend_from_slice(&revision.0);
            for n in [
                freshness.observed_at,
                freshness.recorded_at,
                freshness.expires_at,
            ] {
                bytes.extend_from_slice(&n.to_be_bytes());
            }
        }
        Command::WithdrawClaim { id, revision } => {
            bytes.push(6);
            bytes.extend_from_slice(&id.0);
            bytes.extend_from_slice(&revision.0);
        }
    }
    if bytes.len() > MAX_BYTES {
        return Err(Error::LimitExceeded);
    }
    // Round-trip validation keeps producer-side validation equal to receiver
    // validation before bytes are placed on a durable/authenticated transport.
    decode(&bytes)?;
    Ok(bytes)
}

pub(super) fn decode(bytes: &[u8]) -> Result<Command> {
    if bytes.len() > MAX_BYTES {
        return Err(Error::LimitExceeded);
    }
    let mut d = Decoder { bytes, pos: 0 };
    if d.take(1)? != [1] {
        return Err(Error::Corrupt("regional event version"));
    }
    let tag = d.take(1)?[0];
    let command = match tag {
        1 => {
            let freshness = read_freshness(&mut d)?;
            let set = rrsets::codec::decode(d.take(bytes.len() - d.pos)?)?;
            Command::ReplaceRrset { set, freshness }
        }
        2 => Command::RefreshRrset {
            id: rrsets::SetId(d.take(32)?.try_into().unwrap()),
            revision: d.n()?,
            freshness: read_freshness(&mut d)?,
        },
        3 => Command::WithdrawRrset {
            id: rrsets::SetId(d.take(32)?.try_into().unwrap()),
            revision: d.n()?,
        },
        4 => Command::PutClaim(profiles::decode_claim(d.take(bytes.len() - d.pos)?)?),
        5 => Command::RefreshClaim {
            id: profiles::ClaimId(d.take(32)?.try_into().unwrap()),
            revision: profiles::Revision(d.take(32)?.try_into().unwrap()),
            freshness: profiles::Freshness {
                observed_at: d.n()?,
                recorded_at: d.n()?,
                expires_at: d.n()?,
            },
        },
        6 => Command::WithdrawClaim {
            id: profiles::ClaimId(d.take(32)?.try_into().unwrap()),
            revision: profiles::Revision(d.take(32)?.try_into().unwrap()),
        },
        _ => return Err(Error::Corrupt("regional event type")),
    };
    d.finish()?;
    match &command {
        Command::RefreshRrset { revision, .. } | Command::WithdrawRrset { revision, .. }
            if *revision == 0 =>
        {
            return Err(Error::Corrupt("zero RRset event revision"));
        }
        Command::RefreshClaim { freshness, .. } if freshness.expires_at < freshness.observed_at => {
            return Err(Error::Corrupt("claim expiry before observation"));
        }
        _ => {}
    }
    Ok(command)
}
