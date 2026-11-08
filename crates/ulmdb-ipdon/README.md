# IPDon regional database foundation

This package provides native Rust database APIs for collecting and serving IP
intelligence with ULMDB. The original `Projection` API remains available. The
new `regional`, `rrsets`, `profiles` and `journal` modules form an independent,
versioned foundation for regional collectors. They use the public native file
engine and do not change the LMDB C ABI or select the experimental Resident
backend.

## Features, purpose and implementation

| Feature | Why it exists | How it works |
| --- | --- | --- |
| Incremental RRset replacement | A small DNS change should require a small relationship update. | Canonical sorted address sets are merged; only added/removed memberships touch hostname/IP indexes. Retained members refer to a stable set ID. |
| Compact freshness refreshes | Reobserving unchanged data should avoid resending and rebuilding its content. | A refresh references the exact set revision or claim revision. Freshness is stored independently from stable content. Active RRset refreshes update freshness and the due-time index; field refreshes update one freshness record. |
| Correct expiry before housekeeping | An overloaded expiry worker must not make stale answers look current. | Queries filter the current snapshot at an explicit freshness cutoff. Due-time plans bound cleanup by both sets and memberships and revalidate before deleting edges. Expired-set revival restores memberships and consumes the same work budget. |
| Explainable field enrichment | ASN, country and privacy evidence have different prefix boundaries and trust. | Independent IPv4/IPv6 indexes hold bounded ranked candidates per field/prefix. Lookup compares live covering candidates by priority before specificity, with deterministic source/identity tie-breaking. Results retain source, prefix, evidence, generation and policy references. |
| Competing covering evidence | Applications need to see contradictions and independent observations. | Optional candidate pages return every live covering claim, charge expired/future examinations against scan limits and bind continuations to the snapshot/query/policy/time. Batch limits bound scans, repeated outputs and query slots. |
| Batch enrichment | Callers should amortize root binding and repeated addresses. | A batch binds requested field/family roots once, deduplicates addresses and returns borrowed values in original address/field order from one snapshot. |
| Atomic multi-origin change streams | Replicas must never advance a cursor without its evidence and projections. | A bounded page applies through one savepoint. Application changes, journal events and contiguous origin cursors commit together. Exact replay does no writes; conflicts and gaps fail the entire page. |
| Verified page transport | Equal cursor numbers do not establish shared history. | Public base/tail checkpoints, realm, versions and baseline identity are checked with native event hashes before callbacks. Typed errors distinguish forks, mismatched contexts and snapshot requirements. |
| Recovery and retention | Long partitions and history growth need explicit recovery rules. | Pinned native snapshots contain schema/policy versions and a complete cursor vector. Cumulative origin hashes detect divergent history. Pruning requires checkpoint coverage and caller-authenticated durable peer acknowledgements. Older consumers must install a snapshot. |
| Guarded regional handover | A peer checkpoint must preserve newer local observations and catch late writes. | All-origin rebase replays retained local tails into a new candidate. A handover guard fences both writers while validating versions, origin chains and nonregressing watermarks. The old environment remains intact. |
| Explicit new-realm baselines | Migration should work after old history has been pruned. | A verified application digest, versions, shared import time and complete cursor vector define a new namespace with deterministic anchors. The typed importer copies a pinned native snapshot and verifies actual application state before replacing only the candidate's journal. |

`None` is unknown. An explicit value such as `b"false"` remains a distinct claim.
Priority is an application policy input; this package does not turn it into a
calibrated confidence percentage or an independently validated classification.

## Run a native journey

From the repository root, with each destination directory initially absent:

