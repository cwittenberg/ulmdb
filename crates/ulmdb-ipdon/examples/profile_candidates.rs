//! Deterministic native candidate enumeration probe; no DNS or WAN traffic.
//!
//! cargo run --release --locked -p ulmdb-ipdon --example profile_candidates -- \
//!     target/profile-candidates-new-run [claims-per-prefix]
//!
//! Both the output directory and its database must initially be absent. The
//! report checks known fixture results and bounded work, not statistical speed.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use ulmdb::{Environment, Error, Result};
use ulmdb_ipdon::{Address, Prefix, profiles::*};

const NOW: u64 = 30;
const POLICY: u64 = 17;
const V4: Address = Address::V4(0xc000_0201);
const V6: Address = Address::V6(0x2001_0db8_0000_0000_0000_0000_0000_0001);

fn quoted(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            value if value.is_control() => write!(&mut output, "\\u{:04x}", value as u32).unwrap(),
            value => output.push(value),
        }
    }
    output.push('"');
    output
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    let mut output = String::new();
    for byte in bytes.as_ref() {
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}

fn file_hash(path: &Path) -> Result<String> {
    let mut input = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 131_072];
    loop {
        let length = input.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        hash.update(&buffer[..length]);
    }
    Ok(hex(hash.finalize()))
}

fn fixtures(per_prefix: usize) -> Vec<Claim> {
    let mut claims = Vec::new();
    for (family, address, lengths) in [(0, V4, [0, 16, 24, 32]), (1, V6, [0, 32, 64, 128])] {
        for (group, length) in lengths.into_iter().enumerate() {
            for offset in 0..per_prefix {
                let source = (family * 10_000 + group * per_prefix + offset + 1) as u64;
                let future = offset % 8 == 3;
                let observed_at = if future { 40 } else { 10 };
                claims.push(Claim {
                    id: ClaimId::from_origin_source(family as u64 + 1, source, b"country"),
                    generation: 1,
                    field: FieldId(1),
                    prefix: Prefix::normalize(address, length).unwrap(),
                    source,
                    value: if offset.is_multiple_of(2) {
                        b"NL"
                    } else {
                        b"DE"
                    }
                    .to_vec(),
                    priority: 100 - (offset % 3) as u32,
                    observed_at,
                    recorded_at: observed_at + 1,
                    expires_at: if offset % 8 < 3 { 20 } else { 100 },
                    evidence: Sha256::digest(source.to_be_bytes()).into(),
                });
            }
        }
        let source = family as u64 + 100_001;
        claims.push(Claim {
            id: ClaimId::from_origin_source(family as u64 + 1, source, b"vpn"),
            generation: 1,
            field: FieldId(2),
            prefix: Prefix::normalize(address, lengths[1]).unwrap(),
            source,
            value: b"false".to_vec(),
            priority: 100,
            observed_at: 10,
            recorded_at: 11,
            expires_at: 100,
            evidence: Sha256::digest(source.to_be_bytes()).into(),
        });
    }
    claims
}

fn selected_hash<'a>(candidates: impl IntoIterator<Item = Selected<'a>>) -> String {
    let mut hash = Sha256::new();
    for candidate in candidates {
        hash.update(candidate.claim_id.0);
        hash.update(candidate.revision.0);
        hash.update(candidate.value);
    }
    hex(hash.finalize())
}

struct QueryProbe {
    scanned: usize,
    returned: usize,
    pages: usize,
    max_scanned: usize,
    max_returned: usize,
    expired: usize,
    future: usize,
    elapsed_ns: u128,
    digest: String,
}

impl QueryProbe {
    fn json(&self, family: &str) -> String {
        format!(
            concat!(
                "{{\"family\":{},\"scanned\":{},\"returned\":{},\"pages\":{},",
                "\"max_page_scanned\":{},\"max_page_returned\":{},",
                "\"expired_examined\":{},\"future_examined\":{},",
                "\"elapsed_ns\":{},\"digest\":{}}}"
            ),
            quoted(family),
            self.scanned,
            self.returned,
            self.pages,
            self.max_scanned,
            self.max_returned,
            self.expired,
            self.future,
            self.elapsed_ns,
            quoted(&self.digest)
        )
    }
}

