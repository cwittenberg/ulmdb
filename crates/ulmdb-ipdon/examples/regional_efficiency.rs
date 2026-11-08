//! Deterministic native-engine efficiency probe, without DNS or network traffic.
//!
//! Run with a NEW output directory; all database inputs and results are retained:
//! cargo run --release --locked -p ulmdb-ipdon --example regional_efficiency -- \
//!     target/regional-efficiency-run 1024
//!
//! A single synthetic run does not qualify production capacity or establish a
//! statistically significant speedup. Dirty arena bytes are not physical I/O.
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    io::Read as _,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use ulmdb::{Address, Environment, Error, Prefix, Result};
use ulmdb_ipdon::{
    Dnssec, EvidenceId, ResolverId, Role, ScopeId, SourceId, VantageId, journal, profiles,
    regional::{Command, Regional},
    rrsets,
};

const BASE_TIME: u64 = 1_800_000_000;
const REFRESH_TIME: u64 = BASE_TIME + 1_000;
const POLICY: u64 = 7;
const RRSET_ORIGIN: u64 = 1;
const CLAIM_ORIGIN: u64 = 2;
const BATCH: usize = 256;
const MEMBERS: usize = 4;

#[derive(Default)]
struct Metrics {
    name: &'static str,
    events: usize,
    payload_bytes: usize,
    encoded_bytes: usize,
    dirty_bytes: usize,
    transactions: usize,
    apply_ns: u128,
    commit_ns: u128,
    close_reopen_ns: u128,
    verify_ns: u128,
    added_members: usize,
    removed_members: usize,
    retained_members: usize,
    refreshes: usize,
    claim_changes: usize,
    logical_digest: String,
}

impl Metrics {
    fn json(&self) -> String {
        format!(
            concat!(
                "{{\"name\":{},\"events\":{},\"payload_bytes\":{},",
                "\"journal_encoded_bytes\":{},\"dirty_arena_bytes\":{},",
                "\"transactions\":{},\"apply_ns\":{},\"commit_ns\":{},\"close_reopen_ns\":{},",
                "\"verify_ns\":{},\"added_members\":{},\"removed_members\":{},",
                "\"retained_members\":{},\"refreshes\":{},\"claim_changes\":{},",
                "\"logical_digest\":{}}}"
            ),
            quoted(self.name),
            self.events,
            self.payload_bytes,
            self.encoded_bytes,
            self.dirty_bytes,
            self.transactions,
            self.apply_ns,
            self.commit_ns,
            self.close_reopen_ns,
            self.verify_ns,
            self.added_members,
            self.removed_members,
            self.retained_members,
            self.refreshes,
            self.claim_changes,
            quoted(&self.logical_digest),
        )
    }
}

fn quoted(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => write!(&mut out, "\\u{:04x}", u32::from(ch)).unwrap(),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    let mut out = String::with_capacity(bytes.as_ref().len() * 2);
    for byte in bytes.as_ref() {
        write!(&mut out, "{byte:02x}").unwrap();
    }
    out
}

fn prefix(index: usize) -> Prefix {
    if index.is_multiple_of(2) {
        Prefix::new(Address::V4(0x0a00_0000 + (index as u32) * 256), 24).unwrap()
    } else {
        Prefix::new(
            Address::V6(0x2001_0db8_0000_0000_0000_0000_0000_0000 + ((index as u128) << 64)),
            64,
        )
        .unwrap()
    }
}

fn member(index: usize, offset: usize) -> Address {
    match prefix(index).network() {
        Address::V4(value) => Address::V4(value + offset as u32),
        Address::V6(value) => Address::V6(value + offset as u128),
    }
}

fn rrset(index: usize) -> rrsets::Rrset {
    rrsets::Rrset {
        key: rrsets::Key {
            origin: RRSET_ORIGIN,
            source: SourceId(11),
            scope: ScopeId(1),
            resolver: ResolverId(1),
            vantage: VantageId(1),
            role: Role::Direct,
            hostname: format!("host-{index:08}.example.test"),
            kind: if index.is_multiple_of(2) {
                rrsets::RecordType::A
            } else {
                rrsets::RecordType::Aaaa
            },
        },
        revision: 1,
        addresses: (1..=MEMBERS).map(|offset| member(index, offset)).collect(),
        dnssec: Dnssec::Secure,
    }
}

