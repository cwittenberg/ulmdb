//! Native database journeys for the regional IPDon foundation; fixture data only.
use std::net::IpAddr;
use ulmdb::Environment;
use ulmdb_ipdon::{
    Address, Dnssec, Error, EvidenceId, Prefix, ResolverId, Result, Role, ScopeId, SourceId,
    VantageId, journal, profiles,
    regional::{Command, Regional},
    rrsets,
};

#[path = "ipdon-regional/backup_control.rs"]
mod backup_control;

const TIME: u64 = 1_700_000_000;
const USAGE: &str = "usage: ipdon-regional init DIRECTORY REALM POLICY; demo DIRECTORY; hostname DIRECTORY TIME HOST; lookup DIRECTORY TIME IP; candidates DIRECTORY TIME IP FIELD; checkpoint DIRECTORY ORIGIN SEQUENCE; replicate SOURCE DESTINATION ORIGIN; baseline SOURCE NEW_DIRECTORY NEW_REALM IMPORT_TIME; changes DIRECTORY ORIGIN AFTER; expire DIRECTORY TIME; snapshot DIRECTORY BUNDLE [backup options]; verify-snapshot BUNDLE [backup options]; restore BUNDLE NEW_DIRECTORY; recover BUNDLE LOCAL_DIRECTORY NEW_DIRECTORY";
fn n(text: &str) -> Result<u64> {
    text.parse()
        .map_err(|_| Error::InvalidInput("expected unsigned integer"))
}
fn address(text: &str) -> Result<Address> {
    text.parse::<IpAddr>()
        .map(Address::from)
        .map_err(|_| Error::InvalidInput("invalid IP address"))
}
fn ip(a: Address) -> IpAddr {
    match a {
        Address::V4(v) => IpAddr::V4(v.into()),
        Address::V6(v) => IpAddr::V6(v.into()),
    }
}

fn demo(path: &str) -> Result<()> {
    let env = Environment::create(path)?;
    let mut write = env.begin_write()?;
    Regional::initialize_in_realm(&mut write, 1, 0x4950444f4e)?;
    let set = rrsets::Rrset {
        key: rrsets::Key {
            origin: 1,
            source: SourceId(7),
            scope: ScopeId(1),
            resolver: ResolverId(1),
            vantage: VantageId(1),
            role: Role::Direct,
            hostname: "example.test".into(),
            kind: rrsets::RecordType::A,
        },
        revision: 1,
        addresses: vec![address("192.0.2.1")?, address("192.0.2.2")?],
        dnssec: Dnssec::Secure,
    };
    let id = set.key.id()?;
    let freshness = rrsets::Freshness {
        observed_at: TIME,
        recorded_at: TIME + 1,
        expires_at: TIME + 300,
        evidence: EvidenceId::from_bytes(b"frozen dns evidence"),
    };
    let mut events = vec![Command::ReplaceRrset { set, freshness }.event(1, 1, TIME + 1)?];
    for (sequence, field, network, len, value, priority) in [
        (1u64, 1, "192.0.2.0", 24, "NL", 100),
        (2, 2, "192.0.0.0", 16, "AS64500", 100),
        (3, 3, "192.0.2.1", 32, "false", 50),
        (4, 1, "2001:db8::", 32, "NL", 100),
    ] {
        let claim = profiles::Claim {
            id: profiles::ClaimId::from_origin_source(2, 8, &sequence.to_be_bytes()),
            generation: 1,
            field: profiles::FieldId(field),
            prefix: Prefix::new(address(network)?, len)
                .map_err(|_| Error::InvalidInput("fixture prefix"))?,
            source: 8,
            value: value.as_bytes().to_vec(),
            priority,
            observed_at: TIME,
            recorded_at: TIME + 1,
            expires_at: TIME + 86_400,
            evidence: EvidenceId::from_bytes(value.as_bytes()).0,
        };
        events.push(Command::PutClaim(claim).event(2, sequence, TIME + 1)?);
    }
    let report = Regional::apply_page(&mut write, &events, journal::Limits::default())?;
    println!("initial {:?}; native commit {}", report, write.commit()?);
    let fresh = rrsets::Freshness {
        observed_at: TIME + 60,
        recorded_at: TIME + 61,
        expires_at: TIME + 360,
        evidence: EvidenceId::from_bytes(b"second frozen dns observation"),
    };
    let refresh = Command::RefreshRrset {
        id,
        revision: 1,
        freshness: fresh,
    }
    .event(1, 2, TIME + 61)?;
    println!("compact refresh payload {} bytes", refresh.payload.len());
    let mut write = env.begin_write()?;
    let report = Regional::apply_page(&mut write, &[refresh], journal::Limits::default())?;
    println!("refresh {:?}; native commit {}", report, write.commit()?);
    let read = env.begin_read();
    let view = Regional::view(&read)?;
    for row in view
        .rrsets
        .query(
            &rrsets::Query::Hostname {
                scope: ScopeId(1),
                role: Role::Direct,
                hostname: "example.test".into(),
            },
            TIME + 70,
            None,
            rrsets::Limits::default(),
        )?
        .relations
    {
        println!(
            "{} -> {} revision={} expires={}",
            row.hostname,
            ip(row.address),
            row.revision,
            row.freshness.expires_at
        );
    }
    for result in view.profiles.batch(
        &[address("192.0.2.1")?, address("2001:db8::1")?],
        &[
            profiles::FieldId(1),
            profiles::FieldId(2),
            profiles::FieldId(3),
        ],
        TIME + 70,
        profiles::BatchLimits::default(),
    )? {
        println!(
            "{} {:?}",
            ip(result.address),
            result
                .fields
                .iter()
                .map(|f| f.as_ref().map(|f| String::from_utf8_lossy(f.value)))
                .collect::<Vec<_>>()
        );
    }
    println!(
        "watermarks {:?}; use query time {}",
        view.watermarks,
        TIME + 70
    );
    Ok(())
}