```sh
cargo run --release --locked -p ulmdb-ipdon --bin ipdon-regional -- demo /tmp/ipdon-live
cargo run --release --locked -p ulmdb-ipdon --bin ipdon-regional -- hostname /tmp/ipdon-live 1700000070 example.test
cargo run --release --locked -p ulmdb-ipdon --bin ipdon-regional -- lookup /tmp/ipdon-live 1700000070 192.0.2.1
cargo run --release --locked -p ulmdb-ipdon --bin ipdon-regional -- changes /tmp/ipdon-live 1 0
cargo run --release --locked -p ulmdb-ipdon --bin ipdon-regional -- snapshot /tmp/ipdon-live /tmp/ipdon-bundle
cargo run --release --locked -p ulmdb-ipdon --bin ipdon-regional -- restore /tmp/ipdon-bundle /tmp/ipdon-restored
```

The demonstration uses frozen documentation addresses and an explicit fixture
time. It performs no DNS calls, packet scanning or external network requests.
Field IDs 1/2/3 in this fixture represent country, ASN and VPN classification;
applications define their own field schemas.

`ipdon-regional expire DIRECTORY TIME` performs one bounded cleanup pass.
`ipdon-regional recover BUNDLE LOCAL_DIRECTORY NEW_DIRECTORY` creates and validates
an all-origin recovered candidate. It preserves the local directory; selecting
that candidate as an active application database is a separate handover.

## Integrate a collector

Initialize all regions with the same configured, nonzero replication realm and
policy version:

```rust,no_run
use ulmdb::{Environment, Result};
use ulmdb_ipdon::{journal, regional::Regional};

fn initialize(path: &str) -> Result<Environment> {
    let env = Environment::create(path)?;
    let mut write = env.begin_write()?;
    Regional::initialize_in_realm(&mut write, 1, 0x4950444f4e)?;
    write.commit()?;
    Ok(env)
}
```

Construct a `regional::Command`, call `command.event(origin, sequence,
recorded_at)`, and submit its `journal::Event` through
`Regional::apply_page(&mut write, events, limits)`. Commit once per count/deadline
batch. Parsing and network I/O happen before acquiring the database writer.

Each origin has one externally fenced producer. Ordinary sequences begin at one;
imported origins continue above their explicit baseline anchors. Sequences never
reset within a realm. Origins are independent namespaces; arrival order cannot overwrite
another origin's RRsets or field claims. Use
`profiles::ClaimId::from_origin_source` for regional evidence. `from_source` is
appropriate for a globally assigned feed owner. Typed application checks reject
attempts to modify another origin's existing records.

An RRset's replacement boundary includes origin, source, scope, resolver,
vantage, role, normalized hostname and A/AAAA type. Revisions strictly increase
on content replacement, including after withdrawal. A `profiles::Claim` has a
nonzero, strictly increasing logical `generation`, so distinct changes within
the same timestamp second remain representable. Its `Revision` hashes the full
immutable baseline including that generation.

Freshness refreshes must reference the exact baseline revision. Their observation
and recording times may not regress; at least one advances for a changed refresh.
Explicit withdrawals fence later stale replacement or refresh. Timestamped field
withdrawals use `Profiles::withdraw_at`; the typed facade supplies the journal
event's recording time.

An empty RRset replaces that source's membership with zero addresses. The caller
must retain the distinction between validated NXDOMAIN, NODATA and transport
errors in its collection/evidence model; a timeout must not be emitted as an
empty replacement. The caller computes effective expiry from TTLs, negative
caching policy and CNAME dependencies. This library does not implement a DNS
resolver or infer CNAME lifetimes.

Use `Regional::view(&read)` for synchronous serving. Its RRset pages bind resume
tokens to the commit, query and freshness cutoff. One relationship represents
one attributable RRset membership; multiple independent sources may support the
same hostname/address. Paging bounds returned results and scanned stale entries.

The lower-level journal exposes `ApplyMode::EvidenceOnly` and `mark_serving` for
applications implementing asynchronous materialization. Applied and serving
watermarks stay distinct. `Regional::view` rejects such a backlog; its typed
facade deliberately serves only fully materialized synchronous snapshots.

## Inspect competing prefix evidence