fn fresh(time: u64) -> rrsets::Freshness {
    rrsets::Freshness {
        observed_at: time,
        recorded_at: time + 1,
        expires_at: time + 500,
        evidence: EvidenceId::from_bytes(&time.to_be_bytes()),
    }
}

fn claim(index: usize, field: u64) -> profiles::Claim {
    let mut key = (index as u64).to_be_bytes().to_vec();
    key.extend_from_slice(&field.to_be_bytes());
    profiles::Claim {
        id: profiles::ClaimId::from_origin_source(CLAIM_ORIGIN, 12, &key),
        generation: 1,
        field: profiles::FieldId(field),
        prefix: prefix(index),
        source: 12,
        value: match field {
            1 => b"NL".to_vec(),
            2 => (64_512 + index as u64).to_be_bytes().to_vec(),
            _ => unreachable!(),
        },
        priority: 100,
        observed_at: BASE_TIME,
        recorded_at: BASE_TIME + 1,
        expires_at: BASE_TIME + 500,
        evidence: EvidenceId::from_bytes(&key).0,
    }
}

fn event_digest(events: &[journal::Event]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"regional-efficiency/input/v1\0");
    for event in events {
        digest.update(event.origin.to_be_bytes());
        digest.update(event.sequence.to_be_bytes());
        digest.update(event.recorded_at.to_be_bytes());
        digest.update((event.payload.len() as u64).to_be_bytes());
        digest.update(&event.payload);
    }
    hex(digest.finalize())
}

fn seeds(count: usize) -> Result<Vec<journal::Event>> {
    let mut events = Vec::with_capacity(count * 3);
    for index in 0..count {
        events.push(
            Command::ReplaceRrset {
                set: rrset(index),
                freshness: fresh(BASE_TIME),
            }
            .event(RRSET_ORIGIN, index as u64 + 1, BASE_TIME + 1)?,
        );
    }
    for index in 0..count {
        for field in 1..=2 {
            events.push(Command::PutClaim(claim(index, field)).event(
                CLAIM_ORIGIN,
                index as u64 * 2 + field,
                BASE_TIME + 1,
            )?);
        }
    }
    Ok(events)
}

fn apply(env: &Environment, events: &[journal::Event], metrics: &mut Metrics) -> Result<()> {
    for batch in events.chunks(BATCH) {
        let started = Instant::now();
        let mut write = env.begin_write()?;
        let report = Regional::apply_page(&mut write, batch, journal::Limits::default())?;
        metrics.apply_ns += started.elapsed().as_nanos();
        metrics.dirty_bytes += write.dirty_bytes();
        let started = Instant::now();
        write.commit()?;
        metrics.commit_ns += started.elapsed().as_nanos();
        metrics.transactions += 1;
        metrics.events += batch.len();
        metrics.payload_bytes += batch.iter().map(|event| event.payload.len()).sum::<usize>();
        metrics.encoded_bytes += report.journal.encoded_bytes;
        metrics.added_members += report.work.added_members;
        metrics.removed_members += report.work.removed_members;
        metrics.retained_members += report.work.retained_members;
        metrics.refreshes += report.work.refreshes;
        metrics.claim_changes += report.work.claim_changes;
        assert_eq!(report.journal.applied, batch.len());
        assert_eq!(report.journal.replayed, 0);
    }
    Ok(())
}

fn initialized(base: &Path, name: &str, seed: &[journal::Event]) -> Result<Environment> {
    let env = Environment::create(base.join(name))?;
    assert_eq!(env.publication_mode(), ulmdb::PublicationMode::Legacy);
    let mut write = env.begin_write()?;
    Regional::initialize(&mut write, POLICY)?;
    write.commit()?;
    apply(&env, seed, &mut Metrics::default())?;
    let read = env.begin_read();
    let mut actual = Vec::with_capacity(seed.len());
    for origin in [RRSET_ORIGIN, CLAIM_ORIGIN] {
        let mut after = 0;
        while after < journal::cursor(&read, origin)?.applied {
            let page = journal::page(&read, origin, after, journal::Limits::default())?;
            after = page.events.last().unwrap().sequence;
            actual.extend(page.events);
        }
    }
    assert_eq!(
        actual, seed,
        "every comparison must start from identical stored events"
    );
    Ok(env)
}