fn execute() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let at = |index: usize| {
        args.get(index)
            .map(String::as_str)
            .ok_or(Error::InvalidInput(USAGE))
    };
    let command = at(0)?;
    let path = at(1)?;
    if command == "init" {
        let realm = at(2)?
            .parse()
            .map_err(|_| Error::InvalidInput("expected decimal u128 realm"))?;
        let policy = n(at(3)?)?;
        if realm == 0 || policy == 0 {
            return Err(Error::InvalidInput("realm and policy must be nonzero").into());
        }
        let env = Environment::create(path)?;
        let mut write = env.begin_write()?;
        Regional::initialize_in_realm(&mut write, policy, realm)?;
        println!("initialized realm={realm}; commit {}", write.commit()?);
        return Ok(());
    }
    if command == "demo" {
        demo(path)?;
        return Ok(());
    }
    if command == "snapshot" {
        let target = at(2)?.to_owned();
        let options = backup_control::Options::parse(&args[3..])?;
        let source = path.to_owned();
        let job = ulmdb::backup::BackupJob::spawn(options.control.clone(), move |control| {
            control.scope(|| {
                let env = Environment::open(source)?;
                let read = env.begin_read();
                Regional::view(&read)?;
                journal::export_snapshot_with_control(&env, &read, target, &control)
            })
        })?;
        let receipt = options.run(job)?;
        println!("snapshot {:?}; watermarks {:?}", receipt.id(), receipt.cursors());
        return Ok(());
    }
    if command == "verify-snapshot" {
        let options = backup_control::Options::parse(&args[2..])?;
        let job = journal::verify_snapshot_job(path, options.control.clone())?;
        let receipt = options.run(job)?;
        println!("verified snapshot {:?}; watermarks {:?}", receipt.id(), receipt.cursors());
        return Ok(());
    }
    if command == "replicate" {
        let source = Environment::open(path)?;
        let destination = Environment::open(at(2)?)?;
        let origin = n(at(3)?)?;
        let after = journal::cursor(&destination.begin_read(), origin)?.applied;
        let limits = journal::Limits {
            max_events: 8,
            ..journal::Limits::default()
        };
        let read = source.begin_read();
        let page = journal::verified_page(&read, origin, after, limits)?;
        let mut write = destination.begin_write()?;
        let report = Regional::apply_verified_page(&mut write, &page, limits)?;
        println!(
            "verified {:?}; commit {}; more={}",
            report,
            write.commit()?,
            page.more()
        );
        return Ok(());
    }
    if command == "baseline" {
        let source = Environment::open(path)?;
        let read = source.begin_read();
        let baseline = journal::Baseline {
            realm: at(3)?
                .parse()
                .map_err(|_| Error::InvalidInput("expected decimal u128 realm"))?,
            application_digest: Regional::application_digest(&read)?,
            versions: journal::versions(&read)?,
            imported_at: n(at(4)?)?,
            origins: journal::cursors(&read)?
                .into_iter()
                .map(|(id, c)| (id, c.applied))
                .collect(),
        };
        let (candidate, receipt) =
            Regional::create_from_baseline(&source, &read, at(2)?, &baseline)?;
        let guard = Regional::validate_baseline_handover(&source, &candidate)?;
        println!(
            "verified baseline candidate {}; realm={}; baseline={:?}; source directory preserved",
            at(2)?,
            baseline.realm,
            receipt.id()
        );
        drop(guard);
        return Ok(());
    }
    if command == "restore" {
        let env = journal::restore_snapshot(path, at(2)?)?;
        let read = env.begin_read();
        Regional::view(&read)?;
        println!(
            "restored commit {} {:?}",
            read.commit_identity(),
            journal::cursors(&read)?
        );
        return Ok(());
    }
    if command == "recover" {
        let local = Environment::open(at(2)?)?;
        // Each worst-case RRset replacement touches at most8192 memberships;
        // eight events bound replay membership work to65536 per transaction.
        let limits = journal::Limits {
            max_events: 8,
            ..journal::Limits::default()
        };
        let candidate =
            journal::rebase_all_snapshot(path, &local, at(3)?, limits, Regional::apply_event)?;
        let guard = journal::validate_full_handover(&local, &candidate)?;
        Regional::view(&candidate.begin_read())?;
        println!(
            "validated recovered candidate {}; old directory preserved",
            at(3)?
        );
        drop(guard);
        return Ok(());
    }
    let env = Environment::open(path)?;
    let read = env.begin_read();
    match command {
        "checkpoint" => {
            println!(
                "realm={} baseline={:?} {:?}",
                journal::realm(&read)?,
                journal::baseline_identity(&read)?,
                journal::checkpoint(&read, n(at(2)?)?, n(at(3)?)?)?
            );
        }
        "candidates" => {
            let view = Regional::view(&read)?;
            let page = view.profiles.covering_candidates(
                address(at(3)?)?,
                profiles::FieldId(n(at(4)?)?),
                n(at(2)?)?,
                None,
                profiles::CandidateLimits::default(),
            )?;
            for value in page.candidates {
                println!(
                    "value={} prefix={}/{} source={} generation={} priority={} expires={} policy={}",
                    String::from_utf8_lossy(value.value),
                    ip(value.prefix.network()),
                    value.prefix.length(),
                    value.source,
                    value.generation,
                    value.priority,
                    value.expires_at,
                    value.policy_version
                );
            }
            println!(
                "scanned={} complete={} resumable={}",
                page.scanned,
                page.complete,
                page.resume.is_some()
            );
        }
        "hostname" => {
            let view = Regional::view(&read)?;
            let query = rrsets::Query::Hostname {
                scope: ScopeId(1),
                role: Role::Direct,
                hostname: at(3)?.into(),
            };
            let page = view
                .rrsets
                .query(&query, n(at(2)?)?, None, rrsets::Limits::default())?;
            for row in page.relations {
                println!(
                    "{}\t{}\torigin={} source={} observed={} expires={}",
                    row.hostname,
                    ip(row.address),
                    row.origin,
                    row.source.0,
                    row.freshness.observed_at,
                    row.freshness.expires_at
                );
            }
            println!("scanned={} more={}", page.scanned, page.resume.is_some());
        }
        "lookup" => {
            let view = Regional::view(&read)?;
            for field in [1, 2, 3] {
                match view.profiles.lookup(
                    address(at(3)?)?,
                    profiles::FieldId(field),
                    n(at(2)?)?,
                )? {
                    Some(value) => println!(
                        "field={} value={} prefix={}/{} source={} priority={} expires={} policy={}",
                        field,
                        String::from_utf8_lossy(value.value),
                        ip(value.prefix.network()),
                        value.prefix.length(),
                        value.source,
                        value.priority,
                        value.expires_at,
                        value.policy_version
                    ),
                    None => println!("field={field} unknown"),
                }
            }
            println!("watermarks {:?}", view.watermarks);
        }
        "changes" => {
            let page = journal::page(&read, n(at(2)?)?, n(at(3)?)?, journal::Limits::default())?;
            for event in page.events {
                println!(
                    "origin={} seq={} recorded={} {:?}",
                    event.origin,
                    event.sequence,
                    event.recorded_at,
                    Command::decode(&event.payload)?
                );
            }
            println!("cursor {:?} more={}", page.cursor, page.more);
        }
        "expire" => {
            let plan = Regional::view(&read)?.rrsets.plan_expiry(n(at(2)?)?, 256)?;
            let mut write = env.begin_write()?;
            let report = rrsets::Rrsets::expire(&mut write, &plan)?;
            println!("expired {:?}; commit {}", report, write.commit()?);
        }
        _ => return Err(Error::InvalidInput(USAGE).into()),
    }
    Ok(())
}
fn main() {
    if let Err(error) = execute() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
