# Completed lookup capability and native LMDB comparison

The explicit snapshot-scoped hash API is implemented and functionally qualified. **The requested broad no-regression condition is not established.** The measurements support selecting it for stable snapshots serving many exact reads. Existing readers never automatically construct an index; there is no writer hook or automatic rebuild.

The original link describes [Bf-Tree, a larger-than-memory range index](https://vldb.org/pvldb/vol17/p3442-hao.pdf). LMDB describes its own [memory-mapped Btree design](https://raw.githubusercontent.com/LMDB/lmdb/mdb.master/libraries/liblmdb/lmdb.h). This campaign compares the completed ULMDB hash strategy with original native ULMDB and pinned native LMDB. It does **not** run Bf-Tree or reproduce the paper's larger-than-memory experiments.

## Decision and gates

| Gate | Result | Evidence |
|---|---|---|
| Snapshot, borrowed bytes, admission and lifetime behavior | Pass, scoped | 17 current-workspace integration checks; controlled allocator refusal and same-snapshot recovery |
| Material exact-read improvement | Pass | Every point workload improved materially in every process on both backends, against both native ULMDB and LMDB |
| Default throughput centers within 5% | Pass, observed centers only | All 30 canonical cells: −4.84% to +4.76%; intervals do not establish equivalence |
| All default metrics within 5% | Not clear | Five adverse observed centers exceed 5%; 47 of 92 target intervals still permit deterioration exceeding 5% |
| Active ordered fallback within 5% | Not clear | One paired median costs 5.08% more; other individual adverse repeats remain visible |
| Automatic/default promotion | Not admitted | Build, memory, scan, lifecycle and uncertainty tradeoffs remain material |

## Complete mixed exact-lookup matrix

Nanoseconds per query; lower is better. All six datasets, 50% hits/misses, 32-byte values. Values are medians across process medians; speedups elsewhere use medians of paired process ratios. These two aggregations need not divide exactly.

| Keys | Key type | Legacy native | Legacy hash | Resident native | Resident hash | Native LMDB |
|---:|---|---:|---:|---:|---:|---:|
| 8,192 | fixed16 | 367.88 | 60.05 | 250.04 | 60.12 | 253.20 |
| 8,192 | hostname | 368.08 | 70.30 | 388.66 | 68.96 | 276.46 |
| 100,000 | fixed16 | 474.03 | 96.64 | 388.37 | 93.99 | 352.88 |
| 100,000 | hostname | 480.42 | 132.38 | 574.67 | 132.24 | 369.18 |
| 1,000,000 | fixed16 | 1,021.77 | 241.58 | 894.33 | 248.28 | 738.55 |
| 1,000,000 | hostname | 1,106.54 | 254.19 | 1,305.00 | 268.75 | 828.33 |

All 48 read rows, including pure hits, pure misses, hot reads, forward/reverse scans and bounded ranges, are in [the full direct table](LOOKUP_FULL_TABLE.md). The [CSV](LOOKUP_FULL_TABLE.csv) retains all **1,308 metric rows** across five arms: 48 read, 54 durable-write, 96 lifecycle, 1,092 memory, 12 storage and six index-capacity rows. No adverse row is filtered out.

## Construction, amortization and memory

Resident results below charge explicit full preparation and a separate new-generation rebuild. Approximate break-even is the median paired marginal preparation cost divided by the measured mixed-read saving. It excludes application scheduling and assumes the same immutable snapshot remains useful. It is a workload estimate, not an automatic selection rule.

| Keys | Key type | Owned slot MiB per index | Initial hash API build ms | New-generation rebuild ms | Approximate mixed reads to amortize extra preparation |
|---:|---|---:|---:|---:|---:|
| 8,192 | fixed16 | 0.625 | 1.362 | 0.916 | 7,197 |
| 8,192 | hostname | 0.625 | 1.424 | 1.096 | 4,466 |
| 100,000 | fixed16 | 10.000 | 21.265 | 19.471 | 72,325 |
| 100,000 | hostname | 10.000 | 24.217 | 24.752 | 53,783 |
| 1,000,000 | fixed16 | 80.000 | 283.632 | 284.115 | 442,069 |
| 1,000,000 | hostname | 80.000 | 338.955 | 341.521 | 327,902 |

Keys and values are borrowed. The slot-vector budget excludes allocator bookkeeping, native cursor workspace and pages retained by the caller's read transaction. Two simultaneous million-key indices require about **160 MiB in owned slots**, plus their source snapshots. Actual process RSS checkpoints, old/new overlap, destruction and source-only controls are in the full direct table/CSV. All 12 native/hash apparent/allocated storage comparisons match within their backend; this is not a persistent-storage reduction.

## Every default-path workload

This is the complete 15-workload × two-filesystem matrix with the accelerator unused. Throughput increases are favorable; p99 increases are adverse. Values below are candidate changes against the frozen before-capability baseline in the **same canonical build directory**, nine fresh process pairs per filesystem. Sampled p99 is the geometric mean of timer-inclusive trial percentiles, not a pooled population percentile. Shared-host measurements have substantial uncertainty.

| Workload | Disk throughput change | Disk sampled p99 change | RAM throughput change | RAM sampled p99 change |
|---|---:|---:|---:|---:|
| durable_batch1 | +1.75% | +1.67% | -0.74% | -0.65% |
| durable_batch64 | -3.55% | +7.18% | +0.20% | -0.84% |
| ipv4_lpm_hits | +0.82% | -1.32% | -2.50% | +4.01% |
| ipv4_lpm_inside_gap_misses | -4.59% | +4.95% | -0.55% | +0.13% |
| ipv4_lpm_misses | +0.20% | +0.76% | +0.07% | -0.24% |
| ipv4_lpm_mixed | +0.40% | -0.98% | -0.74% | +0.27% |
| ipv4_lpm_outside_cover_misses | -0.30% | +1.80% | -0.73% | +0.00% |
| ipv6_lpm_hits | -4.84% | +4.38% | +0.63% | -0.63% |
| ipv6_lpm_inside_gap_misses | +2.10% | -1.71% | -0.53% | +0.97% |
| ipv6_lpm_misses | -0.29% | -1.43% | +0.55% | +0.25% |
| ipv6_lpm_mixed | -4.07% | +2.41% | +0.21% | -0.49% |
| ipv6_lpm_outside_cover_misses | +4.76% | -6.68% | +0.16% | -0.91% |
| ordered_kv_hits | -2.67% | +3.71% | -0.65% | +1.63% |
| ordered_kv_misses | -1.10% | +1.03% | -0.79% | +0.65% |
| ordered_kv_mixed | -1.04% | +2.33% | -0.75% | +2.23% |

[Full baseline/candidate/LMDB rates, p99 values, controls and intervals](DEFAULT_CANONICAL.md) and [all 92 target metric statuses](regression-canonical/ADMISSION_REVIEW.md) are retained. Native LMDB controls are never normalized away.

### All canonical default target centers worse by more than 5%

| Exact metric path | Change | Adjusted candidate/baseline interval |
|---|---:|---|
| `full/disk/ulmdb/durable_batch1/pooled_sampled_p99` | +48.896% | 0.528–4.198 |
| `full/disk/ulmdb/durable_batch64/sampled_p99` | +7.180% | 0.826–1.392 |
| `full/disk/ulmdb/reopen/complete_cold_check_ns` | +5.412% | 0.833–1.334 |
| `full/disk/ulmdb/reopen/validation_ns` | +10.082% | 0.741–1.636 |
| `full/ram/ulmdb/reopen/experimental_reenable_ns` | +7.720% | 0.768–1.511 |

These intervals are inconclusive; they neither establish causality nor exclude a material regression. The field `complete_cold_check_ns` names an existing reopen journey; OS caches were not evicted. It includes environment close/open, experimental re-enablement and old/new validation. Its validation phase is also reported separately above.

The original three-pair run and its independent nine-pair repeat remain in [primary](DEFAULT_PRIMARY.md), [confirmation](DEFAULT_CONFIRMATION.md) and [every primary-versus-confirmation cell](DEFAULT_REPEATS.md). The earlier 8–10% IPv4 miss-tail pattern did not reproduce under canonical build paths. Static inspection found identical core/prefix fingerprints and normalized instructions with matching function order/alignment after that control; raw `.text` still differs. Prior adverse captures remain evidence, rather than being replaced by the canonical result.

## Other adverse observations

- With an active hash facade, Resident 100K fixed-key reverse ranges cost **5.08% more** at the paired median; the three process changes are +5.08%, +1.95% and +6.66%. Three Legacy and six Resident ordered cases exceed 5% in at least one process. The worst individual ordered increase is 9.78%. Inlining cursor forwarding reduced the earlier wrapper overhead; both complete 90-process campaigns are retained. [Earlier direct table](prior-direct/FULL_TABLE.md).
- Resident million-key fixed-key `unpinned_before/durable_batch64` costs **7.15–10.43% more** in the hash-labelled processes before hash construction, with +8.64% paired median cost. This is an observed process difference, not evidence that an index which has not yet been built caused it. The row is retained. All 54 durable-write cells show native, hash and LMDB costs.
- LMDB remains faster for these ordered scans/ranges. Legacy hash-labelled durable writes also lose to LMDB across this matrix. Exact-read acceleration does not establish whole-engine superiority.
- Rebuilding costs O(N); no benchmark result justifies rebuilding automatically after every commit. Cold-storage throughput, controlled OS page-reclaim pressure and concurrent application performance are not established. Functional File reader-thread/writer checks are separate from performance measurements; no Resident multithreaded benchmark is claimed.

## Validation and source control

Both direct campaigns complete 90 fresh processes, three seeds × three sizes × two key types × five arms, with seven samples per read/write workload. Reads consume complete key/value bytes and every corresponding checksum/count is checked across all arms. Initial, held-old, new and reopened snapshots are verified against the deterministic full-byte oracle. The default campaigns retain 12 + 36 + 36 fresh processes, each executing the existing 15-workload engine suite with its own native LMDB controls.

The controlled benchmark compares frozen before/after working trees with an owned-only runtime delta. One existing IPDon cursor generic mismatch was corrected identically in both inputs before the workspace suite could compile; the original bytes and prerequisite receipt are archived. No pre-existing failing test was weakened. The final frozen workspace run has the same three failed targets as baseline: `ulmdb --test compaction`, `ulmdb --test kv_outcome_resources`, and `ulmdb-storage --lib`. All 14 new checks in that frozen candidate pass.

Concurrent unrelated workspace development is excluded from the controlled A/B delta. Additional integration was qualified against a newly frozen current workspace, including its newer Resident comparator API: **16 focused integration tests plus one compile-fail lifetime test pass**. The only strategy implementation compatibility difference is the Resident cursor's native comparator generic, matching each native engine API; forwarding bodies are identical. Current non-bytewise Resident and managed Range admission tests pass. This functional promotion check does not turn those unrelated current engine edits into measured performance changes.

The allocator-refusal gate loads two million source records, then limits only its owned child to current virtual size plus 8 MiB. A valid-budget 160 MiB index allocation returns `LimitExceeded`; the same original native/Ordered readers verify every source record and preserve commit identity. Restoring the same child's original limit lets identical hash preparation succeed and verifies every value. This qualifies graceful File allocation refusal/recovery, not Resident allocator refusal or real physical-memory pressure performance.

### Benchmark capacity and failed attempts

Initial failed attempts remain in the evidence archive. Resident source-only and hash arms both reproduced pinned-snapshot `Busy` under insufficient pool geometry. Before choosing the long-lived snapshot, both final Resident arms perform the same explicit ordinary `collect()`, charged as `maintenance_before_pin`. Final common benchmark capacity is `ceil(keys/24) + 8192` KV slots, 256 MiB metadata admission and the existing 256 MiB WAL cap, enough for two live generations plus transaction headroom. These are disclosed harness settings; engine defaults and guards are unchanged. The actual API never performs collection automatically.

LMDB uses the pinned vendored **1.0.2** reference with normal locking and synchronized commits. Direct comparisons set only `MDB_NOTLS` (0x200000) to permit the retained old reader and same-thread writer; the existing default regression suite uses flags zero. There is no NOSYNC comparison. Initial loader APIs differ and are identified in raw lifecycle reports; warm reopen is not an evicted-cache test.

## Reproduction and retained evidence

- [Complete direct Markdown table](LOOKUP_FULL_TABLE.md) and [all 1,308 metric rows in CSV](LOOKUP_FULL_TABLE.csv).
- [Default full comparison](DEFAULT_CANONICAL.md), [gate review](regression-canonical/ADMISSION_REVIEW.md), and [canonical codegen record](regression-canonical/CODEGEN_REVIEW.json).
- [Source, executable, checksums and qualification](qualification.json), [outer manifest](MANIFEST.json), and [checksums](SHA256SUMS).
- `evidence.tar.xz` contains exact frozen source trees, raw JSON, per-trial latency traces, captured tooling, baseline/final test logs, failed preflights, allocation refusal/recovery evidence and the additional live-integration receipt. It excludes database fixtures and compiled target trees; executable bytes are verified against retained hashes before packaging.

```sh
python3 benchmark/lookup-strategies/direct_campaign.py \
  --source /path/to/frozen/source --output benchmark/work/NEW_DIRECT \
  --cpu 12 --rounds 3 --sizes 8192,100000,1000000 \
  --queries 65536 --samples 7 --write-transactions 4

python3 benchmark/lookup-strategies/regression.py \
  --baseline /path/to/frozen/before --candidate /path/to/frozen/after \
  --output benchmark/work/NEW_REGRESSION \
  --canonical-build-dir benchmark/work/NEW_CANONICAL_BUILD \
  --cpu 12 --rounds 9

python3 benchmark/lookup-strategies/retain_completed.py \
  --output benchmark/lookup-strategies/completed --verify-only
```

Choose an available CPU and fresh output directories. Freeze sources before building; `--build-only` then `--resume` separates compilation from timing with exact configuration/hash checks. The machine is an AMD EPYC 7702P, CPU 12, shared host without exclusive CPU reservation. The full direct table records compiler, kernel, executable and source hashes.