/// Current-state semantic checksum intentionally excludes immutable baseline
/// revision identities and native commit identities. New-revision replacements
/// and compact refreshes can provide identical effective values/freshness while
/// retaining different correct revision identities. Exact revision equality is
/// asserted separately for the same-revision RRset retransmission comparison.
fn verify_rrsets(
    env: &Environment,
    count: usize,
    expected_revision: u64,
    changed: bool,
    time: u64,
    expected_sequence: u64,
) -> Result<(String, u128)> {
    let started = Instant::now();
    let read = env.begin_read();
    let view = Regional::view(&read)?;
    let watermark = view.watermarks[&RRSET_ORIGIN];
    assert_eq!(watermark.applied, expected_sequence);
    assert_eq!(watermark.serving, expected_sequence);
    let mut digest = Sha256::new();
    digest.update(b"regional-efficiency/current-rrsets/v1\0");
    for index in 0..count {
        let mut expected = rrset(index);
        expected.revision = expected_revision;
        if changed {
            expected.addresses[MEMBERS - 1] = member(index, 9);
        }
        let (actual, state) = view.rrsets.rrset(expected.key.id()?)?.unwrap();
        assert_eq!(actual, expected);
        assert_eq!(state.status, rrsets::Status::Active);
        assert_eq!(state.freshness, fresh(time));
        let query = rrsets::Query::Hostname {
            scope: ScopeId(1),
            role: Role::Direct,
            hostname: expected.key.hostname.clone(),
        };
        let rows = view
            .rrsets
            .query(&query, time + 2, None, rrsets::Limits::default())?;
        assert_eq!(rows.relations.len(), MEMBERS);
        assert!(rows.resume.is_none());
        assert_eq!(
            rows.relations
                .iter()
                .map(|row| row.address)
                .collect::<Vec<_>>(),
            expected.addresses
        );
        digest.update((expected.key.hostname.len() as u64).to_be_bytes());
        digest.update(expected.key.hostname.as_bytes());
        for row in rows.relations {
            assert_eq!(row.freshness, fresh(time));
            assert_eq!(row.revision, expected_revision);
            digest.update(address_bytes(row.address));
            digest.update(row.origin.to_be_bytes());
            digest.update(row.source.0.to_be_bytes());
        }
        let freshness = state.freshness;
        for n in [
            freshness.observed_at,
            freshness.recorded_at,
            freshness.expires_at,
        ] {
            digest.update(n.to_be_bytes());
        }
        digest.update(freshness.evidence.0);
    }
    Ok((hex(digest.finalize()), started.elapsed().as_nanos()))
}

fn address_bytes(address: Address) -> [u8; 17] {
    let mut bytes = [0; 17];
    match address {
        Address::V4(value) => {
            bytes[0] = 4;
            bytes[1..5].copy_from_slice(&value.to_be_bytes());
        }
        Address::V6(value) => {
            bytes[0] = 6;
            bytes[1..].copy_from_slice(&value.to_be_bytes());
        }
    }
    bytes
}

fn rrset_case(
    base: &Path,
    name: &'static str,
    count: usize,
    seed: &[journal::Event],
    mode: u8,
) -> Result<Metrics> {
    let env = initialized(base, name, seed)?;
    let mut events = Vec::with_capacity(count);
    for index in 0..count {
        let mut set = rrset(index);
        if mode >= 2 {
            set.revision = 2;
        }
        if mode == 3 {
            set.addresses[MEMBERS - 1] = member(index, 9);
        }
        let command = if mode == 0 {
            Command::RefreshRrset {
                id: set.key.id()?,
                revision: 1,
                freshness: fresh(REFRESH_TIME),
            }
        } else {
            Command::ReplaceRrset {
                set,
                freshness: fresh(REFRESH_TIME),
            }
        };
        events.push(command.event(
            RRSET_ORIGIN,
            count as u64 + index as u64 + 1,
            REFRESH_TIME + 1,
        )?);
    }
    let mut metrics = Metrics {
        name,
        ..Metrics::default()
    };
    apply(&env, &events, &mut metrics)?;
    let started = Instant::now();
    drop(env);
    let env = Environment::open(base.join(name))?;
    metrics.close_reopen_ns = started.elapsed().as_nanos();
    let (digest, verify_ns) = verify_rrsets(
        &env,
        count,
        if mode >= 2 { 2 } else { 1 },
        mode == 3,
        REFRESH_TIME,
        count as u64 * 2,
    )?;
    metrics.logical_digest = digest;
    metrics.verify_ns = verify_ns;
    if mode == 3 {
        assert_eq!(metrics.added_members, count);
        assert_eq!(metrics.removed_members, count);
        assert_eq!(metrics.retained_members, count * (MEMBERS - 1));
    } else {
        assert_eq!(metrics.added_members, 0);
        assert_eq!(metrics.removed_members, 0);
    }
    Ok(metrics)
}