fn probe(view: &View<'_>, address: Address, claims: &[Claim]) -> Result<QueryProbe> {
    let relevant = claims
        .iter()
        .filter(|claim| claim.field == FieldId(1) && claim.prefix.contains_address(address))
        .collect::<Vec<_>>();
    let mut expected = relevant
        .iter()
        .copied()
        .filter(|claim| claim.observed_at <= NOW && claim.expires_at > NOW)
        .collect::<Vec<_>>();
    expected.sort_by_key(|claim| {
        (
            std::cmp::Reverse(claim.prefix.length()),
            std::cmp::Reverse(claim.priority),
            claim.source,
            claim.id,
        )
    });
    let expected_ids = expected.iter().map(|claim| claim.id).collect::<Vec<_>>();
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    let mut after = None;
    let mut report = QueryProbe {
        scanned: 0,
        returned: 0,
        pages: 0,
        max_scanned: 0,
        max_returned: 0,
        expired: relevant
            .iter()
            .filter(|claim| claim.expires_at <= NOW)
            .count(),
        future: relevant
            .iter()
            .filter(|claim| claim.observed_at > NOW)
            .count(),
        elapsed_ns: 0,
        digest: String::new(),
    };
    let start = Instant::now();
    loop {
        let page = view.covering_candidates(
            address,
            FieldId(1),
            NOW,
            after.as_ref(),
            CandidateLimits {
                max_results: 3,
                max_scan: 5,
            },
        )?;
        assert!(page.scanned <= 5 && page.candidates.len() <= 3);
        report.pages += 1;
        report.scanned += page.scanned;
        report.returned += page.candidates.len();
        report.max_scanned = report.max_scanned.max(page.scanned);
        report.max_returned = report.max_returned.max(page.candidates.len());
        for candidate in &page.candidates {
            assert!(
                seen.insert(candidate.claim_id),
                "continuation repeated a candidate"
            );
            assert_eq!(candidate.policy_version, POLICY);
            ids.push(candidate.claim_id);
        }
        candidates.extend(page.candidates);
        after = page.resume;
        if page.complete {
            assert!(after.is_none());
            break;
        }
        assert!(after.is_some());
    }
    report.elapsed_ns = start.elapsed().as_nanos();
    assert_eq!(
        report.scanned,
        relevant.len(),
        "expired/future candidates must be charged exactly once"
    );
    assert_eq!(
        ids, expected_ids,
        "all live contradictory evidence must survive paging"
    );
    assert!(candidates.iter().any(|candidate| candidate.value == b"NL"));
    assert!(candidates.iter().any(|candidate| candidate.value == b"DE"));
    report.digest = selected_hash(candidates.iter().copied());
    let full = view.covering_candidates(
        address,
        FieldId(1),
        NOW,
        None,
        CandidateLimits {
            max_results: 65_536,
            max_scan: 65_536,
        },
    )?;
    assert!(full.complete && full.resume.is_none());
    assert_eq!(full.scanned, report.scanned);
    assert_eq!(selected_hash(full.candidates), report.digest);
    Ok(report)
}