`profiles::View::covering_candidates(address, field, now, after, limits)` returns
all live evidence, including contradictory equal-rank claims at the same prefix.
Order is most-specific covering prefix first, then the existing bucket's
priority/source/ID order. Existing `lookup` and `batch` winner semantics are
unchanged.

`CandidateLimits` bounds both returned claims and examined candidates. Expired
and future observations consume the scan budget. A page can be empty and
incomplete; always inspect `complete` and retain `resume` for continuation.
Tokens bind commit identity, generation/sequence, address, field, policy version
and freshness cutoff. Resumption skips previously examined candidates directly.
Prefix routing and bucket headers are separately bounded by the 33/129 possible
IPv4/IPv6 prefix lengths and the existing per-prefix bucket limit.

`batch_candidates` accepts `CandidateBatchLimits` with per-query limits and
aggregate query-slot, result and scan bounds. It binds each requested field/family
root once and deduplicates address/field work while preserving caller order.
Repeated output candidates count toward the aggregate result budget. Empty
complete fields mean unknown; `b"false"` remains a real candidate. Empty
incomplete fields mean the budget requires continuation. Result storage grows
with actual returned candidates; stale-only pages do not preallocate unused
result capacity.

The application still enforces source-global record ordering and performs
multi-source inference. If an IPDon source record moves outside a queried prefix,
the application must suppress its previous copy; covering enumeration alone
cannot establish that global fence.

## Verify journal transport before applying

`journal::checkpoint(read, origin, sequence)` exposes a cumulative chain position,
including an imported or pruned floor anchor. `journal::encoded_len(event)`
returns the exact persisted-record byte charge, including native digest/chain
overhead; adapters can stop pinning a private header-size constant.

`verified_page` returns events plus realm, current schema/policy, baseline
identity, source cursor, and checkpoints at the requested base and returned tail.
All fields come from one pinned read. Public fields can be encoded by the
service's authenticated transport.

Receive through `journal::apply_verified_page` or the typed
`Regional::apply_verified_page` facade. They validate byte/count bounds, context,
the destination's existing base and overlap hashes, and the complete returned
tail using the native encoder before application callbacks. Callback failures
roll back projections and cursors together. The typed facade also preserves its
membership-work budget; `apply_verified_page_with_budget` can lower that bound.
Exact and partial replay retain existing callback semantics.

`ProtocolError` provides explicit `SnapshotRequired`, `ForeignRealm`,
`VersionMismatch`, `Fork` and `BaselineMismatch` categories, plus malformed-page
and underlying-storage errors. Digests prove common history and integrity;
the service continues to authenticate peers and acknowledgements.

Snapshot receipts expose `realm()` and an allocation-free `checkpoints()`
iterator. Ordinary snapshots retain their original `IPDJNL01` manifest format.
Imported-baseline snapshots use `IPDJNL02`, binding full baseline provenance;
readers support both. Historical baseline versions remain distinct from later
explicitly migrated current schema/policy versions.

## Import an explicit new history baseline

`journal::Baseline` binds a distinct nonzero realm, verified application digest,
schema/policy versions, shared `imported_at` timestamp and complete origin cursor
vector. All peers must use the same shared manifest, including its timestamp.
Positive imported cursors become applied/serving/floor anchors; zero positions
remain implicit native genesis cursors but are still bound by baseline identity.
The next event continues at imported position plus one. Earlier history requires
a snapshot; native hashes for missing old events are never invented.

For the native typed model, use:

```rust,no_run
use ulmdb::{Environment, ReadTxn};
use ulmdb_ipdon::{journal, regional::Regional};

fn migrate(source: &Environment, read: &ReadTxn, destination: &str)
    -> journal::ProtocolResult<()> {
    let baseline = journal::Baseline {
        realm: 0x4950444f4e02,
        application_digest: Regional::application_digest(read)?,
        versions: journal::versions(read)?,
        imported_at: 1_793_900_000, // Shared cutover manifest time.
        origins: journal::cursors(read)?.into_iter()
            .map(|(origin, cursor)| (origin, cursor.applied)).collect(),
    };
    let (candidate, _receipt) =
        Regional::create_from_baseline(source, read, destination, &baseline)?;
    let guard = Regional::validate_baseline_handover(source, &candidate)?;
    // Switch the application's active handle while guard remains alive.
    drop(guard);
    Ok(())
}
```