fn expiry_case(base: &Path, count: usize, seed: &[journal::Event]) -> Result<Metrics> {
    let env = initialized(base, "rrset_expiry", seed)?;
    let mut metrics = Metrics {
        name: "rrset_expiry",
        ..Metrics::default()
    };
    loop {
        let started = Instant::now();
        let read = env.begin_read();
        let view = rrsets::Rrsets::view(&read)?;
        let plan = view.plan_expiry_with_budget(REFRESH_TIME, 128, 512)?;
        if plan.is_empty() {
            break;
        }
        assert!(plan.len() <= 128);
        let mut write = env.begin_write()?;
        let delta = rrsets::Rrsets::expire(&mut write, &plan)?;
        assert!(delta.removed <= 512);
        metrics.apply_ns += started.elapsed().as_nanos();
        metrics.dirty_bytes += write.dirty_bytes();
        let started = Instant::now();
        write.commit()?;
        metrics.commit_ns += started.elapsed().as_nanos();
        metrics.transactions += 1;
        metrics.removed_members += delta.removed;
    }
    assert_eq!(metrics.removed_members, count * MEMBERS);
    let started = Instant::now();
    drop(env);
    let env = Environment::open(base.join("rrset_expiry"))?;
    metrics.close_reopen_ns = started.elapsed().as_nanos();
    let started = Instant::now();
    let read = env.begin_read();
    let view = Regional::view(&read)?;
    assert_eq!(view.watermarks[&RRSET_ORIGIN].applied, count as u64);
    assert_eq!(view.watermarks[&RRSET_ORIGIN].serving, count as u64);
    let mut digest = Sha256::new();
    for index in 0..count {
        let set = rrset(index);
        let (content, state) = view.rrsets.rrset(set.key.id()?)?.unwrap();
        assert_eq!(content, set);
        assert_eq!(state.status, rrsets::Status::Expired);
        let rows = view.rrsets.query(
            &rrsets::Query::Hostname {
                scope: ScopeId(1),
                role: Role::Direct,
                hostname: set.key.hostname.clone(),
            },
            REFRESH_TIME,
            None,
            rrsets::Limits::default(),
        )?;
        assert!(rows.relations.is_empty());
        assert!(rows.resume.is_none());
        digest.update(set.key.id()?.0);
    }
    metrics.logical_digest = hex(digest.finalize());
    metrics.verify_ns = started.elapsed().as_nanos();
    Ok(metrics)
}