fn execute() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let directory = PathBuf::from(arguments.next().ok_or(Error::InvalidInput(
        "usage: profile_candidates NEW_DIRECTORY [8..256 claims-per-prefix]",
    ))?);
    let per_prefix = arguments
        .next()
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| Error::InvalidInput("invalid claims-per-prefix"))?
        .unwrap_or(64);
    if !(8..=256).contains(&per_prefix) || arguments.next().is_some() {
        return Err(Error::InvalidInput("claims-per-prefix must be 8..256"));
    }
    fs::create_dir(&directory)?;
    let environment = Environment::create(directory.join("database"))?;
    let claims = fixtures(per_prefix);
    let mut write = environment.begin_write()?;
    Profiles::initialize(&mut write, POLICY)?;
    for claim in &claims {
        Profiles::upsert(&mut write, claim)?;
    }
    write.commit()?;
    // Reopening exercises persisted candidate buckets rather than writer views.
    drop(environment);
    let environment = Environment::open(directory.join("database"))?;
    let read = environment.begin_read();
    let view = Profiles::view(&read)?;
    let empty_stale = view.covering_candidates(
        V4,
        FieldId(1),
        NOW,
        None,
        CandidateLimits {
            max_results: 65_536,
            max_scan: 1,
        },
    )?;
    assert_eq!(empty_stale.scanned, 1);
    assert!(empty_stale.candidates.is_empty() && !empty_stale.complete);
    assert_eq!(
        empty_stale.candidates.capacity(),
        0,
        "empty stale pages must not multiply unused aggregate result capacity"
    );
    let ipv4 = probe(&view, V4, &claims)?;
    let ipv6 = probe(&view, V6, &claims)?;
    let addresses = [V6, V4, V6, V4];
    let fields = [FieldId(1), FieldId(2), FieldId(3), FieldId(1)];
    let batch_start = Instant::now();
    let batch = view.batch_candidates(
        &addresses,
        &fields,
        NOW,
        CandidateBatchLimits {
            per_query: CandidateLimits {
                max_results: 65_536,
                max_scan: 65_536,
            },
            max_queries: 16,
            max_results: 8192,
            max_scan: 65_536,
        },
    )?;
    let batch_elapsed_ns = batch_start.elapsed().as_nanos();
    assert!(batch.complete);
    assert_eq!(batch.rows[0], batch.rows[2]);
    assert_eq!(batch.rows[1], batch.rows[3]);
    assert_eq!(
        batch.scanned,
        ipv4.scanned + ipv6.scanned + 2,
        "duplicate addresses/fields must not be rescanned"
    );
    for (row, address) in batch.rows.iter().zip(addresses) {
        assert_eq!(row.address, address);
        assert_eq!(row.fields[0], row.fields[3]);
        assert_eq!(row.fields[1].candidates.len(), 1);
        assert_eq!(row.fields[1].candidates[0].value, b"false");
        assert!(row.fields[2].complete && row.fields[2].candidates.is_empty());
    }
    let logical_results = batch
        .rows
        .iter()
        .flat_map(|row| &row.fields)
        .map(|page| page.candidates.len())
        .sum::<usize>();
    let bounded = view.batch_candidates(
        &addresses,
        &fields,
        NOW,
        CandidateBatchLimits {
            per_query: CandidateLimits {
                max_results: 8,
                max_scan: 5,
            },
            max_queries: 16,
            max_results: 5,
            max_scan: 7,
        },
    )?;
    let bounded_results = bounded
        .rows
        .iter()
        .flat_map(|row| &row.fields)
        .map(|page| page.candidates.len())
        .sum::<usize>();
    let incomplete = bounded
        .rows
        .iter()
        .flat_map(|row| &row.fields)
        .filter(|page| !page.complete)
        .count();
    assert!(!bounded.complete && incomplete > 0 && bounded_results <= 5 && bounded.scanned <= 7);
    assert!(
        bounded
            .rows
            .iter()
            .flat_map(|row| &row.fields)
            .all(|page| page.complete == page.resume.is_none())
    );
    let executable = std::env::current_exe()?;
    let report = format!(
        concat!(
            "{{\n\"format\":\"ipdon-profile-candidates-efficiency-v1\",\n",
            "\"qualification\":\"single-process synthetic native fixture; counters and bounds, no DNS/WAN or statistical speed claim\",\n",
            "\"backend\":\"native-public-default-Legacy\",\n",
            "\"measured_at_unix_seconds\":{},\n\"output_directory\":{},\n",
            "\"build_profile\":{},\n\"executable_sha256\":{},\n",
            "\"claims_per_prefix\":{},\n\"covering_prefixes_per_family\":4,\n\"total_stored_claims\":{},\n",
            "\"selected_result_size_bytes\":{},\n\"candidate_page_size_bytes\":{},\n",
            "\"empty_stale_query\":{{\"scanned\":{},\"returned\":{},\"allocated_result_capacity\":{}}},\n",
            "\"page_limits\":{{\"max_results\":3,\"max_scan\":5}},\n",
            "\"queries\":[{},{}],\n",
            "\"complete_batch\":{{\"requested_addresses\":4,\"unique_addresses\":2,\"requested_fields\":4,\"unique_fields\":3,\"query_slots\":16,\"distinct_queries\":6,\"scanned\":{},\"logical_returned_candidates\":{},\"elapsed_ns\":{},\"unknown_field_pages\":4,\"explicit_false_pages\":4}},\n",
            "\"aggregate_bounded_batch\":{{\"max_results\":5,\"max_scan\":7,\"returned\":{},\"scanned\":{},\"incomplete_pages\":{},\"all_truncation_has_continuation\":true}},\n",
            "\"resume_known_fixture_and_full_page_digests_match\":true,\n",
            "\"routing_work_bound\":\"at most 33 IPv4/129 IPv6 bucket headers per query page, separate from candidate record scan budget\"\n}}\n"
        ),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InvalidInput("system clock before epoch"))?
            .as_secs(),
        quoted(&directory.to_string_lossy()),
        quoted(if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }),
        quoted(&file_hash(&executable)?),
        per_prefix,
        claims.len(),
        std::mem::size_of::<Selected<'_>>(),
        std::mem::size_of::<CandidatePage<'_>>(),
        empty_stale.scanned,
        empty_stale.candidates.len(),
        empty_stale.candidates.capacity(),
        ipv4.json("IPv4"),
        ipv6.json("IPv6"),
        batch.scanned,
        logical_results,
        batch_elapsed_ns,
        bounded_results,
        bounded.scanned,
        incomplete
    );
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join("report.json"))?;
    output.write_all(report.as_bytes())?;
    output.sync_all()?;
    print!("{report}");
    Ok(())
}
fn main() {
    if let Err(error) = execute() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