The concrete importer hashes all owned typed application tables/indexes with
bounded-memory streaming, checks versions and the full source cursor vector,
copies the pinned native snapshot to a new destination, and verifies the copy.
Only that unactivated candidate's old journal is replaced; new realm, provenance,
anchors and serving watermarks commit together. The source can already have
pruned or snapshot-bootstrapped history. There is no full record/index reinsertion.

Normal same-realm handover deliberately rejects this namespace change. The
explicit baseline handover fences both writers and verifies actual source and
candidate state against the imported baseline. It rejects late observations,
unjournaled application changes, changed versions and post-baseline candidate
tails. Hash verification is linear in application size and can pause acquisition
during cutover; reads continue. Stop/reroute maintenance as well as ordinary
writers, because expiry cleanup also changes the exact materialized baseline.

Native copying also carries local-only tables. The service must rebind its local
origin identity, frontier leases and other regional metadata before activation.
Those tables are outside the typed application digest; this API does not assign
a collector identity or reroute queued users of an old handle.

For a different legacy application model, `journal::import_baseline` and
`create_from_baseline` require a mandatory `BaselineActivation` callback. That
callback installs and verifies actual application state and returns its computed
digest, versions and complete vector. Returning the expected descriptor without
verification is insufficient. A callback error or mismatch restores its entire
savepoint. Existing journals reject this importer even when empty. Generic
`validate_baseline_handover` likewise requires the service's model-specific
source/candidate verifier. The typed facade supplies its concrete verifier.

Failed staged imports leave the active source intact; a partial destination is
not automatically activated. Successful peers share deterministic anchors and
can exchange verified tails, export/restore snapshots, prune and rebase normally
within the new realm. Baseline identity also fences previously unseen origins.

## Recovery, handover and retention

1. `journal::export_snapshot(env, &read, bundle)` copies exactly one pinned
   native commit. The completion manifest is written and synchronized last.
   It binds the native identity, shared realm, versions and every origin cursor
   and cumulative head hash. An interrupted bundle without a valid manifest
   cannot be restored.
2. `verify_snapshot` validates the manifest and native storage. `restore_snapshot`
   creates a new environment; existing destinations are rejected.
3. For an existing region, prefer `rebase_all_snapshot`. It verifies matching
   realms/versions/baselines, checks shared origin history, restores the peer checkpoint
   and replays every missing retained local-origin tail. A missing tail fails
   explicitly. `rebase_snapshot` preserves only the one designated origin;
   other origins may require catch-up before serving.
4. Before activation, stop/reroute queued users of the old writer and obtain
   `validate_full_handover`. Keep its guard alive while switching the active
   handle. Both writer fences remain held; readers continue using snapshots.
   The guard rejects late source commits, forks and regressing watermarks.
   Stopping old-handle producers remains the application's responsibility.
5. Keep a verified recovery bundle available, collect distinct durable peer
   acknowledgements, then call bounded `journal::prune`. A cursor below its
   retained floor gets an explicit snapshot-required error. Checkpoint data and
   retained floor hashes preserve withdrawal and replay ordering.

Recovery replay limits bound bytes and event count. For typed events, eight
worst-case RRset replacements per replay transaction cap membership work at
65,536. The CLI uses this conservative count; tune against the actual profile.
Normal `Regional::apply_page_with_budget` additionally enforces an explicit
membership-work budget and rolls back pages that exceed it.

Digests and origin chains prove integrity and lineage, not sender authority.
Authenticate event transport, snapshot distribution and acknowledgement sources
in the regional service. Acknowledgement records are explicit application inputs.
Choose a nonzero peer requirement for the intended durable-copy policy. The
pruner does not guarantee that a caller will keep its recovery bundle afterward.