fn profile_case(
    base: &Path,
    name: &'static str,
    count: usize,
    seed: &[journal::Event],
    refresh: bool,
) -> Result<(Metrics, String)> {
    let env = initialized(base, name, seed)?;
    let mut events = Vec::with_capacity(count * 2);
    for index in 0..count {
        for field in 1..=2 {
            let mut original = claim(index, field);
            let revision = original.revision()?;
            let command = if refresh {
                Command::RefreshClaim {
                    id: original.id,
                    revision,
                    freshness: profiles::Freshness {
                        observed_at: REFRESH_TIME,
                        recorded_at: REFRESH_TIME + 1,
                        expires_at: REFRESH_TIME + 500,
                    },
                }
            } else {
                original.generation += 1;
                original.observed_at = REFRESH_TIME;
                original.recorded_at = REFRESH_TIME + 1;
                original.expires_at = REFRESH_TIME + 500;
                Command::PutClaim(original)
            };
            events.push(command.event(
                CLAIM_ORIGIN,
                count as u64 * 2 + index as u64 * 2 + field,
                REFRESH_TIME + 1,
            )?);
        }
    }
    let mut metrics = Metrics {
        name,
        ..Metrics::default()
    };
    apply(&env, &events, &mut metrics)?;
    let started = Instant::now();
    drop(env);
    let env = Environment::open(base.join(name))?;
    metrics.close_reopen_ns = started.elapsed().as_nanos();
    let started = Instant::now();
    let read = env.begin_read();
    let view = Regional::view(&read)?;
    assert_eq!(view.watermarks[&CLAIM_ORIGIN].applied, count as u64 * 4);
    assert_eq!(view.watermarks[&CLAIM_ORIGIN].serving, count as u64 * 4);
    let mut digest = Sha256::new();
    for index in 0..count {
        for field in 1..=2 {
            let mut expected = claim(index, field);
            if !refresh {
                expected.generation += 1;
                expected.observed_at = REFRESH_TIME;
                expected.recorded_at = REFRESH_TIME + 1;
                expected.expires_at = REFRESH_TIME + 500;
            }
            let selected = view
                .profiles
                .lookup(member(index, 1), profiles::FieldId(field), REFRESH_TIME + 2)?
                .unwrap();
            assert_eq!(selected.claim_id, expected.id);
            assert_eq!(selected.value, expected.value);
            assert_eq!(selected.source, expected.source);
            assert_eq!(selected.evidence, expected.evidence);
            assert_eq!(selected.policy_version, POLICY);
            assert_eq!(selected.observed_at, REFRESH_TIME);
            assert_eq!(selected.recorded_at, REFRESH_TIME + 1);
            assert_eq!(selected.expires_at, REFRESH_TIME + 500);
            assert_eq!(selected.revision, expected.revision()?);
            digest.update(selected.claim_id.0);
            digest.update((selected.value.len() as u64).to_be_bytes());
            digest.update(selected.value);
            digest.update(selected.source.to_be_bytes());
            digest.update(selected.evidence);
            for time in [
                selected.observed_at,
                selected.recorded_at,
                selected.expires_at,
            ] {
                digest.update(time.to_be_bytes());
            }
        }
    }
    metrics.logical_digest = hex(digest.finalize());
    metrics.verify_ns = started.elapsed().as_nanos();
    let batch_json = batch_lookup(&view.profiles, count)?;
    Ok((metrics, batch_json))
}

fn batch_lookup(view: &profiles::View<'_>, count: usize) -> Result<String> {
    let addresses: Vec<_> = (0..count).flat_map(|index| [member(index, 1); 2]).collect();
    let fields = [profiles::FieldId(1), profiles::FieldId(2)];
    let limits = profiles::BatchLimits {
        max_results: count * 4,
    };
    let started = Instant::now();
    let rows = view.batch(&addresses, &fields, REFRESH_TIME + 2, limits)?;
    let first_batch_ns = started.elapsed().as_nanos();
    assert_eq!(rows.len(), count * 2);
    for (position, row) in rows.iter().enumerate() {
        let index = position / 2;
        assert_eq!(row.address, member(index, 1));
        assert_eq!(row.fields[0].unwrap().value, b"NL");
        assert_eq!(
            row.fields[1].unwrap().value,
            (64_512 + index as u64).to_be_bytes()
        );
    }
    let repetitions = 8;
    let started = Instant::now();
    for _ in 0..repetitions {
        std::hint::black_box(view.batch(&addresses, &fields, REFRESH_TIME + 2, limits)?);
    }
    let repeated_batch_ns = started.elapsed().as_nanos();
    Ok(format!(
        "{{\"requested_addresses\":{},\"unique_addresses\":{},\"requested_fields\":2,\"selected_results\":{},\"first_batch_ns\":{},\"warm_repetitions\":{},\"warm_total_ns\":{}}}",
        addresses.len(),
        count,
        count * 4,
        first_batch_ns,
        repetitions,
        repeated_batch_ns
    ))
}