Logical journal pruning does not itself reduce native allocated files. Physical
compaction, inventory pruning, reclamation, disk headroom and live-reader limits
remain the selected engine's maintenance policy. Automatic bounded native
maintenance is still part of the separate LMDB parity backlog. Avoid retaining
live read transactions for unbounded historical investigations.

Failed export/rebase can leave an incomplete bundle or unactivated candidate.
The live source remains unchanged; verify or remove only the failed operation's
own destination. A rebase copies a captured source snapshot; the final handover
guard is required to detect writes that arrived afterward.

## History and application boundaries

Events retain recording time, observation time, content incarnation and explicit
withdrawals. Hot retained journal pages and pinned checkpoints can be exported
or replayed independently. `at`/`now` query arguments evaluate freshness within
the current snapshot; they are not arbitrary bitemporal time-travel queries.
Compressed long-term archives, historical query indexing and object-store
retention belong to the separate history service.

This package supplies the database foundation. DNS/CT/BGP/RIR acquisition,
classification models, geolocation validation, source rights policy, customer
authentication, network replication daemons and deployment automation remain
application integrations. No 17-region service deployment is implied by local
tests. The legacy `Projection` API keeps its original semantics and tables;
applications opt into `Regional` explicitly. Existing data is not silently
migrated into the new regional model.

## Verification and efficiency evidence

```sh
cargo test --workspace --locked
cargo test --locked -p ulmdb-ipdon --features fault-injection --test regional_faults
cargo test --locked -p ulmdb-ipdon --features fault-injection --test baseline_faults
cargo clippy --locked -p ulmdb-ipdon --all-targets --no-deps -- -D warnings
cargo run --release --locked -p ulmdb-ipdon --example regional_efficiency -- target/ipdon-efficiency-new-run 1024
cargo run --release --locked -p ulmdb-ipdon --example profile_candidates -- target/ipdon-candidates-new-run
```

Tests cover exact replay, malformed/gapped/conflicting input, callback rollback,
expiry fallback, stale refresh, same-second claim changes, origin ownership,
snapshot corruption, local-tail recovery, retention floors and guarded handover.
The 17-database journey exercises all 272 directed paths in-process, including
partition recovery. Child-process tests interrupt commits at actual native
publication hooks; OS caches remain present, so they do not establish physical
power-loss durability.

The CLI also exposes `init DIRECTORY REALM POLICY`,
`candidates DIRECTORY TIME IP FIELD`,
`checkpoint DIRECTORY ORIGIN SEQUENCE`, `replicate SOURCE DESTINATION ORIGIN`
and `baseline SOURCE NEW_DIRECTORY NEW_REALM IMPORT_TIME`. `replicate` uses
verified pages against the destination's existing cursor. `baseline` validates
a new-realm candidate while preserving the source; it does not switch a service's
active handle. CLI realms are decimal integers.

The local candidate probe report, `benchmark/IPDON_CANDIDATES_RESULTS.json`, verifies
paged results against a complete fixture for both address families, including
expired/future scan charges, duplicate batch accounting, false/unknown fields,
explicit resumable truncation and zero result capacity for stale-only pages.
The local integration report, `benchmark/IPDON_INTEGRATION_QUALIFICATION.json`,
records checks for the new APIs and the native CLI journey.

The local synthetic report, `benchmark/IPDON_REGIONAL_RESULTS.json`,
uses 1,024 mixed IPv4/IPv6 RRsets with four members each and 2,048 field claims.
Independent cases verify identical seed events and logical results after reopen.
Compact RRset refreshes use 36.5% fewer encoded journal bytes than full
same-revision retransmissions. Compact field refreshes use 23.4% fewer encoded
journal bytes and 77.7% fewer writer-arena bytes than full same-value replacements.
These are representation/work measurements from one synthetic run. Dirty arena
bytes exclude commit publication work and are not measured physical I/O;
recorded timings are not production capacity or statistical speedup claims.