fn execute() -> Result<()> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.is_empty() || arguments.len() > 2 {
        return Err(Error::InvalidInput(
            "usage: regional_efficiency NEW_DIRECTORY [SETS=1024]",
        ));
    }
    let base = PathBuf::from(&arguments[0]);
    let count = arguments.get(1).map_or(Ok(1024), |value| {
        value
            .to_str()
            .and_then(|s| s.parse::<usize>().ok())
            .ok_or(Error::InvalidInput("invalid RRset count"))
    })?;
    if count == 0 || count > 4096 {
        return Err(Error::InvalidInput("RRset count must be 1..4096"));
    }
    std::fs::create_dir(&base)?;
    let seed = seeds(count)?;
    let seed_digest = event_digest(&seed);
    let mut cases = Vec::new();
    for (name, mode) in [
        ("rrset_compact_refresh", 0),
        ("rrset_full_same_revision", 1),
        ("rrset_new_revision_same_members", 2),
        ("rrset_one_member_change", 3),
    ] {
        cases.push(rrset_case(&base, name, count, &seed, mode)?);
    }
    assert_eq!(cases[0].logical_digest, cases[1].logical_digest);
    assert_eq!(cases[0].logical_digest, cases[2].logical_digest);
    cases.push(expiry_case(&base, count, &seed)?);
    let (compact_claim, compact_batch) =
        profile_case(&base, "profile_compact_refresh", count, &seed, true)?;
    let (full_claim, full_batch) =
        profile_case(&base, "profile_full_same_value", count, &seed, false)?;
    assert_eq!(compact_claim.logical_digest, full_claim.logical_digest);
    cases.push(compact_claim);
    cases.push(full_claim);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::InvalidInput("system clock precedes epoch"))?
        .as_secs();
    let mut executable = std::fs::File::open(std::env::current_exe()?)?;
    let mut executable_digest = Sha256::new();
    let mut buffer = [0; 65_536];
    loop {
        let read = executable.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        executable_digest.update(&buffer[..read]);
    }
    let executable_digest = hex(executable_digest.finalize());
    let report = format!(
        concat!(
            "{{\n\"format\":\"ipdon-regional-efficiency-v1\",\n",
            "\"measured_at_unix_seconds\":{},\n\"output_directory\":{},\n",
            "\"os\":{},\n\"arch\":{},\n\"build_profile\":{},\n",
            "\"executable_sha256\":{},\n",
            "\"backend\":\"native-public-default-Legacy\",\n",
            "\"durability\":\"ordinary native durable commits on caller-selected filesystem\",\n",
            "\"qualification\":\"single-process synthetic run; no DNS or WAN; no production capacity or statistical speedup claim\",\n",
            "\"dirty_bytes_definition\":\"sum of writer-private arena bytes before commits; excludes commit-time catalog/publication work and is not measured physical I/O\",\n",
            "\"timing_scope\":\"seed setup excluded; apply includes begin/validation; commit and close/reopen separate; verification includes assertions/hash; batch inputs warmed by verification\",\n",
            "\"comparison_semantics\":\"same initial event bytes in independent directories; final checksum includes selected values/provenance/freshness and excludes immutable baseline revisions; same-revision RRset equality is separately asserted\",\n",
            "\"sets\":{},\n\"members_per_set\":{},\n\"ipv4_sets\":{},\n\"ipv6_sets\":{},\n",
            "\"profile_claims\":{},\n\"write_batch_events\":{},\n",
            "\"seed_events\":{},\n\"seed_input_sha256\":{},\n",
            "\"cases\":[\n{}\n],\n",
            "\"batch_lookup_after_compact_profile_refresh\":{},\n",
            "\"batch_lookup_after_full_profile_replacement\":{}\n}}\n"
        ),
        timestamp,
        quoted(&base.to_string_lossy()),
        quoted(std::env::consts::OS),
        quoted(std::env::consts::ARCH),
        quoted(if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }),
        quoted(&executable_digest),
        count,
        MEMBERS,
        count.div_ceil(2),
        count / 2,
        count * 2,
        BATCH,
        seed.len(),
        quoted(&seed_digest),
        cases
            .iter()
            .map(Metrics::json)
            .collect::<Vec<_>>()
            .join(",\n"),
        compact_batch,
        full_batch,
    );
    std::fs::write(base.join("report.json"), &report)?;
    print!("{report}");
    Ok(())
}

fn main() {
    if let Err(error) = execute() {
        eprintln!("regional efficiency probe failed: {error}");
        std::process::exit(1);
    }
}
